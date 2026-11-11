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
    EVIDENCE_TABLE, EvidenceRow, LEDGER_TABLE, LedgerRecord, SurrealEventLedger,
    SurrealEvidenceStore,
};
