//! Unit tests for update_policy use case
//!
//! These tests verify the behavior of the UpdatePolicyUseCase in isolation,
//! using mocks to simulate external dependencies.

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::features::update_policy::{
        dto::UpdatePolicyCommand,
        error::UpdatePolicyError,
        mocks::{MockPolicyValidator, MockUpdatePolicyPort},
        use_case::UpdatePolicyUseCase,
    };

    use crate::features::update_policy::ports::UpdatePolicyPort;

    // ============================================================================
    // Helper Functions
    // ============================================================================

    fn create_test_command() -> UpdatePolicyCommand {
        UpdatePolicyCommand::update_content("test-policy", "permit(principal, action, resource);")
    }

    fn create_test_command_with_description() -> UpdatePolicyCommand {
        UpdatePolicyCommand::update_description("test-policy", "Updated description")
    }

    fn create_test_command_with_both() -> UpdatePolicyCommand {
        UpdatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: Some("permit(principal, action, resource);".to_string()),
            description: Some("Updated description".to_string()),
        }
    }

    // ============================================================================
    // Tests
    // ============================================================================

    #[tokio::test]
    async fn test_update_policy_content_success() {
        // Arrange
        let validator = Arc::new(MockPolicyValidator::new());
        let port = Arc::new(MockUpdatePolicyPort::new());
        let use_case = UpdatePolicyUseCase::new(validator, port);
        let command = create_test_command();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok(), "Expected successful policy update");
        let view = result.unwrap();
        assert_eq!(view.name, "test-policy");
        assert_eq!(view.content, "permit(principal, action, resource);");
    }

    #[tokio::test]
    async fn test_update_policy_description_only() {
        // Arrange
        let validator = Arc::new(MockPolicyValidator::new());
        let port = Arc::new(MockUpdatePolicyPort::new());
        let use_case = UpdatePolicyUseCase::new(validator, port);
        let command = create_test_command_with_description();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok(), "Expected successful description update");
        let view = result.unwrap();
        assert_eq!(view.name, "test-policy");
        assert_eq!(view.description, Some("Updated description".to_string()));
    }

    #[tokio::test]
    async fn test_update_policy_content_and_description() {
        // Arrange
        let validator = Arc::new(MockPolicyValidator::new());
        let port = Arc::new(MockUpdatePolicyPort::new());
        let use_case = UpdatePolicyUseCase::new(validator, port);
        let command = create_test_command_with_both();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(
            result.is_ok(),
            "Expected successful update of both content and description"
        );
        let view = result.unwrap();
        assert_eq!(view.name, "test-policy");
        assert_eq!(view.content, "permit(principal, action, resource);");
        assert_eq!(view.description, Some("Updated description".to_string()));
    }

    #[tokio::test]
    async fn test_update_policy_empty_id_fails() {
        // Arrange
        let validator = Arc::new(MockPolicyValidator::new());
        let port = Arc::new(MockUpdatePolicyPort::new());
        let use_case = UpdatePolicyUseCase::new(validator, port);
        let command = UpdatePolicyCommand {
            policy_id: "".to_string(),
            policy_content: Some("permit(principal, action, resource);".to_string()),
            description: None,
        };

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err(), "Expected error due to empty policy ID");
        match result.unwrap_err() {
            UpdatePolicyError::InvalidPolicyId(msg) => {
                assert!(msg.contains("Policy ID cannot be empty"));
            }
            _ => panic!("Expected InvalidPolicyId error"),
        }
    }

    #[tokio::test]
    async fn test_update_policy_no_updates_fails() {
        // Arrange
        let validator = Arc::new(MockPolicyValidator::new());
        let port = Arc::new(MockUpdatePolicyPort::new());
        let use_case = UpdatePolicyUseCase::new(validator, port);
        let command = UpdatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: None,
            description: None,
        };

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err(), "Expected error due to no updates provided");
        match result.unwrap_err() {
            UpdatePolicyError::NoUpdatesProvided => {}
            _ => panic!("Expected NoUpdatesProvided error"),
        }
    }

    #[tokio::test]
    async fn test_update_policy_empty_content_fails() {
        // Arrange
        let validator = Arc::new(MockPolicyValidator::new());
        let port = Arc::new(MockUpdatePolicyPort::new());
        let use_case = UpdatePolicyUseCase::new(validator, port);
        let command = UpdatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: Some("   ".to_string()), // Whitespace only
            description: None,
        };

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(
            result.is_err(),
            "Expected error due to empty policy content"
        );
        match result.unwrap_err() {
            UpdatePolicyError::EmptyPolicyContent => {}
            _ => panic!("Expected EmptyPolicyContent error"),
        }
    }

    #[tokio::test]
    async fn test_update_policy_invalid_content_fails() {
        // Arrange
        let validator = Arc::new(MockPolicyValidator::with_errors(vec![
            "Syntax error: invalid token".to_string(),
        ]));
        let port = Arc::new(MockUpdatePolicyPort::new());
        let use_case = UpdatePolicyUseCase::new(validator, port);
        let command = UpdatePolicyCommand::update_content("test-policy", "invalid cedar syntax!!!");

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(
            result.is_err(),
            "Expected error due to invalid policy content"
        );
        match result.unwrap_err() {
            UpdatePolicyError::InvalidPolicyContent(msg) => {
                assert!(msg.contains("Syntax error: invalid token"));
            }
            _ => panic!("Expected InvalidPolicyContent error"),
        }
    }

    #[tokio::test]
    async fn test_update_policy_not_found() {
        // Arrange
        let validator = Arc::new(MockPolicyValidator::new());
        let port = Arc::new(MockUpdatePolicyPort::with_not_found_error());
        let use_case = UpdatePolicyUseCase::new(validator, port);
        let command = UpdatePolicyCommand::update_content(
            "nonexistent",
            "permit(principal, action, resource);",
        );

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err(), "Expected error due to policy not found");
        match result.unwrap_err() {
            UpdatePolicyError::PolicyNotFound(policy_id) => {
                assert_eq!(policy_id, "nonexistent");
            }
            _ => panic!("Expected PolicyNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_update_policy_storage_error() {
        // Arrange
        let validator = Arc::new(MockPolicyValidator::new());
        let port = Arc::new(MockUpdatePolicyPort::with_storage_error());
        let use_case = UpdatePolicyUseCase::new(validator, port);
        let command = create_test_command();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err(), "Expected error due to storage failure");
        match result.unwrap_err() {
            UpdatePolicyError::StorageError(msg) => {
                assert!(msg.contains("Mock storage error"));
            }
            _ => panic!("Expected StorageError"),
        }
    }

    #[tokio::test]
    async fn test_update_policy_validation_service_error() {
        // Arrange
        let validator = Arc::new(MockPolicyValidator::with_service_error());
        let port = Arc::new(MockUpdatePolicyPort::new());
        let use_case = UpdatePolicyUseCase::new(validator, port);
        let command = create_test_command();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(
            result.is_err(),
            "Expected error due to validation service failure"
        );
        match result.unwrap_err() {
            UpdatePolicyError::ValidationFailed(msg) => {
                assert!(msg.contains("Mock validation service error"));
            }
            _ => panic!("Expected ValidationFailed error"),
        }
    }

    #[tokio::test]
    async fn test_update_policy_trait_implementation() {
        // Arrange
        let validator = Arc::new(MockPolicyValidator::new());
        let port = Arc::new(MockUpdatePolicyPort::new());
        let use_case = UpdatePolicyUseCase::new(validator, port);
        let command = create_test_command();

        // Act - Using the UpdatePolicyPort trait directly
        let result = use_case.update(command).await;

        // Assert
        assert!(result.is_ok(), "Expected successful update via trait");
        let view = result.unwrap();
        assert_eq!(view.name, "test-policy");
    }

    #[tokio::test]
    async fn test_update_policy_with_complex_content() {
        // Arrange
        let validator = Arc::new(MockPolicyValidator::new());
        let port = Arc::new(MockUpdatePolicyPort::new());
        let use_case = UpdatePolicyUseCase::new(validator, port);
        let complex_policy = r#"permit(
            principal in Group::"admins",
            action in [Action::"read", Action::"write"],
            resource
        ) when {
            resource.owner == principal
        };"#
        .to_string();

        let command = UpdatePolicyCommand::update_content("complex-policy", &complex_policy);

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(
            result.is_ok(),
            "Expected successful update of complex policy"
        );
        let view = result.unwrap();
        assert_eq!(view.name, "complex-policy");
        assert!(view.content.contains("permit"));
        assert!(view.content.contains("when"));
    }

    #[tokio::test]
    async fn test_update_policy_clear_description() {
        // Arrange
        let validator = Arc::new(MockPolicyValidator::new());
        let port = Arc::new(MockUpdatePolicyPort::new());
        let use_case = UpdatePolicyUseCase::new(validator, port);
        let command = UpdatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: None,
            description: Some("".to_string()), // Empty string should clear description
        };

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok(), "Expected successful description clearing");
        let view = result.unwrap();
        assert_eq!(view.name, "test-policy");
        assert_eq!(view.description, None); // Description should be cleared
    }

    #[tokio::test]
    async fn test_update_policy_with_multiple_validation_errors() {
        // Arrange
        let validator = Arc::new(MockPolicyValidator::with_errors(vec![
            "Error 1: Invalid syntax".to_string(),
            "Error 2: Unknown action".to_string(),
            "Error 3: Missing principal".to_string(),
        ]));
        let port = Arc::new(MockUpdatePolicyPort::new());
        let use_case = UpdatePolicyUseCase::new(validator, port);
        let command = create_test_command();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(
            result.is_err(),
            "Expected error due to multiple validation errors"
        );
        match result.unwrap_err() {
            UpdatePolicyError::InvalidPolicyContent(msg) => {
                assert!(msg.contains("Error 1: Invalid syntax"));
                assert!(msg.contains("Error 2: Unknown action"));
                assert!(msg.contains("Error 3: Missing principal"));
            }
            _ => panic!("Expected InvalidPolicyContent error"),
        }
    }

    #[tokio::test]
    async fn test_update_policy_with_whitespace_content() {
        // Arrange
        let validator = Arc::new(MockPolicyValidator::new());
        let port = Arc::new(MockUpdatePolicyPort::new());
        let use_case = UpdatePolicyUseCase::new(validator, port);
        let command = UpdatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: Some("  permit(principal, action, resource);  ".to_string()), // With surrounding whitespace
            description: None,
        };

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(
            result.is_ok(),
            "Expected successful update with whitespace content"
        );
        let view = result.unwrap();
        assert_eq!(view.name, "test-policy");
        assert_eq!(view.content, "  permit(principal, action, resource);  ");
    }
}
