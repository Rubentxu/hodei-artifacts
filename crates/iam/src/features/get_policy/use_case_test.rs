// crates/iam/src/features/get_policy/use_case_test.rs

#[cfg(test)]
mod tests {
    use crate::features::get_policy::use_case::GetPolicyUseCase;
    use crate::domain::policy::{Policy, PolicyStatus};
    use crate::features::get_policy::dto::{GetPolicyQuery, GetPolicyResponse};
    use crate::features::get_policy::ports::PolicyReader;
    use crate::infrastructure::errors::IamError;
    use crate::test_utils::{create_test_policy, policy_id};
    use async_trait::async_trait;
    use shared::hrn::PolicyId;
    use std::sync::Arc;

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

    fn create_test_query() -> GetPolicyQuery {
        let policy_id = policy_id();
        GetPolicyQuery::new(policy_id)
    }

    #[tokio::test]
    async fn test_get_policy_success() {
        // Arrange
        let test_policy = create_test_policy();
        let reader = Arc::new(MockPolicyReader::with_policy(test_policy.clone()));
        let use_case = GetPolicyUseCase::new(reader);
        let query = create_test_query();

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policy.id, test_policy.id);
        assert_eq!(response.policy.name, test_policy.name);
        assert_eq!(response.policy.status, PolicyStatus::Active);
        assert_eq!(response.policy.metadata.version, 1);
    }

    #[tokio::test]
    async fn test_get_policy_not_found() {
        // Arrange
        let reader = Arc::new(MockPolicyReader::new());
        let use_case = GetPolicyUseCase::new(reader);
        let query = create_test_query();

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            IamError::PolicyNotFound(id) => {
                assert_eq!(id, policy_id());
            }
            _ => panic!("Expected PolicyNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_get_policy_database_failure() {
        // Arrange
        let reader = Arc::new(MockPolicyReader::with_failure());
        let use_case = GetPolicyUseCase::new(reader);
        let query = create_test_query();

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            IamError::DatabaseError(msg) => {
                assert_eq!(msg, "Mock database error");
            }
            _ => panic!("Expected database error"),
        }
    }

    #[tokio::test]
    async fn test_get_policy_query_validation() {
        // Test valid query
        let query = create_test_query();
        assert!(query.validate().is_ok());
    }

    #[tokio::test]
    async fn test_get_policy_response_creation() {
        // Arrange
        let test_policy = create_test_policy();

        // Act
        let response = GetPolicyResponse::new(test_policy.clone());

        // Assert
        assert_eq!(response.policy.id, test_policy.id);
        assert_eq!(response.policy.name, test_policy.name);
    }
}