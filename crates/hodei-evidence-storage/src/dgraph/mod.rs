//! The adapters of the evidence context over the graph authority.
//!
//! They replace the retired Surreal adapters: the
//! deployment chooses one store, the domain chooses neither. What is
//! shared is the port — the contracts of `hodei-evidence` — and what
//! the contract proves is the SEMANTICS of each adapter over the engine
//! the deployment runs.

pub mod delta;
pub mod evidence_store;
pub mod frontier;
pub mod graph;
pub mod ledger;

pub use delta::DgraphDeltaLedger;
pub use evidence_store::{
    DgraphEvidenceError, DgraphEvidenceStore, EVIDENCE_EXTERNAL_TYPE, EVIDENCE_TYPE,
};
pub use frontier::{
    CHECKPOINT_TYPE, CheckpointRow, DELTA_TYPE, DgraphProjectionFrontier, FrontierCommit,
    FrontierError,
};
pub use graph::{
    ACTIVE_PROJECTION, DgraphGraphStore, GRAPH_NODE_TYPE, GRAPH_RELATION_TYPE, GRAPH_SUPPORT_TYPE,
    STATUS_ASSERTED, STATUS_UNKNOWN,
};
pub use ledger::{
    DgraphEventLedger, DgraphLedgerError, LEDGER_EVENT_TYPE, LedgerEventRecord,
    MeteredDgraphEventLedger,
};
