// crates/iam/src/features/get_policy/use_case.rs

use crate::features::get_policy::dto::{GetPolicyQuery, GetPolicyResponse};
use crate::features::get_policy::ports::PolicyReader;
use crate::infrastructure::errors::IamError;
use std::sync::Arc;

/// Use case for getting policies by ID
/// Contains pure business logic without infrastructure concerns
pub struct GetPolicyUseCase {
    reader: Arc<dyn PolicyReader>,
}

impl GetPolicyUseCase {
    /// Create a new get policy use case
    pub fn new(reader: Arc<dyn PolicyReader>) -> Self {
        Self { reader }
    }

    /// Execute the get policy use case
    pub async fn execute(&self, query: GetPolicyQuery) -> Result<GetPolicyResponse, IamError> {
        // 1. Validate query
        query.validate()?;

        // 2. Get policy from repository
        let policy = self
            .reader
            .get_by_id(&query.id)
            .await?
            .ok_or_else(|| IamError::PolicyNotFound(query.id.clone()))?;

        // 3. Return response
        Ok(GetPolicyResponse::new(policy))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::policy::{Policy, PolicyMetadata, PolicyStatus};
    use async_trait::async_trait;
    use shared::hrn::{Hrn, PolicyId};
    use time::OffsetDateTime;

    // Mock implementation for testing
    struct MockPolicyReader {
        policy: Option<Policy>,
        should_fail: bool,
    }

    impl MockPolicyReader {
        fn new() -> Self {
            Self {
                policy: None,
                should_fail: false,
            }
        }

        fn with_policy(policy: Policy) -> Self {
            Self {
                policy: Some(policy),
                should_fail: false,
            }
        }

        fn with_failure() -> Self {
            Self {
                policy: None,
                should_fail: true,
            }
        }
    }

    #[async_trait]
    impl PolicyReader for MockPolicyReader {
        async fn get_by_id(&self, _id: &PolicyId) -> Result<Option<Policy>, IamError> {
            if self.should_fail {
                return Err(IamError::DatabaseError("Mock database error".to_string()));
            }
            Ok(self.policy.clone())
        }
    }

    fn create_test_policy() -> Policy {
        let policy_id = PolicyId(
            Hrn::new("hrn:hodei:iam:global:policy/test_policy").expect("Valid HRN"),
        );
        Policy {
            id: policy_id,
            name: "Test Policy".to_string(),
            description: Some("A test policy".to_string()),
            content: "permit(principal, action, resource);".to_string(),
            status: PolicyStatus::Active,
            metadata: PolicyMetadata {
                created_at: OffsetDateTime::now_utc(),
                created_by: "user_123".to_string(),
                updated_at: OffsetDateTime::now_utc(),
                updated_by: "user_123".to_string(),
                version: 1,
                tags: vec!["test".to_string()],
            },
        }
    }

    #[tokio::test]
    async fn test_get_policy_success() {
        let test_policy = create_test_policy();
        let reader = Arc::new(MockPolicyReader::with_policy(test_policy.clone()));
        let use_case = GetPolicyUseCase::new(reader);
        
        let query = GetPolicyQuery::new(test_policy.id.clone());
        let result = use_case.execute(query).await;
        
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policy.id, test_policy.id);
        assert_eq!(response.policy.name, "Test Policy");
        assert_eq!(response.policy.description, Some("A test policy".to_string()));
        assert_eq!(response.policy.status, PolicyStatus::Active);
    }

    #[tokio::test]
    async fn test_get_policy_not_found() {
        let reader = Arc::new(MockPolicyReader::new());
        let use_case = GetPolicyUseCase::new(reader);
        
        let policy_id = PolicyId(
            Hrn::new("hrn:hodei:iam:global:policy/nonexistent").expect("Valid HRN"),
        );
        let query = GetPolicyQuery::new(policy_id.clone());
        let result = use_case.execute(query).await;
        
        assert!(result.is_err());
        match result.unwrap_err() {
            IamError::PolicyNotFound(id) => {
                assert_eq!(id, policy_id);
            }
            _ => panic!("Expected PolicyNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_get_policy_repository_failure() {
        let reader = Arc::new(MockPolicyReader::with_failure());
        let use_case = GetPolicyUseCase::new(reader);
        
        let policy_id = PolicyId(
            Hrn::new("hrn:hodei:iam:global:policy/test").expect("Valid HRN"),
        );
        let query = GetPolicyQuery::new(policy_id);
        let result = use_case.execute(query).await;
        
        assert!(result.is_err());
        match result.unwrap_err() {
            IamError::DatabaseError(msg) => {
                assert_eq!(msg, "Mock database error");
            }
            _ => panic!("Expected DatabaseError"),
        }
    }

    #[test]
    fn test_query_validation() {
        let policy_id = PolicyId(
            Hrn::new("hrn:hodei:iam:global:policy/test").expect("Valid HRN"),
        );
        let query = GetPolicyQuery::new(policy_id);
        
        // Query validation should pass for valid PolicyId
        assert!(query.validate().is_ok());
    }
}