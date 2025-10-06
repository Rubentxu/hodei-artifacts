//! Mock implementations for testing List Policies feature

use async_trait::async_trait;

use super::dto::{ListPoliciesQuery, ListPoliciesResponse, PageInfo, PolicySummary};
use super::error::ListPoliciesError;
use super::ports::PolicyLister;

/// Mock PolicyLister for testing
pub struct MockPolicyLister {
    policies: Vec<PolicySummary>,
    should_fail: bool,
}

impl MockPolicyLister {
    /// Create a new empty mock lister
    pub fn empty() -> Self {
        Self {
            policies: vec![],
            should_fail: false,
        }
    }

    /// Create a mock lister with policies
    pub fn with_policies(policies: Vec<PolicySummary>) -> Self {
        Self {
            policies,
            should_fail: false,
        }
    }

    /// Create a mock lister that returns an error
    pub fn with_error() -> Self {
        Self {
            policies: vec![],
            should_fail: true,
        }
    }
}

#[async_trait]
impl PolicyLister for MockPolicyLister {
    async fn list(
        &self,
        query: ListPoliciesQuery,
    ) -> Result<ListPoliciesResponse, ListPoliciesError> {
        if self.should_fail {
            return Err(ListPoliciesError::RepositoryError(
                "Mock repository error".to_string(),
            ));
        }

        let total_count = self.policies.len() as u64;
        let limit = query.effective_limit() as usize;
        let offset = query.effective_offset() as usize;

        let page_policies: Vec<PolicySummary> = self
            .policies
            .iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect();

        let actual_count = page_policies.len();
        let page_info = PageInfo::from_query(&query, total_count, actual_count);

        Ok(ListPoliciesResponse::new(page_policies, page_info))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::Hrn;

    fn create_test_policy(id: &str) -> PolicySummary {
        PolicySummary {
            hrn: Hrn::new(
                "aws".to_string(),
                "iam".to_string(),
                "123".to_string(),
                "Policy".to_string(),
                id.to_string(),
            ),
            name: format!("Policy {}", id),
            description: None,
        }
    }

    #[tokio::test]
    async fn test_mock_empty() {
        let lister = MockPolicyLister::empty();
        let query = ListPoliciesQuery::default();
        let result = lister.list(query).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.policies.is_empty());
    }

    #[tokio::test]
    async fn test_mock_with_policies() {
        let policies = vec![create_test_policy("p1"), create_test_policy("p2")];
        let lister = MockPolicyLister::with_policies(policies);
        let query = ListPoliciesQuery::default();
        let result = lister.list(query).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policies.len(), 2);
    }

    #[tokio::test]
    async fn test_mock_with_error() {
        let lister = MockPolicyLister::with_error();
        let query = ListPoliciesQuery::default();
        let result = lister.list(query).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ListPoliciesError::RepositoryError(_)
        ));
    }
}

