// crates/iam/src/features/create_policy/use_case_test.rs

#[cfg(test)]
mod tests {
    use crate::domain::policy::{Policy, PolicyStatus};
    use crate::domain::validation::ValidationResult;
    use crate::features::create_policy::dto::CreatePolicyCommand;
    use crate::features::create_policy::ports::{
        PolicyCreator, PolicyEventPublisher, PolicyValidator,
    };
    use crate::features::create_policy::use_case::CreatePolicyUseCase;
    use crate::infrastructure::errors::{IamError, ValidationError};
    use async_trait::async_trait;
    use shared::hrn::PolicyId;
    use std::sync::{Arc, Mutex};

    // Mock implementations for testing
    struct MockPolicyCreator {
        should_fail: bool,
        should_exist: bool,
        created_policies: Arc<Mutex<Vec<Policy>>>,
    }

    impl MockPolicyCreator {
        fn new() -> Self {
            Self {
                should_fail: false,
                should_exist: false,
                created_policies: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn with_failure() -> Self {
            Self {
                should_fail: true,
                should_exist: false,
                created_policies: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn with_existing_policy() -> Self {
            Self {
                should_fail: false,
                should_exist: true,
                created_policies: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn get_created_policies(&self) -> Vec<Policy> {
            self.created_policies.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl PolicyCreator for MockPolicyCreator {
        async fn create(&self, policy: Policy) -> Result<Policy, IamError> {
            if self.should_fail {
                return Err(IamError::DatabaseError("Mock database error".to_string()));
            }

            self.created_policies.lock().unwrap().push(policy.clone());
            Ok(policy)
        }

        async fn exists(&self, _id: &PolicyId) -> Result<bool, IamError> {
            Ok(self.should_exist)
        }
    }

    struct MockPolicyValidator {
        should_fail: bool,
    }

    impl MockPolicyValidator {
        fn new() -> Self {
            Self { should_fail: false }
        }

        fn with_failure() -> Self {
            Self { should_fail: true }
        }
    }

    #[async_trait]
    impl PolicyValidator for MockPolicyValidator {
        async fn validate_syntax(&self, _content: &str) -> Result<ValidationResult, IamError> {
            if self.should_fail {
                Ok(ValidationResult::invalid(vec![ValidationError {
                    message: "Mock validation error".to_string(),
                    line: Some(1),
                    column: Some(1),
                }]))
            } else {
                Ok(ValidationResult::valid())
            }
        }

        async fn validate_semantics(&self, _content: &str) -> Result<(), IamError> {
            if self.should_fail {
                Err(IamError::validation_error(
                    "Mock semantic validation error".to_string(),
                ))
            } else {
                Ok(())
            }
        }
    }

    struct MockEventPublisher {
        published_events: Arc<Mutex<Vec<String>>>,
    }

    impl MockEventPublisher {
        fn new() -> Self {
            Self {
                published_events: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn get_published_events(&self) -> Vec<String> {
            self.published_events.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl PolicyEventPublisher for MockEventPublisher {
        async fn publish_policy_created(&self, policy: &Policy) -> Result<(), IamError> {
            self.published_events
                .lock()
                .unwrap()
                .push(format!("PolicyCreated: {}", policy.id.0.to_string()));
            Ok(())
        }
    }

    fn create_test_command() -> CreatePolicyCommand {
        CreatePolicyCommand::new(
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "test_user".to_string(),
        )
    }

    #[tokio::test]
    async fn test_create_policy_success() {
        // Arrange
        let creator = Arc::new(MockPolicyCreator::new());
        let validator = Arc::new(MockPolicyValidator::new());
        let publisher = Arc::new(MockEventPublisher::new());

        let use_case = CreatePolicyUseCase::new(creator.clone(), validator, publisher.clone());

        let command = create_test_command();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policy.name, "Test Policy");
        assert_eq!(response.policy.status, PolicyStatus::Draft);
        assert_eq!(response.policy.metadata.version, 1);
        assert_eq!(response.policy.metadata.created_by, "test_user");

        // Verify policy was created
        let created_policies = creator.get_created_policies();
        assert_eq!(created_policies.len(), 1);

        // Verify event was published
        let events = publisher.get_published_events();
        assert_eq!(events.len(), 1);
        assert!(events[0].starts_with("PolicyCreated:"));
    }

    #[tokio::test]
    async fn test_create_policy_validation_failure() {
        // Arrange
        let creator = Arc::new(MockPolicyCreator::new());
        let validator = Arc::new(MockPolicyValidator::with_failure());
        let publisher = Arc::new(MockEventPublisher::new());

        let use_case = CreatePolicyUseCase::new(creator.clone(), validator, publisher);
        let command = create_test_command();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            IamError::PolicyValidationFailed { errors } => {
                assert!(!errors.is_empty());
                assert_eq!(errors[0].message, "Mock validation error");
            }
            _ => panic!("Expected validation error"),
        }

        // Verify no policy was created
        let created_policies = creator.get_created_policies();
        assert_eq!(created_policies.len(), 0);
    }

    #[tokio::test]
    async fn test_create_policy_already_exists() {
        // Arrange
        let creator = Arc::new(MockPolicyCreator::with_existing_policy());
        let validator = Arc::new(MockPolicyValidator::new());
        let publisher = Arc::new(MockEventPublisher::new());

        let use_case = CreatePolicyUseCase::new(creator.clone(), validator, publisher);
        let command = create_test_command();

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            IamError::PolicyAlreadyExists(_) => {
                // Expected
            }
            _ => panic!("Expected PolicyAlreadyExists error"),
        }

        // Verify no policy was created
        let created_policies = creator.get_created_policies();
        assert_eq!(created_policies.len(), 0);
    }

    #[tokio::test]
    async fn test_create_policy_database_failure() {
        // Arrange
        let creator = Arc::new(MockPolicyCreator::with_failure());
        let validator = Arc::new(MockPolicyValidator::new());
        let publisher = Arc::new(MockEventPublisher::new());

        let use_case = CreatePolicyUseCase::new(creator, validator, publisher);
        let command = create_test_command();

        // Act
        let result = use_case.execute(command).await;

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
    async fn test_create_policy_with_description_and_tags() {
        // Arrange
        let creator = Arc::new(MockPolicyCreator::new());
        let validator = Arc::new(MockPolicyValidator::new());
        let publisher = Arc::new(MockEventPublisher::new());

        let use_case = CreatePolicyUseCase::new(creator.clone(), validator, publisher);

        let command = CreatePolicyCommand::new(
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "test_user".to_string(),
        )
        .with_description("Test description".to_string())
        .with_tags(vec!["test".to_string(), "policy".to_string()]);

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(
            response.policy.description,
            Some("Test description".to_string())
        );
        assert_eq!(response.policy.metadata.tags, vec!["test", "policy"]);
    }

    #[tokio::test]
    async fn test_create_policy_command_validation() {
        // Test empty name
        let mut command = CreatePolicyCommand::new(
            "".to_string(),
            "permit(principal, action, resource);".to_string(),
            "test_user".to_string(),
        );
        assert!(command.validate().is_err());

        // Test empty content
        command = CreatePolicyCommand::new(
            "Test Policy".to_string(),
            "".to_string(),
            "test_user".to_string(),
        );
        assert!(command.validate().is_err());

        // Test empty created_by
        command = CreatePolicyCommand::new(
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "".to_string(),
        );
        assert!(command.validate().is_err());

        // Test too many tags
        command = CreatePolicyCommand::new(
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "test_user".to_string(),
        )
        .with_tags((0..15).map(|i| format!("tag{}", i)).collect());
        assert!(command.validate().is_err());

        // Test valid command
        command = create_test_command();
        assert!(command.validate().is_ok());
    }
}
