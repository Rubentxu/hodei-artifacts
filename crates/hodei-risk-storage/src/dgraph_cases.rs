//! The security cases of the risk, in the SAME Dgraph the walk reads.
//!
//! This is the Dgraph port of the retired Surreal security-case store:
//! the same [`SecurityCaseSink`] contract — the case of a vulnerability is
//! opened ONCE, and asking again gives the case that is there — mapped onto
//! the engine the stack already proves, with the conventions of
//! [`crate::dgraph_view::DgraphImpactView`]: one node per (tenant, key), the
//! key exact-indexed and `@upsert`, the account as the first half of the key
//! AND as a filter on every read.
//!
//! # Isolation
//!
//! A store is bound to ONE account. The case id is derived from the
//! vulnerability (`case-<advisory>`), so two accounts that react to the same
//! advisory have two different cases: the node key is `tenant|case id`, and
//! every read filters on the tenant as well. A subject of another account is
//! refused when the case is opened: a blast radius that names somebody
//! else's artifacts is a leak, not a case.
//!
//! # Fail-closed
//!
//! A store that fails answers with an error, never with a case that looks
//! like "nothing happened": an unreadable record, a write that did not
//! commit and a case that is not there after the write are all errors.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_graph_client::{
    GraphClient, GraphClientError, GraphMutationBlock, GraphMutationPort, GraphUpsertTransaction,
    dql_string,
};
use hodei_reactive::reactions::{ImpactedSubject, SecurityCaseSink};
use hodei_risk::case::{BlastRadius, SecurityCase, SubjectDepth, VerificationCriteria};
use kernel::Hrn;

use crate::RiskError;

/// The type tag of the security case nodes in the graph.
pub const SECURITY_CASE_TYPE: &str = "HodeiSecurityCase";

/// How many times an open may lose the write race before giving up.
const MAX_ATTEMPTS: usize = 8;

/// The cases of security of ONE account, in Dgraph.
pub struct DgraphSecurityCaseStore {
    client: Arc<GraphClient>,
    tenant: Hrn,
}

impl DgraphSecurityCaseStore {
    /// The store of the cases of an account, over a connected client.
    pub fn new(client: Arc<GraphClient>, tenant: Hrn) -> Self {
        Self { client, tenant }
    }

    /// The identity key of one case: the account is half of it, so the
    /// same case id of two accounts is two nodes.
    pub fn case_key(tenant: &str, id: &str) -> String {
        format!("{tenant}|{id}")
    }

    /// The case of an identifier, in this account. `None` is a response
    /// (there is no such case); an unreadable store is an error.
    pub async fn get(&self, id: &str) -> Result<Option<SecurityCase>, RiskError> {
        let query = format!(
            "{{ q(func: eq(hodei.case.key, {})) @filter(eq(hodei.case.tenant, {})) \
             {{ hodei.case.record }} }}",
            dql_string(&Self::case_key(&self.tenant.to_string(), id)),
            dql_string(&self.tenant.to_string()),
        );
        let mut cases = self.read_cases(&query).await?;
        Ok(if cases.is_empty() {
            None
        } else {
            Some(cases.remove(0))
        })
    }

    /// The cases of a trigger, in this account.
    pub async fn of_trigger(&self, trigger: &str) -> Result<Vec<SecurityCase>, RiskError> {
        let query = format!(
            "{{ q(func: eq(hodei.case.trigger, {})) @filter(eq(hodei.case.tenant, {})) \
             {{ hodei.case.record }} }}",
            dql_string(trigger),
            dql_string(&self.tenant.to_string()),
        );
        let mut cases = self.read_cases(&query).await?;
        cases.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(cases)
    }

    /// Runs a read of case records and decodes every row: a row that
    /// cannot be decoded is an error, never a case that silently vanishes.
    async fn read_cases(&self, query: &str) -> Result<Vec<SecurityCase>, RiskError> {
        let response = self
            .client
            .execute_dql(query)
            .await
            .map_err(|error| RiskError::Unavailable(error.to_string()))?;
        let rows = response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        rows.iter()
            .map(|row| {
                let record = row
                    .get("hodei.case.record")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        RiskError::Unavailable("a case node has no record".to_owned())
                    })?;
                serde_json::from_str::<SecurityCase>(record).map_err(|error| {
                    RiskError::Unavailable(format!("a case record cannot be read: {error}"))
                })
            })
            .collect()
    }

    /// Refuses a blast radius that names a subject outside the account.
    fn check_subjects(&self, impacted: &[ImpactedSubject]) -> Result<(), String> {
        for one in impacted {
            match Hrn::from_string(&one.subject) {
                Some(hrn) if hrn.account_id() == self.tenant.account_id() => {}
                Some(_) => {
                    return Err(format!(
                        "the subject '{}' belongs to another account than the case store",
                        one.subject
                    ));
                }
                None => {
                    return Err(format!(
                        "the subject '{}' is not an HRN of the account",
                        one.subject
                    ));
                }
            }
        }
        Ok(())
    }

    /// The nquads of a new case node.
    fn case_nquads(&self, case: &SecurityCase) -> Result<String, String> {
        let record = serde_json::to_string(case).map_err(|error| error.to_string())?;
        let tenant = self.tenant.to_string();
        let node = |predicate: &str, value: &str| {
            format!("_:case <{predicate}> {} .\n", dql_string(value))
        };
        let mut nquads = format!("_:case <dgraph.type> \"{SECURITY_CASE_TYPE}\" .\n");
        nquads.push_str(&node("hodei.case.key", &Self::case_key(&tenant, &case.id)));
        nquads.push_str(&node("hodei.case.tenant", &tenant));
        nquads.push_str(&node("hodei.case.trigger", &case.trigger));
        nquads.push_str(&node("hodei.case.record", &record));
        nquads.push_str(&node("hodei.case.opened_at", &case.opened_at.to_rfc3339()));
        Ok(nquads)
    }
}

#[async_trait]
impl SecurityCaseSink for DgraphSecurityCaseStore {
    async fn open_or_update(
        &self,
        advisory: &str,
        impacted: &[ImpactedSubject],
        now: DateTime<Utc>,
    ) -> Result<String, String> {
        self.check_subjects(impacted)?;

        // The case is opened ONCE: a case that already exists is the case
        // that is returned, because a case opened twice for the same
        // thing is two cases and nobody owns either.
        let id = SecurityCase::id_for(advisory);
        if let Some(existing) = self.get(&id).await.map_err(|error| error.to_string())? {
            return Ok(existing.id);
        }

        let radius = BlastRadius {
            affected_subjects: impacted.iter().map(|one| one.subject.clone()).collect(),
            affected_deployments: impacted
                .iter()
                .filter_map(|one| one.deployment.clone())
                .collect(),
            affected_components: Vec::new(),
            depths: impacted
                .iter()
                .map(|one| SubjectDepth {
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
            VerificationCriteria::new(None),
            now,
        );

        // Create-if-absent, decided by the server: the key is `@upsert`,
        // so two reactions racing for the same case meet there, and the
        // loser retries on top of whatever committed (and finds it).
        let key = Self::case_key(&self.tenant.to_string(), &id);
        let query = format!(
            "{{\n  existing as var(func: eq(hodei.case.key, {}))\n}}\n",
            dql_string(&key)
        );
        let blocks = vec![GraphMutationBlock {
            cond: Some("eq(len(existing), 0)".to_owned()),
            set_nquads: self.case_nquads(&case)?,
            delete_nquads: String::new(),
        }];
        let mut committed = false;
        for attempt in 0..MAX_ATTEMPTS {
            match self
                .client
                .upsert(GraphUpsertTransaction {
                    query: query.clone(),
                    blocks: blocks.clone(),
                    commit_now: true,
                })
                .await
            {
                Ok(_) => {
                    committed = true;
                    break;
                }
                Err(GraphClientError::UpsertConflict { detail }) if attempt + 1 < MAX_ATTEMPTS => {
                    tracing::debug!(attempt, %detail, "the case write conflicted; revalidating");
                    tokio::time::sleep(std::time::Duration::from_millis(
                        10u64 << attempt.min(6) as u32,
                    ))
                    .await;
                }
                Err(error) => return Err(error.to_string()),
            }
        }
        if !committed {
            return Err(format!(
                "the case write did not commit after {MAX_ATTEMPTS} attempts"
            ));
        }

        // The answer is what the store holds, not what was attempted: a
        // case that is not there after the write is an error.
        match self.get(&id).await.map_err(|error| error.to_string())? {
            Some(stored) => Ok(stored.id),
            None => Err(format!(
                "the case '{id}' is not in the store after the write"
            )),
        }
    }
}
