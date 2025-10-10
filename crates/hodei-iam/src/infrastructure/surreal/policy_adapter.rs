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
use crate::features::get_policy::dto::PolicyView as GetPolicyView;
use crate::features::get_policy::error::GetPolicyError;
use crate::features::list_policies::dto::{ListPoliciesQuery, ListPoliciesResponse, PolicySummary};
use crate::features::list_policies::error::ListPoliciesError;
use crate::features::update_policy::dto::{PolicyView as UpdatePolicyView, UpdatePolicyCommand};
use crate::features::update_policy::error::UpdatePolicyError;

// Import internal domain entities

// Import kernel policy types
use kernel::domain::policy::{HodeiPolicy, PolicyId};

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

        // Create HRN for the policy
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

        let created: Result<Option<HodeiPolicy>, surrealdb::Error> = self
            .db
            .create((policy_table, policy_id))
            .content(policy.clone())
            .await;

        match created {
            Ok(Some(created_policy)) => {
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
                Err(CreatePolicyError::StorageError(e.to_string()))
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

        let result: Result<Option<HodeiPolicy>, surrealdb::Error> =
            self.db.select((policy_table, policy_id)).await;

        match result {
            Ok(Some(policy)) => {
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
        let policies_result: Result<Vec<HodeiPolicy>, surrealdb::Error> = self
            .db
            .query(policies_query)
            .bind(("limit", limit))
            .bind(("offset", offset))
            .await
            .map_err(|e| ListPoliciesError::RepositoryError(e.to_string()))?
            .take(0);

        let policies = match policies_result {
            Ok(policies) => policies
                .into_iter()
                .map(|policy| {
                    let hrn = Hrn::new(
                        "hodei".to_string(),
                        "iam".to_string(),
                        "default".to_string(),
                        "Policy".to_string(),
                        policy.id().to_string(),
                    );

                    PolicySummary {
                        hrn: hrn.clone(),
                        name: policy.id().to_string(),
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

        // First check if policy exists
        let existing: Result<Option<HodeiPolicy>, surrealdb::Error> =
            self.db.select((policy_table, policy_id.clone())).await;

        match existing {
            Ok(Some(_)) => {
                // Update the policy
                let updated: Result<Option<HodeiPolicy>, surrealdb::Error> = self
                    .db
                    .update((policy_table, policy_id))
                    .merge(serde_json::json!({
                        "content": command.policy_content,
                    }))
                    .await;

                match updated {
                    Ok(Some(updated_policy)) => {
                        let hrn = Hrn::new(
                            "hodei".to_string(),
                            "iam".to_string(),
                            "default".to_string(),
                            "Policy".to_string(),
                            command.policy_id,
                        );
                        info!("Policy updated successfully: {}", hrn);
                        Ok(UpdatePolicyView {
                            hrn,
                            name: updated_policy.id().to_string(),
                            content: updated_policy.content().to_string(),
                            description: None, // HodeiPolicy doesn't have description field
                        })
                    }
                    Ok(None) => {
                        error!("Failed to update policy - no policy returned");
                        Err(UpdatePolicyError::StorageError(
                            "Failed to update policy".to_string(),
                        ))
                    }
                    Err(e) => {
                        error!("Database error while updating policy: {}", e);
                        Err(UpdatePolicyError::StorageError(e.to_string()))
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

        let deleted: Result<Option<HodeiPolicy>, surrealdb::Error> =
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
    ) -> Result<Vec<HodeiPolicy>, Box<dyn std::error::Error + Send + Sync>> {
        debug!("Finding policies for principal: {}", principal_hrn);

        // This is a graph query in SurrealDB - find all policies attached to the principal
        let query = "SELECT * FROM policy WHERE $principal_hrn IN attached_principals";

        let mut result = self
            .db
            .query(query)
            .bind(("principal_hrn", principal_hrn.to_string()))
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

        let policies: Vec<surrealdb::sql::Object> = result
            .take(0)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

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
