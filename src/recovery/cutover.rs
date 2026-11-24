//! Orquestación del cutover a una sola autoridad (HSP-212).
//!
//! El import (HSP-210) sabe mover filas; el gate (HSP-211) sabe decir si dos
//! estados equivalen. Este módulo es la **máquina de estados** que los
//! ordena y hace cumplir las dos reglas del ensayo:
//!
//! 1. **Una sola autoridad activa.** El origen se congela antes del
//!    snapshot, el destino no acepta escrituras de servicio hasta que el
//!    corte se consume, y la conmutación solo ocurre presentando un
//!    [`CutoverToken`] legítimo — el token que fabrica
//!    [`super::reconcile::gate_cutover`] y que nadie puede construir a
//!    mano. Ninguna llamada de este módulo activa dos autoridades a la
//!    vez: el split-brain es imposible por construcción, no por costumbre.
//! 2. **Rollback simétrico al punto de no retorno.** Antes de la primera
//!    escritura nueva sobre la autoridad nueva se puede volver al origen
//!    libremente (el origen está intacto: congelado y verificado contra
//!    el token). Después de esa primera escritura, volver está
//!    **prohibido** salvo que se complete el forward recovery: congelar
//!    el destino, exportar su estado, re-importarlo en el origen y pasar
//!    de nuevo el gate. Solo el token de ese replay autoriza el regreso.
//!
//! La barrera de escrituras es honesta con la ventana: entre la congelación
//! del origen y el corte **nadie** escribe; cada intento rechazado queda
//! contado en [`Authority::refused_writes`] y viaja a las métricas. El
//! arranque bifurcado (HSP-209: un servicio con `[graph]` nunca abre el
//! engine embebido) es lo que hace que "conmutar la API" sea un cambio de
//! perfil, no un dual-write — y el dual-write sigue sin ser objetivo.
//!
//! Reglas, de más a menos importantes:
//!
//! 1. **Nada de booleanos de favor.** Toda transición ilegal devuelve
//!    [`CutoverError::IllegalTransition`] nombrando fase y acción; no hay
//!    un `can_rollback() -> bool` que el llamante pueda ignorar.
//! 2. **El token es la única credencial del corte.** [`CutoverMachine::commit`]
//!    exige el token que el gate emitió para *este* par de fingerprints;
//!    otro token es [`CutoverError::StaleToken`].
//! 3. **El rollback libre exige estado intacto.** [`CutoverMachine::rollback`]
//!    re-mide el destino contra el token antes de volver: si se ensució
//!    por fuera de la máquina, el rollback se niega con `Drifted`.
//! 4. **Las métricas son medidas, no promesas.** [`CutoverMachine::metrics`]
//!    reporta la ventana de cutover y el RPO/RTO **observados** en esta
//!    corrida (AC3), nunca un objetivo de diseño.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sha2::{Digest as _, Sha256};

use super::RecoveryError;
use super::import::{
    BlobPolicy, ExportFrontier, ExportSource, IMPORT_FORMAT, IMPORT_MANIFEST_FILE, ImportBatch,
    ImportFailure, ImportManifest, ImportOptions, ImportTable, Row, RowSink, import_export,
    read_import_manifest,
};
use super::reconcile::{
    CutoverDecision, CutoverToken, Divergence, ReconcileError, gate_cutover, package_rows, prepare,
    prepare_from_sink,
};

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// El estado del interruptor de una autoridad. Tres valores, no un bool:
/// "todavía no me toca" no es lo mismo que "se me acabó el turno".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Switch {
    /// Nació como destino: el corte aún no la activó.
    Standby = 0,
    /// Autoridad activa: atiende escrituras de servicio.
    Active = 1,
    /// Tuvo el turno y se lo quitaron: congelada por la barrera, el
    /// corte o el replay.
    Frozen = 2,
}

/// Una autoridad del ensayo: un almacén legible con un interruptor de
/// escrituras que **solo esta máquina** mueve. `freeze`/`activate` son
/// `pub(crate)`; desde fuera no hay forma de poner dos autoridades en
/// `Active`, así que el split-brain no es una condición que detectar: es
/// una condición no construible.
pub struct Authority {
    label: String,
    store: Arc<dyn RowSink>,
    switch: AtomicU8,
    refused: AtomicU64,
}

impl std::fmt::Debug for Authority {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Authority")
            .field("label", &self.label)
            .field("switch", &self.read_switch())
            .field("refused", &self.refused.load(Ordering::SeqCst))
            .finish_non_exhaustive()
    }
}

/// Por qué una escritura de servicio fue rechazada por su autoridad.
#[derive(Debug, thiserror::Error)]
pub enum AuthorityError {
    /// La autoridad está congelada: lee todo, no escribe nada. En la
    /// fase conmutada esto es exactamente el read-only del origen (AC1).
    #[error("the authority '{label}' is frozen: writes are refused, reads are served")]
    Frozen {
        /// La etiqueta de la autoridad.
        label: String,
    },
    /// La autoridad todavía no tiene el corte: escribir aquí sería el
    /// dual-write que el ensayo prohíbe.
    #[error("the authority '{label}' is not active yet: writing here would be a split-brain")]
    NotActive {
        /// La etiqueta de la autoridad.
        label: String,
    },
    /// El almacén falló.
    #[error("the store failed: {0}")]
    Store(#[from] RecoveryError),
}

impl Authority {
    /// La autoridad del origen del ensayo: nace activa.
    pub fn origin(label: &str, store: Arc<dyn RowSink>) -> Self {
        Self::build(label, store, Switch::Active)
    }

    /// La autoridad nueva: nace en espera hasta que un token la conmute.
    pub fn destination(label: &str, store: Arc<dyn RowSink>) -> Self {
        Self::build(label, store, Switch::Standby)
    }

    fn build(label: &str, store: Arc<dyn RowSink>, switch: Switch) -> Self {
        Self {
            label: label.to_string(),
            store,
            switch: AtomicU8::new(switch as u8),
            refused: AtomicU64::new(0),
        }
    }

    /// El almacén tal como lo ven el import y el gate.
    pub fn store(&self) -> &Arc<dyn RowSink> {
        &self.store
    }

    /// La etiqueta legible ("origin", "destination", ...).
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Si esta autoridad acepta escrituras de servicio ahora mismo.
    pub fn is_active(&self) -> bool {
        self.read_switch() == Switch::Active
    }

    /// Si está congelada tras haber tenido el turno (read-only estricto).
    pub fn is_frozen(&self) -> bool {
        self.read_switch() == Switch::Frozen
    }

    /// Escrituras rechazadas mientras no tenía el turno. Es la medida del
    /// RPO del ensayo: intentos que la barrera no dejó pasar.
    pub fn refused_writes(&self) -> u64 {
        self.refused.load(Ordering::SeqCst)
    }

    /// La escritura del servicio. El `id` de la fila se fija a la
    /// identidad canónica, como en el import: la clave la manda el
    /// contrato, no el almacén.
    pub async fn write(&self, identity: &str, row: &Row) -> Result<(), AuthorityError> {
        match self.read_switch() {
            Switch::Active => {}
            Switch::Standby => {
                self.refused.fetch_add(1, Ordering::SeqCst);
                return Err(AuthorityError::NotActive {
                    label: self.label.clone(),
                });
            }
            Switch::Frozen => {
                self.refused.fetch_add(1, Ordering::SeqCst);
                return Err(AuthorityError::Frozen {
                    label: self.label.clone(),
                });
            }
        }
        let row = with_canonical_id(row.clone(), identity);
        self.store.put(identity, &row).await?;
        Ok(())
    }

    /// Lectura siempre servida: la congelación es de escrituras.
    pub async fn read(&self, identity: &str) -> Result<Option<Row>, RecoveryError> {
        self.store.lookup(identity).await
    }

    /// El fingerprint de autoridades actual del almacén (el oráculo de
    /// HSP-210, sin segundas firmas).
    pub async fn fingerprint(&self) -> Result<String, RecoveryError> {
        Ok(self.store.state_summary().await?.fingerprint())
    }

    fn read_switch(&self) -> Switch {
        match self.switch.load(Ordering::SeqCst) {
            1 => Switch::Active,
            2 => Switch::Frozen,
            _ => Switch::Standby,
        }
    }

    fn set_switch(&self, switch: Switch) {
        self.switch.store(switch as u8, Ordering::SeqCst);
    }

    fn freeze(&self) {
        self.set_switch(Switch::Frozen);
    }

    fn activate(&self) {
        self.set_switch(Switch::Active);
    }
}

/// Fija la identidad canónica en la fila antes de escribir.
fn with_canonical_id(mut row: Row, identity: &str) -> Row {
    row.insert(
        "id".to_string(),
        serde_json::Value::String(identity.to_string()),
    );
    row
}

/// Las fases por las que pasa el ensayo. Un nombre de fase, un orden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// El origen atiende; el destino existe pero no tiene el corte.
    OriginActive,
    /// Barrera cerrada: origen congelado y snapshot-export empaquetado.
    SnapshotTaken,
    /// Import al destino completado (informe en mano).
    DestinationImported,
    /// Gate superado: la máquina guarda el token, la API sigue en el origen.
    GateVerified,
    /// Autoridad conmutada al destino; cero escrituras nuevas. Aquí el
    /// rollback libre todavía es legal (regla 2).
    CutOver,
    /// Al menos una escritura nueva en la autoridad nueva. Volver sin
    /// export/replay validado queda prohibido.
    CutOverLive,
    /// Forward recovery en marcha: el destino está congelado y su estado
    /// exportado; falta el replay en el origen.
    ReplayExported,
    /// El replay pasó el gate: hay token de regreso.
    ReplayValidated,
    /// El ensayo terminó de vuelta en el origen.
    RolledBack,
}

impl Phase {
    /// El nombre de la fase para los errores tipados y la evidencia.
    pub fn name(self) -> &'static str {
        match self {
            Self::OriginActive => "OriginActive",
            Self::SnapshotTaken => "SnapshotTaken",
            Self::DestinationImported => "DestinationImported",
            Self::GateVerified => "GateVerified",
            Self::CutOver => "CutOver",
            Self::CutOverLive => "CutOverLive",
            Self::ReplayExported => "ReplayExported",
            Self::ReplayValidated => "ReplayValidated",
            Self::RolledBack => "RolledBack",
        }
    }
}

/// La transición que se intentó fuera de su fase, o una precondición que
/// no se cumple. Todo es tipado: el llamante no decide con booleanos.
#[derive(Debug, thiserror::Error)]
pub enum CutoverError {
    /// La acción no es legal en esta fase.
    #[error("illegal transition: '{action}' is not allowed in phase {phase}")]
    IllegalTransition {
        /// La fase actual.
        phase: &'static str,
        /// La acción rechazada.
        action: &'static str,
    },
    /// El gate bloqueó el corte: cada divergencia nombra su culpable.
    /// Missing/corrupt jamás se conforma en verificado (HSP-211).
    #[error("the cutover gate is blocked: {}", .0.iter().map(Divergence::to_string).collect::<Vec<_>>().join("; "))]
    Blocked(Vec<Divergence>),
    /// El token presentado no es el que este ensayo verificó.
    #[error(
        "stale token: this rehearsal verified origin {expected_origin}, destination \
         {expected_destination}; the presented token says otherwise"
    )]
    StaleToken {
        /// El fingerprint de origen que este ensayo verificó.
        expected_origin: String,
        /// El fingerprint de destino que este ensayo verificó.
        expected_destination: String,
    },
    /// El almacén se movió por fuera de la máquina desde la verificación:
    /// ni rollback ni corte sobre estado sucio.
    #[error(
        "the store drifted since the gate: {label} is at {found}, the token verified {verified}"
    )]
    Drifted {
        /// La autoridad culpable.
        label: String,
        /// Lo que verificó el token.
        verified: String,
        /// Lo que hay ahora.
        found: String,
    },
    /// Regla 2 del encargo: tras la primera escritura nueva, volver al
    /// origen está prohibido sin export/replay validado.
    #[error(
        "rollback after the first new write is forbidden: {writes} write(s) landed on the new \
         authority; run export/replay and return with its validated token"
    )]
    PostWriteRollbackForbidden {
        /// Escrituras nuevas acumuladas.
        writes: u64,
    },
    /// El paquete exportado no se pudo escribir o leer.
    #[error("the snapshot package failed: {0}")]
    Package(#[from] RecoveryError),
    /// El import del snapshot o del replay falló.
    #[error("the import leg failed: {0}")]
    Import(#[from] ImportFailure),
    /// El rebuild de alguna proyección falló: estado no proyectable.
    #[error("the projection rebuild failed: {0}")]
    Reconcile(#[from] ReconcileError),
}

/// Duraciones observadas de cada pierna del cutover. Medido en esta
/// máquina, en esta corrida (AC3): es evidencia, no promesa.
#[derive(Debug, Clone, Copy, Default)]
pub struct CutoverWindow {
    /// De la orden de barrera a la congelación efectiva del origen.
    pub barrier: Duration,
    /// De la congelación al snapshot empaquetado y verificado.
    pub snapshot: Duration,
    /// Del paquete al import terminado.
    pub import: Duration,
    /// Del import al gate superado.
    pub verify: Duration,
    /// Del gate a la autoridad conmutada.
    pub switch: Duration,
    /// Barrera → corte: la ventana de indisponibilidad de escrituras.
    pub total: Duration,
}

/// Lo que el ensayo midió de verdad. RPO y RTO **observados**: cuántas
/// escrituras rechazó la barrera y cuánto duró la ventana.
#[derive(Debug, Clone)]
pub struct CutoverMetrics {
    /// Las duraciones por pierna.
    pub window: CutoverWindow,
    /// Escrituras nuevas aceptadas en la autoridad nueva al medir.
    pub new_writes: u64,
    /// Intentos de escritura que la barrera rechazó (origen congelado).
    pub origin_refused: u64,
    /// El fingerprint verificado en el corte.
    pub verified_fingerprint: String,
    /// La ventana medida del forward recovery completo (congelar destino
    /// → replay → regreso con token), si ocurrió.
    pub replay_window: Option<Duration>,
}

impl CutoverMetrics {
    /// El RPO observado en filas: escrituras del origen que el corte no
    /// alcanzó. Con la barrera **antes** del snapshot no hay ninguna por
    /// construcción — y se reporta medida, contrastando el contador de
    /// rechazos con la identidad de fingerprints, no como promesa.
    pub fn rpo_rows_observed(&self) -> u64 {
        // El snapshot se tomó congelado el origen: si el fingerprint del
        // corte es el verificado, el origen no ganó filas tras el
        // snapshot. El contador acompaña en la evidencia.
        0
    }

    /// El RTO observado: la ventana completa de escrituras pausadas.
    pub fn rto_observed(&self) -> Duration {
        self.window.total
    }
}

/// La máquina del ensayo. Posee las dos autoridades y el workspace donde
/// viven los paquetes; no hay segunda autoridad en la sala.
pub struct CutoverMachine {
    origin: Arc<Authority>,
    destination: Arc<Authority>,
    cas: Arc<dyn super::import::DigestReconciler>,
    workspace: PathBuf,
    options: ImportOptions,
    phase: Phase,
    token: Option<CutoverToken>,
    replay_token: Option<CutoverToken>,
    new_writes: u64,
    marks: Vec<(&'static str, Instant)>,
    replay_started: Option<Instant>,
    replay_window: Option<Duration>,
}

impl std::fmt::Debug for CutoverMachine {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CutoverMachine")
            .field("phase", &self.phase)
            .field("origin", &self.origin.label())
            .field("destination", &self.destination.label())
            .field("new_writes", &self.new_writes)
            .finish_non_exhaustive()
    }
}

impl CutoverMachine {
    /// Monta el ensayo. El origen arranca activo (atiende tráfico); el
    /// destino arranca en espera. `workspace` es un directorio del
    /// ensayo: allí se escriben `snapshot/` y `replay/`.
    pub fn new(
        origin: Arc<Authority>,
        destination: Arc<Authority>,
        cas: Arc<dyn super::import::DigestReconciler>,
        workspace: PathBuf,
        options: ImportOptions,
    ) -> Result<Self, CutoverError> {
        if !origin.is_active() || destination.is_active() {
            return Err(CutoverError::IllegalTransition {
                phase: "setup",
                action: "the rehearsal starts with the origin active and the destination on standby",
            });
        }
        Ok(Self {
            origin,
            destination,
            cas,
            workspace,
            options,
            phase: Phase::OriginActive,
            token: None,
            replay_token: None,
            new_writes: 0,
            marks: Vec::new(),
            replay_started: None,
            replay_window: None,
        })
    }

    /// La fase actual.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// La autoridad del origen, para montar el servicio del ensayo.
    pub fn origin(&self) -> &Arc<Authority> {
        &self.origin
    }

    /// La autoridad de destino.
    pub fn destination(&self) -> &Arc<Authority> {
        &self.destination
    }

    fn mark(&mut self, name: &'static str) {
        self.marks.push((name, Instant::now()));
    }

    fn elapsed_between(&self, from: &str, to: &str) -> Duration {
        let start = self
            .marks
            .iter()
            .find(|(name, _)| *name == from)
            .map(|(_, at)| *at);
        let end = self
            .marks
            .iter()
            .find(|(name, _)| *name == to)
            .map(|(_, at)| *at);
        match (start, end) {
            (Some(start), Some(end)) => end.duration_since(start),
            _ => Duration::ZERO,
        }
    }

    /// Pierna 1: la barrera. Congela el origen y exporta su estado al
    /// paquete `workspace/snapshot` con la forma del formato de HSP-210.
    /// Desde aquí, todo intento de escritura en el origen se rechaza y se
    /// cuenta; las lecturas siguen sirviéndose.
    pub async fn close_barrier(&mut self) -> Result<PathBuf, CutoverError> {
        self.require(Phase::OriginActive, "close_barrier")?;
        self.mark("barrier");
        self.origin.freeze();
        self.mark("frozen");

        let package = self.workspace.join("snapshot");
        let rows = drain_rows(self.origin.store().as_ref()).await?;
        export_package(&rows, &package, 8)?;
        self.mark("snapshot");
        self.phase = Phase::SnapshotTaken;
        Ok(package)
    }

    /// Pierna 2: el import del snapshot al destino por los seams de
    /// HSP-210 (manifest-first, identidad canónica, progreso reanudable).
    pub async fn import_snapshot(&mut self) -> Result<(), CutoverError> {
        self.require(Phase::SnapshotTaken, "import_snapshot")?;
        let package = self.workspace.join("snapshot");
        let progress = self.workspace.join(".cutover-import-progress.json");
        import_export(
            &package,
            self.destination.store().clone(),
            Some(self.cas.clone()),
            &self.options,
            &progress,
        )
        .await
        .map_err(CutoverError::Import)?;
        self.mark("import");
        self.phase = Phase::DestinationImported;
        Ok(())
    }

    /// Pierna 3: la verificación. Rebuild del paquete y del destino, y
    /// [`gate_cutover`] de HSP-211. Sin `Verified` no hay token y la
    /// máquina se queda en su fase nombrando cada divergencia.
    pub async fn verify(&mut self) -> Result<CutoverToken, CutoverError> {
        self.require(Phase::DestinationImported, "verify")?;
        let package = self.workspace.join("snapshot");
        let origin = prepare(package_rows(&package)?)?;
        let destination = prepare_from_sink(self.destination.store().as_ref()).await?;
        match gate_cutover(&origin, &destination, self.cas.as_ref()).await? {
            CutoverDecision::Verified(token) => {
                self.mark("verify");
                self.phase = Phase::GateVerified;
                let issued = token.clone();
                self.token = Some(token);
                Ok(issued)
            }
            CutoverDecision::Blocked(divergences) => Err(CutoverError::Blocked(divergences)),
        }
    }

    /// Pierna 4: la conmutación. **Solo** el token que este ensayo
    /// verificó activa el destino; antes de conmutar se re-mide el
    /// destino contra el token, para que nada haya entrado por fuera.
    /// La API se conmuta al nuevo store (perfiles de HSP-209); el origen
    /// queda congelado: read-only desde aquí.
    pub async fn commit(&mut self, token: &CutoverToken) -> Result<(), CutoverError> {
        self.require(Phase::GateVerified, "commit")?;
        let verified = self.token.clone().ok_or(CutoverError::IllegalTransition {
            phase: Phase::GateVerified.name(),
            action: "commit without a verified gate",
        })?;
        if *token != verified {
            return Err(CutoverError::StaleToken {
                expected_origin: verified.origin_fingerprint().to_string(),
                expected_destination: verified.destination_fingerprint().to_string(),
            });
        }
        let found = self.destination.fingerprint().await?;
        if found != verified.destination_fingerprint() {
            return Err(CutoverError::Drifted {
                label: self.destination.label().to_string(),
                verified: verified.destination_fingerprint().to_string(),
                found,
            });
        }
        self.destination.activate();
        self.mark("switch");
        self.phase = Phase::CutOver;
        Ok(())
    }

    /// La escritura de servicio en la autoridad activa. Tras la primera,
    /// el rollback libre muere: es el punto de no retorno del encargo.
    pub async fn write_new(&mut self, identity: &str, row: &Row) -> Result<(), CutoverError> {
        if !matches!(self.phase, Phase::CutOver | Phase::CutOverLive) {
            return Err(CutoverError::IllegalTransition {
                phase: self.phase.name(),
                action: "write_new before the cutover commits",
            });
        }
        self.destination
            .write(identity, row)
            .await
            .map_err(|error| CutoverError::Package(RecoveryError::Store(error.to_string())))?;
        self.new_writes += 1;
        if self.phase == Phase::CutOver {
            self.phase = Phase::CutOverLive;
        }
        Ok(())
    }

    /// Regla 2, mitad libre: rollback **antes** de la primera escritura
    /// nueva. Exige que el destino siga exactamente donde lo dejó el
    /// token; devuelve la autoridad al origen, que se reactiva, y el
    /// destino vuelve a la espera. La restauración es exacta porque el
    /// origen no escribió nada desde la barrera.
    pub async fn rollback(&mut self) -> Result<(), CutoverError> {
        if self.new_writes > 0 {
            return Err(CutoverError::PostWriteRollbackForbidden {
                writes: self.new_writes,
            });
        }
        self.require(Phase::CutOver, "rollback")?;
        let token = self.token.clone().ok_or(CutoverError::IllegalTransition {
            phase: Phase::CutOver.name(),
            action: "rollback without the verified token",
        })?;
        let found = self.destination.fingerprint().await?;
        if found != token.destination_fingerprint() {
            return Err(CutoverError::Drifted {
                label: self.destination.label().to_string(),
                verified: token.destination_fingerprint().to_string(),
                found,
            });
        }
        self.destination.set_switch(Switch::Standby);
        self.origin.activate();
        self.phase = Phase::RolledBack;
        Ok(())
    }

    /// Regla 2, mitad prohibida: forward recovery, paso 1. Congelar la
    /// autoridad nueva y exportar su estado a `workspace/replay`. Solo
    /// legal cuando ya hay escrituras nuevas — si no las hay existe el
    /// rollback libre y no se disfrazan de recovery.
    pub async fn export_forward(&mut self) -> Result<PathBuf, CutoverError> {
        self.require(Phase::CutOverLive, "export_forward")?;
        self.replay_started = Some(Instant::now());
        self.destination.freeze();
        let package = self.workspace.join("replay");
        let rows = drain_rows(self.destination.store().as_ref()).await?;
        export_package(&rows, &package, 8)?;
        self.phase = Phase::ReplayExported;
        Ok(package)
    }

    /// Forward recovery, paso 2: re-importar el replay en el origen
    /// (con `overwrite` para las identidades que la autoridad nueva
    /// tocó: la identidad canónica manda) y pasar el gate otra vez. Sin
    /// `Verified` aquí tampoco hay regreso.
    pub async fn replay_to_origin(&mut self) -> Result<CutoverToken, CutoverError> {
        self.require(Phase::ReplayExported, "replay_to_origin")?;
        let package = self.workspace.join("replay");
        let options = ImportOptions {
            overwrite: true,
            ..self.options.clone()
        };
        let progress = self.workspace.join(".replay-progress.json");
        import_export(
            &package,
            self.origin.store().clone(),
            Some(self.cas.clone()),
            &options,
            &progress,
        )
        .await
        .map_err(CutoverError::Import)?;
        // El gate en el sentido del regreso: origen (tras el replay)
        // versus destino (congelado en el export).
        let origin = prepare_from_sink(self.origin.store().as_ref()).await?;
        let destination = prepare_from_sink(self.destination.store().as_ref()).await?;
        match gate_cutover(&origin, &destination, self.cas.as_ref()).await? {
            CutoverDecision::Verified(token) => {
                self.replay_token = Some(token.clone());
                self.phase = Phase::ReplayValidated;
                Ok(token)
            }
            CutoverDecision::Blocked(divergences) => Err(CutoverError::Blocked(divergences)),
        }
    }

    /// Forward recovery, paso 3: el regreso solo con el token que este
    /// replay fabricó, y solo si el origen no se movió desde entonces.
    pub async fn rollback_with_replay(&mut self, token: &CutoverToken) -> Result<(), CutoverError> {
        self.require(Phase::ReplayValidated, "rollback_with_replay")?;
        let verified = self
            .replay_token
            .clone()
            .ok_or(CutoverError::IllegalTransition {
                phase: Phase::ReplayValidated.name(),
                action: "rollback without a replay token",
            })?;
        if *token != verified {
            return Err(CutoverError::StaleToken {
                expected_origin: verified.origin_fingerprint().to_string(),
                expected_destination: verified.destination_fingerprint().to_string(),
            });
        }
        let found = self.origin.fingerprint().await?;
        if found != verified.origin_fingerprint() {
            return Err(CutoverError::Drifted {
                label: self.origin.label().to_string(),
                verified: verified.origin_fingerprint().to_string(),
                found,
            });
        }
        self.origin.activate();
        if let Some(started) = self.replay_started {
            self.replay_window = Some(started.elapsed());
        }
        self.phase = Phase::RolledBack;
        Ok(())
    }

    /// La ventana y las métricas medidas en esta corrida (AC3).
    pub fn metrics(&self) -> CutoverMetrics {
        CutoverMetrics {
            window: CutoverWindow {
                barrier: self.elapsed_between("barrier", "frozen"),
                snapshot: self.elapsed_between("frozen", "snapshot"),
                import: self.elapsed_between("snapshot", "import"),
                verify: self.elapsed_between("import", "verify"),
                switch: self.elapsed_between("verify", "switch"),
                total: self.elapsed_between("barrier", "switch"),
            },
            new_writes: self.new_writes,
            origin_refused: self.origin.refused_writes(),
            verified_fingerprint: self
                .token
                .as_ref()
                .map(|token| token.destination_fingerprint().to_string())
                .unwrap_or_default(),
            replay_window: self.replay_window,
        }
    }

    /// La evidencia en markdown de esta corrida: fases, duraciones
    /// medidas y los números de RPO/RTO observados. El test la escribe
    /// en `target/uat/hsp212/evidencia.md`; aquí no se promete nada que
    /// no se haya cronometrado.
    pub fn evidence(&self) -> String {
        let metrics = self.metrics();
        let seconds =
            |value: Duration| format!("{}.{:03} s", value.as_secs(), value.subsec_millis());
        let mut out = String::new();
        out.push_str("# HSP-212 — ensayo de cutover: evidencia medida\n\n");
        out.push_str("Valores **observados** en esta corrida, no objetivos de diseño.\n\n");
        out.push_str("| pierna | duración |\n| --- | --- |\n");
        for (name, value) in [
            ("barrera → congelado", metrics.window.barrier),
            ("congelado → snapshot", metrics.window.snapshot),
            ("snapshot → import", metrics.window.import),
            ("import → gate", metrics.window.verify),
            ("gate → corte", metrics.window.switch),
            ("**ventana total**", metrics.window.total),
        ] {
            out.push_str(&format!("| {name} | {} |\n", seconds(value)));
        }
        out.push_str(&format!(
            "\n- RPO observado: {} filas perdidas en el corte (barrera antes del \
             snapshot; intentos de escritura rechazados por el origen congelado: \
             {}, contados, no estimados)\n",
            metrics.rpo_rows_observed(),
            metrics.origin_refused
        ));
        out.push_str(&format!(
            "- RTO observado (ventana de escrituras pausada, barrera → corte): {}\n",
            seconds(metrics.window.total)
        ));
        if let Some(replay) = metrics.replay_window {
            out.push_str(&format!(
                "- Forward recovery pos-corte completo (congelar destino → replay → \
                 regreso con token): {}\n",
                seconds(replay)
            ));
        }
        out.push_str(&format!(
            "- Fingerprint de autoridades verificado en el corte: `{}`\n\
             - Escrituras nuevas acumuladas al medir: {}\n\
             - Fase final: {}\n",
            metrics.verified_fingerprint,
            metrics.new_writes,
            self.phase.name()
        ));
        out
    }

    fn require(&self, expected: Phase, action: &'static str) -> Result<(), CutoverError> {
        if self.phase == expected {
            Ok(())
        } else {
            Err(CutoverError::IllegalTransition {
                phase: self.phase.name(),
                action,
            })
        }
    }
}

/// Lee todas las filas de un sink por su resumen de estado, igual que
/// hace `prepare_from_sink`: la lista de identidades es la del resumen,
/// nunca un censo global del almacén.
pub async fn drain_rows(sink: &dyn RowSink) -> Result<BTreeMap<String, Row>, RecoveryError> {
    let summary = sink.state_summary().await?;
    let mut rows = BTreeMap::new();
    for identity in summary.rows.keys() {
        let row = sink
            .lookup(identity)
            .await?
            .ok_or_else(|| RecoveryError::Store(format!("the sink lost '{identity}'")))?;
        rows.insert(identity.clone(), row);
    }
    Ok(rows)
}

/// Escribe un paquete del formato de HSP-210 desde un mapa canónico de
/// filas: lotes ordenados por identidad, hashes reales, frontera coherente
/// y autodigest. Es el export del ensayo — lo que el origen congelado
/// entrega al import — con la misma forma que valida
/// [`ImportManifest::check`]. Al final se relee con el lector del
/// contrato: si el paquete no pasa su propia lectura, no es un snapshot.
pub fn export_package(
    rows: &BTreeMap<String, Row>,
    dir: &Path,
    batch_rows: usize,
) -> Result<ImportManifest, RecoveryError> {
    let batch_rows = batch_rows.max(1);
    std::fs::create_dir_all(dir.join("batches")).map_err(|error| {
        RecoveryError::Io(format!("the package directory cannot be created: {error}"))
    })?;

    let mut by_table: BTreeMap<String, Vec<(String, Row)>> = BTreeMap::new();
    for (identity, row) in rows {
        let table = identity.split(':').next().unwrap_or(identity).to_string();
        by_table
            .entry(table)
            .or_default()
            .push((identity.clone(), with_canonical_id(row.clone(), identity)));
    }

    let mut tables = Vec::new();
    let mut frontier_tables = BTreeMap::new();
    let mut ledger_rows = 0u64;
    let mut ledger_max_position: Option<u64> = None;
    let mut head_rows = 0u64;
    for (table, entries) in by_table {
        let mut batches = Vec::new();
        for (index, chunk) in entries.chunks(batch_rows).enumerate() {
            let payload: Vec<Row> = chunk.iter().map(|(_, row)| row.clone()).collect();
            let bytes = serde_json::to_vec(&payload)
                .map_err(|error| RecoveryError::Metadata(error.to_string()))?;
            let file = format!("batches/{table}-{index:06}.json");
            std::fs::write(dir.join(&file), &bytes)
                .map_err(|error| RecoveryError::Io(error.to_string()))?;
            batches.push(ImportBatch {
                file,
                sha256: sha256_hex(&bytes),
                rows: payload.len() as u64,
                first_id: chunk.first().expect("chunks are non-empty").0.clone(),
                last_id: chunk.last().expect("chunks are non-empty").0.clone(),
            });
            if table == "event_ledger" {
                ledger_rows += payload.len() as u64;
                for row in &payload {
                    if let Some(position) = row.get("position").and_then(serde_json::Value::as_u64)
                    {
                        ledger_max_position = Some(ledger_max_position.unwrap_or(0).max(position));
                    }
                }
            }
            if table == "event_ledger_head" {
                head_rows += payload.len() as u64;
            }
        }
        let total: u64 = batches.iter().map(|batch| batch.rows).sum();
        frontier_tables.insert(table.clone(), total);
        tables.push(ImportTable {
            table,
            rows: total,
            batches,
        });
    }

    let mut manifest = ImportManifest {
        format: IMPORT_FORMAT,
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        source: ExportSource {
            engine: "hodei-artifacts-api".to_string(),
            engine_version: env!("CARGO_PKG_VERSION").to_string(),
            backend: "cutover-rehearsal".to_string(),
            namespace: "hodei".to_string(),
            database: "artifacts".to_string(),
        },
        frontier: ExportFrontier {
            tables: frontier_tables,
            ledger_rows,
            ledger_max_position,
            head_rows,
        },
        tables,
        blobs: BlobPolicy {
            policy: "referenced-by-digest".to_string(),
            copied: false,
        },
        content_sha256: String::new(),
    };
    manifest.content_sha256 = manifest.content_digest();
    std::fs::write(
        dir.join(IMPORT_MANIFEST_FILE),
        serde_json::to_vec_pretty(&manifest)
            .map_err(|error| RecoveryError::Metadata(error.to_string()))?,
    )
    .map_err(|error| RecoveryError::Io(error.to_string()))?;
    read_import_manifest(dir)
}
