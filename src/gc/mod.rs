//! Garbage collection of the content store.
//!
//! - [`plan`]: the pure core. Given what is there, what the catalog
//!   references and the retentions, it says what is kept, what is in
//!   grace and what is reclaimable, and why.
//! - [`service`]: ties it to the store, the catalog and the ledger, and offers
//!   the dry-run and the durable sweep.
//! - [`pass`]: the coordinated, fenced pass across replicas (HSP-192).

pub mod pass;
pub mod plan;
pub mod service;

pub use pass::{CoordinatedPass, PassAction, PassConfig, PassReport, ROOT_PAGE_LIMIT};
pub use plan::{Entry, Plan, Reason, Summary, Verdict};
pub use service::{GcError, GcService, SweepReport};
