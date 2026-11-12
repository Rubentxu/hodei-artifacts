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

/// The table the external index of the evidence lives in.
///
/// The index says, for the identity that a producer gives to a thing, which
/// record of evidence it is. It is a table of its own, read by its own
/// identity: finding the evidence of an external identity used to be a
/// scan of the whole table, which made the admission quadratic as the
/// evidence grew.
pub const EVIDENCE_INDEX_TABLE: &str = "evidence_external";

/// Escapes a part of a record identifier.
fn escape_id_part(value: &str) -> String {
    value.replace('~', "~0").replace(':', "~1")
}

/// The entry of the external index, as it is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexRow {
    /// The identity of the producer.
    pub producer_id: String,
    /// The external identity the producer gave to the evidence.
    pub external_id: String,
    /// The evidence it points at.
    pub evidence_id: String,
}

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
        surrealdb::sql::Thing::from((EVIDENCE_TABLE.to_string(), escape_id_part(evidence_id)))
    }

    /// The identifier of the entry of the external index.
    fn index_id(producer_id: &str, external_id: &str) -> surrealdb::sql::Thing {
        surrealdb::sql::Thing::from((
            EVIDENCE_INDEX_TABLE.to_string(),
            format!(
                "{}~{}",
                escape_id_part(producer_id),
                escape_id_part(external_id)
            ),
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
        let mut attempts = 0;
        loop {
            // The record and the entry of the external index go together,
            // in one transaction: the index never points at a record that
            // is not there.
            let index = IndexRow {
                producer_id: evidence.producer().id.clone(),
                external_id: evidence.source().external_id.clone().unwrap_or_default(),
                evidence_id: evidence.id().as_str().to_string(),
            };
            let written = self
                .db
                .query(
                    "UPSERT $id CONTENT $content; \
                     UPSERT $index_id CONTENT $index",
                )
                .bind(("id", Self::record_id(evidence.id().as_str())))
                .bind(("content", row.clone()))
                .bind((
                    "index_id",
                    Self::index_id(
                        &evidence.producer().id,
                        evidence.source().external_id.as_deref().unwrap_or_default(),
                    ),
                ))
                .bind(("index", index))
                .await
                .map_err(|error| StoreError::Unavailable(error.to_string()));

            let failure = match written {
                Ok(response) => response.check().err().map(|error| error.to_string()),
                Err(error) => Some(error.to_string()),
            };

            match failure {
                None => return Ok(evidence.id().clone()),
                Some(message)
                    if crate::is_conflict(&message) && attempts < crate::CONFLICT_RETRIES =>
                {
                    // Another writer got there at the same moment. The write
                    // is the same one, so trying it again is safe.
                    attempts += 1;
                }
                Some(message) => return Err(StoreError::InvalidRecord(message)),
            }
        }
    }

    /// Reads a piece of evidence by its identity.
    async fn get(&self, id: &EvidenceId) -> Result<Option<EvidenceRecord>, StoreError> {
        let mut response = self
            .db
            .query("SELECT * FROM $id")
            .bind(("id", Self::record_id(id.as_str())))
            .await
            .map_err(|error| StoreError::Unavailable(error.to_string()))?;
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
        // The index says which record it is, and the record says what it
        // is: two reads by identity, and no scan.
        let mut response = self
            .db
            .query("SELECT * FROM $id")
            .bind(("id", Self::index_id(&producer.id, external_id)))
            .await
            .map_err(|error| StoreError::Unavailable(error.to_string()))?;
        let entries: Vec<IndexRow> = response
            .take(0)
            .map_err(|error| StoreError::InvalidRecord(error.to_string()))?;
        let Some(entry) = entries.first() else {
            return Ok(None);
        };

        let mut response = self
            .db
            .query("SELECT * FROM $id")
            .bind(("id", Self::record_id(&entry.evidence_id)))
            .await
            .map_err(|error| StoreError::Unavailable(error.to_string()))?;
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
