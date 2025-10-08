//! In-memory adapter for PolicyLister port
//!
//! This adapter is used for integration testing and development purposes.
//! It provides a simple, thread-safe storage implementation without external dependencies.

use async_trait::async_trait;
use kernel::{Hrn, domain::policy::{HodeiPolicy, PolicyId}};
use std::collections::HashMap;
use std::sync::RwLock;
use tracing::{debug, info, warn};

// Import the port trait
use crate::features::list_policies::ports::PolicyLister;
use crate::features::list_policies::dto::{ListPoliciesQuery, ListPoliciesResponse, PolicySummary, PageInfo};
use crate::features::list_policies::error::ListPoliciesError;

/// In-memory adapter for PolicyLister port
pub struct InMemoryPolicyListerAdapter {
    store: RwLock<HashMap<String, HodeiPolicy>>,
}

impl InMemoryPolicyListerAdapter {
    /// Create a new InMemoryPolicyListerAdapter
    pub fn new() -> Self {
        Self {
            store: RwLock::new(HashMap::new()),
        }
    }

    /// Add a policy to the store (for testing purposes)
    pub fn add_policy(&self, policy_id: String, content: String) {
        let policy_id_obj = PolicyId::new(policy_id.clone());
        let policy = HodeiPolicy::new(policy_id_obj, content);
        
        let mut guard = self.store.write().unwrap();
        guard.insert(policy_id, policy);
    }

    /// Add multiple policies to the store (for testing purposes)
    pub fn add_policies(&self, policies: Vec<(String, String)>) {
        let mut guard = self.store.write().unwrap();
        for (policy_id, content) in policies {
            let policy_id_obj = PolicyId::new(policy_id.clone());
            let policy = HodeiPolicy::new(policy_id_obj, content);
            guard.insert(policy_id, policy);
        }
    }
}

#[async_trait]
impl PolicyLister for InMemoryPolicyListerAdapter {
    async fn list(&self, query: ListPoliciesQuery) -> Result<ListPoliciesResponse, ListPoliciesError> {
        info!("Listing policies with pagination: limit={}, offset={}", query.limit, query.offset);
        
        let guard = self.store.read().unwrap();
        let all_policies: Vec<&HodeiPolicy> = guard.values().collect();
        
        let total_count = all_policies.len();
        
        // Apply pagination
        let start = query.offset as usize;
        let limit = query.limit as usize;
        let end = (start + limit).min(total_count);
        let policies_page = if start < total_count {
            all_policies[start..end].to_vec()
        } else {
            vec![]
        };
        
        let now = chrono::Utc::now();
        let policy_summaries: Vec<PolicySummary> = policies_page
            .into_iter()
            .map(|policy| PolicySummary {
                id: policy.id().as_str().to_string(),
                description: None, // HodeiPolicy from kernel doesn't have description
                created_at: now,
                updated_at: now,
            })
            .collect();

        let next_offset = if end < total_count { Some(end as u32) } else { None };
        let page_info = PageInfo {
            total_count: total_count as u32,
            has_next_page: end < total_count,
            next_offset,
        };

        let response = ListPoliciesResponse {
            policies: policy_summaries,
            page_info,
        };

        info!("Found {} policies, returning {} with pagination", total_count, response.policies.len());
        Ok(response)
    }
}
