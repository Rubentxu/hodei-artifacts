//! The adapters that let the reactive runtime talk to the risk graph.
//!
//! The runtime knows nothing about graphs: it asks "what does this
//! vulnerability reach" and "where is the case". These are the two
//! answers, over the real store.

use std::sync::Arc;

use async_trait::async_trait;
use hodei_reactive::reactions::{BlastRadiusSource, ImpactedSubject, SecurityCaseSink};
use hodei_risk::case::SecurityCase;
use surrealdb::Connection;
use surrealdb::Surreal;

use crate::RiskError;

/// The blast radius over the real graph.
pub struct SurrealBlastRadius<C: Connection> {
    graph: Arc<hodei_evidence_storage::SurrealGraphStore<C>>,
    tenant: kernel::Hrn,
}

impl<C: Connection> SurrealBlastRadius<C> {
    /// The radius over a graph and a tenant.
    pub fn new(
        graph: Arc<hodei_evidence_storage::SurrealGraphStore<C>>,
        tenant: kernel::Hrn,
    ) -> Self {
        Self { graph, tenant }
    }
}

#[async_trait]
impl<C: Connection> BlastRadiusSource for SurrealBlastRadius<C> {
    async fn impacted(
        &self,
        _account: &str,
        advisory: &str,
    ) -> Result<Vec<ImpactedSubject>, String> {
        let Some(vulnerability) = kernel::Hrn::from_string(advisory) else {
            return Ok(Vec::new());
        };

        let radius = crate::blast_radius(self.graph.as_ref(), &self.tenant, &vulnerability)
            .await
            .map_err(|error| error.to_string())?;

        Ok(radius
            .depths
            .into_iter()
            .map(|entry| ImpactedSubject {
                subject: entry.subject,
                deployment: entry.deployments.first().cloned(),
                depth: entry.depth,
            })
            .collect())
    }
}

/// The cases of security, in the real store.
pub struct SurrealSecurityCaseStore<C: Connection> {
    db: Arc<Surreal<C>>,
}

impl<C: Connection> SurrealSecurityCaseStore<C> {
    /// The store over a database.
    pub fn new(db: Arc<Surreal<C>>) -> Self {
        Self { db }
    }

    /// The case of an identifier.
    pub async fn get(&self, id: &str) -> Result<Option<SecurityCase>, RiskError> {
        let mut response = self
            .db
            .query("SELECT * FROM type::table($table) WHERE id = $id")
            .bind(("table", "security_case"))
            .bind((
                "id",
                surrealdb::sql::Thing::from(("security_case".to_string(), id.to_string())),
            ))
            .await
            .map_err(|error| RiskError::Unavailable(error.to_string()))?;

        let cases: Vec<CaseRow> = response
            .take(0)
            .map_err(|error| RiskError::Unavailable(error.to_string()))?;
        Ok(cases.into_iter().next().map(|row| row.case))
    }

    /// The cases of a trigger.
    pub async fn of_trigger(&self, trigger: &str) -> Result<Vec<SecurityCase>, RiskError> {
        let mut response = self
            .db
            .query("SELECT * FROM type::table($table) WHERE case.trigger = $trigger")
            .bind(("table", "security_case"))
            .bind(("trigger", trigger.to_string()))
            .await
            .map_err(|error| RiskError::Unavailable(error.to_string()))?;

        let cases: Vec<CaseRow> = response
            .take(0)
            .map_err(|error| RiskError::Unavailable(error.to_string()))?;
        Ok(cases.into_iter().map(|row| row.case).collect())
    }
}

/// The case of a row.
#[derive(Debug, Clone, serde::Deserialize)]
struct CaseRow {
    /// The case itself.
    #[serde(rename = "case")]
    case: SecurityCase,
}

#[async_trait]
impl<C: Connection> SecurityCaseSink for SurrealSecurityCaseStore<C> {
    async fn open_or_update(
        &self,
        advisory: &str,
        impacted: &[ImpactedSubject],
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<String, String> {
        // The case is opened ONCE: asking again for a case that already
        // exists gives the one that is there, because a case opened twice
        // for the same thing is two cases and nobody owns either.
        let id = format!("case-{advisory}");
        if let Some(existing) = self.get(&id).await.map_err(|error| error.to_string())? {
            return Ok(existing.id);
        }

        let radius = hodei_risk::case::BlastRadius {
            affected_subjects: impacted.iter().map(|one| one.subject.clone()).collect(),
            affected_deployments: impacted
                .iter()
                .filter_map(|one| one.deployment.clone())
                .collect(),
            affected_components: Vec::new(),
            depths: impacted
                .iter()
                .map(|one| hodei_risk::case::SubjectDepth {
                    subject: one.subject.clone(),
                    depth: one.depth,
                    deployments: one.deployment.clone().into_iter().collect(),
                })
                .collect(),
        };

        let case = SecurityCase::open(
            id.clone(),
            advisory,
            Vec::new(),
            radius,
            hodei_risk::case::VerificationCriteria::new(None),
            now,
        );

        self.db
            .query("UPSERT $id CONTENT $content")
            .bind((
                "id",
                surrealdb::sql::Thing::from(("security_case".to_string(), id.clone())),
            ))
            .bind(("content", serde_json::json!({"case": case})))
            .await
            .map_err(|error| error.to_string())?
            .check()
            .map_err(|error| error.to_string())?;

        Ok(case.id)
    }
}
