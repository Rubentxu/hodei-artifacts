//! The store of accepted evidence, over the store that already exists.
//!
//! The record of the evidence is stored under its own identity, so storing
//! the same evidence twice is the same write: the store is idempotent for
//! the same record, and the ingestion decides what a repetition means.

use hodei_evidence::{
    ContentDigest, EvidenceId, EvidenceRecord, EvidenceStore, Producer, StoreError,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use surrealdb::Connection;
use surrealdb::Surreal;

/// The table the evidence lives in.
pub const EVIDENCE_TABLE: &str = "evidence";

/// The record of an evidence, as it is stored.
///
/// The producer and the external identity are kept as plain fields, next to
/// the record, so that the lookup by external identity does not have to read
/// and unwrap the whole structure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRow {
    /// The identity of the record.
    pub evidence_id: String,
    /// The account the evidence belongs to.
    pub tenant_hrn: String,
    /// The identity of the producer.
    pub producer_id: String,
    /// The external identity the producer gave to the evidence.
    pub external_id: Option<String>,
    /// The kind of evidence.
    pub evidence_type: String,
    /// The digest of its content.
    pub content_digest: String,
    /// The content itself, as it was stored.
    pub content: serde_json::Value,
    /// When Hodei learned about it.
    pub observed_at: chrono::DateTime<chrono::Utc>,
    /// When it was accepted.
    pub recorded_at: chrono::DateTime<chrono::Utc>,
    /// How far its verification goes.
    pub verification_status: String,
    /// The whole record, so that it can be read back whole.
    pub record: serde_json::Value,
}

/// The store of accepted evidence.
pub struct SurrealEvidenceStore<C: Connection> {
    db: Arc<Surreal<C>>,
}

impl<C: Connection> SurrealEvidenceStore<C> {
    /// A store over a database.
    pub fn new(db: Arc<Surreal<C>>) -> Self {
        Self { db }
    }

    /// The identifier of the record of an evidence.
    fn record_id(evidence_id: &str) -> surrealdb::sql::Thing {
        surrealdb::sql::Thing::from((
            EVIDENCE_TABLE.to_string(),
            evidence_id.replace('~', "~0").replace(':', "~1"),
        ))
    }
}

#[async_trait::async_trait]
impl<C: Connection> EvidenceStore for SurrealEvidenceStore<C> {
    /// Stores an accepted piece of evidence.
    ///
    /// The write is an upsert: storing the same record twice leaves one
    /// record, and the store says the same identity.
    async fn put(&self, evidence: EvidenceRecord) -> Result<EvidenceId, StoreError> {
        let row = row_from(&evidence);
        self.db
            .query("UPSERT $id CONTENT $content")
            .bind(("id", Self::record_id(evidence.id().as_str())))
            .bind(("content", row))
            .await
            .map_err(|_error| StoreError::Unavailable)?
            .check()
            .map_err(|error| StoreError::InvalidRecord(error.to_string()))?;
        Ok(evidence.id().clone())
    }

    /// Reads a piece of evidence by its identity.
    async fn get(&self, id: &EvidenceId) -> Result<Option<EvidenceRecord>, StoreError> {
        let mut response = self
            .db
            .query("SELECT * FROM $id")
            .bind(("id", Self::record_id(id.as_str())))
            .await
            .map_err(|_error| StoreError::Unavailable)?;
        let rows: Vec<EvidenceRow> = response
            .take(0)
            .map_err(|error| StoreError::InvalidRecord(error.to_string()))?;
        Ok(rows.into_iter().next().and_then(|row| record_from(&row)))
    }

    /// Reads the last piece of evidence a producer delivered under an external
    /// identity.
    async fn find_by_external_id(
        &self,
        producer: &Producer,
        external_id: &str,
    ) -> Result<Option<EvidenceRecord>, StoreError> {
        let mut response = self
            .db
            .query(
                "SELECT * FROM type::table($table) \
                 WHERE producer_id = $producer AND external_id = $external \
                 ORDER BY recorded_at DESC LIMIT 1",
            )
            .bind(("table", EVIDENCE_TABLE.to_string()))
            .bind(("producer", producer.id.clone()))
            .bind(("external", external_id.to_string()))
            .await
            .map_err(|_error| StoreError::Unavailable)?;
        let rows: Vec<EvidenceRow> = response
            .take(0)
            .map_err(|error| StoreError::InvalidRecord(error.to_string()))?;
        Ok(rows.into_iter().next().and_then(|row| record_from(&row)))
    }
}

/// The row of a record of evidence.
fn row_from(evidence: &EvidenceRecord) -> EvidenceRow {
    EvidenceRow {
        evidence_id: evidence.id().as_str().to_string(),
        tenant_hrn: evidence.tenant_hrn().to_string(),
        producer_id: evidence.producer().id.clone(),
        external_id: evidence.source().external_id.clone(),
        evidence_type: evidence.evidence_type().to_string(),
        content_digest: evidence.content().digest.to_string(),
        content: serde_json::json!({
            "digest": evidence.content().digest.to_string(),
            "object_ref": evidence.content().object_ref,
        }),
        observed_at: evidence.observed_at(),
        recorded_at: evidence.recorded_at(),
        verification_status: evidence.verification().status.to_string(),
        record: serde_json::to_value(evidence).unwrap_or(serde_json::Value::Null),
    }
}

/// The record of a row.
fn record_from(row: &EvidenceRow) -> Option<EvidenceRecord> {
    serde_json::from_value(row.record.clone()).ok()
}

/// The digest of a row, when it has one.
pub fn digest_of(row: &EvidenceRow) -> Option<ContentDigest> {
    let (algorithm, value) = row.content_digest.split_once(':')?;
    match algorithm {
        "sha256" => Some(ContentDigest::sha256(value)),
        _ => None,
    }
}
