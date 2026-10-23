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
use kernel::domain::policy::{HodeiPolicy, PolicyId};

/// Intermediate structure for deserializing HodeiPolicy from SurrealDB
#[derive(Debug, Clone, Deserialize)]
struct HodeiPolicyDbRow {
    /// The SurrealDB Thing ID (will be converted to PolicyId)
    id: surrealdb::sql::Thing,
    /// The policy content
    content: String,
}

/// Intermediate structure for reading back the content of a policy record.
///
/// The record `id` is deliberately absent: SurrealDB returns it as a Thing
/// value on writes, which does not deserialize into a record id.
#[derive(Debug, Clone, Deserialize)]
struct HodeiPolicyContentRow {
    /// The policy content
    content: String,
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

/// SurrealDB adapter for Policy persistence operations
pub struct SurrealPolicyAdapter<C: surrealdb::Connection> {
    db: Arc<Surreal<C>>,
}

impl<C: surrealdb::Connection> SurrealPolicyAdapter<C> {
    /// Create a new SurrealPolicyAdapter
    pub fn new(db: Arc<Surreal<C>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl<C: surrealdb::Connection> CreatePolicyPort for SurrealPolicyAdapter<C> {
    async fn create(&self, command: CreatePolicyCommand) -> Result<HodeiPolicy, CreatePolicyError> {
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

        let content_value = serde_json::json!({
            "content": policy.content()
        });

        let created: Result<Option<HodeiPolicyDbRow>, surrealdb::Error> = self
            .db
            .create((policy_table, policy_id))
            .content(content_value)
            .await;

        match created {
            Ok(Some(db_row)) => {
                let created_policy = HodeiPolicy::from(db_row);
                info!("Policy created successfully");
                Ok(created_policy)
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
                let policy = HodeiPolicy::from(db_row);
                debug!("Policy found: {}", hrn);
                Ok(GetPolicyView {
                    hrn: hrn.clone(),
                    name: policy.id().to_string(),
                    content: policy.content().to_string(),
                    description: None, // HodeiPolicy doesn't have description field
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
                let query = match command.policy_content {
                    Some(_) => "UPDATE $policy SET content = $content RETURN content",
                    None => "UPDATE $policy RETURN content",
                };

                let mut response = self
                    .db
                    .query(query)
                    .bind((
                        "policy",
                        surrealdb::sql::Thing::from((policy_table, policy_id.as_str())),
                    ))
                    .bind(("content", command.policy_content))
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
