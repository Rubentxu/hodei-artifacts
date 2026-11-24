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
//! 6. **El dry-run comparte censo.** Un plano del almacen vale lo mismo
//!    para veinte peticiones que para una, y calcularlo veinte veces a la
//!    vez no es mas fresco: es veinte grafos completos apilados en
//!    memoria y en disco. Lo que borra --el barrido y el scrub-- toma
//!    siempre su propio censo, al momento.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

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

/// Como el censo trata un manifiesto que no se puede leer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ManifestReads {
    /// Un manifiesto ilegible es estado desconocido: la recoleccion no actua.
    Strict,
    /// Un manifiesto ausente o corrupto no aporta aristas y el censo sigue.
    Lenient,
}

/// La ventana maxima de validez del plano compartido del dry-run: un
/// plano viejo por treinta segundos es un plano informado, no un plano
/// roto; un plano viejo por horas no es un plano.
const PLAN_CACHE_MAX_TTL: Duration = Duration::from_secs(30);

/// El ultimo plano del dry-run: la generacion con la que se calculo, el
/// instante en que se publico y el plano compartido.
type CachedPlan = (u64, Instant, Arc<Plan>);

/// La recoleccion de basura del almacen.
pub struct GcService {
    ledger: Arc<dyn ContentLedger>,
    store: Arc<dyn BlobStore>,
    grace: Duration,
    /// Una sola pasada a la vez: dos barridos concurrentes escribirian
    /// dos marcas y se pisarian el rastro.
    gate: Mutex<()>,
    /// Cuanto vale un plano del dry-run: lo que mande la gracia, cortado
    /// por el maximo. Con una gracia de milisegundos el plano no puede
    /// valer mas que ella, porque envejece dentro de su propia ventana.
    plan_ttl: Duration,
    /// El plano compartido. Se lee, se escribe y se tira sin sostener el
    /// cerrojo a traves de un `await`: lo que tarda un censo se sostiene
    /// con [`Self::plan_flight`], y aqui solo se mira y se publica.
    plan_cache: Mutex<Option<CachedPlan>>,
    /// El vuelo unico: quien calcula un censo lo sostiene; los demas
    /// esperan aqui detras y entran al cache para recoger el resultado.
    plan_flight: Mutex<()>,
    /// La generacion del mundo. Cada mutacion que cambia lo que el plan
    /// diria --retenciones, barridos-- la rompe, y un plano de una
    /// generacion muerta no se sirve jamas.
    plan_generation: AtomicU64,
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
            plan_ttl: grace.min(PLAN_CACHE_MAX_TTL),
            plan_cache: Mutex::new(None),
            plan_flight: Mutex::new(()),
            plan_generation: AtomicU64::new(0),
        })
    }

    /// El periodo de gracia.
    pub fn grace(&self) -> Duration {
        self.grace
    }

    /// The durable ledger behind the service (for the coordinated pass).
    pub(crate) fn ledger(&self) -> &Arc<dyn ContentLedger> {
        &self.ledger
    }

    /// The content store behind the service.
    pub(crate) fn store(&self) -> &Arc<dyn BlobStore> {
        &self.store
    }

    /// The single-pass gate, shared by the legacy sweep and the
    /// coordinated pass: one writer per process, whatever the mode.
    pub(crate) fn gate(&self) -> &Mutex<()> {
        &self.gate
    }

    fn reclaimer(&self) -> Result<&dyn BlobReclaim, GcError> {
        self.store.reclaimer().ok_or(GcError::Unsupported)
    }

    /// El plan de ahora: el dry-run. No escribe ni borra nada.
    ///
    /// The plan is shared within a freshness window and computed by one
    /// census at a time: concurrent callers wait on the census instead of
    /// piling full graphs into memory and disk. A result computed while
    /// the world changed (the generation moved under it) is returned but
    /// never cached. Sweep and scrub never come through here: what
    /// deletes always takes its own fresh census.
    pub async fn dry_run(&self) -> Result<Arc<Plan>, GcError> {
        // Lock order, always the same here and in the invalidation:
        // flight first, cache second. Nothing else ever takes the flight.
        let _flight = self.plan_flight.lock().await;
        let generation = self.plan_generation.load(Ordering::Acquire);
        let cached = self.plan_cache.lock().await.clone();
        if let Some((cached_generation, at, plan)) = cached
            && cached_generation == generation
            && at.elapsed() < self.plan_ttl
        {
            return Ok(plan);
        }
        let (plan, _) = self.census().await?;
        let plan = Arc::new(plan);
        let mut cache = self.plan_cache.lock().await;
        if self.plan_generation.load(Ordering::Acquire) == generation {
            *cache = Some((generation, Instant::now(), plan.clone()));
        }
        Ok(plan)
    }

    /// Drops the shared plan: the next dry-run computes a fresh census.
    ///
    /// Called after anything that changes what the plan would say. When
    /// a census is in flight the generation moves anyway, so the in-flight
    /// result is returned but not cached.
    pub(crate) fn invalidate_plan_cache(&self) {
        self.plan_generation.fetch_add(1, Ordering::AcqRel);
        if let Ok(mut cache) = self.plan_cache.try_lock() {
            *cache = None;
        }
    }

    /// El censo y el plan que sale de el.
    ///
    /// Devuelve tambien los blobs, porque el barrido necesita saber que
    /// sigue en el almacen.
    pub(crate) async fn census(&self) -> Result<(Plan, Vec<StoredBlob>), GcError> {
        self.census_with(ManifestReads::Strict).await
    }

    /// Como [`Self::census`], con la politica de lectura de manifiestos.
    ///
    /// `Lenient` es para el scrub: un manifiesto ausente o corrupto no
    /// aborta el censo, porque eso es justo lo que el scrub tiene que
    /// informar. Otros fallos de lectura siguen siendo estado desconocido.
    pub(crate) async fn census_with(
        &self,
        reads: ManifestReads,
    ) -> Result<(Plan, Vec<StoredBlob>), GcError> {
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
            let bytes = match self.store.get_with(&root.digest, ReadPolicy::Verify).await {
                Ok(bytes) => bytes,
                Err(error @ (BlobError::NotFound | BlobError::Corrupted { .. }))
                    if reads == ManifestReads::Lenient =>
                {
                    tracing::warn!(
                        digest = %root.digest,
                        ?error,
                        "a manifest is missing or damaged: it contributes no edges"
                    );
                    continue;
                }
                Err(error) => {
                    return Err(GcError::Unknown(format!(
                        "the manifest {} could not be read: {error:?}",
                        root.digest
                    )));
                }
            };
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
            roots_are_complete: true,
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
                    // Modo legacy: sin scope ni generacion de claim.
                    scope: None,
                    generation: None,
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
                // Modo legacy: el barrido no observa version de blob.
                marker: None,
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
        // The world changed bytes: the shared plan of the dry-run is
        // stale from here on, whatever this pass deleted.
        self.invalidate_plan_cache();

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
        self.invalidate_plan_cache();
        Ok(hold)
    }

    /// Libera una retencion. Devuelve si existia.
    pub async fn release_hold(&self, id: &str) -> Result<bool, GcError> {
        let released = self.ledger.release_hold(id).await?;
        if released {
            self.invalidate_plan_cache();
        }
        Ok(released)
    }
}

pub(crate) fn parse_digest(text: &str) -> Option<ContentDigest> {
    let value = text.strip_prefix("sha256:")?;
    Some(ContentDigest::sha256(value))
}

impl From<BlobError> for GcError {
    fn from(error: BlobError) -> Self {
        Self::Store(format!("{error:?}"))
    }
}
