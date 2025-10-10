//! Unit tests for list_policies use case
//!
//! These tests verify the behavior of the ListPoliciesUseCase in isolation,
//! using mocks to simulate external dependencies.

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use kernel::Hrn;

    use crate::features::list_policies::{
        dto::{ListPoliciesQuery, PolicySummary},
        error::ListPoliciesError,
        mocks::MockPolicyLister,
        use_case::ListPoliciesUseCase,
    };
    use crate::list_policies::PolicyLister;
    // ============================================================================
    // Helper Functions
    // ============================================================================

    fn create_test_policy(id: &str) -> PolicySummary {
        PolicySummary {
            hrn: Hrn::new(
                "hodei".to_string(),
                "iam".to_string(),
                "account123".to_string(),
                "Policy".to_string(),
                id.to_string(),
            ),
            name: format!("Policy {}", id),
            description: Some(format!("Test policy {}", id)),
        }
    }

    fn create_test_policies(count: usize) -> Vec<PolicySummary> {
        (0..count)
            .map(|i| create_test_policy(&format!("policy-{}", i)))
            .collect()
    }

    fn create_default_query() -> ListPoliciesQuery {
        ListPoliciesQuery::with_limit(10)
    }

    // ============================================================================
    // Tests
    // ============================================================================

    #[tokio::test]
    async fn test_list_policies_success() {
        // Arrange
        let policies = create_test_policies(3);
        let lister = MockPolicyLister::with_policies(policies.clone());
        let use_case = ListPoliciesUseCase::new(Arc::new(lister));
        let query = create_default_query();

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_ok(), "Expected successful policy listing");
        let response = result.unwrap();
        assert_eq!(response.policies.len(), 3);
        assert_eq!(response.total_count, 3);
        assert!(!response.has_next_page);
        assert!(!response.has_previous_page);
    }

    #[tokio::test]
    async fn test_list_policies_with_pagination() {
        // Arrange
        let policies = create_test_policies(25);
        let lister = MockPolicyLister::with_policies(policies.clone());
        let use_case = ListPoliciesUseCase::new(Arc::new(lister));

        // Act - First page
        let query = ListPoliciesQuery::with_pagination(10, 0);
        let result = use_case.execute(query).await;

        // Assert - First page
        assert!(result.is_ok(), "Expected successful first page retrieval");
        let response = result.unwrap();
        assert_eq!(response.policies.len(), 10);
        assert_eq!(response.total_count, 25);
        assert!(response.has_next_page);
        assert!(!response.has_previous_page);

        // Act - Second page
        let query = ListPoliciesQuery::with_pagination(10, 10);
        let result = use_case.execute(query).await;

        // Assert - Second page
        assert!(result.is_ok(), "Expected successful second page retrieval");
        let response = result.unwrap();
        assert_eq!(response.policies.len(), 10);
        assert_eq!(response.total_count, 25);
        assert!(response.has_next_page);
        assert!(response.has_previous_page);

        // Act - Third page (partial)
        let query = ListPoliciesQuery::with_pagination(10, 20);
        let result = use_case.execute(query).await;

        // Assert - Third page
        assert!(result.is_ok(), "Expected successful third page retrieval");
        let response = result.unwrap();
        assert_eq!(response.policies.len(), 5);
        assert_eq!(response.total_count, 25);
        assert!(!response.has_next_page);
        assert!(response.has_previous_page);
    }

    #[tokio::test]
    async fn test_list_policies_empty() {
        // Arrange
        let lister = MockPolicyLister::empty();
        let use_case = ListPoliciesUseCase::new(Arc::new(lister));
        let query = create_default_query();

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_ok(), "Expected successful empty listing");
        let response = result.unwrap();
        assert!(response.policies.is_empty());
        assert_eq!(response.total_count, 0);
        assert!(!response.has_next_page);
        assert!(!response.has_previous_page);
    }

    #[tokio::test]
    async fn test_list_policies_invalid_limit_zero() {
        // Arrange
        let lister = MockPolicyLister::empty();
        let use_case = ListPoliciesUseCase::new(Arc::new(lister));
        let query = ListPoliciesQuery {
            limit: 0,
            offset: 0,
        };

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_err(), "Expected error due to zero limit");
        match result.unwrap_err() {
            ListPoliciesError::InvalidPagination(msg) => {
                assert!(msg.contains("Limit must be greater than 0"));
            }
            _ => panic!("Expected InvalidPagination error"),
        }
    }

    #[tokio::test]
    async fn test_list_policies_invalid_limit_too_high() {
        // Arrange
        let lister = MockPolicyLister::empty();
        let use_case = ListPoliciesUseCase::new(Arc::new(lister));
        let query = ListPoliciesQuery {
            limit: 101, // Exceeds maximum limit of 100
            offset: 0,
        };

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_err(), "Expected error due to limit too high");
        match result.unwrap_err() {
            ListPoliciesError::InvalidPagination(msg) => {
                assert!(msg.contains("Limit must be less than or equal to 100"));
            }
            _ => panic!("Expected InvalidPagination error"),
        }
    }

    #[tokio::test]
    async fn test_list_policies_repository_error() {
        // Arrange
        let lister = MockPolicyLister::with_error();
        let use_case = ListPoliciesUseCase::new(Arc::new(lister));
        let query = create_default_query();

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_err(), "Expected error due to repository failure");
        match result.unwrap_err() {
            ListPoliciesError::RepositoryError(msg) => {
                assert!(msg.contains("Mock repository error"));
            }
            _ => panic!("Expected RepositoryError"),
        }
    }

    #[tokio::test]
    async fn test_list_policies_with_valid_limits() {
        // Arrange
        let policies = create_test_policies(50);
        let lister = MockPolicyLister::with_policies(policies.clone());
        let use_case = ListPoliciesUseCase::new(Arc::new(lister));

        // Test various valid limits
        let valid_limits = [1, 10, 25, 50, 100];

        for limit in valid_limits {
            let query = ListPoliciesQuery { limit, offset: 0 };

            // Act
            let result = use_case.execute(query).await;

            // Assert
            assert!(
                result.is_ok(),
                "Expected successful listing with limit {}",
                limit
            );
            let response = result.unwrap();
            assert!(response.policies.len() <= limit);
            assert_eq!(response.total_count, 50);
        }
    }

    #[tokio::test]
    async fn test_list_policies_trait_implementation() {
        // Arrange
        let policies = create_test_policies(5);
        let lister = MockPolicyLister::with_policies(policies.clone());
        let use_case = ListPoliciesUseCase::new(Arc::new(lister));
        let query = create_default_query();

        // Act - Using the PolicyLister trait directly
        let result = use_case.list(query).await;

        // Assert
        assert!(result.is_ok(), "Expected successful listing via trait");
        let response = result.unwrap();
        assert_eq!(response.policies.len(), 5);
        assert_eq!(response.total_count, 5);
    }

    #[tokio::test]
    async fn test_list_policies_use_case_port_implementation() {
        // Arrange
        let policies = create_test_policies(5);
        let lister = MockPolicyLister::with_policies(policies.clone());
        let use_case = ListPoliciesUseCase::new(Arc::new(lister));
        let query = create_default_query();

        // Act - Using the ListPoliciesUseCasePort trait
        let result = use_case.execute(query).await;

        // Assert
        assert!(
            result.is_ok(),
            "Expected successful listing via use case port"
        );
        let response = result.unwrap();
        assert_eq!(response.policies.len(), 5);
        assert_eq!(response.total_count, 5);
    }

    #[tokio::test]
    async fn test_list_policies_with_large_offset() {
        // Arrange
        let policies = create_test_policies(10);
        let lister = MockPolicyLister::with_policies(policies.clone());
        let use_case = ListPoliciesUseCase::new(Arc::new(lister));

        // Act - Offset beyond total count
        let query = ListPoliciesQuery {
            limit: 10,
            offset: 15, // Beyond total count of 10
        };
        let result = use_case.execute(query).await;

        // Assert
        assert!(
            result.is_ok(),
            "Expected successful listing with large offset"
        );
        let response = result.unwrap();
        assert!(response.policies.is_empty());
        assert_eq!(response.total_count, 10);
        assert!(!response.has_next_page);
        assert!(response.has_previous_page);
    }

    #[tokio::test]
    async fn test_list_policies_single_policy() {
        // Arrange
        let policies = vec![create_test_policy("single")];
        let lister = MockPolicyLister::with_policies(policies.clone());
        let use_case = ListPoliciesUseCase::new(Arc::new(lister));
        let query = create_default_query();

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(
            result.is_ok(),
            "Expected successful listing of single policy"
        );
        let response = result.unwrap();
        assert_eq!(response.policies.len(), 1);
        assert_eq!(response.total_count, 1);
        assert!(!response.has_next_page);
        assert!(!response.has_previous_page);

        let policy = &response.policies[0];
        assert_eq!(policy.name, "Policy single");
        assert_eq!(policy.description, Some("Test policy single".to_string()));
    }

    #[tokio::test]
    async fn test_list_policies_without_descriptions() {
        // Arrange
        let policy_without_description = PolicySummary {
            hrn: Hrn::new(
                "hodei".to_string(),
                "iam".to_string(),
                "account123".to_string(),
                "Policy".to_string(),
                "no-desc".to_string(),
            ),
            name: "Policy Without Description".to_string(),
            description: None,
        };

        let policies = vec![policy_without_description.clone()];
        let lister = MockPolicyLister::with_policies(policies.clone());
        let use_case = ListPoliciesUseCase::new(Arc::new(lister));
        let query = create_default_query();

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(
            result.is_ok(),
            "Expected successful listing of policy without description"
        );
        let response = result.unwrap();
        assert_eq!(response.policies.len(), 1);
        let policy = &response.policies[0];
        assert_eq!(policy.name, "Policy Without Description");
        assert_eq!(policy.description, None);
    }
}
