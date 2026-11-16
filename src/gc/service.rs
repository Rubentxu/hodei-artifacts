//! El servicio de recoleccion: el censo, el dry-run y el barrido.
//!
//! Une las tres fuentes --el catalogo (raices y retenciones), el almacen
//! (los blobs y su edad) y el libro (la marca y el rastro)-- y deja que
//! el plan puro decida. Sus reglas, de mas a menos importantes:
//!
//! 1. **Ante la duda, no se borra.** Si el censo o un manifiesto no se
//!    puede leer, la pasada aborta: un grafo a medias dejaria sin
//!    proteger lo que no se vio.
//! 2. **El dry-run es el plan, y el barrido es el mismo plan.** Lo que
//!    el informe dice reclamable es lo que el barrido puede borrar, y
//!    nada mas.
//! 3. **Marca y barrido son durables e idempotentes.** La pasada se
//!    escribe antes de tocar un byte; un corte entre medias se reanuda
//!    sin borrar dos veces y sin perder lo marcado.
//! 4. **El borrado deja rastro.** La intencion se anota antes de borrar
//!    y se confirma despues; el evento de auditoria sale al confirmar.
//! 5. **Una sola pasada a la vez**, y cada borrado vuelve a comprobar la
//!    edad bajo la barrera del almacen.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use hodei_catalog::{
    CatalogError, ContentHold, ContentLedger, DeletionState, GcDeletion, GcRun, HoldScope,
    RootKind, RunCandidate, RunState,
};
use hodei_evidence::ContentDigest;
use hodei_hub::{BlobError, BlobReclaim, BlobStore, ReadPolicy, Reclaimed, StoredBlob};
use serde::Serialize;
use tokio::sync::Mutex;

use super::plan::{self, EdgeError, Input, Plan, Verdict};

/// Por que una recoleccion no se hizo.
#[derive(Debug, thiserror::Error)]
pub enum GcError {
    /// El almacen no sabe listar ni borrar con barrera.
    #[error("the content store does not support reclamation")]
    Unsupported,
    /// Ya hay una pasada en curso.
    #[error("a collection is already running")]
    Busy,
    /// No se pudo saber que esta vivo, y por eso no se toca nada.
    #[error("the state of the content is unknown, nothing was deleted: {0}")]
    Unknown(String),
    /// El catalogo o el libro fallaron.
    #[error("the catalog failed: {0}")]
    Ledger(#[from] CatalogError),
    /// El almacen fallo.
    #[error("the content store failed: {0}")]
    Store(String),
    /// La peticion no es valida.
    #[error("{0}")]
    Invalid(String),
}

/// El resultado de un barrido.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SweepReport {
    /// La pasada.
    pub run_id: String,
    /// Si es una pasada marcada antes y reanudada ahora.
    pub resumed: bool,
    /// Cuantos blobs estaban marcados.
    pub marked: u64,
    /// Cuantos se borraron en esta pasada.
    pub deleted: u64,
    /// Los bytes liberados.
    pub bytes_freed: u64,
    /// Cuantos se dejaron sin borrar porque dejaron de ser reclamables.
    pub skipped: u64,
}

/// La recoleccion de basura del almacen.
pub struct GcService {
    ledger: Arc<dyn ContentLedger>,
    store: Arc<dyn BlobStore>,
    grace: Duration,
    /// Una sola pasada a la vez: dos barridos concurrentes escribirian
    /// dos marcas y se pisarian el rastro.
    gate: Mutex<()>,
}

impl GcService {
    /// Crea el servicio. Falla si el almacen no sabe reclamar.
    pub fn new(
        ledger: Arc<dyn ContentLedger>,
        store: Arc<dyn BlobStore>,
        grace: Duration,
    ) -> Result<Self, GcError> {
        if store.reclaimer().is_none() {
            return Err(GcError::Unsupported);
        }
        Ok(Self {
            ledger,
            store,
            grace,
            gate: Mutex::new(()),
        })
    }

    /// El periodo de gracia.
    pub fn grace(&self) -> Duration {
        self.grace
    }

    fn reclaimer(&self) -> Result<&dyn BlobReclaim, GcError> {
        self.store.reclaimer().ok_or(GcError::Unsupported)
    }

    /// El plan de ahora: el dry-run. No escribe ni borra nada.
    pub async fn dry_run(&self) -> Result<Plan, GcError> {
        let (plan, _) = self.census().await?;
        Ok(plan)
    }

    /// El censo y el plan que sale de el.
    ///
    /// Devuelve tambien los blobs, porque el barrido necesita saber que
    /// sigue en el almacen.
    async fn census(&self) -> Result<(Plan, Vec<StoredBlob>), GcError> {
        let blobs = self.reclaimer()?.list_blobs().await.map_err(|error| {
            GcError::Unknown(format!("the store could not be listed: {error:?}"))
        })?;
        let roots =
            self.ledger.content_roots().await.map_err(|error| {
                GcError::Unknown(format!("the roots could not be read: {error}"))
            })?;
        let holds =
            self.ledger.holds().await.map_err(|error| {
                GcError::Unknown(format!("the holds could not be read: {error}"))
            })?;

        // Las aristas: lo que cada manifiesto vivo nombra. Se leen de
        // los bytes del manifiesto, que es donde esta la verdad, y no
        // se persisten: un indice aparte seria una segunda verdad.
        let mut edges: HashMap<ContentDigest, Vec<ContentDigest>> = HashMap::new();
        let mut seen: HashSet<ContentDigest> = HashSet::new();
        for root in &roots {
            // Un manifiesto OCI es el asset `manifests/<su digest>`. Una
            // ruta cruda cualquiera que empiece por `manifests/` no lo es,
            // y no puede dejar la recoleccion en estado desconocido.
            if root.kind != RootKind::Asset
                || root.subject.strip_prefix("manifests/") != Some(root.digest.value.as_str())
            {
                continue;
            }
            if !seen.insert(root.digest.clone()) {
                continue;
            }
            let bytes = self
                .store
                .get_with(&root.digest, ReadPolicy::Verify)
                .await
                .map_err(|error| {
                    GcError::Unknown(format!(
                        "the manifest {} could not be read: {error:?}",
                        root.digest
                    ))
                })?;
            // Las aristas son defensa en profundidad: cada capa y cada
            // configuracion tiene ya su propia raiz `blobs/<digest>`, y
            // cada hijo de un indice es un manifiesto con la suya. Un
            // manifiesto que no se entiende (un fichero crudo con esa
            // ruta, un tipo que el registro acepta y el censo no conoce)
            // no aporta aristas, pero tampoco bloquea la recoleccion de
            // todo el almacen: su propio contenido sigue protegido por la
            // raiz que lo nombra.
            match plan::manifest_edges(&bytes) {
                Ok(children) => {
                    edges.insert(root.digest.clone(), children);
                }
                Err(error @ EdgeError::NotJson)
                | Err(error @ EdgeError::Unrecognized)
                | Err(error @ EdgeError::BadDigest) => {
                    tracing::warn!(
                        digest = %root.digest,
                        %error,
                        "a manifest is not understood: it contributes no edges"
                    );
                }
            }
        }

        let plan = plan::plan(&Input {
            blobs: &blobs,
            roots: &roots,
            holds: &holds,
            edges: &edges,
            grace: self.grace,
            now: Utc::now(),
        });
        Ok((plan, blobs))
    }

    /// Barre: marca lo reclamable y lo borra.
    ///
    /// Si hay una pasada marcada sin completar --un corte entre la marca
    /// y el barrido--, la **reanuda** en lugar de abrir otra.
    pub async fn sweep(&self, actor: &str, account: &str) -> Result<SweepReport, GcError> {
        let _single = self.gate.try_lock().map_err(|_| GcError::Busy)?;
        let reclaimer = self.reclaimer()?;

        let pending = self.ledger.pending_run().await?;
        let resumed = pending.is_some();
        let run = match pending {
            Some(run) => run,
            None => {
                // Primer escaneo: lo que se marca.
                let (plan, _) = self.census().await?;
                let candidates: Vec<RunCandidate> = plan
                    .reclaimable()
                    .filter_map(|entry| {
                        parse_digest(&entry.digest).map(|digest| RunCandidate {
                            digest,
                            length: entry.length,
                        })
                    })
                    .collect();
                let run = GcRun {
                    id: format!("gc-{}", uuid::Uuid::new_v4().simple()),
                    state: RunState::Marked,
                    grace_secs: self.grace.as_secs(),
                    actor: actor.to_string(),
                    candidates,
                    marked_at: Utc::now(),
                    deleted: 0,
                    bytes_freed: 0,
                    swept_at: None,
                };
                // La marca se escribe antes de tocar un byte.
                self.ledger.open_run(&run).await?;
                run
            }
        };

        // Segundo escaneo: lo marcado se reevalua con un censo fresco.
        // Algo pudo publicarse, retenerse o rejuvenecer desde la marca;
        // solo se borra lo que sigue siendo reclamable ahora.
        let (fresh, blobs) = self.census().await?;
        let reclaimable_now: HashSet<String> = fresh
            .entries
            .iter()
            .filter(|entry| matches!(entry.verdict, Verdict::Reclaimable { .. }))
            .map(|entry| entry.digest.clone())
            .collect();
        let present: HashSet<String> = blobs.iter().map(|blob| blob.digest.to_string()).collect();

        let mut skipped = 0u64;
        for candidate in &run.candidates {
            let key = candidate.digest.to_string();
            let trace = self.ledger.deletion_of(&candidate.digest).await?;
            let mine = trace.filter(|trace| trace.run_id == run.id);

            match mine {
                // Ya confirmado por esta pasada: no se repite.
                Some(trace) if trace.state == DeletionState::Confirmed => continue,
                // Se anoto la intencion y el blob ya no esta: el corte
                // fue entre el borrado y la confirmacion.
                Some(trace) if !present.contains(&key) => {
                    self.confirm(&trace, candidate.length, account).await?;
                    continue;
                }
                _ => {}
            }

            if !reclaimable_now.contains(&key) {
                skipped += 1;
                continue;
            }

            let intended = GcDeletion {
                digest: candidate.digest.clone(),
                length: candidate.length,
                run_id: run.id.clone(),
                actor: actor.to_string(),
                account: account.to_string(),
                state: DeletionState::Intended,
                at: Utc::now(),
            };
            self.ledger.record_deletion(&intended).await?;

            match reclaimer
                .delete_if_older_than(&candidate.digest, self.grace)
                .await
            {
                Ok(Reclaimed::Deleted { length }) => {
                    self.confirm(&intended, length, account).await?;
                }
                Ok(Reclaimed::Gone) => {
                    // Se fue entre el censo y el borrado: no lo borramos
                    // nosotros ahora, y no se cuenta como borrado nuestro.
                    skipped += 1;
                }
                Ok(Reclaimed::Fresh) => skipped += 1,
                Err(error) => {
                    return Err(GcError::Store(format!(
                        "{} could not be deleted: {error:?}",
                        candidate.digest
                    )));
                }
            }
        }

        // Los totales salen del rastro durable, no de esta ejecucion:
        // una pasada reanudada cuenta lo que borro antes del corte.
        let mut deleted = 0u64;
        let mut bytes_freed = 0u64;
        for candidate in &run.candidates {
            if let Some(trace) = self.ledger.deletion_of(&candidate.digest).await?
                && trace.run_id == run.id
                && trace.state == DeletionState::Confirmed
            {
                deleted += 1;
                bytes_freed += trace.length;
            }
        }
        self.ledger
            .complete_run(&run.id, deleted, bytes_freed, Utc::now())
            .await?;

        Ok(SweepReport {
            run_id: run.id,
            resumed,
            marked: run.candidates.len() as u64,
            deleted,
            bytes_freed,
            skipped,
        })
    }

    async fn confirm(
        &self,
        intended: &GcDeletion,
        length: u64,
        account: &str,
    ) -> Result<(), GcError> {
        let confirmed = GcDeletion {
            state: DeletionState::Confirmed,
            length,
            account: account.to_string(),
            at: Utc::now(),
            ..intended.clone()
        };
        self.ledger.record_deletion(&confirmed).await?;
        Ok(())
    }

    /// Las retenciones registradas.
    pub async fn holds(&self) -> Result<Vec<ContentHold>, GcError> {
        Ok(self.ledger.holds().await?)
    }

    /// Pone una retencion.
    pub async fn place_hold(
        &self,
        actor: &str,
        scope: HoldScope,
        reason: &str,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<ContentHold, GcError> {
        if reason.trim().is_empty() {
            return Err(GcError::Invalid("a hold needs a reason".to_string()));
        }
        if expires_at.is_some_and(|at| at <= Utc::now()) {
            return Err(GcError::Invalid(
                "a hold cannot expire in the past".to_string(),
            ));
        }
        let hold = ContentHold {
            id: format!("hold-{}", uuid::Uuid::new_v4().simple()),
            scope,
            reason: reason.trim().to_string(),
            actor: actor.to_string(),
            created_at: Utc::now(),
            expires_at,
        };
        self.ledger.place_hold(&hold).await?;
        Ok(hold)
    }

    /// Libera una retencion. Devuelve si existia.
    pub async fn release_hold(&self, id: &str) -> Result<bool, GcError> {
        Ok(self.ledger.release_hold(id).await?)
    }
}

fn parse_digest(text: &str) -> Option<ContentDigest> {
    let value = text.strip_prefix("sha256:")?;
    Some(ContentDigest::sha256(value))
}

impl From<BlobError> for GcError {
    fn from(error: BlobError) -> Self {
        Self::Store(format!("{error:?}"))
    }
}
