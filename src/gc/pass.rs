//! La pasada coordinada y fenced entre replicas (HSP-192).
//!
//! El `sweep` del servicio es el modo legacy: un proceso, un cerrojo de
//! memoria. La pasada coordinada es el modo entre replicas, y por eso
//! nada de su estado vive en la memoria del worker:
//!
//! 1. **El claim es durable.** Se toma con `claim_gc_pass` sobre el
//!    scope; expira por lease; cada operacion posterior presenta el
//!    token de su generacion. Un worker zombie con la generacion
//!    vencida no puede marcar, ni confirmar, ni cerrar: la autoridad
//!    lo rechaza. El exito del claim se decide **releyendo despues de
//!    commitear** (regla HSP-205), no porque la transaccion de claim
//!    haya commiteado.
//! 2. **La marca es transaccional con guardas.** `mark_run_fenced`
//!    escribe la pasada verificando el claim vigente y las
//!    referencias vivas en la MISMA transaccion (patron de frontier,
//!    HSP-199) y devuelve lo marcado leido post-commit.
//! 3. **Antes de tocar un byte, se revalida.** `live_reference_count`
//!    contra el estado real: si un publisher nombro el blob, se
//!    descarta y su intencion no se confirma. Tras el efecto, el
//!    veredicto sale de releer el almacén (confirmacion solo
//!    post-aplicado), y una intencion huérfana de un corte se
//!    reconcilia por relectura: intentada + blob ido = confirmado.
//! 4. **El object store decide como se retira (AC2).** Con delete
//!    condicional, el borrado va condicionado a la version marcada:
//!    una reescritura concurrente produce `NotApplied` y la clave NO
//!    se reutiliza hasta reconciliar. Con capacidad de version pero
//!    sin delete condicional, cuarentena versionada: los bytes
//!    marcados van a un nombre NUEVO (nunca reescritura) y la clave
//!    queda reservada en el libro. Sin ninguna de las dos, la pasada
//!    **no borra nada** y todo queda `Unknown`. La pregunta la
//!    responde el tipo del almacen, no una configuracion.
//! 5. **El censo va por paginas (AC3).** `root_page` recorre las
//!    raices con cota `page_limit` sin materializar el grafo. Mientras
//!    el censo no termina, `roots_are_complete` es falso y el plan
//!    puro degrada lo huerfano a `Unknown`: **Unknown jamas borra**.
//! 6. **La reanudacion sale del libro.** Un corte entre marca y
//!    barrido se recupera con `pending_scoped_run` + `mark_of_run`:
//!    el rastro durable, no la memoria.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use hodei_catalog::{
    CatalogError, ContentLedger, DeletionState, GcDeletion, GcLease, GcRun, Quarantined,
    RunCandidate, RunState,
};
use hodei_evidence::ContentDigest;
use hodei_hub::{ConditionalBlob, ConditionalOutcome, VersionMarker};
use serde::Serialize;

use super::plan::{self, Input, Plan};
use super::service::{GcError, GcService};

/// Cuantas raices caben en una pagina del censo coordinado.
///
/// Es la cota de materializacion: el censo nunca apila el grafo
/// completo en memoria. En un deployment se configura; aqui es
/// constante para que la prueba la exercite.
pub const ROOT_PAGE_LIMIT: usize = 512;

/// Los knobs de una pasada coordinada.
#[derive(Debug, Clone)]
pub struct PassConfig {
    /// El scope de autoridad bajo el que se coordina (p. ej. la HRN de
    /// la cuenta unica del deployment). Dos workers con el mismo scope
    /// se excluyen por el claim; con scopes distintos no se ven, que
    /// es como se demuestra "entre replicas" sobre una sola autoridad.
    pub scope: String,
    /// Quien es este worker. Un claim ajeno vigente ocupa el scope
    /// (Busy); un claim vencido se take-over con generacion nueva.
    pub owner: String,
    /// Cuanto vale el claim. Se renueva entre marcas.
    pub lease_for: Duration,
    /// La cota de paginas del censo.
    pub page_limit: usize,
}

impl PassConfig {
    /// El constructor minimo: scope, owner y lease; pagina por defecto.
    pub fn new(scope: impl Into<String>, owner: impl Into<String>, lease_for: Duration) -> Self {
        Self {
            scope: scope.into(),
            owner: owner.into(),
            lease_for,
            page_limit: ROOT_PAGE_LIMIT,
        }
    }

    /// Cambia la cota de pagina (las pruebas la bajan para forzar el
    /// recorrido paginado con cientos de raices).
    pub fn with_page_limit(mut self, page_limit: usize) -> Self {
        self.page_limit = page_limit.max(1);
        self
    }
}

/// Que hizo la pasada sobre un candidato.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PassAction {
    /// El blob se borro condicionado a la version marcada.
    Deleted,
    /// Los bytes se movieron a cuarentena versionada (AC2, almacenes
    /// sin delete condicional).
    Quarantined,
    /// Algo nombra el contenido: se descarto sin tocarlo.
    Republished,
    /// La version cambio desde la marca: **no se reutilizo la clave**
    /// hasta reconciliar (AC2).
    NotApplied,
    /// El blob ya no estaba: otra pasada lo actuo antes.
    Gone,
    /// No hay guarda aplicable: `Unknown`, nunca se borra.
    Skipped,
}

/// El resultado de una pasada coordinada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PassReport {
    /// La pasada.
    pub run_id: String,
    /// La generacion del claim bajo la que se barre.
    pub generation: u64,
    /// Si la pasada reanudo una marca pendiente.
    pub resumed: bool,
    /// Cuantos candidatos quedaron marcados.
    pub marked: u64,
    /// Cuantos se borraron (incluidos los confirmados por relectura).
    pub deleted: u64,
    /// Los bytes liberados.
    pub bytes_freed: u64,
    /// Cuantos quedaron en cuarentena versionada.
    pub quarantined: u64,
    /// Cuantos se descartaron porque algo los nombro (revalidacion).
    pub republished: u64,
    /// Cuantos no se tocaron porque su version cambio (AC2).
    pub not_applied: u64,
    /// Cuantos quedaron en `Unknown`: nunca se borraron.
    pub unknown: u64,
    /// Cuantas paginas de raices se leyeron (AC3: la cota visible).
    /// Cero si la pasada reanudo y no necesito censo nuevo.
    pub census_pages: u64,
    /// Accion por digest, para las aserciones por membresia.
    pub actions: HashMap<String, PassAction>,
}

/// Como el almacén permite retirar la version marcada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StoreMode {
    /// Delete condicionado a la version: la carrera la cierra el
    /// almacén. `NotApplied` veda la reutilizacion de clave.
    Conditional,
    /// Solo cuarentena versionada: nombre nuevo, nunca reescritura.
    Quarantine,
    /// Ninguna de las dos: la pasada coordinada no toca nada.
    Refuse,
}

/// El worker de la pasada coordinada.
///
/// Vive sin estado entre llamadas: todo lo que necesita para reanudar
/// esta en el libro. Dos instancias sobre el mismo scope compiten por
/// el claim; la autoridad decide.
pub struct CoordinatedPass {
    service: Arc<GcService>,
    config: PassConfig,
}

impl CoordinatedPass {
    /// Crea el worker sobre el servicio existente.
    pub fn new(service: Arc<GcService>, config: PassConfig) -> Self {
        Self { service, config }
    }

    /// El libro detras de la pasada.
    fn ledger(&self) -> &Arc<dyn ContentLedger> {
        self.service.ledger()
    }

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    /// El claim fenced: `claim_gc_pass` + relectura post-commit
    /// (regla HSP-205: manda lo que la autoridad dice al leer, no el
    /// hecho de que el claim haya commiteado).
    async fn claim(&self) -> Result<GcLease, GcError> {
        if !self.ledger().supports_fenced_pass() {
            return Err(GcError::Unsupported);
        }
        let lease = self
            .ledger()
            .claim_gc_pass(
                &self.config.scope,
                &self.config.owner,
                self.config.lease_for,
                Self::now(),
            )
            .await
            .map_err(Self::busy_or_ledger)?;
        let back = self
            .ledger()
            .current_gc_lease(&self.config.scope, Self::now())
            .await?
            .ok_or_else(|| {
                GcError::Unknown("the claim was not readable after commit".to_string())
            })?;
        if back.token != lease.token || back.generation != lease.generation {
            // Otra replica gano la carrera del claim: nosotros NO
            // somos el holder y no barremos.
            return Err(GcError::Busy);
        }
        Ok(lease)
    }

    /// El error de claim: si el scope esta ocupado o la condicion del
    /// CAS no se cumplio, es Busy; lo demas, del libro.
    fn busy_or_ledger(error: CatalogError) -> GcError {
        match error {
            CatalogError::Conflict { .. } | CatalogError::PreconditionFailed { .. } => {
                GcError::Busy
            }
            other => GcError::Ledger(other),
        }
    }

    /// La marca pendiente bajo el claim vigente, leida de la
    /// autoridad. `None` si no hay pasada que reanudar.
    async fn pending_mark(&self, lease: &GcLease) -> Result<Option<GcRun>, GcError> {
        let Some(run) = self
            .ledger()
            .pending_scoped_run(&self.config.scope, Self::now())
            .await?
        else {
            return Ok(None);
        };
        if run.scope.as_deref() != Some(self.config.scope.as_str()) {
            return Err(GcError::Unknown(format!(
                "the pending run {} is not of this scope",
                run.id
            )));
        }
        // La reconfirmacion fenced: si el rastro ya no existe bajo la
        // generacion vigente (la autoridad lo veto), no se reanuda; se
        // abre marca nueva.
        Ok(self
            .ledger()
            .mark_of_run(&self.config.scope, lease, &run.id)
            .await?)
    }

    /// El censo paginado (AC3): recorre `root_page` con cota
    /// `page_limit` sin materializar el grafo y produce el plan con el
    /// mismo nucleo puro del dry-run.
    ///
    /// `roots_are_complete` solo es cierto cuando el recorrido termino
    /// sin `has_more`: con un censo a medias nada es reclamable — el
    /// veredicto es `Unknown`, que jamas se borra.
    pub async fn paged_plan(&self) -> Result<(Plan, u64), GcError> {
        let store = Arc::clone(self.service.store());
        let blobs = store
            .reclaimer()
            .ok_or(GcError::Unsupported)?
            .list_blobs()
            .await
            .map_err(|error| {
                GcError::Unknown(format!("the store could not be listed: {error:?}"))
            })?;
        let holds =
            self.ledger().holds().await.map_err(|error| {
                GcError::Unknown(format!("the holds could not be read: {error}"))
            })?;

        let mut roots = Vec::new();
        let mut pages = 0u64;
        let mut cursor: Option<String> = None;
        let mut complete;
        loop {
            let page = self
                .ledger()
                .root_page(
                    &self.config.scope,
                    cursor.as_deref(),
                    self.config.page_limit,
                )
                .await?;
            pages += 1;
            let last = page.roots.last().map(|root| root.digest.to_string());
            let has_more = page.has_more;
            roots.extend(page.roots);
            complete = !has_more;
            if complete {
                break;
            }
            cursor = Some(last.ok_or_else(|| {
                GcError::Unknown("a full root page carried no cursor".to_string())
            })?);
        }

        // Las aristas de manifiesto, sobre las raices ya paginadas.
        let mut edges: HashMap<ContentDigest, Vec<ContentDigest>> = HashMap::new();
        let mut seen: HashSet<ContentDigest> = HashSet::new();
        for root in &roots {
            if root.kind != hodei_catalog::RootKind::Asset
                || root.subject.strip_prefix("manifests/") != Some(root.digest.value.as_str())
            {
                continue;
            }
            if !seen.insert(root.digest.clone()) {
                continue;
            }
            let bytes = match store
                .get_with(&root.digest, hodei_hub::ReadPolicy::Verify)
                .await
            {
                Ok(bytes) => bytes,
                Err(error) => {
                    return Err(GcError::Unknown(format!(
                        "the manifest {} could not be read: {error:?}",
                        root.digest
                    )));
                }
            };
            if let Ok(children) = plan::manifest_edges(&bytes) {
                edges.insert(root.digest.clone(), children);
            }
        }

        let plan = plan::plan(&Input {
            blobs: &blobs,
            roots: &roots,
            holds: &holds,
            edges: &edges,
            grace: self.service.grace(),
            now: Self::now(),
            roots_are_complete: complete,
        });
        Ok((plan, pages))
    }

    /// Una pasada completa: claim -> marca durable -> revalidacion ->
    /// destino segun el modo del almacén -> cierre fenced.
    ///
    /// Si el lease se pierde en medio (otra replica tomo el scope),
    /// la pasada para con `GcError::Busy` y ni una marca mas: el
    /// zombie no continua borrando la generacion nueva (AC1).
    pub async fn run(&self) -> Result<PassReport, GcError> {
        let _single = self.service.gate().try_lock().map_err(|_| GcError::Busy)?;
        let mut lease = self.claim().await?;

        // 1. Reanudacion durable o marca nueva fenced.
        let pending = self.pending_mark(&lease).await?;
        let resumed = pending.is_some();
        let (run, census_pages, plan_unknown) = match pending {
            Some(run) => (run, 0u64, 0u64),
            None => {
                let (plan, pages) = self.paged_plan().await?;
                let candidates: Vec<RunCandidate> = plan
                    .reclaimable()
                    .filter_map(|entry| {
                        super::service::parse_digest(&entry.digest).map(|digest| RunCandidate {
                            digest,
                            length: entry.length,
                        })
                    })
                    .collect();
                let draft = GcRun {
                    id: format!("gc-{}", uuid::Uuid::new_v4().simple()),
                    state: RunState::Marked,
                    grace_secs: self.service.grace().as_secs(),
                    actor: self.config.owner.clone(),
                    candidates,
                    marked_at: Self::now(),
                    deleted: 0,
                    bytes_freed: 0,
                    swept_at: None,
                    scope: Some(self.config.scope.clone()),
                    generation: Some(lease.generation),
                };
                // La marca va con guardas en la MISMA transaccion;
                // lo que se barre es lo que la autoridad devolvio
                // leido post-commit, no lo que el plan soñ6.
                let marked = self
                    .ledger()
                    .mark_run_fenced(
                        &self.config.scope,
                        &lease,
                        &draft,
                        &draft
                            .candidates
                            .iter()
                            .map(|candidate| candidate.digest.clone())
                            .collect::<Vec<ContentDigest>>(),
                    )
                    .await
                    .map_err(Self::busy_or_ledger)?;
                (
                    GcRun {
                        candidates: marked,
                        ..draft
                    },
                    pages,
                    plan.summary.unknown,
                )
            }
        };

        // 2. Sweep con revalidacion por candidato.
        let store = Arc::clone(self.service.store());
        let mode = match store.conditional() {
            Some(blob) if blob.supports_conditional_delete() => StoreMode::Conditional,
            Some(_) => StoreMode::Quarantine,
            None => StoreMode::Refuse,
        };

        let mut actions: HashMap<String, PassAction> = HashMap::new();
        let mut deleted = 0u64;
        let mut bytes_freed = 0u64;
        let mut quarantined = 0u64;
        let mut not_applied = 0u64;
        let mut unknown = plan_unknown;
        let mut republished = 0u64;

        for candidate in &run.candidates {
            // El lease se renueva en cada candidato: un worker cuyo
            // claim paso a otra generacion deja de barrer al instante.
            lease = self.renew(&lease).await?;

            let key = candidate.digest.to_string();

            // El rastro durable propio primero: una intencion de este
            // run puede venir de antes del corte y se resuelve SOLO
            // releyendo el estado real.
            if let Some(trace) = self.ledger().deletion_of(&candidate.digest).await?
                && trace.run_id == run.id
            {
                match trace.state {
                    DeletionState::Confirmed => {
                        deleted += 1;
                        bytes_freed += trace.length;
                        actions.insert(key, PassAction::Deleted);
                        continue;
                    }
                    DeletionState::Intended => {
                        let vanished = if mode == StoreMode::Refuse {
                            // Sin capacidad condicional no se escriben
                            // intenciones propias; si llegan de un
                            // almacén reemplazado, se decide por `exists`.
                            !store
                                .exists(&candidate.digest)
                                .await
                                .map_err(|error| GcError::Store(format!("{error:?}")))?
                        } else {
                            self.version_of(&candidate.digest).await?.is_none()
                        };
                        if vanished {
                            // El corte cayo entre el retiro y la
                            // confirmacion: el estado real dice que la
                            // version marcada ya no esta, y eso
                            // confirma (veredicto post-commit).
                            self.confirm(&lease, &run, candidate, trace.marker.as_deref())
                                .await?;
                            deleted += 1;
                            bytes_freed += trace.length;
                            actions.insert(key, PassAction::Deleted);
                            continue;
                        }
                        // Sigue presente: se reintenta el destino,
                        // idempotente bajo la guarda de version.
                    }
                }
            }

            // Revalidacion post-censo: la autoridad fresca. Un
            // publisher concurrente que nombro el blob lo saca aqui
            // sin tocar un byte (AC1: las referencias vivas nunca se
            // borran).
            let live = self
                .ledger()
                .live_reference_count(&candidate.digest)
                .await
                .map_err(|error| {
                    GcError::Unknown(format!("the references could not be read: {error}"))
                })?;
            if live > 0 {
                republished += 1;
                actions.insert(key, PassAction::Republished);
                continue;
            }

            // AC2/fail-closed: sin capacidad de guarda en el almacén,
            // esta pasada NO borra. Queda `Unknown`.
            if mode == StoreMode::Refuse {
                unknown += 1;
                actions.insert(key, PassAction::Skipped);
                continue;
            }

            // La marca de version es lo que hace seguro el retiro:
            // sin version observable no hay guarda, y no se toca.
            let Some(marker) = self.version_of(&candidate.digest).await? else {
                actions.insert(key, PassAction::Gone);
                continue;
            };

            let intended = GcDeletion {
                digest: candidate.digest.clone(),
                length: candidate.length,
                run_id: run.id.clone(),
                actor: self.config.owner.clone(),
                account: self.config.scope.clone(),
                state: DeletionState::Intended,
                at: Self::now(),
                marker: Some(marker.marker.clone()),
            };
            self.ledger()
                .record_deletion_fenced(&self.config.scope, &lease, &intended)
                .await
                .map_err(Self::busy_or_ledger)?;

            let action = self.retire(&lease, &run, candidate, &marker, mode).await?;
            match action {
                PassAction::Deleted => {
                    deleted += 1;
                    bytes_freed += candidate.length;
                }
                PassAction::Quarantined => quarantined += 1,
                PassAction::NotApplied => not_applied += 1,
                PassAction::Republished => republished += 1,
                PassAction::Skipped => unknown += 1,
                PassAction::Gone => {}
            }
            actions.insert(key, action);
        }

        // 3. Cierre fenced bajo la generacion vigente.
        lease = self.renew(&lease).await?;
        self.ledger()
            .complete_scoped_run(
                &self.config.scope,
                &lease,
                &run.id,
                deleted,
                bytes_freed,
                quarantined,
                Self::now(),
            )
            .await
            .map_err(Self::busy_or_ledger)?;
        self.service.invalidate_plan_cache();

        Ok(PassReport {
            run_id: run.id,
            generation: lease.generation,
            resumed,
            marked: run.candidates.len() as u64,
            deleted,
            bytes_freed,
            quarantined,
            republished,
            not_applied,
            unknown,
            census_pages,
            actions,
        })
    }

    /// Renueva el propio claim entre marcas. Un token de generacion
    /// anterior no puede renovar: PreconditionFailed -> Busy -> la
    /// pasada deja de borrar.
    async fn renew(&self, lease: &GcLease) -> Result<GcLease, GcError> {
        self.ledger()
            .renew_gc_lease(
                &self.config.scope,
                lease,
                &self.config.owner,
                self.config.lease_for,
                Self::now(),
            )
            .await
            .map_err(Self::busy_or_ledger)
    }

    /// La marca de version actual del blob, si sigue presente.
    async fn version_of(&self, digest: &ContentDigest) -> Result<Option<VersionMarker>, GcError> {
        self.service
            .store()
            .conditional()
            .ok_or(GcError::Unsupported)?
            .head_marker(digest)
            .await
            .map_err(|error| GcError::Store(format!("{error:?}")))
    }

    /// El destino de un candidato ya marcado, segun el modo del
    /// almacén. La confirmacion del rastro solo va despues de que la
    /// accion se haya APLICADO (veredicto post-efecto).
    async fn retire(
        &self,
        lease: &GcLease,
        run: &GcRun,
        candidate: &RunCandidate,
        marker: &VersionMarker,
        mode: StoreMode,
    ) -> Result<PassAction, GcError> {
        let blob: &dyn ConditionalBlob = self
            .service
            .store()
            .conditional()
            .ok_or(GcError::Unsupported)?;
        match mode {
            StoreMode::Conditional => match blob
                .delete_if_untouched(&candidate.digest, marker)
                .await
                .map_err(|error| GcError::Store(format!("{error:?}")))?
            {
                ConditionalOutcome::Applied { .. } => {
                    self.confirm(lease, run, candidate, Some(&marker.marker))
                        .await?;
                    Ok(PassAction::Deleted)
                }
                ConditionalOutcome::Gone => Ok(PassAction::Gone),
                // AC2: la version marcada ya no es la vigente. No se
                // toca la nueva y su clave queda vedada a la
                // reutilizacion hasta reconciliar: el registro guarda
                // la marca intentada y `NotApplied` se reporta, no se
                // re-borra "a ver si ahora si".
                ConditionalOutcome::NotApplied => Ok(PassAction::NotApplied),
            },
            StoreMode::Quarantine => {
                // Cuarentena versionada: nombre NUEVO, nunca
                // reescritura de la clave original.
                let key = blob.quarantine_key_of(&candidate.digest, &run.id);
                match blob
                    .quarantine_if_untouched(&candidate.digest, marker, &key)
                    .await
                    .map_err(|error| GcError::Store(format!("{error:?}")))?
                {
                    ConditionalOutcome::Applied { length } => {
                        self.ledger()
                            .record_quarantine(
                                &self.config.scope,
                                lease,
                                &Quarantined {
                                    digest: candidate.digest.clone(),
                                    length,
                                    run_id: run.id.clone(),
                                    quarantine_key: key,
                                    marker: marker.marker.clone(),
                                    at: Self::now(),
                                },
                            )
                            .await
                            .map_err(Self::busy_or_ledger)?;
                        self.confirm(lease, run, candidate, Some(&marker.marker))
                            .await?;
                        Ok(PassAction::Quarantined)
                    }
                    ConditionalOutcome::Gone => Ok(PassAction::Gone),
                    ConditionalOutcome::NotApplied => Ok(PassAction::NotApplied),
                }
            }
            StoreMode::Refuse => Ok(PassAction::Skipped),
        }
    }

    /// Confirma el rastro bajo el claim vigente.
    async fn confirm(
        &self,
        lease: &GcLease,
        run: &GcRun,
        candidate: &RunCandidate,
        marker: Option<&str>,
    ) -> Result<(), GcError> {
        let confirmed = GcDeletion {
            digest: candidate.digest.clone(),
            length: candidate.length,
            run_id: run.id.clone(),
            actor: self.config.owner.clone(),
            account: self.config.scope.clone(),
            state: DeletionState::Confirmed,
            at: Self::now(),
            marker: marker.map(str::to_string),
        };
        self.ledger()
            .record_deletion_fenced(&self.config.scope, lease, &confirmed)
            .await
            .map_err(Self::busy_or_ledger)?;
        Ok(())
    }
}
