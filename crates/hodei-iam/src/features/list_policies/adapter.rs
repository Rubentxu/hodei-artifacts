//! Infrastructure adapters for List Policies feature

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::RwLock;

use super::dto::{ListPoliciesQuery, ListPoliciesResponse, PageInfo, PolicySummary};
use super::error::ListPoliciesError;
use super::ports::PolicyLister;

pub struct InMemoryPolicyLister {
    policies: RwLock<HashMap<String, PolicySummary>>,
}

impl InMemoryPolicyLister {
    pub fn new() -> Self {
        Self {
            policies: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_policies(policies: Vec<PolicySummary>) -> Self {
        let mut map = HashMap::new();
        for policy in policies {
            map.insert(policy.hrn.to_string(), policy);
        }
        Self {
            policies: RwLock::new(map),
        }
    }

    pub fn add_policy(&self, policy: PolicySummary) {
        let mut policies = self.policies.write().unwrap();
        policies.insert(policy.hrn.to_string(), policy);
    }

    pub fn policy_count(&self) -> usize {
        let policies = self.policies.read().unwrap();
        policies.len()
    }

    pub fn clear(&self) {
        let mut policies = self.policies.write().unwrap();
        policies.clear();
    }
}

impl Default for InMemoryPolicyLister {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PolicyLister for InMemoryPolicyLister {
    async fn list(
        &self,
        query: ListPoliciesQuery,
    ) -> Result<ListPoliciesResponse, ListPoliciesError> {
        let policies = self
            .policies
            .read()
            .map_err(|_| ListPoliciesError::RepositoryError("Lock poisoned".to_string()))?;

        let total_count = policies.len() as u64;
        let limit = query.effective_limit() as usize;
        let offset = query.effective_offset() as usize;

        let mut all_policies: Vec<PolicySummary> = policies.values().cloned().collect();
        all_policies.sort_by(|a, b| a.name.cmp(&b.name));

        let page_policies: Vec<PolicySummary> = all_policies
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect();

        let actual_count = page_policies.len();
        let page_info = PageInfo::from_query(&query, total_count, actual_count);

        Ok(ListPoliciesResponse::new(page_policies, page_info))
    }
}

