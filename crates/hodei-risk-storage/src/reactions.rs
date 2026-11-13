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

/// The blast radius over the real graph. Si le dan una vista del
/// impacto, SIRVE de la vista con fallback seguro: la respuesta es la
/// que el recorrido autoritativo dice, venga de donde venga, y una
/// vista atrasada nunca se presenta como actual.
pub struct SurrealBlastRadius<C: Connection> {
    graph: Arc<hodei_evidence_storage::SurrealGraphStore<C>>,
    view: Option<Arc<crate::impact_view::SurrealImpactView<C>>>,
    tenant: kernel::Hrn,
}

impl<C: Connection> SurrealBlastRadius<C> {
    /// The radius over a graph and a tenant.
    pub fn new(
        graph: Arc<hodei_evidence_storage::SurrealGraphStore<C>>,
        tenant: kernel::Hrn,
    ) -> Self {
        Self {
            graph,
            view: None,
            tenant,
        }
    }

    /// The radius that serves from the materialized view, with the
    /// authoritative walk as the fallback when the view is stale or
    /// absent.
    pub fn with_view(
        graph: Arc<hodei_evidence_storage::SurrealGraphStore<C>>,
        view: Arc<crate::impact_view::SurrealImpactView<C>>,
        tenant: kernel::Hrn,
    ) -> Self {
        Self {
            graph,
            view: Some(view),
            tenant,
        }
    }
}

#[async_trait]
impl<C: Connection> BlastRadiusSource for SurrealBlastRadius<C> {
    async fn impacted(
        &self,
        account: &str,
        advisory: &str,
    ) -> Result<Vec<ImpactedSubject>, String> {
        // The account of the request, the tenant of the adapter and the
        // account of the advisory have to be the SAME one. The account
        // comes from an event, and an event is a claim, not an identity:
        // the only authority here is the tenant the adapter was built
        // with. A request that names somebody else is a closed door, and
        // an identity error is an error, never an empty answer that looks
        // like zero impact.
        if account.trim().is_empty() {
            return Err("the request does not say which account it belongs to".to_string());
        }

        let Some(vulnerability) = kernel::Hrn::from_string(advisory) else {
            return Err(format!("the advisory {advisory} is not a canonical HRN"));
        };

        if account != self.tenant.account_id() {
            return Err(format!(
                "the request names account {account} and this radius belongs to {}",
                self.tenant.account_id()
            ));
        }

        if vulnerability.account_id() != self.tenant.account_id() {
            return Err(format!(
                "the advisory {advisory} belongs to account {} and this radius belongs to {}",
                vulnerability.account_id(),
                self.tenant.account_id()
            ));
        }

        // Con vista: la respuesta del serve, que es la del recorrido
        // autoritativo sea cual sea su frescura -- la vista al dia
        // responde de la vista, y la atrasada o ausente del walk de
        // AHORA. Los sujetos son SOLO los artefactos, el contrato de
        // siempre: los componentes de profundidad cero no son sujetos
        // de decision.
        if let Some(view) = &self.view {
            let answer = view
                .serve_impact(&self.tenant, &vulnerability)
                .await
                .map_err(|error| error.to_string())?;
            let deployments = answer.deployments.first().cloned();
            return Ok(answer
                .subjects
                .into_iter()
                .filter(|subject| subject.depth >= 1)
                .map(|subject| ImpactedSubject {
                    subject: subject.subject,
                    deployment: deployments.clone(),
                    depth: subject.depth,
                })
                .collect());
        }

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
