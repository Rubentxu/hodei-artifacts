//! Unit tests for get_effective_policies use case
//!
//! These tests verify the behavior of the GetEffectivePoliciesUseCase in isolation,
//! using mocks to simulate external dependencies.

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use kernel::domain::{HodeiPolicy, Hrn, PolicyId};

    use crate::features::get_effective_policies::{
        dto::{GetEffectivePoliciesQuery, GroupLookupDto, UserLookupDto},
        error::GetEffectivePoliciesError,
        mocks::{MockGroupFinderPort, MockPolicyFinderPort, MockUserFinderPort},
        use_case::GetEffectivePoliciesUseCase,
    };

    // ============================================================================
    // Helper Functions
    // ============================================================================

    fn create_test_user_hrn() -> Hrn {
        Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "User".to_string(),
            "alice".to_string(),
        )
    }

    fn create_test_group_hrn() -> Hrn {
        Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "Group".to_string(),
            "developers".to_string(),
        )
    }

    fn create_test_user_dto() -> UserLookupDto {
        UserLookupDto::new(
            create_test_user_hrn().to_string(),
            "Alice".to_string(),
            "alice@example.com".to_string(),
        )
    }

    fn create_test_group_dto() -> GroupLookupDto {
        GroupLookupDto::new(
            create_test_group_hrn().to_string(),
            "Developers".to_string(),
        )
    }

    fn create_test_policy() -> HodeiPolicy {
        HodeiPolicy::new(
            PolicyId::new("policy1".to_string()),
            "permit(principal, action, resource);".to_string(),
        )
    }

    fn create_test_query() -> GetEffectivePoliciesQuery {
        GetEffectivePoliciesQuery {
            principal_hrn: create_test_user_hrn().to_string(),
        }
    }

    // ============================================================================
    // Tests
    // ============================================================================

    #[tokio::test]
    async fn test_get_effective_policies_success() {
        // Arrange
        let user_dto = create_test_user_dto();
        let group_dto = create_test_group_dto();
        let policy = create_test_policy();

        let user_finder = Arc::new(MockUserFinderPort::new().with_user(user_dto.clone()));
        let group_finder = Arc::new(MockGroupFinderPort::new().with_groups(vec![group_dto.clone()]));
        let policy_finder = Arc::new(MockPolicyFinderPort::new().with_policies(vec![policy.clone()]));

        let use_case = GetEffectivePoliciesUseCase::new(
            user_finder,
            group_finder,
            policy_finder,
        );

        let query = create_test_query();

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_ok(), "Expected successful policy retrieval");
        let response = result.unwrap();
        assert_eq!(response.principal_hrn, create_test_user_hrn().to_string());
        assert_eq!(response.policies.len(), 1);
        assert!(response.policies.contains(&policy));
    }

    #[tokio::test]
    async fn test_get_effective_policies_user_not_found() {
        // Arrange
        let user_finder = Arc::new(MockUserFinderPort::new()); // No user
        let group_finder = Arc::new(MockGroupFinderPort::new());
        let policy_finder = Arc::new(MockPolicyFinderPort::new());

        let use_case = GetEffectivePoliciesUseCase::new(
            user_finder,
            group_finder,
            policy_finder,
        );

        let query = create_test_query();

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_err(), "Expected error due to user not found");
        match result.unwrap_err() {
            GetEffectivePoliciesError::PrincipalNotFound(hrn) => {
                assert_eq!(hrn, create_test_user_hrn().to_string());
            }
            _ => panic!("Expected PrincipalNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_get_effective_policies_invalid_principal_hrn() {
        // Arrange
        let user_finder = Arc::new(MockUserFinderPort::new());
        let group_finder = Arc::new(MockGroupFinderPort::new());
        let policy_finder = Arc::new(MockPolicyFinderPort::new());

        let use_case = GetEffectivePoliciesUseCase::new(
            user_finder,
            group_finder,
            policy_finder,
        );

        let query = GetEffectivePoliciesQuery {
            principal_hrn: "invalid-hrn".to_string(),
        };

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_err(), "Expected error due to invalid HRN");
        match result.unwrap_err() {
            GetEffectivePoliciesError::InvalidPrincipalHrn(hrn) => {
                assert_eq!(hrn, "invalid-hrn");
            }
            _ => panic!("Expected InvalidPrincipalHrn error"),
        }
    }

    #[tokio::test]
    async fn test_get_effective_policies_invalid_principal_type() {
        // Arrange
        let user_finder = Arc::new(MockUserFinderPort::new());
        let group_finder = Arc::new(MockGroupFinderPort::new());
        let policy_finder = Arc::new(MockPolicyFinderPort::new());

        let use_case = GetEffectivePoliciesUseCase::new(
            user_finder,
            group_finder,
            policy_finder,
        );

        let invalid_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "InvalidType".to_string(), // Invalid resource type
            "test".to_string(),
        );

        let query = GetEffectivePoliciesQuery {
            principal_hrn: invalid_hrn.to_string(),
        };

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_err(), "Expected error due to invalid principal type");
        match result.unwrap_err() {
            GetEffectivePoliciesError::InvalidPrincipalType(resource_type) => {
                assert_eq!(resource_type, "InvalidType");
            }
            _ => panic!("Expected InvalidPrincipalType error"),
        }
    }

    #[tokio::test]
    async fn test_get_effective_policies_user_finder_error() {
        // Arrange
        let user_finder = Arc::new(MockUserFinderPort::new().with_failure());
        let group_finder = Arc::new(MockGroupFinderPort::new());
        let policy_finder = Arc::new(MockPolicyFinderPort::new());

        let use_case = GetEffectivePoliciesUseCase::new(
            user_finder,
            group_finder,
            policy_finder,
        );

        let query = create_test_query();

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_err(), "Expected error due to user finder failure");
        match result.unwrap_err() {
            GetEffectivePoliciesError::RepositoryError(msg) => {
                assert!(msg.contains("Mock user finder failure"));
            }
            _ => panic!("Expected RepositoryError"),
        }
    }

    #[tokio::test]
    async fn test_get_effective_policies_group_finder_error() {
        // Arrange
        let user_dto = create_test_user_dto();
        let user_finder = Arc::new(MockUserFinderPort::new().with_user(user_dto));
        let group_finder = Arc::new(MockGroupFinderPort::new().with_failure());
        let policy_finder = Arc::new(MockPolicyFinderPort::new());

        let use_case = GetEffectivePoliciesUseCase::new(
            user_finder,
            group_finder,
            policy_finder,
        );

        let query = create_test_query();

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_err(), "Expected error due to group finder failure");
        match result.unwrap_err() {
            GetEffectivePoliciesError::RepositoryError(msg) => {
                assert!(msg.contains("Mock group finder failure"));
            }
            _ => panic!("Expected RepositoryError"),
        }
    }

    #[tokio::test]
    async fn test_get_effective_policies_policy_finder_error() {
        // Arrange
        let user_dto = create_test_user_dto();
        let user_finder = Arc::new(MockUserFinderPort::new().with_user(user_dto));
        let group_finder = Arc::new(MockGroupFinderPort::new());
        let policy_finder = Arc::new(MockPolicyFinderPort::new().with_failure());

        let use_case = GetEffectivePoliciesUseCase::new(
            user_finder,
            group_finder,
            policy_finder,
        );

        let query = create_test_query();

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_err(), "Expected error due to policy finder failure");
        match result.unwrap_err() {
            GetEffectivePoliciesError::RepositoryError(msg) => {
                assert!(msg.contains("Mock policy finder failure"));
            }
            _ => panic!("Expected RepositoryError"),
        }
    }

    #[tokio::test]
    async fn test_get_effective_policies_with_groups() {
        // Arrange
        let user_dto = create_test_user_dto();
        let group1 = GroupLookupDto::new(
            Hrn::new(
                "hodei".to_string(),
                "iam".to_string(),
                "account123".to_string(),
                "Group".to_string(),
                "developers".to_string(),
            ).to_string(),
            "Developers".to_string(),
        );
        let group2 = GroupLookupDto::new(
            Hrn::new(
                "hodei".to_string(),
                "iam".to_string(),
                "account123".to_string(),
                "Group".to_string(),
                "admins".to_string(),
            ).to_string(),
            "Admins".to_string(),
        );

        let user_policy = HodeiPolicy::new(
            PolicyId::new("user_policy".to_string()),
            "permit(principal, action, resource);".to_string(),
        );
        let group1_policy = HodeiPolicy::new(
            PolicyId::new("group1_policy".to_string()),
            "forbid(principal, action, resource);".to_string(),
        );
        let group2_policy = HodeiPolicy::new(
            PolicyId::new("group2_policy".to_string()),
            "permit(principal, action, resource);".to_string(),
        );

        let user_finder = Arc::new(MockUserFinderPort::new().with_user(user_dto.clone()));
        let group_finder = Arc::new(MockGroupFinderPort::new().with_groups(vec![group1.clone(), group2.clone()]));
        
        // Mock policy finder that returns different policies based on principal
        let policy_finder = Arc::new(MockPolicyFinderPort::new().with_policies(vec![
            user_policy.clone(),
            group1_policy.clone(),
            group2_policy.clone(),
        ]));

        let use_case = GetEffectivePoliciesUseCase::new(
            user_finder,
            group_finder,
            policy_finder,
        );

        let query = create_test_query();

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_ok(), "Expected successful policy retrieval with groups");
        let response = result.unwrap();
        assert_eq!(response.policies.len(), 3);
        assert!(response.policies.contains(&user_policy));
        assert!(response.policies.contains(&group1_policy));
        assert!(response.policies.contains(&group2_policy));
    }

    #[tokio::test]
    async fn test_get_effective_policies_no_groups() {
        // Arrange
        let user_dto = create_test_user_dto();
        let user_policy = create_test_policy();

        let user_finder = Arc::new(MockUserFinderPort::new().with_user(user_dto.clone()));
        let group_finder = Arc::new(MockGroupFinderPort::new()); // No groups
        let policy_finder = Arc::new(MockPolicyFinderPort::new().with_policies(vec![user_policy.clone()]));

        let use_case = GetEffectivePoliciesUseCase::new(
            user_finder,
            group_finder,
            policy_finder,
        );

        let query = create_test_query();

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_ok(), "Expected successful policy retrieval without groups");
        let response = result.unwrap();
        assert_eq!(response.policies.len(), 1);
        assert!(response.policies.contains(&user_policy));
    }

    #[tokio::test]
    async fn test_get_effective_policies_no_policies() {
        // Arrange
        let user_dto = create_test_user_dto();
        let group_dto = create_test_group_dto();

        let user_finder = Arc::new(MockUserFinderPort::new().with_user(user_dto.clone()));
        let group_finder = Arc::new(MockGroupFinderPort::new().with_groups(vec![group_dto.clone()]));
        let policy_finder = Arc::new(MockPolicyFinderPort::new()); // No policies

        let use_case = GetEffectivePoliciesUseCase::new(
            user_finder,
            group_finder,
            policy_finder,
        );

        let query = create_test_query();

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_ok(), "Expected successful retrieval with no policies");
        let response = result.unwrap();
        assert_eq!(response.policies.len(), 0);
    }

    #[tokio::test]
    async fn test_get_effective_policies_service_account_principal() {
        // Arrange
        let service_account_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "ServiceAccount".to_string(),
            "api-service".to_string(),
        );

        let service_account_dto = UserLookupDto::new(
            service_account_hrn.to_string(),
            "API Service".to_string(),
            "api@example.com".to_string(),
        );

        let policy = create_test_policy();

        let user_finder = Arc::new(MockUserFinderPort::new().with_user(service_account_dto.clone()));
        let group_finder = Arc::new(MockGroupFinderPort::new());
        let policy_finder = Arc::new(MockPolicyFinderPort::new().with_policies(vec![policy.clone()]));

        let use_case = GetEffectivePoliciesUseCase::new(
            user_finder,
            group_finder,
            policy_finder,
        );

        let query = GetEffectivePoliciesQuery {
            principal_hrn: service_account_hrn.to_string(),
        };

        // Act
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_ok(), "Expected successful retrieval for service account");
        let response = result.unwrap();
        assert_eq!(response.principal_hrn, service_account_hrn.to_string());
        assert_eq!(response.policies.len(), 1);
        assert!(response.policies.contains(&policy));
    }
}
