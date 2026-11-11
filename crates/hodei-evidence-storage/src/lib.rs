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

pub mod surreal;

pub use surreal::{LEDGER_TABLE, LedgerRecord, SurrealEventLedger};
