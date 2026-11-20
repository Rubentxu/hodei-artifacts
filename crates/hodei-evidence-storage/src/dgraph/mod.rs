//! The adapters of the evidence context over the graph authority.
//!
//! They sit beside the Surreal adapters, not instead of them: the
//! deployment chooses one store, the domain chooses neither. What is
//! shared is the port — the contracts of `hodei-evidence` — and what
//! the contract proves is the SEMANTICS of each adapter over the engine
//! the deployment runs.

pub mod evidence_store;
pub mod ledger;

pub use evidence_store::{
    DgraphEvidenceError, DgraphEvidenceStore, EVIDENCE_EXTERNAL_TYPE, EVIDENCE_TYPE,
};
pub use ledger::{DgraphEventLedger, DgraphLedgerError, LEDGER_EVENT_TYPE, LedgerEventRecord};
