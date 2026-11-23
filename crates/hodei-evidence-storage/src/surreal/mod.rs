//! The adapters of the evidence context over SurrealDB.
//!
//! They live in their own crate, and not in the crate of the domain, for a
//! reason that is not tidiness: the domain crate **cannot** name a database
//! engine, so the contracts it publishes cannot accidentally depend on one.
//! A check of the architecture proves it, and the compiler proves it too.

pub mod delta;
pub mod evidence_store;
pub mod graph;
pub mod ledger;

pub use evidence_store::{
    EVIDENCE_SUBJECT_TABLE, EVIDENCE_TABLE, EvidenceRow, SubjectRow, SurrealEvidenceStore,
    the_evidence_put_statement, the_subject_pair_read, the_subject_records_read,
};
pub use graph::{
    ACTIVE_PROJECTION, CHECKPOINT_TABLE, GRAPH_INDEXES, REBUILD_PROJECTION, SurrealGraphStore,
    SurrealProjectionCheckpoints, declare_index_statement,
};
pub use ledger::{LEDGER_TABLE, LedgerRecord, SurrealEventLedger};

use surrealdb::Connection;
use surrealdb::Surreal;

/// The tables of the evidence itself, and the indexes each one needs.
///
/// The read of one subject -- the read the trust capsule leans on -- used
/// to ask the evidence table a containment over its array of subjects.
/// The planner cannot bind a containment into an index read: it binds the
/// equality of the account and filters the containment row by row, so the
/// read grew with the size of the evidence and not with the evidence the
/// subject holds (211 ms at 100k records, 16.1 s at 1M). The store
/// answers the question from the pair table instead, and this is the
/// index that makes that read bounded: the account and the subject of
/// every entry, both bound into the index read when the capsule asks for
/// the evidence of one subject.
pub const EVIDENCE_INDEXES: &[(&str, &str)] =
    &[("evidence_subject", "account, subject, evidence_id")];

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
    for (table, fields) in GRAPH_INDEXES.iter().chain(EVIDENCE_INDEXES.iter()) {
        db.query(declare_index_statement(table, fields))
            .await
            .map_err(|error| error.to_string())?
            .check()
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}
