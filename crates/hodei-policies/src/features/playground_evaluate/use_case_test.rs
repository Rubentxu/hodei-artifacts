//! Unit tests for the playground_evaluate use case
//!
//! These tests verify the use case logic in isolation using mocks
//! for all external dependencies.

#[cfg(test)]
mod tests {
    use super::super::dto::{
        Decision, DeterminingPolicy, PlaygroundAuthorizationRequest, PlaygroundEvaluateCommand,
        PolicyEffect,
    };
    use super::super::error::PlaygroundEvaluateError;
    use super::super::mocks::{
        MockContextConverter, MockPolicyEvaluator, MockPolicyValidator, MockSchemaLoader,
    };
    use super::super::use_case::PlaygroundEvaluateUseCase;
    use kernel::Hrn;
    use std::sync::Arc;

    /// Helper to create a basic authorization request for testing
    fn create_test_request() -> PlaygroundAuthorizationRequest {
        PlaygroundAuthorizationRequest::new(
            Hrn::new(
                "hodei".to_string(),
                "iam".to_string(),
                "default".to_string(),
                "User".to_string(),
                "alice".to_string(),
            ),
            Hrn::action("api", "read"),
            Hrn::new(
                "hodei".to_string(),
                "storage".to_string(),
                "default".to_string(),
                "Document".to_string(),
                "document1".to_string(),
            ),
        )
    }

    /// Helper to create a basic command with inline schema
    fn create_test_command() -> PlaygroundEvaluateCommand {
        PlaygroundEvaluateCommand::new_with_inline_schema(
            "{}".to_string(),
            vec!["permit(principal, action, resource);".to_string()],
            create_test_request(),
        )
    }

    #[tokio::test]
    async fn test_successful_evaluation_with_allow_decision() {
        // Arrange
        let schema_loader = Arc::new(MockSchemaLoader::new_with_success());
        let policy_validator = Arc::new(MockPolicyValidator::new_with_success());
        let policy_evaluator = Arc::new(MockPolicyEvaluator::new_with_allow());
        let context_converter = Arc::new(MockContextConverter::new());

        let use_case = PlaygroundEvaluateUseCase::new(
            schema_loader.clone(),
            policy_validator.clone(),
            policy_evaluator.clone(),
            context_converter.clone(),
        );

        let command = create_test_command();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.decision, Decision::Allow);
        assert_eq!(result.diagnostics.total_policies, 1);
        assert_eq!(result.diagnostics.matched_policies, 0);
        assert!(result.diagnostics.schema_validated);
        assert_eq!(result.errors.len(), 0);

        // Verify all mocks were called
        assert_eq!(schema_loader.load_call_count(), 1);
        assert_eq!(policy_validator.validate_call_count(), 1);
        assert_eq!(policy_evaluator.evaluate_call_count(), 1);
        assert_eq!(context_converter.convert_call_count(), 1);
    }

    #[tokio::test]
    async fn test_successful_evaluation_with_deny_decision() {
        // Arrange
        let schema_loader = Arc::new(MockSchemaLoader::new_with_success());
        let policy_validator = Arc::new(MockPolicyValidator::new_with_success());
        let policy_evaluator = Arc::new(MockPolicyEvaluator::new_with_deny());
        let context_converter = Arc::new(MockContextConverter::new());

        let use_case = PlaygroundEvaluateUseCase::new(
            schema_loader,
            policy_validator,
            policy_evaluator,
            context_converter,
        );

        let command = create_test_command();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.decision, Decision::Deny);
    }

    #[tokio::test]
    async fn test_evaluation_with_determining_policies() {
        // Arrange
        let determining_policies = vec![
            DeterminingPolicy::new("policy1".to_string(), PolicyEffect::Permit),
            DeterminingPolicy::new("policy2".to_string(), PolicyEffect::Forbid),
        ];

        let schema_loader = Arc::new(MockSchemaLoader::new_with_success());
        let policy_validator = Arc::new(MockPolicyValidator::new_with_success());
        let policy_evaluator = Arc::new(MockPolicyEvaluator::new_with_result(
            Decision::Allow,
            determining_policies.clone(),
        ));
        let context_converter = Arc::new(MockContextConverter::new());

        let use_case = PlaygroundEvaluateUseCase::new(
            schema_loader,
            policy_validator,
            policy_evaluator,
            context_converter,
        );

        let command = create_test_command();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.determining_policies.len(), 2);
        assert_eq!(result.diagnostics.matched_policies, 2);
    }

    #[tokio::test]
    async fn test_evaluation_with_validation_errors() {
        // Arrange
        let validation_errors = vec![
            "Policy syntax error on line 1".to_string(),
            "Invalid action reference".to_string(),
        ];

        let schema_loader = Arc::new(MockSchemaLoader::new_with_success());
        let policy_validator = Arc::new(MockPolicyValidator::new_with_errors(
            validation_errors.clone(),
        ));
        let policy_evaluator = Arc::new(MockPolicyEvaluator::new_with_deny());
        let context_converter = Arc::new(MockContextConverter::new());

        let use_case = PlaygroundEvaluateUseCase::new(
            schema_loader,
            policy_validator,
            policy_evaluator,
            context_converter,
        );

        let command = create_test_command();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.errors.len(), 2);
        assert_eq!(result.diagnostics.validation_errors.len(), 2);
        assert_eq!(
            result.diagnostics.validation_errors[0],
            "Policy syntax error on line 1"
        );
    }

    #[tokio::test]
    async fn test_evaluation_fails_with_schema_loading_error() {
        // Arrange
        let schema_loader = Arc::new(MockSchemaLoader::new_with_error(
            PlaygroundEvaluateError::SchemaError("Invalid schema JSON".to_string()),
        ));
        let policy_validator = Arc::new(MockPolicyValidator::new_with_success());
        let policy_evaluator = Arc::new(MockPolicyEvaluator::new_with_allow());
        let context_converter = Arc::new(MockContextConverter::new());

        let use_case = PlaygroundEvaluateUseCase::new(
            schema_loader.clone(),
            policy_validator.clone(),
            policy_evaluator.clone(),
            context_converter.clone(),
        );

        let command = create_test_command();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(matches!(error, PlaygroundEvaluateError::SchemaError(_)));

        // Verify that schema loader was called but others were not
        assert_eq!(schema_loader.load_call_count(), 1);
        assert_eq!(policy_validator.validate_call_count(), 0);
        assert_eq!(policy_evaluator.evaluate_call_count(), 0);
        assert_eq!(context_converter.convert_call_count(), 0);
    }

    #[tokio::test]
    async fn test_evaluation_fails_with_invalid_command() {
        // Arrange
        let schema_loader = Arc::new(MockSchemaLoader::new_with_success());
        let policy_validator = Arc::new(MockPolicyValidator::new_with_success());
        let policy_evaluator = Arc::new(MockPolicyEvaluator::new_with_allow());
        let context_converter = Arc::new(MockContextConverter::new());

        let use_case = PlaygroundEvaluateUseCase::new(
            schema_loader.clone(),
            policy_validator,
            policy_evaluator,
            context_converter,
        );

        // Create an invalid command (no schema)
        let request = create_test_request();
        let command = PlaygroundEvaluateCommand {
            inline_schema: None,
            schema_version: None,
            inline_policies: vec!["permit(principal, action, resource);".to_string()],
            request,
        };

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(matches!(error, PlaygroundEvaluateError::InvalidCommand(_)));

        // Verify no mocks were called (validation failed early)
        assert_eq!(schema_loader.load_call_count(), 0);
    }

    #[tokio::test]
    async fn test_evaluation_fails_with_no_policies() {
        // Arrange
        let schema_loader = Arc::new(MockSchemaLoader::new_with_success());
        let policy_validator = Arc::new(MockPolicyValidator::new_with_success());
        let policy_evaluator = Arc::new(MockPolicyEvaluator::new_with_allow());
        let context_converter = Arc::new(MockContextConverter::new());

        let use_case = PlaygroundEvaluateUseCase::new(
            schema_loader,
            policy_validator,
            policy_evaluator,
            context_converter,
        );

        // Create command with no policies
        let command = PlaygroundEvaluateCommand::new_with_inline_schema(
            "{}".to_string(),
            vec![], // Empty policies
            create_test_request(),
        );

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(matches!(error, PlaygroundEvaluateError::InvalidCommand(_)));
    }

    #[tokio::test]
    async fn test_evaluation_with_schema_version_reference() {
        // Arrange
        let schema_loader = Arc::new(MockSchemaLoader::new_with_success());
        let policy_validator = Arc::new(MockPolicyValidator::new_with_success());
        let policy_evaluator = Arc::new(MockPolicyEvaluator::new_with_allow());
        let context_converter = Arc::new(MockContextConverter::new());

        let use_case = PlaygroundEvaluateUseCase::new(
            schema_loader.clone(),
            policy_validator,
            policy_evaluator,
            context_converter,
        );

        // Create command with schema version (not inline)
        let request = PlaygroundAuthorizationRequest::new(
            Hrn::new(
                "hodei".to_string(),
                "iam".to_string(),
                "default".to_string(),
                "User".to_string(),
                "alice".to_string(),
            ),
            Hrn::action("api", "read"),
            Hrn::new(
                "hodei".to_string(),
                "storage".to_string(),
                "default".to_string(),
                "Document".to_string(),
                "document1".to_string(),
            ),
        );

        let command = PlaygroundEvaluateCommand::new_with_schema_version(
            "v1.0.0".to_string(),
            vec!["permit(principal, action, resource);".to_string()],
            request,
        );

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok());
        assert_eq!(schema_loader.load_call_count(), 1);

        // Verify the loader was called with schema_version
        let calls = schema_loader.load_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].0.is_none()); // inline_schema is None
        assert_eq!(calls[0].1, Some("v1.0.0".to_string())); // schema_version is Some
    }

    #[tokio::test]
    async fn test_evaluation_with_multiple_policies() {
        // Arrange
        let schema_loader = Arc::new(MockSchemaLoader::new_with_success());
        let policy_validator = Arc::new(MockPolicyValidator::new_with_success());
        let policy_evaluator = Arc::new(MockPolicyEvaluator::new_with_allow());
        let context_converter = Arc::new(MockContextConverter::new());

        let use_case = PlaygroundEvaluateUseCase::new(
            schema_loader,
            policy_validator,
            policy_evaluator,
            context_converter,
        );

        // Create command with multiple policies
        let policies = vec![
            "permit(principal, action, resource);".to_string(),
            "forbid(principal, action, resource) when { principal.age < 18 };".to_string(),
            "permit(principal, action, resource) when { principal.role == \"admin\" };".to_string(),
        ];

        let command = PlaygroundEvaluateCommand::new_with_inline_schema(
            "{}".to_string(),
            policies,
            create_test_request(),
        );

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.diagnostics.total_policies, 3);
    }

    #[tokio::test]
    async fn test_diagnostics_are_complete() {
        // Arrange
        let schema_loader = Arc::new(MockSchemaLoader::new_with_success());
        let policy_validator = Arc::new(MockPolicyValidator::new_with_success());
        let determining_policies = vec![DeterminingPolicy::new(
            "policy1".to_string(),
            PolicyEffect::Permit,
        )];
        let policy_evaluator = Arc::new(MockPolicyEvaluator::new_with_result(
            Decision::Allow,
            determining_policies,
        ));
        let context_converter = Arc::new(MockContextConverter::new());

        let use_case = PlaygroundEvaluateUseCase::new(
            schema_loader,
            policy_validator,
            policy_evaluator,
            context_converter,
        );

        let command = create_test_command();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok());
        let result = result.unwrap();

        // Verify diagnostics
        assert_eq!(result.diagnostics.total_policies, 1);
        assert_eq!(result.diagnostics.matched_policies, 1);
        assert!(result.diagnostics.schema_validated);
        assert_eq!(result.diagnostics.validation_errors.len(), 0);
        assert_eq!(result.diagnostics.warnings.len(), 0);
    }
}
