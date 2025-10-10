//! Unit tests for get_policy use case
//!
//! These tests verify the behavior of the GetPolicyUseCase in isolation,
//! using mocks to simulate external dependencies.

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use kernel::Hrn;

    use crate::features::get_policy::{
        dto::{GetPolicyQuery, PolicyView},
        error::GetPolicyError,
        mocks::MockPolicyReader,
        ports::PolicyReader,
        use_case::GetPolicyUseCase,
    };

    // ============================================================================
    // Helper Functions
    // ============================================================================

    fn create_test_policy_hrn() -> Hrn {
        Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "Policy".to_string(),
            "test-policy".to_string(),
        )
    }

    fn create_test_policy_view() -> PolicyView {
        PolicyView {
            hrn: create_test_policy_hrn(),
            name: "Test Policy".to_string(),
            content: "permit(principal, action, resource);".to_string(),
            description: Some("A test policy for unit testing".to_string()),
        }
    }

    fn create_test_query() -> GetPolicyQuery {
        GetPolicyQuery {
            policy_hrn: create_test_policy_hrn(),
        }
    }

    // ============================================================================
    // Tests
    // ============================================================================

    #[tokio::test]
    async fn test_get_policy_success() {
        // Arrange
        let policy_view = create_test_policy_view();
        let reader = MockPolicyReader::with_policy(policy_view.clone());
        let use_case = GetPolicyUseCase::new(Arc::new(reader));
        let query = create_test_query();

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_ok(), "Expected successful policy retrieval");
        let retrieved = result.unwrap();
        assert_eq!(retrieved.hrn, create_test_policy_hrn());
        assert_eq!(retrieved.name, "Test Policy");
        assert_eq!(retrieved.content, "permit(principal, action, resource);");
        assert_eq!(retrieved.description, Some("A test policy for unit testing".to_string()));
    }

    #[tokio::test]
    async fn test_get_policy_not_found() {
        // Arrange
        let reader = MockPolicyReader::empty();
        let use_case = GetPolicyUseCase::new(Arc::new(reader));
        let query = create_test_query();

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_err(), "Expected error due to policy not found");
        match result.unwrap_err() {
            GetPolicyError::PolicyNotFound(hrn) => {
                assert_eq!(hrn, create_test_policy_hrn().to_string());
            }
            _ => panic!("Expected PolicyNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_get_policy_invalid_hrn_type() {
        // Arrange
        let invalid_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "User".to_string(), // Wrong type - should be "Policy"
            "test-user".to_string(),
        );

        let reader = MockPolicyReader::empty();
        let use_case = GetPolicyUseCase::new(Arc::new(reader));
        let query = GetPolicyQuery {
            policy_hrn: invalid_hrn,
        };

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_err(), "Expected error due to invalid HRN type");
        match result.unwrap_err() {
            GetPolicyError::InvalidHrn(msg) => {
                assert!(msg.contains("Expected Policy HRN"));
                assert!(msg.contains("User"));
            }
            _ => panic!("Expected InvalidHrn error"),
        }
    }

    #[tokio::test]
    async fn test_get_policy_with_multiple_policies() {
        // Arrange
        let policy1 = PolicyView {
            hrn: Hrn::new(
                "hodei".to_string(),
                "iam".to_string(),
                "account123".to_string(),
                "Policy".to_string(),
                "policy-1".to_string(),
            ),
            name: "Policy 1".to_string(),
            content: "permit(principal, action, resource);".to_string(),
            description: Some("First policy".to_string()),
        };

        let policy2 = PolicyView {
            hrn: Hrn::new(
                "hodei".to_string(),
                "iam".to_string(),
                "account123".to_string(),
                "Policy".to_string(),
                "policy-2".to_string(),
            ),
            name: "Policy 2".to_string(),
            content: "forbid(principal, action, resource);".to_string(),
            description: Some("Second policy".to_string()),
        };

        let reader = MockPolicyReader::with_policies(vec![policy1.clone(), policy2.clone()]);
        let use_case = GetPolicyUseCase::new(Arc::new(reader));

        // Test retrieving first policy
        let query1 = GetPolicyQuery {
            policy_hrn: policy1.hrn.clone(),
        };
        let result1 = use_case.execute(query1).await;

        // Test retrieving second policy
        let query2 = GetPolicyQuery {
            policy_hrn: policy2.hrn.clone(),
        };
        let result2 = use_case.execute(query2).await;

        // Assert
        assert!(result1.is_ok(), "Expected successful retrieval of first policy");
        assert!(result2.is_ok(), "Expected successful retrieval of second policy");

        let retrieved1 = result1.unwrap();
        let retrieved2 = result2.unwrap();

        assert_eq!(retrieved1.hrn, policy1.hrn);
        assert_eq!(retrieved1.name, "Policy 1");
        assert_eq!(retrieved2.hrn, policy2.hrn);
        assert_eq!(retrieved2.name, "Policy 2");
    }

    #[tokio::test]
    async fn test_get_policy_without_description() {
        // Arrange
        let policy_view = PolicyView {
            hrn: create_test_policy_hrn(),
            name: "Policy Without Description".to_string(),
            content: "permit(principal, action, resource);".to_string(),
            description: None,
        };

        let reader = MockPolicyReader::with_policy(policy_view.clone());
        let use_case = GetPolicyUseCase::new(Arc::new(reader));
        let query = create_test_query();

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_ok(), "Expected successful policy retrieval without description");
        let retrieved = result.unwrap();
        assert_eq!(retrieved.hrn, create_test_policy_hrn());
        assert_eq!(retrieved.name, "Policy Without Description");
        assert_eq!(retrieved.description, None);
    }

    #[tokio::test]
    async fn test_get_policy_trait_implementation() {
        // Arrange
        let policy_view = create_test_policy_view();
        let reader = MockPolicyReader::with_policy(policy_view.clone());
        let use_case = GetPolicyUseCase::new(Arc::new(reader));

        // Act - Using the PolicyReader trait directly
        let result = use_case.get_by_hrn(&create_test_policy_hrn()).await;

        // Assert
        assert!(result.is_ok(), "Expected successful retrieval via trait");
        let retrieved = result.unwrap();
        assert_eq!(retrieved.hrn, create_test_policy_hrn());
        assert_eq!(retrieved.name, "Test Policy");
    }

    #[tokio::test]
    async fn test_get_policy_use_case_port_implementation() {
        // Arrange
        let policy_view = create_test_policy_view();
        let reader = MockPolicyReader::with_policy(policy_view.clone());
        let use_case = GetPolicyUseCase::new(Arc::new(reader));
        let query = create_test_query();

        // Act - Using the GetPolicyUseCasePort trait
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_ok(), "Expected successful retrieval via use case port");
        let retrieved = result.unwrap();
        assert_eq!(retrieved.hrn, create_test_policy_hrn());
        assert_eq!(retrieved.name, "Test Policy");
    }

    #[tokio::test]
    async fn test_get_policy_with_complex_content() {
        // Arrange
        let complex_policy = PolicyView {
            hrn: create_test_policy_hrn(),
            name: "Complex Policy".to_string(),
            content: r#"permit(
                principal in Group::"admins",
                action in [Action::"read", Action::"write"],
                resource
            ) when {
                resource.owner == principal
            };"#.to_string(),
            description: Some("A complex policy with conditions".to_string()),
        };

        let reader = MockPolicyReader::with_policy(complex_policy.clone());
        let use_case = GetPolicyUseCase::new(Arc::new(reader));
        let query = create_test_query();

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_ok(), "Expected successful retrieval of complex policy");
        let retrieved = result.unwrap();
        assert_eq!(retrieved.hrn, create_test_policy_hrn());
        assert_eq!(retrieved.name, "Complex Policy");
        assert!(retrieved.content.contains("permit"));
        assert!(retrieved.content.contains("when"));
    }
}
