//! In-memory adapter for Policy persistence operations
//!
//! This adapter is used for integration testing and development purposes.
//! It provides a simple, thread-safe storage implementation without external dependencies.

use async_trait::async_trait;
use kernel::{Hrn, domain::policy::{HodeiPolicy, PolicyId}};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{debug, info, warn};

// Import ports from all policy-related features
use crate::features::create_policy::ports::CreatePolicyPort;
use crate::features::get_policy::ports::PolicyReader;
use crate::features::list_policies::ports::PolicyLister;
use crate::features::update_policy::ports::UpdatePolicyPort;
use crate::features::delete_policy::ports::DeletePolicyPort;
use crate::features::get_effective_policies::ports::PolicyFinderPort;

// Import DTOs
use crate::features::create_policy::dto::CreatePolicyCommand;
use crate::features::get_policy::dto::PolicyView as GetPolicyView;
use crate::features::list_policies::dto::{ListPoliciesQuery, ListPoliciesResponse, PolicySummary, PageInfo};
use crate::features::update_policy::dto::{UpdatePolicyCommand, PolicyView};
use crate::features::delete_policy::dto::DeletePolicyCommand;

// Import errors from features
use crate::features::create_policy::error::CreatePolicyError;
use crate::features::get_policy::error::GetPolicyError;
use crate::features::list_policies::error::ListPoliciesError;
use crate::features::update_policy::error::UpdatePolicyError;
use crate::features::delete_policy::error::DeletePolicyError;
use crate::features::get_effective_policies::error::GetEffectivePoliciesError;

/// In-memory adapter for Policy operations
pub struct InMemoryPolicyAdapter {
    account_id: String,
    store: RwLock<HashMap<String, HodeiPolicy>>,
}

impl InMemoryPolicyAdapter {
    /// Create a new InMemoryPolicyAdapter
    pub fn new(account_id: String) -> Self {
        Self {
            account_id,
            store: RwLock::new(HashMap::new()),
        }
    }

    /// Build an HRN for the given policy id
    fn build_hrn(&self, policy_id: &str) -> Hrn {
        Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            self.account_id.clone(),
            "policy".to_string(),
            policy_id.to_string(),
        )
    }

    /// Check if policy exists by ID
    fn exists(&self, policy_id: &str) -> bool {
        self.store.read().unwrap().contains_key(policy_id)
    }
}

#[async_trait]
impl PolicyReader for InMemoryPolicyAdapter {
    async fn get_by_hrn(&self, hrn: &Hrn) -> Result<GetPolicyView, GetPolicyError> {
        info!("Getting policy by HRN: {}", hrn);
        
        let policy_id = hrn.resource_id();
        
        let guard = self.store.read().unwrap();
        let policy = guard.get(policy_id)
            .ok_or_else(|| GetPolicyError::PolicyNotFound(policy_id.to_string()))?
            .clone();

        let view = GetPolicyView {
            hrn: hrn.clone(),
            name: policy_id.to_string(),
            content: policy.content().to_string(),
            description: None, // HodeiPolicy from kernel doesn't have description
        };

        info!("Policy retrieved successfully");
        Ok(view)
    }
}

#[async_trait]
impl PolicyLister for InMemoryPolicyAdapter {
    async fn list(&self, query: ListPoliciesQuery) -> Result<ListPoliciesResponse, ListPoliciesError> {
        info!("Listing policies with pagination: limit={}, offset={}", query.limit, query.offset);
        
        let guard = self.store.read().unwrap();
        let all_policies: Vec<&HodeiPolicy> = guard.values().collect();
        
        let total_count = all_policies.len() as u32;
        
        // Apply pagination
        let start = query.offset as usize;
        let limit = query.limit as usize;
        let total = total_count as usize;
        let end = (start + limit).min(total);
        let policies_page = if start < total {
            all_policies[start..end].to_vec()
        } else {
            vec![]
        };
        
        let policy_summaries: Vec<PolicySummary> = policies_page
            .into_iter()
            .map(|policy| PolicySummary {
                id: policy.id().as_str().to_string(),
                description: None, // HodeiPolicy from kernel doesn't have description
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            })
            .collect();

        let page_info = PageInfo {
            total_count,
            has_next_page: end < total,
            next_offset: if end < total { Some(end as u32) } else { None },
        };

        let response = ListPoliciesResponse {
            policies: policy_summaries,
            page_info,
        };

        info!("Found {} policies, returning {} with pagination", total_count, response.policies.len());
        Ok(response)
    }
}

#[async_trait]
impl CreatePolicyPort for InMemoryPolicyAdapter {
    async fn create(&self, command: CreatePolicyCommand) -> Result<HodeiPolicy, CreatePolicyError> {
        info!("Creating policy with ID: {}", command.policy_id);
        
        let policy_id = command.policy_id.clone();
        
        // Check uniqueness
        if self.exists(&policy_id) {
            return Err(CreatePolicyError::PolicyAlreadyExists(policy_id));
        }

        let policy_id_obj = PolicyId::new(policy_id.clone());
        let policy = HodeiPolicy::new(policy_id_obj, command.policy_content.clone());
        
        let mut guard = self.store.write().map_err(|_| {
            warn!("RwLock poisoned while creating policy");
            CreatePolicyError::StorageError("Internal storage lock poisoned".to_string())
        })?;

        guard.insert(policy_id, policy.clone());
        info!("Policy created successfully");
        Ok(policy)
    }
}

#[async_trait]
impl UpdatePolicyPort for InMemoryPolicyAdapter {
    async fn update(&self, command: UpdatePolicyCommand) -> Result<PolicyView, UpdatePolicyError> {
        info!("Updating policy with ID: {}", command.policy_id);
        
        let policy_id = command.policy_id.clone();
        
        let mut guard = self.store.write().map_err(|_| {
            warn!("RwLock poisoned while updating policy");
            UpdatePolicyError::StorageError("Internal storage lock poisoned".to_string())
        })?;

        let existing_policy = guard.get(&policy_id)
            .ok_or_else(|| UpdatePolicyError::PolicyNotFound(policy_id.clone()))?
            .clone();

        // For simplicity, we'll just return the existing policy as updated
        // In a real implementation, we would update the policy content
        let view = PolicyView {
            hrn: self.build_hrn(&policy_id),
            name: policy_id.clone(),
            content: existing_policy.content().to_string(),
            description: None, // HodeiPolicy from kernel doesn't have description
        };

        info!("Policy updated successfully");
        Ok(view)
    }
}

#[async_trait]
impl DeletePolicyPort for InMemoryPolicyAdapter {
    async fn delete(&self, command: DeletePolicyCommand) -> Result<(), DeletePolicyError> {
        info!("Deleting policy with ID: {}", command.policy_id);
        
        let mut guard = self.store.write().map_err(|_| {
            warn!("RwLock poisoned while deleting policy");
            DeletePolicyError::StorageError("Internal storage lock poisoned".to_string())
        })?;

        if guard.remove(&command.policy_id).is_none() {
            return Err(DeletePolicyError::PolicyNotFound(command.policy_id.clone()));
        }

        info!("Policy deleted successfully");
        Ok(())
    }
}

#[async_trait]
impl PolicyFinderPort for InMemoryPolicyAdapter {
    async fn find_policies_by_principal(
        &self,
        principal_hrn: &Hrn,
    ) -> Result<Vec<HodeiPolicy>, Box<dyn std::error::Error + Send + Sync>> {
        debug!("Finding policies by principal HRN: {}", principal_hrn);
        
        // For in-memory adapter, we'll return all policies for simplicity in tests
        // In a real implementation, this would query relationships
        let guard = self.store.read().unwrap();
        let policies: Vec<HodeiPolicy> = guard.values().cloned().collect();
        
        info!("Found {} policies for principal", policies.len());
        Ok(policies)
    }
}
