//! The adapters of the evidence context over SurrealDB.
//!
//! They live in their own crate, and not in the crate of the domain, for a
//! reason that is not tidiness: the domain crate **cannot** name a database
//! engine, so the contracts it publishes cannot accidentally depend on one.
//! A check of the architecture proves it, and the compiler proves it too.

pub mod evidence_store;
pub mod delta;
pub mod graph;
pub mod ledger;

pub use evidence_store::{EVIDENCE_TABLE, EvidenceRow, SurrealEvidenceStore};
pub use graph::{
    ACTIVE_PROJECTION, CHECKPOINT_TABLE, GRAPH_INDEXES, REBUILD_PROJECTION, SurrealGraphStore,
    SurrealProjectionCheckpoints, declare_index_statement,
};
pub use ledger::{LEDGER_TABLE, LedgerRecord, SurrealEventLedger};

use surrealdb::Connection;
use surrealdb::Surreal;

/// Declares the indexes that the reading of the evidence leans on.
///
/// Defining an index on the local engine keeps the database directory
/// locked by the process that defined it, so this cannot run inside the
/// service: the service would serve correctly but the same process could
/// not open the database again. It belongs to a command of its own, which
/// releases the lock when it exits.
///
/// The statements are idempotent, so the command can be run on every
/// deployment.
pub async fn declare_indexes<C: Connection>(db: &Surreal<C>) -> Result<(), String> {
    for (table, fields) in GRAPH_INDEXES {
        db.query(declare_index_statement(table, fields))
            .await
            .map_err(|error| error.to_string())?
            .check()
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}
