//! The one-shot portable semantic export of the registry.
//!
//! The registry has one authority: the persistent store (SurrealDB 2.7 over
//! RocksDB). The future graph authority (Dgraph) needs the *semantics* of
//! that authority -- accounts, repositories, assets, versions, references,
//! tags, grants, provenance receipts, the audit outbox and the evidence
//! ledger -- not its files. Copying RocksDB files is forbidden: they are
//! engine-private, and the target engine cannot read them.
//!
//! This crate is the export side of the migration contract (doc-20): it
//! reads the source in bounded batches, writes one JSON batch file per
//! batch, and closes with a deterministic manifest whose digests make the
//! whole export verifiable without replaying it.
//!
//! The rules, in order of importance:
//!
//! 1. **The export is semantic.** Rows travel as canonical JSON keyed by
//!    their canonical identities (account, repository, path, digest, HRN
//!    fields); blobs never travel: they are referenced by digest, because
//!    the CAS already holds them and the target must reconcile against it.
//! 2. **The manifest is deterministic.** Nothing wall-clock enters it: the
//!    same source content always produces byte-identical manifests, which
//!    is what makes "same input, same SHA256" a testable contract.
//! 3. **The work is bounded.** Rows are read by keyset pagination in
//!    batches of a bounded size; no table is ever materialized in memory
//!    (the HSP-174 lesson). The same bound scopes the duplicate check:
//!    a repeated canonical identity inside one batch stops the export;
//!    a pair spread across two batches is stopped by the import, when
//!    it applies the unique indexes of the schema.
//! 4. **A silent half-export does not exist.** Every batch is hashed as it
//!    is written; the progress marker records what is done; a resume
//!    re-verifies each recorded batch against its hash before continuing.
//! 5. **The source is presumed still.** Quiesce is guaranteed by the
//!    engine's exclusive lock (the service cannot run while the tool holds
//!    the database), and the frontier (row counts and ledger head) captured
//!    before the export is re-captured after it: any movement fails the
//!    export with a typed error instead of producing a torn snapshot.
//!
//! Module map:
//!
//! - [`manifest`]: the manifest, the progress marker and their soundness
//!   checks -- the pure core.
//! - [`error`]: the typed failures, each naming the row, the batch or the
//!   table that caused it.
//! - [`export`]: the paginated export service, its frontier capture and
//!   its resume flow.
//!
//! The operator entry point is the [`bin/hodei-export`] binary.

pub mod error;
pub mod export;
pub mod manifest;

pub use error::ExportError;
pub use export::{
    DEFAULT_BATCH_ROWS, ExportConfig, ExportRun, capture_frontier, export, export_with_frontier,
};
pub use manifest::{
    BATCHES_DIR, BatchEntry, ExportManifest, FORMAT, Frontier, MANIFEST_FILE, PROGRESS_FILE,
    ProgressEntry, ProgressState, SourceInfo, TableEntry,
};
