//! The adapters of the evidence context over SurrealDB.
//!
//! They live in their own crate, and not in the crate of the domain, for a
//! reason that is not tidiness: the domain crate **cannot** name a database
//! engine, so the contracts it publishes cannot accidentally depend on one.
//! A check of the architecture proves it, and the compiler proves it too.

pub mod evidence_store;
pub mod graph;
pub mod ledger;

pub use evidence_store::{EVIDENCE_TABLE, EvidenceRow, SurrealEvidenceStore};
pub use graph::{
    ACTIVE_PROJECTION, CHECKPOINT_TABLE, REBUILD_PROJECTION, SurrealGraphStore,
    SurrealProjectionCheckpoints,
};
pub use ledger::{LEDGER_TABLE, LedgerRecord, SurrealEventLedger};
