//! Registry recovery: backup, restore and scrub.
//!
//! The registry has two truths that are written at different times: the
//! catalog (SurrealDB) says **what** exists, and the content store
//! holds **the bytes**. A copy of only one of the two leaves a registry
//! that looks healthy and is not: references to bytes that are missing, or bytes that
//! nobody references. This module exists so that this does not happen silently.
//!
//! - [`manifest`]: the pure core. What a backup says about itself and when it is
//!   restorable.
//! - [`scrub`]: the pure core of the report (corrupt, broken reference,
//!   orphan) and the service that computes it by reusing the census of the
//!   garbage collection.
//! - [`export`]: the fix to the engine's export so that it can be imported.
//! - [`import`]: the idempotent import of the portable semantic bundle
//!   (HSP-210): same canonical key, same order, no UID copied.
//! - [`reconcile`]: the projection rebuild and the semantic reconciliation
//!   of the cutover (HSP-211): origin and destination are equivalent per
//!   account, grants, tags, paths, digests and cursors — never by
//!   isolated counts — and missing/corrupt never escalates to verified.
//!
//! Rules, from most to least important:
//!
//! 1. **Bytes are never invented.** Content only returns to the store from
//!    a backup that contains it and whose hash matches its digest.
//! 2. **Order makes coherence.** Metadata is exported *before* the
//!    blobs: the backup's blobs are a superset of what the
//!    metadata references, and the surplus is orphaned, never a
//!    broken reference.
//! 3. **A half-written backup does not exist.** It is written to a
//!    temporary directory and published with an atomic rename.
//! 4. **Restore requires a clean installation.** Mixing with live data
//!    is exactly what this avoids.
//! 5. **It is an operator operation, with no running service.** The
//!    embedded engine locks the database while the service is alive; neither
//!    hot backup nor high availability is promised.

pub mod cutover;
pub mod export;
pub mod import;
pub mod manifest;
pub mod migrate;
pub mod reconcile;
pub mod repair;
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
pub use migrate::{MigrateError, MigrationReport, Migrator, Moved};
pub use reconcile::{
    ACCOUNT_TABLES, CutoverDecision, CutoverToken, Divergence, GRANT_TABLE, HEAD_TABLE,
    LEDGER_TABLE, PATH_TABLES, ReconcileError, ReconcileSubject, SemanticProjection, TAG_TABLE,
    gate_cutover, package_rows, prepare, prepare_from_sink,
};
pub use repair::{RepairError, RepairOutcome, Repairer, Was};
pub use scrub::{Finding, RepairAction, RootRef, ScrubReport, ScrubSummary, Scrubber};

/// Why a recovery operation was not performed.
#[derive(Debug, thiserror::Error)]
pub enum RecoveryError {
    /// The request is not valid.
    #[error("{0}")]
    Invalid(String),
    /// The destination of a restore is not clean.
    #[error("the destination is not clean: {0}")]
    NotClean(String),
    /// The backup is not intact or is not restorable.
    #[error("the backup is not sound: {0}")]
    Unsound(String),
    /// The metadata could not be exported or imported.
    #[error("the metadata could not be moved: {0}")]
    Metadata(String),
    /// The store or the catalog failed.
    #[error("the store failed: {0}")]
    Store(String),
    /// The resolution bundle could not be exported or imported.
    #[error("the resolution bundle failed: {0}")]
    Bundle(String),
    /// The filesystem failed.
    #[error("the file system failed: {0}")]
    Io(String),
}

impl From<std::io::Error> for RecoveryError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}
