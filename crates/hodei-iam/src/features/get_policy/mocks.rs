//! Mock implementations for testing Get Policy feature

use async_trait::async_trait;
use kernel::Hrn;
use std::collections::HashMap;

use super::dto::PolicyView;
use super::error::GetPolicyError;
use super::ports::PolicyReader;

/// Mock PolicyReader for testing
pub struct MockPolicyReader {
    policies: HashMap<String, PolicyView>,
}

impl MockPolicyReader {
    /// Create a new empty mock reader
    pub fn empty() -> Self {
        Self {
            policies: HashMap::new(),
        }
    }

    /// Create a mock reader with a single policy
    pub fn with_policy(policy: PolicyView) -> Self {
        let mut policies = HashMap::new();
        policies.insert(policy.hrn.to_string(), policy);
        Self { policies }
    }

    /// Create a mock reader with multiple policies
    pub fn with_policies(policies: Vec<PolicyView>) -> Self {
        let mut map = HashMap::new();
        for policy in policies {
            map.insert(policy.hrn.to_string(), policy);
        }
        Self { policies: map }
    }
}

#[async_trait]
impl PolicyReader for MockPolicyReader {
    async fn get_by_hrn(&self, hrn: &Hrn) -> Result<PolicyView, GetPolicyError> {
        self.policies
            .get(&hrn.to_string())
            .cloned()
            .ok_or_else(|| GetPolicyError::PolicyNotFound(hrn.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_empty_mock() {
        let reader = MockPolicyReader::empty();
        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123".to_string(),
            "Policy".to_string(),
            "test".to_string(),
        );
        let result = reader.get_by_hrn(&hrn).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_with_policy() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123".to_string(),
            "Policy".to_string(),
            "test".to_string(),
        );
        let policy = PolicyView {
            hrn: hrn.clone(),
            name: "Test".to_string(),
            content: "permit(principal, action, resource);".to_string(),
            description: None,
        };
        let reader = MockPolicyReader::with_policy(policy.clone());
        let result = reader.get_by_hrn(&hrn).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name, "Test");
    }
}

