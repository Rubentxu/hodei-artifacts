//! Recuperacion del registro: copia de seguridad, restauracion y scrub.
//!
//! El registro tiene dos verdades que se escriben en momentos distintos: el
//! catalogo (SurrealDB) dice **que** existe, y el almacen de contenido
//! guarda **los bytes**. Una copia de solo una de las dos deja un registro
//! que parece sano y no lo es: referencias a bytes que no estan, o bytes que
//! nadie referencia. Este modulo existe para que eso no pase en silencio.
//!
//! - [`manifest`]: el nucleo puro. Que dice una copia de si misma y cuando es
//!   restaurable.
//! - [`scrub`]: el nucleo puro del informe (corrupto, referencia rota,
//!   huerfano) y el servicio que lo calcula reutilizando el censo de la
//!   recoleccion de basura.
//! - [`export`]: la correccion del export del motor para que se pueda importar.
//! - [`import`]: el import idempotente del paquete semantico portable
//!   (HSP-210): misma clave canonica, mismo orden, ninguna copia de UID.
//! - [`reconcile`]: el rebuild de proyecciones y la reconciliacion
//!   semantica del cutover (HSP-211): origen y destino equivalen por
//!   cuenta, grants, tags, caminos, digests y cursores — nunca por
//!   conteos aislados — y missing/corrupt jamas escala a verificado.
//!
//! Reglas, de mas a menos importantes:
//!
//! 1. **Nunca se inventan bytes.** Un contenido solo vuelve al almacen desde
//!    una copia que lo contiene y cuyo hash coincide con su digest.
//! 2. **El orden hace la coherencia.** Los metadatos se exportan *antes* que
//!    los blobs: los blobs de la copia son un superconjunto de lo que los
//!    metadatos referencian, y lo que sobra es huerfano, nunca una
//!    referencia rota.
//! 3. **Una copia a medias no existe.** Se escribe en un directorio
//!    temporal y se publica con un renombrado atomico.
//! 4. **La restauracion exige una instalacion limpia.** Mezclar con datos
//!    vivos es justo lo que esto evita.
//! 5. **Es una operacion de operador, sin servicio en marcha.** El motor
//!    embebido bloquea la base mientras el servicio vive; no se promete
//!    copia en caliente ni alta disponibilidad.

pub mod cutover;
pub mod export;
pub mod import;
pub mod manifest;
pub mod reconcile;
pub mod scrub;

pub use cutover::{
    Authority, AuthorityError, CutoverError, CutoverMachine, CutoverMetrics, CutoverWindow, Phase,
    drain_rows, export_package,
};
pub use import::{
    BatchOutcome, BlobPolicy, DIGEST_FIELDS, DestinationState, DigestReconciler, ExportFrontier,
    ExportSource, IMPORT_BATCHES_DIR, IMPORT_FORMAT, IMPORT_MANIFEST_FILE, IMPORT_PROGRESS_FILE,
    ImportBatch, ImportFailure, ImportManifest, ImportOptions, ImportReport, ImportTable,
    MemorySink, Row, RowSink, SetReconciler, canonical_digest, canonicalize_id, import_export,
    read_batch, read_import_manifest,
};
pub use manifest::{BackupManifest, BlobEntry, Excluded, FORMAT, MetadataEntry};
pub use reconcile::{
    ACCOUNT_TABLES, CutoverDecision, CutoverToken, Divergence, GRANT_TABLE, HEAD_TABLE,
    LEDGER_TABLE, PATH_TABLES, ReconcileError, ReconcileSubject, SemanticProjection, TAG_TABLE,
    gate_cutover, package_rows, prepare, prepare_from_sink,
};
pub use scrub::{Finding, RepairAction, RootRef, ScrubReport, ScrubSummary, Scrubber};

/// Por que una operacion de recuperacion no se hizo.
#[derive(Debug, thiserror::Error)]
pub enum RecoveryError {
    /// La peticion no es valida.
    #[error("{0}")]
    Invalid(String),
    /// El destino de una restauracion no esta limpio.
    #[error("the destination is not clean: {0}")]
    NotClean(String),
    /// La copia no es integra o no es restaurable.
    #[error("the backup is not sound: {0}")]
    Unsound(String),
    /// Los metadatos no se pudieron exportar o importar.
    #[error("the metadata could not be moved: {0}")]
    Metadata(String),
    /// El almacen o el catalogo fallaron.
    #[error("the store failed: {0}")]
    Store(String),
    /// El paquete de resolucion no pudo exportarse o importarse.
    #[error("the resolution bundle failed: {0}")]
    Bundle(String),
    /// El sistema de ficheros fallo.
    #[error("the file system failed: {0}")]
    Io(String),
}

impl From<std::io::Error> for RecoveryError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}
