//! Unit tests for evaluate_iam_policies use case
//!
//! These tests verify the behavior of the EvaluateIamPoliciesUseCase in isolation,
//! using mocks to simulate external dependencies.

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use kernel::application::ports::authorization::{
        AuthorizationError, EvaluationRequest, IamPolicyEvaluator,
    };
    use kernel::domain::{
        ActionTrait, AttributeName, AttributeType, AttributeValue, HodeiEntity, HodeiEntityType,
        Hrn, Principal, Resource, ResourceTypeName, ServiceName,
    };
    use policies::features::evaluate_policies::EvaluatePoliciesUseCase;

    use crate::features::evaluate_iam_policies::{
        mocks::{MockEmptyPolicyFinder, MockPolicyFinder, MockPolicyFinderWithError},
        use_case::EvaluateIamPoliciesUseCase,
    };

    // ============================================================================
    // Test Entities
    // ============================================================================

    struct TestUser {
        hrn: Hrn,
        email: String,
    }

    impl TestUser {
        fn new(id: &str, email: &str) -> Self {
            Self {
                hrn: Hrn::new("iam", "user", id),
                email: email.to_string(),
            }
        }
    }

    impl HodeiEntityType for TestUser {
        fn service_name() -> ServiceName {
            ServiceName::new("iam").unwrap()
        }

        fn resource_type_name() -> ResourceTypeName {
            ResourceTypeName::new("User").unwrap()
        }

        fn is_principal_type() -> bool {
            true
        }

        fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
            vec![(AttributeName::new("email").unwrap(), AttributeType::String)]
        }
    }

    impl HodeiEntity for TestUser {
        fn hrn(&self) -> &Hrn {
            &self.hrn
        }

        fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
            let mut attrs = HashMap::new();
            attrs.insert(
                AttributeName::new("email").unwrap(),
                AttributeValue::String(self.email.clone()),
            );
            attrs
        }
    }

    impl Principal for TestUser {}

    struct TestResource {
        hrn: Hrn,
        name: String,
    }

    impl TestResource {
        fn new(id: &str, name: &str) -> Self {
            Self {
                hrn: Hrn::new("s3", "bucket", id),
                name: name.to_string(),
            }
        }
    }

    impl HodeiEntityType for TestResource {
        fn service_name() -> ServiceName {
            ServiceName::new("s3").unwrap()
        }

        fn resource_type_name() -> ResourceTypeName {
            ResourceTypeName::new("Bucket").unwrap()
        }

        fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
            vec![(AttributeName::new("name").unwrap(), AttributeType::String)]
        }
    }

    impl HodeiEntity for TestResource {
        fn hrn(&self) -> &Hrn {
            &self.hrn
        }

        fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
            let mut attrs = HashMap::new();
            attrs.insert(
                AttributeName::new("name").unwrap(),
                AttributeValue::String(self.name.clone()),
            );
            attrs
        }
    }

    impl Resource for TestResource {}

    struct TestAction;

    impl ActionTrait for TestAction {
        fn name() -> &'static str {
            "read"
        }
    }

    // ============================================================================
    // Helper Functions
    // ============================================================================

    fn create_test_request() -> EvaluationRequest {
        let user = TestUser::new("alice", "alice@example.com");
        let resource = TestResource::new("my-bucket", "My Test Bucket");

        EvaluationRequest {
            principal: Box::new(user),
            action: Box::new(TestAction),
            resource: Box::new(resource),
        }
    }

    // ============================================================================
    // Tests
    // ============================================================================

    #[tokio::test]
    async fn test_evaluate_with_allow_policy() {
        // Arrange
        let policy = r#"permit(principal, action == Action::"read", resource);"#.to_string();
        let mock_finder = Arc::new(MockPolicyFinder::with_policies(vec![policy]));
        let policy_evaluator = Arc::new(EvaluatePoliciesUseCase::new());

        let use_case = EvaluateIamPoliciesUseCase::new(mock_finder, policy_evaluator);
        let request = create_test_request();

        // Act
        let result = use_case.evaluate_iam_policies(request).await;

        // Assert
        assert!(result.is_ok(), "Expected successful evaluation");
        let decision = result.unwrap();
        assert_eq!(decision.decision, true, "Expected allow decision");
        assert!(
            decision.reason.contains("Allowed") || decision.reason.contains("Allow"),
            "Expected allow reason, got: {}",
            decision.reason
        );
    }

    #[tokio::test]
    async fn test_evaluate_with_deny_policy() {
        // Arrange
        let policy = r#"forbid(principal, action == Action::"read", resource);"#.to_string();
        let mock_finder = Arc::new(MockPolicyFinder::with_policies(vec![policy]));
        let policy_evaluator = Arc::new(EvaluatePoliciesUseCase::new());

        let use_case = EvaluateIamPoliciesUseCase::new(mock_finder, policy_evaluator);
        let request = create_test_request();

        // Act
        let result = use_case.evaluate_iam_policies(request).await;

        // Assert
        assert!(result.is_ok(), "Expected successful evaluation");
        let decision = result.unwrap();
        assert_eq!(decision.decision, false, "Expected deny decision");
    }

    #[tokio::test]
    async fn test_evaluate_with_no_policies_returns_implicit_deny() {
        // Arrange
        let mock_finder = Arc::new(MockEmptyPolicyFinder::new());
        let policy_evaluator = Arc::new(EvaluatePoliciesUseCase::new());

        let use_case = EvaluateIamPoliciesUseCase::new(mock_finder, policy_evaluator);
        let request = create_test_request();

        // Act
        let result = use_case.evaluate_iam_policies(request).await;

        // Assert
        assert!(result.is_ok(), "Expected successful evaluation");
        let decision = result.unwrap();
        assert_eq!(decision.decision, false, "Expected implicit deny");
        assert!(
            decision.reason.contains("No IAM policies found"),
            "Expected implicit deny reason, got: {}",
            decision.reason
        );
    }

    #[tokio::test]
    async fn test_evaluate_with_multiple_policies() {
        // Arrange
        let policies = vec![
            r#"permit(principal, action == Action::"read", resource);"#.to_string(),
            r#"permit(principal, action == Action::"write", resource);"#.to_string(),
        ];
        let mock_finder = Arc::new(MockPolicyFinder::with_policies(policies));
        let policy_evaluator = Arc::new(EvaluatePoliciesUseCase::new());

        let use_case = EvaluateIamPoliciesUseCase::new(mock_finder, policy_evaluator);
        let request = create_test_request();

        // Act
        let result = use_case.evaluate_iam_policies(request).await;

        // Assert
        assert!(result.is_ok(), "Expected successful evaluation");
        let decision = result.unwrap();
        assert_eq!(decision.decision, true, "Expected allow decision");
    }

    #[tokio::test]
    async fn test_evaluate_handles_policy_finder_error() {
        // Arrange
        let error_msg = "Database connection failed".to_string();
        let mock_finder = Arc::new(MockPolicyFinderWithError::new(error_msg.clone()));
        let policy_evaluator = Arc::new(EvaluatePoliciesUseCase::new());

        let use_case = EvaluateIamPoliciesUseCase::new(mock_finder, policy_evaluator);
        let request = create_test_request();

        // Act
        let result = use_case.evaluate_iam_policies(request).await;

        // Assert
        assert!(result.is_err(), "Expected error");
        match result.unwrap_err() {
            AuthorizationError::EvaluationFailed(msg) => {
                assert!(
                    msg.contains(&error_msg),
                    "Expected error message to contain: {}",
                    error_msg
                );
            }
            e => panic!("Expected EvaluationFailed error, got: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_evaluate_with_deny_overrides_allow() {
        // Arrange - Deny should override Allow in Cedar
        let policies = vec![
            r#"permit(principal, action == Action::"read", resource);"#.to_string(),
            r#"forbid(principal, action == Action::"read", resource);"#.to_string(),
        ];
        let mock_finder = Arc::new(MockPolicyFinder::with_policies(policies));
        let policy_evaluator = Arc::new(EvaluatePoliciesUseCase::new());

        let use_case = EvaluateIamPoliciesUseCase::new(mock_finder, policy_evaluator);
        let request = create_test_request();

        // Act
        let result = use_case.evaluate_iam_policies(request).await;

        // Assert
        assert!(result.is_ok(), "Expected successful evaluation");
        let decision = result.unwrap();
        assert_eq!(
            decision.decision, false,
            "Expected deny (forbid overrides permit)"
        );
    }

    #[tokio::test]
    async fn test_evaluate_decision_contains_principal_action_resource() {
        // Arrange
        let policy = r#"permit(principal, action == Action::"read", resource);"#.to_string();
        let mock_finder = Arc::new(MockPolicyFinder::with_policies(vec![policy]));
        let policy_evaluator = Arc::new(EvaluatePoliciesUseCase::new());

        let use_case = EvaluateIamPoliciesUseCase::new(mock_finder, policy_evaluator);
        let request = create_test_request();

        let expected_principal_hrn = request.principal.hrn().clone();
        let expected_action_name = request.action.name().to_string();
        let expected_resource_hrn = request.resource.hrn().clone();

        // Act
        let result = use_case.evaluate_iam_policies(request).await;

        // Assert
        assert!(result.is_ok());
        let decision = result.unwrap();
        assert_eq!(decision.principal_hrn, expected_principal_hrn);
        assert_eq!(decision.action_name, expected_action_name);
        assert_eq!(decision.resource_hrn, expected_resource_hrn);
    }

    #[tokio::test]
    async fn test_evaluate_with_specific_principal_condition() {
        // Arrange - Policy that allows only specific user
        let policy =
            r#"permit(principal == Iam::User::"alice", action == Action::"read", resource);"#
                .to_string();
        let mock_finder = Arc::new(MockPolicyFinder::with_policies(vec![policy]));
        let policy_evaluator = Arc::new(EvaluatePoliciesUseCase::new());

        let use_case = EvaluateIamPoliciesUseCase::new(mock_finder, policy_evaluator);
        let request = create_test_request(); // alice as principal

        // Act
        let result = use_case.evaluate_iam_policies(request).await;

        // Assert
        assert!(result.is_ok());
        let decision = result.unwrap();
        assert_eq!(decision.decision, true, "Expected allow for alice");
    }

    #[tokio::test]
    async fn test_evaluate_logs_evaluation_metrics() {
        // Arrange
        let policy = r#"permit(principal, action == Action::"read", resource);"#.to_string();
        let mock_finder = Arc::new(MockPolicyFinder::with_policies(vec![policy]));
        let policy_evaluator = Arc::new(EvaluatePoliciesUseCase::new());

        let use_case = EvaluateIamPoliciesUseCase::new(mock_finder, policy_evaluator);
        let request = create_test_request();

        // Act
        let result = use_case.evaluate_iam_policies(request).await;

        // Assert - Just verify it completes successfully
        // Actual logging is verified through tracing-test or manual observation
        assert!(
            result.is_ok(),
            "Expected successful evaluation with metrics"
        );
    }

    #[tokio::test]
    async fn test_evaluate_with_attribute_based_policy() {
        // Arrange - Policy that checks user email attribute
        let policy = r#"
            permit(principal, action == Action::"read", resource)
            when { principal.email like "alice@*" };
        "#
        .to_string();
        let mock_finder = Arc::new(MockPolicyFinder::with_policies(vec![policy]));
        let policy_evaluator = Arc::new(EvaluatePoliciesUseCase::new());

        let use_case = EvaluateIamPoliciesUseCase::new(mock_finder, policy_evaluator);
        let request = create_test_request();

        // Act
        let result = use_case.evaluate_iam_policies(request).await;

        // Assert
        assert!(result.is_ok(), "Expected successful evaluation");
        let decision = result.unwrap();
        assert_eq!(
            decision.decision, true,
            "Expected allow for user with matching email"
        );
    }

    #[tokio::test]
    async fn test_evaluate_with_empty_policy_list_after_retrieval() {
        // Arrange - Empty policy list (different from MockEmptyPolicyFinder)
        let mock_finder = Arc::new(MockPolicyFinder::with_policies(vec![]));
        let policy_evaluator = Arc::new(EvaluatePoliciesUseCase::new());

        let use_case = EvaluateIamPoliciesUseCase::new(mock_finder, policy_evaluator);
        let request = create_test_request();

        // Act
        let result = use_case.evaluate_iam_policies(request).await;

        // Assert
        assert!(result.is_ok(), "Expected successful evaluation");
        let decision = result.unwrap();
        assert_eq!(decision.decision, false, "Expected implicit deny");
        assert!(decision.reason.contains("No IAM policies found"));
    }
}
