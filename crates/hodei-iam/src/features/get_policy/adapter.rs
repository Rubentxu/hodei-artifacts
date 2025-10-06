//! Infrastructure adapters for Get Policy feature
//!
//! This module contains in-memory implementations for testing and demonstration.
//! Production implementations should use real database connections.

use async_trait::async_trait;
use kernel::Hrn;
use std::collections::HashMap;
use std::sync::RwLock;

use super::dto::PolicyView;
use super::error::GetPolicyError;
use super::ports::PolicyReader;

/// In-memory adapter for PolicyReader
///
/// This is a simple in-memory implementation for testing purposes.
/// It stores policies in a HashMap protected by a RwLock.
///
/// # Thread Safety
///
/// This adapter is thread-safe and can be shared across threads using `Arc`.
pub struct InMemoryPolicyReader {
    policies: RwLock<HashMap<String, PolicyView>>,
}

impl InMemoryPolicyReader {
    /// Create a new empty adapter
    pub fn new() -> Self {
        Self {
            policies: RwLock::new(HashMap::new()),
        }
    }

    /// Create an adapter pre-populated with policies
    pub fn with_policies(policies: Vec<PolicyView>) -> Self {
        let mut map = HashMap::new();
        for policy in policies {
            map.insert(policy.hrn.to_string(), policy);
        }
        Self {
            policies: RwLock::new(map),
        }
    }

    /// Add a policy to the adapter (for testing setup)
    pub fn add_policy(&self, policy: PolicyView) {
        let mut policies = self.policies.write().unwrap();
        policies.insert(policy.hrn.to_string(), policy);
    }

    /// Get the number of policies stored
    pub fn policy_count(&self) -> usize {
        let policies = self.policies.read().unwrap();
        policies.len()
    }

    /// Check if a policy exists by HRN
    pub fn has_policy(&self, hrn: &Hrn) -> bool {
        let policies = self.policies.read().unwrap();
        policies.contains_key(&hrn.to_string())
    }
}

impl Default for InMemoryPolicyReader {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PolicyReader for InMemoryPolicyReader {
    async fn get_by_hrn(&self, hrn: &Hrn) -> Result<PolicyView, GetPolicyError> {
        let policies = self.policies.read().map_err(|_| {
            GetPolicyError::RepositoryError("Lock poisoned".to_string())
        })?;

        policies
            .get(&hrn.to_string())
            .cloned()
            .ok_or_else(|| GetPolicyError::PolicyNotFound(hrn.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_policy(id: &str) -> PolicyView {
        PolicyView {
            hrn: Hrn::new(
                "aws".to_string(),
                "iam".to_string(),
                "123456789012".to_string(),
                "Policy".to_string(),
                id.to_string(),
            ),
            name: format!("Policy {}", id),
            content: "permit(principal, action, resource);".to_string(),
            description: Some(format!("Test policy {}", id)),
        }
    }

    #[tokio::test]
    async fn test_in_memory_adapter_get_existing_policy() {
        let policy = create_test_policy("test-policy");
        let adapter = InMemoryPolicyReader::new();
        adapter.add_policy(policy.clone());

        let result = adapter.get_by_hrn(&policy.hrn).await;

        assert!(result.is_ok());
        let retrieved = result.unwrap();
        assert_eq!(retrieved.hrn, policy.hrn);
        assert_eq!(retrieved.name, policy.name);
    }

    #[tokio::test]
    async fn test_in_memory_adapter_get_nonexistent_policy() {
        let adapter = InMemoryPolicyReader::new();
        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "Policy".to_string(),
            "nonexistent".to_string(),
        );

        let result = adapter.get_by_hrn(&hrn).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            GetPolicyError::PolicyNotFound(_) => {}
            e => panic!("Expected PolicyNotFound, got: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_in_memory_adapter_with_policies() {
        let policy1 = create_test_policy("policy1");
        let policy2 = create_test_policy("policy2");
        let adapter = InMemoryPolicyReader::with_policies(vec![policy1.clone(), policy2.clone()]);

        assert_eq!(adapter.policy_count(), 2);
        assert!(adapter.has_policy(&policy1.hrn));
        assert!(adapter.has_policy(&policy2.hrn));

        let result1 = adapter.get_by_hrn(&policy1.hrn).await;
        let result2 = adapter.get_by_hrn(&policy2.hrn).await;

        assert!(result1.is_ok());
        assert!(result2.is_ok());
    }

    #[tokio::test]
    async fn test_in_memory_adapter_multiple_get() {
        let policy = create_test_policy("test");
        let adapter = InMemoryPolicyReader::new();
        adapter.add_policy(policy.clone());

        let result1 = adapter.get_by_hrn(&policy.hrn).await;
        let result2 = adapter.get_by_hrn(&policy.hrn).await;
        let result3 = adapter.get_by_hrn(&policy.hrn).await;

        assert!(result1.is_ok());
        assert!(result2.is_ok());
        assert!(result3.is_ok());
    }
}

