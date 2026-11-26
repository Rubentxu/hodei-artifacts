//! The catalog adapter over Dgraph.
//!
//! The domain lives in `hodei-catalog`, which does not know a database
//! exists. Here is where that is known: the catalog tables, the indexes that
//! decide conflicts, and the transactions that make publishing
//! a single thing and not four.
//!
//! The split is not tidiness. It is a guarantee that the compiler
//! checks: the domain crate **cannot** name a database
//! engine, so its contracts cannot depend on one by accident.
//!
//! The **audit outbox** also lives here: the table where the event of a
//! publication waits, and the adapter that delivers it. It lives in this
//! database and not in another on purpose, because the event row is written
//! inside the same transaction that publishes, and a transaction that touches
//! two databases is not a transaction.

#![forbid(unsafe_code)]

// The catalog's Dgraph port (HSP-187, Raw slice): the same contract
// of HAR-02/HAR-03 over the native graph, with the audit outbox in
// the SAME transaction as the commit.
pub mod dgraph;

pub use dgraph::{CATALOG_DGRAPH_SCHEMA_VERSION, DgraphAuditOutbox, DgraphCatalogStore};
