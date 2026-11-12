//! The store of the evidence of reachability, over SurrealDB.
//!
//! The evidence is kept **whole**: the coverage, the limits, the entry
//! points and the call edges are one thing, and splitting them would make
//! it possible to read an evidence that never existed -- one with a
//! complete coverage and no call edges, which is exactly the shape that
//! would say `UNREACHABLE` without having earned it.
//!
//! The versions are kept: two evidences of the same analyser and the same
//! revision are the same evidence, and the evidence of a *different*
//! analyser or a *different* revision is another one that stays.

#![forbid(unsafe_code)]

use async_trait::async_trait;
use hodei_reachability::evidence::ReachabilityEvidence;
use hodei_reachability::identity::CodeIdentity;
use hodei_reachability::store::{ReachabilityStore, ReachabilityStoreError};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use surrealdb::Connection;
use surrealdb::Surreal;

/// The table the evidence lives in.
pub const EVIDENCE_TABLE: &str = "reachability_evidence";

/// The row of an evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRow {
    /// The identifier of the evidence.
    pub evidence_id: String,
    /// The revision of the source that was analysed.
    pub source_revision: String,
    /// The name of the analyser.
    pub analyzer_name: String,
    /// The version of the analyser.
    pub analyzer_version: String,
    /// Whether the analysis covered all the code.
    pub coverage_complete: bool,
    /// The symbols the analysis covered.
    pub symbols: Vec<String>,
    /// The whole evidence.
    pub evidence: serde_json::Value,
}

/// The store of the evidence of reachability.
pub struct SurrealReachabilityStore<C: Connection> {
    db: Arc<Surreal<C>>,
}

impl<C: Connection> SurrealReachabilityStore<C> {
    /// A store over a database.
    pub fn new(db: Arc<Surreal<C>>) -> Self {
        Self { db }
    }

    /// The identity of a piece of evidence.
    ///
    /// It is the analyser, la revision and el identificador: dos
    /// evidencias del mismo analizador y de la misma revision son la
    /// misma, y las demas son otras.
    fn row_id(evidence: &ReachabilityEvidence) -> surrealdb::sql::Thing {
        let identity = format!(
            "{}-{}-{}",
            evidence.analyzer.name, evidence.analyzer.version, evidence.coverage.source_revision
        );
        surrealdb::sql::Thing::from((EVIDENCE_TABLE.to_string(), identity))
    }
}

/// The row of an evidence.
fn row_from(evidence: &ReachabilityEvidence) -> EvidenceRow {
    EvidenceRow {
        evidence_id: evidence.id.clone(),
        source_revision: evidence.coverage.source_revision.clone(),
        analyzer_name: evidence.analyzer.name.clone(),
        analyzer_version: evidence.analyzer.version.clone(),
        coverage_complete: evidence.coverage.complete,
        symbols: evidence
            .coverage
            .symbols
            .iter()
            .map(|symbol| symbol.to_string())
            .collect(),
        evidence: serde_json::to_value(evidence).unwrap_or(serde_json::Value::Null),
    }
}

/// The evidence of a row.
fn evidence_from(row: &EvidenceRow) -> Option<ReachabilityEvidence> {
    serde_json::from_value(row.evidence.clone()).ok()
}

#[async_trait]
impl<C: Connection> ReachabilityStore for SurrealReachabilityStore<C> {
    /// Writes an evidence down.
    async fn put(&self, evidence: &ReachabilityEvidence) -> Result<(), ReachabilityStoreError> {
        let row = row_from(evidence);
        self.db
            .query("UPSERT $id CONTENT $content")
            .bind(("id", Self::row_id(evidence)))
            .bind(("content", row))
            .await
            .map_err(|error| ReachabilityStoreError::Unavailable(error.to_string()))?;
        Ok(())
    }

    /// The evidence of a symbol, of a revision.
    async fn evidence_of(
        &self,
        source_revision: &str,
        symbol: &CodeIdentity,
    ) -> Result<Vec<ReachabilityEvidence>, ReachabilityStoreError> {
        let all = self.evidence_of_revision(source_revision).await?;
        Ok(all
            .into_iter()
            .filter(|evidence| evidence.coverage.covers(symbol))
            .collect())
    }

    /// All the evidence of a revision.
    async fn evidence_of_revision(
        &self,
        source_revision: &str,
    ) -> Result<Vec<ReachabilityEvidence>, ReachabilityStoreError> {
        let mut response = self
            .db
            .query(
                "SELECT * FROM type::table($table) \
                 WHERE source_revision = $revision \
                 ORDER BY analyzer_version",
            )
            .bind(("table", EVIDENCE_TABLE.to_string()))
            .bind(("revision", source_revision.to_string()))
            .await
            .map_err(|error| ReachabilityStoreError::Unavailable(error.to_string()))?;
        let rows: Vec<EvidenceRow> = response
            .take(0)
            .map_err(|error| ReachabilityStoreError::Unavailable(error.to_string()))?;
        Ok(rows.iter().filter_map(evidence_from).collect())
    }
}
