//! The adapters of the evidence context.
//!
//! The domain contracts live in `hodei-evidence`, and they do not know that
//! a database exists. This crate is where that knowledge lives: the
//! adapters that implement the ports of the domain over the store that the
//! deployment already runs.
//!
//! The split is not tidiness. It is a guarantee that the compiler checks:
//! the crate of the domain **cannot** name a database engine, so the
//! contracts it publishes cannot depend on one by accident.

pub mod authenticator;
pub mod content_store;
pub mod surreal;

pub use authenticator::{HmacEnvelopeAuthenticator, ProducerCredentials};
pub use content_store::InMemoryContentStore;
pub use surreal::{
    ACTIVE_PROJECTION, CHECKPOINT_TABLE, EVIDENCE_TABLE, EvidenceRow, LEDGER_TABLE, LedgerRecord,
    REBUILD_PROJECTION, SurrealEventLedger, SurrealEvidenceStore, SurrealGraphStore,
    SurrealProjectionCheckpoints,
};

/// How many times a write is tried when the transaction of the loser could
/// not commit.
///
/// The store refuses two writers at the same time in two ways: the record
/// is already there, or the transaction of the loser could not confirm
/// because it read what the winner wrote. The second one is not a failure
/// of the write: it is the write arriving a moment too late, and it is
/// tried again.
pub const CONFLICT_RETRIES: usize = 8;

/// Whether a message of the store is a transaction conflict.
///
/// The message is what the store says when two writers meet at the same
/// moment, and it says that the transaction can be retried.
pub fn is_conflict(message: &str) -> bool {
    message.contains("read or write conflict")
        || message.contains("can be retried")
        || message.contains("already exists")
        || message.contains("was unique")
}
