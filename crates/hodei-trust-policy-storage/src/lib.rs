//! The store of the promotion decisions.
//!
//! A decision is written once and read many times. The store keeps it as
//! it was made -- the policy, the context, the inputs and the reasons --
//! so that the decision can be reconstructed later without asking the
//! system to recompute anything, which is the only way to know that what
//! was decided is what is read.
//!
//! The store does not offer a way to change a decision. A decision that
//! can be edited is not a decision.

#![forbid(unsafe_code)]

use async_trait::async_trait;
use hodei_trust_policy::ports::{DecisionStore, DecisionStoreError, PromotionTargets};
use hodei_trust_policy::promotion::{DecisionId, PromotionDecision, PromotionTarget};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use surrealdb::Connection;
use surrealdb::Surreal;

/// The table the decisions live in.
pub const DECISION_TABLE: &str = "promotion_decision";

/// The row of a decision.
///
/// The whole decision travels as one document: the policy, the context,
/// the inputs and the reasons are one thing, and splitting them across
/// columns would make it possible to read half a decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRow {
    /// The identifier of the decision.
    pub decision_id: String,
    /// The correlation of the request.
    pub correlation_id: String,
    /// The account the promotion happened in.
    pub account: String,
    /// The subject that was promoted.
    pub subject: String,
    /// The distribution it went to.
    pub distribution: String,
    /// The outcome, as it reads.
    pub outcome: String,
    /// Which of the two halves decided it.
    pub decided_by: String,
    /// The reasons, as they read.
    pub reasons: Vec<String>,
    /// The version of the policy that produced it.
    pub policy_version: String,
    /// The hash of the capsule it was measured against.
    pub capsule_hash: Option<String>,
    /// Whether the promotion had an effect.
    pub applied: bool,
    /// The moment the decision was made.
    pub decided_at: chrono::DateTime<chrono::Utc>,
    /// The whole decision.
    pub decision: serde_json::Value,
}

/// The store of the decisions, over SurrealDB.
pub struct SurrealDecisionStore<C: Connection> {
    db: Arc<Surreal<C>>,
}

impl<C: Connection> SurrealDecisionStore<C> {
    /// A store over a database.
    pub fn new(db: Arc<Surreal<C>>) -> Self {
        Self { db }
    }

    /// The identity of a decision.
    fn row_id(id: &DecisionId) -> surrealdb::sql::Thing {
        surrealdb::sql::Thing::from((DECISION_TABLE.to_string(), id.as_str().to_string()))
    }
}

/// The row of a decision.
fn row_from(decision: &PromotionDecision) -> DecisionRow {
    DecisionRow {
        decision_id: decision.id.as_str().to_string(),
        correlation_id: decision.request.correlation_id.clone(),
        account: decision.request.account.clone(),
        subject: decision.request.subject.clone(),
        distribution: decision.request.distribution.clone(),
        outcome: decision.outcome.as_str().to_string(),
        decided_by: format!("{:?}", decision.decided_by).to_lowercase(),
        reasons: decision
            .reasons
            .iter()
            .map(|reason| reason.as_str().to_string())
            .collect(),
        policy_version: decision.trust.policy_version.clone(),
        capsule_hash: decision.trust.capsule_hash.clone(),
        applied: decision.applied,
        decided_at: decision.decided_at,
        decision: serde_json::to_value(decision).unwrap_or(serde_json::Value::Null),
    }
}

/// The decision of a row.
fn decision_from(row: &DecisionRow) -> Option<PromotionDecision> {
    serde_json::from_value(row.decision.clone()).ok()
}

#[async_trait]
impl<C: Connection> DecisionStore for SurrealDecisionStore<C> {
    /// Writes a decision down.
    ///
    /// The write is an upsert: writing the same decision twice leaves one
    /// decision, and the store says the same identity. A decision that is
    /// written twice is the same decision -- that is what makes the
    /// delivery of the same request safe.
    async fn put(&self, decision: &PromotionDecision) -> Result<(), DecisionStoreError> {
        let row = row_from(decision);
        let mut attempts = 0;
        loop {
            let written = self
                .db
                .query("UPSERT $id CONTENT $content")
                .bind(("id", Self::row_id(&decision.id)))
                .bind(("content", row.clone()))
                .await
                .map_err(|error| DecisionStoreError::Unavailable(error.to_string()));

            let failure = match written {
                Ok(response) => response.check().err().map(|error| error.to_string()),
                Err(error) => Some(error.to_string()),
            };

            match failure {
                None => return Ok(()),
                Some(message) if hodei_evidence_storage::is_conflict(&message) && attempts < 3 => {
                    attempts += 1;
                }
                Some(message) => return Err(DecisionStoreError::Unavailable(message)),
            }
        }
    }

    /// Reads a decision by its identifier.
    async fn get(&self, id: &DecisionId) -> Result<Option<PromotionDecision>, DecisionStoreError> {
        let mut response = self
            .db
            .query("SELECT * FROM $id")
            .bind(("id", Self::row_id(id)))
            .await
            .map_err(|error| DecisionStoreError::Unavailable(error.to_string()))?;
        let rows: Vec<DecisionRow> = response
            .take(0)
            .map_err(|error| DecisionStoreError::Unavailable(error.to_string()))?;
        Ok(rows.first().and_then(decision_from))
    }

    /// Reads the decisions of a subject.
    async fn list_for_subject(
        &self,
        account: &str,
        subject: &str,
    ) -> Result<Vec<PromotionDecision>, DecisionStoreError> {
        let mut response = self
            .db
            .query(
                "SELECT * FROM type::table($table) \
                 WHERE account = $account AND subject = $subject \
                 ORDER BY decided_at DESC",
            )
            .bind(("table", DECISION_TABLE.to_string()))
            .bind(("account", account.to_string()))
            .bind(("subject", subject.to_string()))
            .await
            .map_err(|error| DecisionStoreError::Unavailable(error.to_string()))?;
        let rows: Vec<DecisionRow> = response
            .take(0)
            .map_err(|error| DecisionStoreError::Unavailable(error.to_string()))?;
        Ok(rows.iter().filter_map(decision_from).collect())
    }

    /// Reads the decision of a correlation.
    async fn find_by_correlation(
        &self,
        correlation_id: &str,
    ) -> Result<Option<PromotionDecision>, DecisionStoreError> {
        let mut response = self
            .db
            .query(
                "SELECT * FROM type::table($table) \
                 WHERE correlation_id = $correlation \
                 ORDER BY decided_at DESC",
            )
            .bind(("table", DECISION_TABLE.to_string()))
            .bind(("correlation", correlation_id.to_string()))
            .await
            .map_err(|error| DecisionStoreError::Unavailable(error.to_string()))?;
        let rows: Vec<DecisionRow> = response
            .take(0)
            .map_err(|error| DecisionStoreError::Unavailable(error.to_string()))?;
        Ok(rows.first().and_then(decision_from))
    }
}

/// The destinations of a promotion, over a fixed list.
///
/// A deployment that has no distribution configured says so, and the
/// promotion answers that the operation is not supported.
#[derive(Debug, Clone, Default)]
pub struct StaticPromotionTargets {
    targets: Vec<PromotionTarget>,
}

impl StaticPromotionTargets {
    /// The destinations that exist.
    pub fn new(targets: Vec<PromotionTarget>) -> Self {
        Self { targets }
    }
}

#[async_trait]
impl PromotionTargets for StaticPromotionTargets {
    async fn target_of(
        &self,
        account: &str,
        _subject: &str,
    ) -> Result<Option<PromotionTarget>, DecisionStoreError> {
        Ok(self
            .targets
            .iter()
            .find(|target| target.account == account)
            .cloned())
    }
}
