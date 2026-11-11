//! SurrealDB adapter for Policy persistence operations
//!
//! This adapter implements all policy-related ports for the IAM system:
//! - CreatePolicyPort: Create new policies
//! - PolicyReader: Get policies by HRN
//! - PolicyLister: List policies with pagination
//! - UpdatePolicyPort: Update existing policies
//! - DeletePolicyPort: Delete policies

use async_trait::async_trait;
use kernel::Hrn;
use serde::Deserialize;
use std::sync::Arc;
use surrealdb::Surreal;
use tracing::{debug, error, info, warn};

// Import the ports from features
use crate::features::create_policy::ports::CreatePolicyPort;
use crate::features::delete_policy::ports::DeletePolicyPort;
use crate::features::evaluate_iam_policies::ports::{
    PolicyFinderError, PolicyFinderPort as IamPolicyFinderPort,
};
use crate::features::get_effective_policies::ports::PolicyFinderPort;
use crate::features::get_policy::ports::PolicyReader;
use crate::features::list_policies::ports::PolicyLister;
use crate::features::update_policy::ports::UpdatePolicyPort;

// Import DTOs and errors from features
use crate::features::create_policy::dto::CreatePolicyCommand;
use crate::features::create_policy::error::CreatePolicyError;

use crate::features::delete_policy::error::DeletePolicyError;
use crate::features::get_effective_policies::error::GetEffectivePoliciesError;
use crate::features::get_policy::dto::PolicyView as GetPolicyView;
use crate::features::get_policy::error::GetPolicyError;
use crate::features::list_policies::dto::{ListPoliciesQuery, ListPoliciesResponse, PolicySummary};
use crate::features::list_policies::error::ListPoliciesError;
use crate::features::update_policy::dto::{PolicyView as UpdatePolicyView, UpdatePolicyCommand};
use crate::features::update_policy::error::UpdatePolicyError;

// Import internal domain entities

// Import kernel policy types
use kernel::domain::policy::{HodeiPolicy, HodeiPolicySet, PolicyId};

/// Intermediate structure for deserializing HodeiPolicy from SurrealDB
#[derive(Debug, Clone, Deserialize)]
struct HodeiPolicyDbRow {
    /// The SurrealDB Thing ID (will be converted to PolicyId)
    id: surrealdb::sql::Thing,
    /// The policy content
    content: String,
    /// The canonical HRNs of the principals the policy is attached to.
    ///
    /// The field is optional so that a record written before the attachment
    /// existed — or one written without an attachment — still reads back
    /// instead of failing the whole query.
    #[serde(default)]
    attached_principals: Vec<String>,
    /// When the policy was created.
    ///
    /// Optional because a row written before the marks existed still has to
    /// read back: reporting no mark is honest, reporting the clock of the
    /// process would not be.
    #[serde(default)]
    created_at: Option<String>,
    /// When the policy was last updated.
    #[serde(default)]
    updated_at: Option<String>,
}

/// Intermediate structure for reading back the content of a policy record.
///
/// The record `id` is deliberately absent: SurrealDB returns it as a Thing
/// value on writes, which does not deserialize into a record id.
#[derive(Debug, Clone, Deserialize)]
struct HodeiPolicyContentRow {
    /// The policy content
    content: String,
    /// When the policy was created.
    #[serde(default)]
    created_at: Option<String>,
    /// When the policy was last updated.
    #[serde(default)]
    updated_at: Option<String>,
}

impl From<HodeiPolicyDbRow> for HodeiPolicy {
    fn from(row: HodeiPolicyDbRow) -> Self {
        let policy_id = PolicyId::new(short_id(&row.id));
        HodeiPolicy::new(policy_id, row.content)
    }
}

/// Extracts the short identifier of a SurrealDB record id.
///
/// `surrealdb::sql::Id` renders a string id in SurrealQL literal form
/// (`` `alice` ``), so `Id::to_string()` produced identifiers such as
/// `` `my-policy` `` that no longer matched the id the caller supplied — and
/// therefore HRNs that could not be resolved back to the stored record.
fn short_id(thing: &surrealdb::sql::Thing) -> String {
    match &thing.id {
        surrealdb::sql::Id::String(value) => value.clone(),
        surrealdb::sql::Id::Number(value) => value.to_string(),
        other => other.to_string(),
    }
}

/// The spelling a timestamp is stored with.
///
/// It is the spelling the responses use, so what a response says is
/// character for character what the database holds.
///
/// The sub-second digits are the ones `chrono` itself writes when it
/// serialises a timestamp -- `AutoSi`, which drops the trailing zeros -- and
/// not a fixed count: a stored mark with nine digits and a response with six
/// are the same instant written differently, and a comparison of the two
/// strings would fail once every thousand marks.
fn mark(instant: chrono::DateTime<chrono::Utc>) -> String {
    instant.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
}

/// The mark of a stored timestamp, as the domain type.
///
/// The store keeps the mark as RFC 3339 text, which reads back the same
/// instants it was written with.
fn parse_mark(value: Option<String>) -> Option<chrono::DateTime<chrono::Utc>> {
    let value = value?;
    match chrono::DateTime::parse_from_rfc3339(&value) {
        Ok(mark) => Some(mark.with_timezone(&chrono::Utc)),
        Err(error) => {
            warn!("The stored timestamp could not be read: {}", error);
            None
        }
    }
}

/// SurrealDB adapter for Policy persistence operations
pub struct SurrealPolicyAdapter<C: surrealdb::Connection> {
    db: Arc<Surreal<C>>,
}

impl<C: surrealdb::Connection> SurrealPolicyAdapter<C> {
    /// Create a new SurrealPolicyAdapter
    pub fn new(db: Arc<Surreal<C>>) -> Self {
        Self { db }
    }

    /// Ensure the tenant-scoped index exists. Idempotent.
    ///
    /// Defining an index on the local engine keeps the database directory
    /// locked by the process that defined it, so this cannot run inside the
    /// service: the service would serve correctly but the same process could
    /// not open the database again. It belongs to a command of its own, which
    /// releases the lock when it exits.
    pub async fn initialize_scope(&self) -> Result<(), String> {
        self.db
            .query(
                "DEFINE FIELD IF NOT EXISTS account ON policy TYPE option<string>; \
                 DEFINE INDEX IF NOT EXISTS policy_account ON policy FIELDS account;",
            )
            .await
            .map_err(|e| e.to_string())?
            .check()
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Whether the account index is in place.
    ///
    /// The listing filters, counts and paginates by account inside SurrealDB,
    /// and that query leans on this index. A missing index costs the listing
    /// its speed, not its isolation, so the answer is reported rather than
    /// enforced: a deployment that has not run the command still serves
    /// correctly.
    pub async fn scope_initialized(&self) -> Result<bool, String> {
        let mut response = self
            .db
            .query("INFO FOR TABLE policy")
            .await
            .map_err(|e| e.to_string())?
            .check()
            .map_err(|e| e.to_string())?;
        let info = response
            .take::<Option<TableInfo>>(0)
            .ok()
            .flatten()
            .unwrap_or_default();
        Ok(info.indexes.contains_key(ACCOUNT_INDEX_NAME))
    }
}

/// The name of the index the IAM listing relies on.
pub const ACCOUNT_INDEX_NAME: &str = "policy_account";

/// What the database reports about a table.
#[derive(Default, Deserialize)]
struct TableInfo {
    #[serde(default)]
    indexes: std::collections::BTreeMap<String, String>,
}

#[async_trait]
impl<C: surrealdb::Connection> CreatePolicyPort for SurrealPolicyAdapter<C> {
    async fn create(
        &self,
        command: CreatePolicyCommand,
    ) -> Result<crate::features::create_policy::dto::CreatedPolicy, CreatePolicyError> {
        info!("Creating policy with ID: {}", command.policy_id);

        // Create HRN for the policy.
        //
        // The resource type is spelled `Policy` (PascalCase) to match the domain
        // convention (`User`, `Group`) and `GetPolicyUseCase`, which rejects any
        // HRN whose resource type is not exactly `Policy`. The SurrealDB table
        // name stays `policy` — it is a different concern.
        let policy_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "default".to_string(), // This should come from context
            "Policy".to_string(),
            command.policy_id.clone(),
        );

        // Create the policy entity
        let policy_id = PolicyId::new(command.policy_id.clone());
        let policy: HodeiPolicy = HodeiPolicy::new(policy_id, command.policy_content);

        let policy_table = "policy";
        let policy_id = policy_hrn.resource_id();

        let account = command
            .policy_id
            .split_once('~')
            .map(|(account, _)| account)
            .unwrap_or("default");
        // The store keeps the marks of the policy, so what a response says
        // about the creation is what the database holds.
        let now = chrono::Utc::now();
        let created_at = mark(now);
        let updated_at = created_at.clone();
        let content_value = serde_json::json!({
            "content": policy.content(),
            "attached_principals": command.attached_principals,
            "account": account,
            "created_at": created_at,
            "updated_at": updated_at,
        });

        let created: Result<Option<HodeiPolicyDbRow>, surrealdb::Error> = self
            .db
            .create((policy_table, policy_id))
            .content(content_value)
            .await;

        match created {
            Ok(Some(db_row)) => {
                let created_at = parse_mark(db_row.created_at.clone());
                let updated_at = parse_mark(db_row.updated_at.clone());
                let created_policy = HodeiPolicy::from(db_row);
                info!("Policy created successfully");
                Ok(crate::features::create_policy::dto::CreatedPolicy {
                    policy: created_policy,
                    // The row carries what was written; if it does not, the
                    // marks are absent rather than invented. Falling back to
                    // the clock of the process would put a time in the
                    // response that the database does not hold.
                    created_at,
                    updated_at,
                })
            }
            Ok(None) => {
                error!("Failed to create policy - no policy returned");
                Err(CreatePolicyError::StorageError(
                    "Failed to create policy".to_string(),
                ))
            }
            Err(e) => {
                error!("Database error while creating policy: {}", e);
                // Check if it's a duplicate record error
                if e.to_string().contains("already exists") {
                    Err(CreatePolicyError::PolicyAlreadyExists(command.policy_id))
                } else {
                    Err(CreatePolicyError::StorageError(e.to_string()))
                }
            }
        }
    }
}

#[async_trait]
impl<C: surrealdb::Connection> PolicyReader for SurrealPolicyAdapter<C> {
    async fn get_by_hrn(&self, hrn: &Hrn) -> Result<GetPolicyView, GetPolicyError> {
        info!("Getting policy by HRN: {}", hrn);

        let policy_table = "policy";
        let policy_id = hrn.resource_id();

        let result: Result<Option<HodeiPolicyDbRow>, surrealdb::Error> =
            self.db.select((policy_table, policy_id)).await;

        match result {
            Ok(Some(db_row)) => {
                // The attachment is read before the row is consumed by the
                // conversion into the domain policy.
                let attached_principals = db_row.attached_principals.clone();
                let created_at = parse_mark(db_row.created_at.clone());
                let updated_at = parse_mark(db_row.updated_at.clone());
                let policy = HodeiPolicy::from(db_row);
                debug!("Policy found: {}", hrn);
                Ok(GetPolicyView {
                    hrn: hrn.clone(),
                    name: policy.id().to_string(),
                    content: policy.content().to_string(),
                    description: None, // HodeiPolicy doesn't have description field
                    attached_principals,
                    created_at,
                    updated_at,
                })
            }
            Ok(None) => {
                warn!("Policy not found: {}", hrn);
                Err(GetPolicyError::PolicyNotFound(hrn.to_string()))
            }
            Err(e) => {
                error!("Database error while getting policy: {}", e);
                Err(GetPolicyError::RepositoryError(e.to_string()))
            }
        }
    }
}

#[async_trait]
impl<C: surrealdb::Connection> PolicyLister for SurrealPolicyAdapter<C> {
    async fn list(
        &self,
        query: ListPoliciesQuery,
    ) -> Result<ListPoliciesResponse, ListPoliciesError> {
        info!(
            "Listing policies with limit={}, offset={}",
            query.limit, query.offset
        );

        let limit = query.limit;
        let offset = query.offset;

        // Get total count
        let count_query = "SELECT count() FROM policy GROUP ALL";
        let count_result: Result<Vec<serde_json::Value>, surrealdb::Error> = self
            .db
            .query(count_query)
            .await
            .map_err(|e| ListPoliciesError::RepositoryError(e.to_string()))?
            .take(0);

        let total_count = match count_result {
            Ok(mut results) if !results.is_empty() => results
                .remove(0)
                .get("count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize,
            _ => 0,
        };

        // Get paginated policies
        let policies_query = "SELECT * FROM policy LIMIT $limit START $offset";
        // Deserialize into the DB row shape, not into `HodeiPolicy`: the record
        // `id` is a SurrealDB Thing (`policy:<short_id>`), so deserializing it
        // straight into a `PolicyId` string produced ids like `policy:my-policy`
        // and therefore HRNs whose `resource_id()` no longer matched the stored
        // record — the HRN returned by list could not be used to get, update or
        // delete the policy it referred to.
        let policies_result: Result<Vec<HodeiPolicyDbRow>, surrealdb::Error> = self
            .db
            .query(policies_query)
            .bind(("limit", limit))
            .bind(("offset", offset))
            .await
            .map_err(|e| ListPoliciesError::RepositoryError(e.to_string()))?
            .take(0);

        let policies = match policies_result {
            Ok(rows) => rows
                .into_iter()
                .map(|row| {
                    let policy_short_id = short_id(&row.id);
                    let hrn = Hrn::new(
                        "hodei".to_string(),
                        "iam".to_string(),
                        "default".to_string(),
                        "Policy".to_string(),
                        policy_short_id.clone(),
                    );

                    PolicySummary {
                        hrn: hrn.clone(),
                        name: policy_short_id,
                        description: None, // HodeiPolicy doesn't have description field
                        created_at: parse_mark(row.created_at.clone()),
                        updated_at: parse_mark(row.updated_at.clone()),
                    }
                })
                .collect(),
            Err(e) => {
                error!("Database error while listing policies: {}", e);
                return Err(ListPoliciesError::RepositoryError(e.to_string()));
            }
        };

        let has_next_page = (offset + limit) < total_count;
        let has_previous_page = offset > 0;

        Ok(ListPoliciesResponse {
            policies,
            total_count,
            has_next_page,
            has_previous_page,
        })
    }
    async fn list_for_account(
        &self,
        account: &str,
        query: ListPoliciesQuery,
    ) -> Result<ListPoliciesResponse, ListPoliciesError> {
        let count_query = "SELECT count() FROM policy WHERE account = $account GROUP ALL";
        let counts: Vec<serde_json::Value> = self
            .db
            .query(count_query)
            .bind(("account", account.to_string()))
            .await
            .map_err(|e| ListPoliciesError::RepositoryError(e.to_string()))?
            .take(0)
            .map_err(|e| ListPoliciesError::RepositoryError(e.to_string()))?;
        let total_count = counts
            .first()
            .and_then(|value| value.get("count"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as usize;

        let rows: Vec<HodeiPolicyDbRow> = self
            .db
            .query("SELECT * FROM policy WHERE account = $account ORDER BY id LIMIT $limit START $offset")
            .bind(("account", account.to_string()))
            .bind(("limit", query.limit))
            .bind(("offset", query.offset))
            .await
            .map_err(|e| ListPoliciesError::RepositoryError(e.to_string()))?
            .take(0)
            .map_err(|e| ListPoliciesError::RepositoryError(e.to_string()))?;
        let policies = rows
            .into_iter()
            .map(|row| {
                let short = short_id(&row.id);
                let short = short
                    .split_once('~')
                    .map(|(_, id)| id.to_string())
                    .unwrap_or(short);
                PolicySummary {
                    hrn: Hrn::new(
                        "hodei".to_string(),
                        "iam".to_string(),
                        account.to_string(),
                        "Policy".to_string(),
                        short.clone(),
                    ),
                    name: short,
                    description: None,
                    created_at: parse_mark(row.created_at.clone()),
                    updated_at: parse_mark(row.updated_at.clone()),
                }
            })
            .collect();
        Ok(ListPoliciesResponse {
            policies,
            total_count,
            has_next_page: query.offset + query.limit < total_count,
            has_previous_page: query.offset > 0,
        })
    }
}

#[async_trait]
impl<C: surrealdb::Connection> UpdatePolicyPort for SurrealPolicyAdapter<C> {
    async fn update(
        &self,
        command: UpdatePolicyCommand,
    ) -> Result<UpdatePolicyView, UpdatePolicyError> {
        info!("Updating policy: {}", command.policy_id);

        let policy_table = "policy";
        let policy_id = command.policy_id.clone();

        // First check if policy exists.
        //
        // The row shape is used deliberately: on a single-record `select`
        // SurrealDB returns the `id` as a Thing value, so deserializing the
        // record into `HodeiPolicy` (whose id is a plain string) failed with
        // "expected a string, found Thing" and turned every update into a 500.
        let existing: Result<Option<HodeiPolicyDbRow>, surrealdb::Error> =
            self.db.select((policy_table, policy_id.clone())).await;

        match existing {
            Ok(Some(_)) => {
                // Update the policy and read the new content back.
                //
                // The write is issued as an explicit query with `RETURN content`
                // instead of `db.update(..).merge(..)`: the update response
                // carries the record `id` as a SurrealDB Thing value, and a
                // Thing cannot be deserialized into a Rust record id — the
                // whole response failed to deserialize, turning every update
                // into a 500. Returning only the content sidesteps the record
                // id entirely.
                //
                // `policy_content: None` means "keep the current content", so
                // the SET clause is only emitted when new content is given.
                //
                // The creation mark is not in the SET clause, so the update
                // leaves it alone; the update mark is set to the moment of the
                // write.
                let query = match command.policy_content {
                    Some(_) => {
                        "UPDATE $policy SET content = $content, updated_at = $updated_at \
                         RETURN content, created_at, updated_at"
                    }
                    None => {
                        "UPDATE $policy SET updated_at = $updated_at \
                         RETURN content, created_at, updated_at"
                    }
                };
                let updated_at = mark(chrono::Utc::now());

                let mut response = self
                    .db
                    .query(query)
                    .bind((
                        "policy",
                        surrealdb::sql::Thing::from((policy_table, policy_id.as_str())),
                    ))
                    .bind(("content", command.policy_content))
                    .bind(("updated_at", updated_at))
                    .await
                    .map_err(|e| {
                        error!("Database error while updating policy: {}", e);
                        UpdatePolicyError::StorageError(e.to_string())
                    })?;

                let updated: Vec<HodeiPolicyContentRow> = response.take(0).map_err(|e| {
                    error!("Database error while reading the updated policy: {}", e);
                    UpdatePolicyError::StorageError(e.to_string())
                })?;

                match updated.into_iter().next() {
                    Some(row) => {
                        let policy_short_id = policy_id;
                        let hrn = Hrn::new(
                            "hodei".to_string(),
                            "iam".to_string(),
                            "default".to_string(),
                            "Policy".to_string(),
                            policy_short_id.clone(),
                        );
                        info!("Policy updated successfully: {}", hrn);
                        Ok(UpdatePolicyView {
                            hrn,
                            name: policy_short_id,
                            content: row.content,
                            description: None, // HodeiPolicy doesn't have description field
                            created_at: parse_mark(row.created_at.clone()),
                            updated_at: parse_mark(row.updated_at.clone()),
                        })
                    }
                    None => {
                        error!("Failed to update policy - no policy returned");
                        Err(UpdatePolicyError::StorageError(
                            "Failed to update policy".to_string(),
                        ))
                    }
                }
            }
            Ok(None) => {
                warn!("Policy not found for update: {}", command.policy_id);
                Err(UpdatePolicyError::PolicyNotFound(command.policy_id))
            }
            Err(e) => {
                error!("Database error while checking policy existence: {}", e);
                Err(UpdatePolicyError::StorageError(e.to_string()))
            }
        }
    }
}

#[async_trait]
impl<C: surrealdb::Connection> DeletePolicyPort for SurrealPolicyAdapter<C> {
    async fn delete(&self, policy_id: &str) -> Result<(), DeletePolicyError> {
        info!("Deleting policy: {}", policy_id);

        let policy_table = "policy";
        let _policy_table = "policy";

        // The row shape is used deliberately: the delete response carries the
        // record `id` as a Thing value, which does not deserialize into the
        // plain string id of `HodeiPolicy`.
        let deleted: Result<Option<HodeiPolicyDbRow>, surrealdb::Error> =
            self.db.delete((policy_table, policy_id)).await;

        match deleted {
            Ok(Some(_)) => {
                info!("Policy deleted successfully: {}", policy_id);
                Ok(())
            }
            Ok(None) => {
                warn!("Policy not found for deletion: {}", policy_id);
                Err(DeletePolicyError::PolicyNotFound(policy_id.to_string()))
            }
            Err(e) => {
                error!("Database error while deleting policy: {}", e);
                Err(DeletePolicyError::StorageError(e.to_string()))
            }
        }
    }
}

#[async_trait]
impl<C: surrealdb::Connection> PolicyFinderPort for SurrealPolicyAdapter<C> {
    async fn find_policies_by_principal(
        &self,
        principal_hrn: &Hrn,
    ) -> Result<Vec<HodeiPolicy>, GetEffectivePoliciesError> {
        debug!("Finding policies for principal: {}", principal_hrn);

        // This is a graph query in SurrealDB - find all policies attached to the principal
        let query = "SELECT * FROM policy WHERE $principal_hrn IN attached_principals";

        let mut result = self
            .db
            .query(query)
            .bind(("principal_hrn", principal_hrn.to_string()))
            .await
            .map_err(|e| GetEffectivePoliciesError::RepositoryError(e.to_string()))?;

        let policies: Vec<surrealdb::sql::Object> = result
            .take(0)
            .map_err(|e| GetEffectivePoliciesError::RepositoryError(e.to_string()))?;

        // Convert surreal objects to HodeiPolicy
        let mut hodei_policies = Vec::new();
        for policy_obj in policies {
            // Extract and clean the id and content values separately to avoid borrowing issues
            let id_opt = policy_obj.get("id").map(|v| {
                let s = v.to_string();
                s.strip_prefix('\"')
                    .unwrap_or(&s)
                    .strip_suffix('\"')
                    .unwrap_or(&s)
                    .to_string()
            });
            let content_opt = policy_obj.get("content").map(|v| {
                let s = v.to_string();
                s.strip_prefix('\"')
                    .unwrap_or(&s)
                    .strip_suffix('\"')
                    .unwrap_or(&s)
                    .to_string()
            });

            if let (Some(id), Some(content)) = (id_opt, content_opt) {
                let policy_id = PolicyId::new(id);
                let hodei_policy = HodeiPolicy::new(policy_id, content);
                hodei_policies.push(hodei_policy);
            }
        }

        info!("Found {} policies for principal", hodei_policies.len());
        Ok(hodei_policies)
    }
}

/// The IAM evaluation's policy finder, backed by the same store.
///
/// The policies that apply to a principal are the ones **attached** to it.
/// The query binds the canonical HRN of the principal and matches it against
/// the `attached_principals` field of every stored policy, which is the field
/// the create path writes.
///
/// The policies that come back are exactly the ones the evaluation consults:
/// a policy that is not attached to the principal is not evaluated for it, so
/// it cannot grant anything either.
#[async_trait]
impl<C: surrealdb::Connection> IamPolicyFinderPort for SurrealPolicyAdapter<C> {
    async fn get_effective_policies(
        &self,
        principal_hrn: &Hrn,
    ) -> Result<HodeiPolicySet, PolicyFinderError> {
        debug!("Finding effective IAM policies for: {}", principal_hrn);

        let query = "SELECT * FROM policy WHERE $principal_hrn IN attached_principals";

        let mut response = self
            .db
            .query(query)
            .bind(("principal_hrn", principal_hrn.to_string()))
            .await
            .map_err(|e| PolicyFinderError::RepositoryError(e.to_string()))?;

        let rows: Vec<HodeiPolicyDbRow> = response
            .take(0)
            .map_err(|e| PolicyFinderError::RepositoryError(e.to_string()))?;

        let policies: Vec<HodeiPolicy> = rows
            .into_iter()
            .map(|row| {
                let policy_id = PolicyId::new(short_id(&row.id));
                HodeiPolicy::new(policy_id, row.content)
            })
            .collect();

        let policy_set = HodeiPolicySet::new(policies);
        info!(
            "Found {} effective IAM policies for: {}",
            policy_set.len(),
            principal_hrn
        );
        Ok(policy_set)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stored mark and the response have to be the same text.
    ///
    /// The response serialises the timestamp it read; the store holds the
    /// text it wrote. If the two spellings differ, a response can say
    /// something the database does not hold -- and a test that compares them
    /// fails once every thousand marks, which is the worst kind of failure:
    /// it looks like a broken deployment and it is a broken format.
    #[test]
    fn the_stored_mark_is_the_text_the_response_says() {
        let awkward = [
            "2023-11-14T22:13:20.000000000Z",
            "2023-11-14T22:13:20.123456000Z",
            "2023-11-14T22:13:20.123456789Z",
            "2023-11-14T22:13:20.000001000Z",
            "2023-11-14T22:13:20.100000000Z",
        ];
        for text in awkward {
            let instant = chrono::DateTime::parse_from_rfc3339(text)
                .expect("a timestamp")
                .with_timezone(&chrono::Utc);

            let stored = mark(instant);
            let said = serde_json::to_string(&instant).expect("a serialised timestamp");
            let said = said.trim_matches('"');

            assert_eq!(
                stored, said,
                "the stored mark and the response must be the same text for {text}"
            );
            // And it reads back as the same instant.
            assert_eq!(parse_mark(Some(stored)), Some(instant));
        }
    }
}
