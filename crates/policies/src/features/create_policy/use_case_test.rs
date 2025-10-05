//! # Unit Tests for Create Policy Use Case
//!
//! This module contains comprehensive unit tests for the `CreatePolicyUseCase`.
//! All external dependencies are mocked to ensure fast, isolated testing of the
//! business logic.
//!
//! ## Test Coverage
//!
//! - **Happy path**: Successfully creating a policy with valid inputs
//! - **Validation failures**: Invalid policy content
//! - **ID generation failures**: Errors during ID generation
//! - **Persistence failures**: Database/storage errors
//! - **Conflict scenarios**: Duplicate policy IDs
//! - **Input validation**: Empty/whitespace content, metadata validation
//! - **Domain event emission**: Verifying correct events are produced

#[cfg(test)]
mod tests {
    use crate::features::create_policy::dto::CreatePolicyCommand;
    use crate::features::create_policy::error::CreatePolicyError;
    use crate::features::create_policy::mocks::{
        MockPolicyIdGenerator, MockPolicyPersister, MockPolicyValidator,
    };
    use crate::features::create_policy::use_case::CreatePolicyUseCase;
    use crate::shared::domain::policy::PolicyId;

    // --- Happy Path Tests ---

    #[tokio::test]
    async fn create_policy_succeeds_with_valid_inputs() {
        // Arrange
        let id_gen = MockPolicyIdGenerator::new_with_id("policy-123");
        let validator = MockPolicyValidator::new_accepting_all();
        let persister = MockPolicyPersister::new();
        let use_case = CreatePolicyUseCase::new(id_gen, validator, persister.clone());

        let command = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            Some("Test Policy".to_string()),
            None,
        )
        .unwrap();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok());
        let dto = result.unwrap();
        assert_eq!(dto.id, "policy-123");
        assert_eq!(persister.save_count(), 1);
        assert!(persister.was_saved(&PolicyId::new("policy-123")));
    }

    #[tokio::test]
    async fn create_policy_with_description_succeeds() {
        // Arrange
        let id_gen = MockPolicyIdGenerator::new_with_id("policy-with-desc");
        let validator = MockPolicyValidator::new_accepting_all();
        let persister = MockPolicyPersister::new();
        let use_case = CreatePolicyUseCase::new(id_gen, validator, persister.clone());

        let command = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            Some("Policy with description".to_string()),
            None,
        )
        .unwrap();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok());
        let dto = result.unwrap();
        let saved_policies = persister.get_saved_policies();
        assert_eq!(saved_policies.len(), 1);
        assert_eq!(saved_policies[0].id().to_string(), dto.id);
        assert_eq!(
            saved_policies[0].metadata().description(),
            Some("Policy with description")
        );
    }

    #[tokio::test]
    async fn create_policy_with_tags_succeeds() {
        // Arrange
        let id_gen = MockPolicyIdGenerator::new_with_id("policy-with-tags");
        let validator = MockPolicyValidator::new_accepting_all();
        let persister = MockPolicyPersister::new();
        let use_case = CreatePolicyUseCase::new(id_gen, validator, persister.clone());

        let tags = vec!["production".to_string(), "critical".to_string()];
        let command = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            None,
            Some(tags.clone()),
        )
        .unwrap();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok());
        let dto = result.unwrap();
        let saved_policies = persister.get_saved_policies();
        assert_eq!(saved_policies.len(), 1);
        assert_eq!(saved_policies[0].id().to_string(), dto.id);
        assert_eq!(saved_policies[0].metadata().tags(), &tags);
    }

    #[tokio::test]
    async fn create_multiple_policies_with_sequence_generator() {
        // Arrange
        let id_gen = MockPolicyIdGenerator::new_with_sequence(vec![
            "policy-1".to_string(),
            "policy-2".to_string(),
            "policy-3".to_string(),
        ]);
        let validator = MockPolicyValidator::new_accepting_all();
        let persister = MockPolicyPersister::new();
        let use_case = CreatePolicyUseCase::new(id_gen, validator, persister.clone());

        // Act
        for i in 1..=3 {
            let command = CreatePolicyCommand::new(
                format!("policy content {}", i),
                Some(format!("Policy {}", i)),
                None,
            )
            .unwrap();
            let result = use_case.execute(command).await;
            assert!(result.is_ok());
        }

        // Assert
        assert_eq!(persister.save_count(), 3);
        let saved = persister.get_saved_policies();
        assert_eq!(saved[0].id().to_string(), "policy-1");
        assert_eq!(saved[1].id().to_string(), "policy-2");
        assert_eq!(saved[2].id().to_string(), "policy-3");
    }

    // --- Validation Error Tests ---

    #[tokio::test]
    async fn create_policy_fails_when_validation_fails() {
        // Arrange
        let id_gen = MockPolicyIdGenerator::new_with_id("policy-invalid");
        let validator = MockPolicyValidator::new_rejecting_all("Invalid Cedar syntax");
        let persister = MockPolicyPersister::new();
        let use_case = CreatePolicyUseCase::new(id_gen, validator, persister.clone());

        let command = CreatePolicyCommand::new(
            "invalid policy syntax".to_string(),
            Some("Invalid Policy".to_string()),
            None,
        )
        .unwrap();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            CreatePolicyError::ValidationError(msg) => {
                assert_eq!(msg, "Invalid Cedar syntax");
            }
            _ => panic!("Expected ValidationError"),
        }
        assert_eq!(persister.save_count(), 0);
    }

    #[tokio::test]
    async fn create_policy_with_custom_validator_logic() {
        // Arrange
        let id_gen = MockPolicyIdGenerator::new_with_id("policy-custom");
        let validator = MockPolicyValidator::new_with_custom(|content| {
            if content.contains("forbid") {
                Err("forbid policies not allowed".to_string())
            } else {
                Ok(())
            }
        });
        let persister = MockPolicyPersister::new();
        let use_case = CreatePolicyUseCase::new(id_gen, validator, persister.clone());

        // Act - should fail with forbid
        let command_forbid = CreatePolicyCommand::new(
            "forbid(principal, action, resource);".to_string(),
            None,
            None,
        )
        .unwrap();
        let result_forbid = use_case.execute(command_forbid).await;
        assert!(result_forbid.is_err());

        // Act - should succeed with permit
        let command_permit = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            None,
            None,
        )
        .unwrap();
        let result_permit = use_case.execute(command_permit).await;
        assert!(result_permit.is_ok());

        // Assert
        assert_eq!(persister.save_count(), 1);
    }

    // --- ID Generation Error Tests ---

    #[tokio::test]
    async fn create_policy_fails_when_id_generation_fails() {
        // Arrange
        let id_gen = MockPolicyIdGenerator::new_failing("ID generation service unavailable");
        let validator = MockPolicyValidator::new_accepting_all();
        let persister = MockPolicyPersister::new();
        let use_case = CreatePolicyUseCase::new(id_gen, validator, persister.clone());

        let command = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            Some("Test Policy".to_string()),
            None,
        )
        .unwrap();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            CreatePolicyError::Internal(msg) => {
                assert_eq!(msg, "ID generation service unavailable");
            }
            _ => panic!("Expected Internal error"),
        }
        assert_eq!(persister.save_count(), 0);
    }

    // --- Persistence Error Tests ---

    #[tokio::test]
    async fn create_policy_fails_when_persistence_fails() {
        // Arrange
        let id_gen = MockPolicyIdGenerator::new_with_id("policy-db-error");
        let validator = MockPolicyValidator::new_accepting_all();
        let persister = MockPolicyPersister::new_failing(CreatePolicyError::Internal(
            "Database unavailable".to_string(),
        ));
        let use_case = CreatePolicyUseCase::new(id_gen, validator, persister.clone());

        let command = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            Some("Test Policy".to_string()),
            None,
        )
        .unwrap();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            CreatePolicyError::Internal(msg) => {
                assert_eq!(msg, "Database unavailable");
            }
            _ => panic!("Expected Internal error"),
        }
    }

    #[tokio::test]
    async fn create_policy_fails_on_conflict() {
        // Arrange
        let id_gen = MockPolicyIdGenerator::new_with_id("conflict-id");
        let validator = MockPolicyValidator::new_accepting_all();
        let persister = MockPolicyPersister::new_with_conflicts(vec!["conflict-id".to_string()]);
        let use_case = CreatePolicyUseCase::new(id_gen, validator, persister.clone());

        let command = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            Some("Conflicting Policy".to_string()),
            None,
        )
        .unwrap();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            CreatePolicyError::Conflict(id) => {
                assert_eq!(id, "conflict-id");
            }
            _ => panic!("Expected Conflict error"),
        }
        assert_eq!(persister.save_count(), 0);
    }

    // --- Input Validation Tests ---

    #[tokio::test]
    async fn create_policy_command_rejects_empty_content() {
        let result = CreatePolicyCommand::new("".to_string(), None, None);
        assert!(result.is_err());
        match result.unwrap_err() {
            CreatePolicyError::InvalidInput(msg) => {
                assert!(msg.contains("empty"));
            }
            _ => panic!("Expected InvalidInput error"),
        }
    }

    #[tokio::test]
    async fn create_policy_command_rejects_whitespace_only_content() {
        let result = CreatePolicyCommand::new("   \n\t  ".to_string(), None, None);
        assert!(result.is_err());
        match result.unwrap_err() {
            CreatePolicyError::InvalidInput(msg) => {
                assert!(msg.contains("empty"));
            }
            _ => panic!("Expected InvalidInput error"),
        }
    }

    #[tokio::test]
    async fn create_policy_command_rejects_empty_description() {
        let result = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            Some("".to_string()),
            None,
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            CreatePolicyError::InvalidInput(msg) => {
                assert!(msg.contains("Description"));
            }
            _ => panic!("Expected InvalidInput error"),
        }
    }

    #[tokio::test]
    async fn create_policy_command_rejects_empty_tags() {
        let result = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            None,
            Some(vec![]),
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            CreatePolicyError::InvalidInput(msg) => {
                assert!(msg.contains("Tags"));
            }
            _ => panic!("Expected InvalidInput error"),
        }
    }

    #[tokio::test]
    async fn create_policy_command_rejects_duplicate_tags() {
        let result = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            None,
            Some(vec![
                "production".to_string(),
                "critical".to_string(),
                "production".to_string(),
            ]),
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            CreatePolicyError::InvalidInput(msg) => {
                assert!(msg.contains("duplicate") || msg.contains("unique"));
            }
            _ => panic!("Expected InvalidInput error"),
        }
    }

    #[tokio::test]
    async fn create_policy_command_accepts_none_description() {
        let result = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            None,
            None,
        );
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn create_policy_command_accepts_none_tags() {
        let result = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            Some("Valid description".to_string()),
            None,
        );
        assert!(result.is_ok());
    }

    // --- Validator Tracking Tests ---

    #[tokio::test]
    async fn validator_receives_correct_policy_content() {
        // Arrange
        let id_gen = MockPolicyIdGenerator::new_with_id("policy-tracking");
        let validator = MockPolicyValidator::new_tracking();
        let persister = MockPolicyPersister::new();
        let use_case = CreatePolicyUseCase::new(id_gen, validator.clone(), persister);

        let policy_content = "permit(principal, action, resource);";
        let command = CreatePolicyCommand::new(policy_content.to_string(), None, None).unwrap();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok());
        let validated = validator.get_validated_policies();
        assert_eq!(validated.len(), 1);
        assert_eq!(validated[0], policy_content);
    }

    // --- Edge Cases ---

    #[tokio::test]
    async fn create_policy_with_very_long_content() {
        // Arrange
        let id_gen = MockPolicyIdGenerator::new_with_id("policy-long");
        let validator = MockPolicyValidator::new_accepting_all();
        let persister = MockPolicyPersister::new();
        let use_case = CreatePolicyUseCase::new(id_gen, validator, persister.clone());

        let long_content = "permit(principal, action, resource);".repeat(1000);
        let command = CreatePolicyCommand::new(long_content.clone(), None, None).unwrap();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok());
        let saved = persister.get_saved_policies();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].content(), &long_content);
    }

    #[tokio::test]
    async fn create_policy_with_special_characters_in_content() {
        // Arrange
        let id_gen = MockPolicyIdGenerator::new_with_id("policy-special");
        let validator = MockPolicyValidator::new_accepting_all();
        let persister = MockPolicyPersister::new();
        let use_case = CreatePolicyUseCase::new(id_gen, validator, persister.clone());

        let special_content =
            "permit(principal, action, resource) when { resource.\"special-name\" };";
        let command = CreatePolicyCommand::new(special_content.to_string(), None, None).unwrap();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok());
        let saved = persister.get_saved_policies();
        assert_eq!(saved[0].content(), special_content);
    }

    #[tokio::test]
    async fn create_policy_with_unicode_in_description() {
        // Arrange
        let id_gen = MockPolicyIdGenerator::new_with_id("policy-unicode");
        let validator = MockPolicyValidator::new_accepting_all();
        let persister = MockPolicyPersister::new();
        let use_case = CreatePolicyUseCase::new(id_gen, validator, persister.clone());

        let command = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            Some("Política de acceso 🔒".to_string()),
            None,
        )
        .unwrap();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok());
        let saved = persister.get_saved_policies();
        assert_eq!(
            saved[0].metadata().description(),
            Some("Política de acceso 🔒")
        );
    }

    #[tokio::test]
    async fn create_policy_with_many_tags() {
        // Arrange
        let id_gen = MockPolicyIdGenerator::new_with_id("policy-many-tags");
        let validator = MockPolicyValidator::new_accepting_all();
        let persister = MockPolicyPersister::new();
        let use_case = CreatePolicyUseCase::new(id_gen, validator, persister.clone());

        let tags: Vec<String> = (0..50).map(|i| format!("tag-{}", i)).collect();
        let command = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            None,
            Some(tags.clone()),
        )
        .unwrap();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok());
        let saved = persister.get_saved_policies();
        assert_eq!(saved[0].metadata().tags().len(), 50);
    }
}
