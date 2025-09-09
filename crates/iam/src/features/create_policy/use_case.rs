// crates/iam/src/features/create_policy/use_case.rs

use crate::domain::policy::{Policy, PolicyMetadata, PolicyStatus};
use crate::features::create_policy::dto::{CreatePolicyCommand, CreatePolicyResponse};
use crate::features::create_policy::ports::{PolicyCreator, PolicyValidator, PolicyEventPublisher};
use crate::infrastructure::errors::IamError;
use shared::hrn::{Hrn, PolicyId};
use std::sync::Arc;
use time::OffsetDateTime;

/// Use case for creating new policies
/// Contains pure business logic without infrastructure concerns
pub struct CreatePolicyUseCase {
    creator: Arc<dyn PolicyCreator>,
    validator: Arc<dyn PolicyValidator>,
    event_publisher: Arc<dyn PolicyEventPublisher>,
}

impl CreatePolicyUseCase {
    /// Create a new create policy use case
    pub fn new(
        creator: Arc<dyn PolicyCreator>,
        validator: Arc<dyn PolicyValidator>,
        event_publisher: Arc<dyn PolicyEventPublisher>,
    ) -> Self {
        Self {
            creator,
            validator,
            event_publisher,
        }
    }

    /// Execute the create policy use case
    pub async fn execute(&self, command: CreatePolicyCommand) -> Result<CreatePolicyResponse, IamError> {
        // 1. Validate command
        command.validate()?;

        // 2. Validate Cedar policy syntax
        let validation_result = self.validator.validate_syntax(&command.content).await?;
        if !validation_result.is_valid {
            return Err(IamError::validation_error(
                validation_result.first_error_message()
                    .unwrap_or("Invalid policy syntax")
                    .to_string(),
            ));
        }

        // 3. Validate Cedar policy semantics against schema
        self.validator.validate_semantics(&command.content).await?;

        // 4. Generate policy ID
        let policy_id = self.generate_policy_id(&command.name)?;

        // 5. Check if policy already exists
        if self.creator.exists(&policy_id).await? {
            return Err(IamError::PolicyAlreadyExists(policy_id));
        }

        // 6. Create domain entity
        let now = OffsetDateTime::now_utc();
        let policy = Policy {
            id: policy_id,
            name: command.name.clone(),
            description: command.description.clone(),
            content: command.content,
            status: PolicyStatus::Draft,
            metadata: PolicyMetadata {
                created_at: now,
                created_by: command.created_by.clone(),
                updated_at: now,
                updated_by: command.created_by,
                version: 1,
                tags: command.tags.unwrap_or_default(),
            },
        };

        // 7. Persist policy
        let created_policy = self.creator.create(policy).await?;

        // 8. Publish domain event
        self.event_publisher
            .publish_policy_created(&created_policy)
            .await?;

        // 9. Return response
        Ok(CreatePolicyResponse::new(created_policy))
    }

    /// Generate a unique policy ID based on the policy name
    fn generate_policy_id(&self, name: &str) -> Result<PolicyId, IamError> {
        // Create a URL-safe version of the name
        let safe_name = name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect::<String>();

        // Add timestamp to ensure uniqueness
        let timestamp = OffsetDateTime::now_utc().unix_timestamp();
        let policy_name = format!("{}_{}", safe_name, timestamp);

        // Create HRN for the policy
        let hrn_string = format!("hrn:hodei:iam:global:policy/{}", policy_name);
        let hrn = Hrn::new(&hrn_string)
            .map_err(|e| IamError::InvalidInput(format!("Failed to create policy ID: {}", e)))?;

        Ok(PolicyId(hrn))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::validation::ValidationResult;
    use crate::infrastructure::errors::ValidationError;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;

    // Mock implementations for testing
    struct MockPolicyCreator {
        policies: Mutex<HashMap<String, Policy>>,
        should_fail_create: bool,
        should_fail_exists: bool,
        policy_exists: bool,
    }

    impl MockPolicyCreator {
        fn new() -> Self {
            Self {
                policies: Mutex::new(HashMap::new()),
                should_fail_create: false,
                should_fail_exists: false,
                policy_exists: false,
            }
        }

        fn with_create_failure() -> Self {
            Self {
                policies: Mutex::new(HashMap::new()),
                should_fail_create: true,
                should_fail_exists: false,
                policy_exists: false,
            }
        }

        fn with_existing_policy() -> Self {
            Self {
                policies: Mutex::new(HashMap::new()),
                should_fail_create: false,
                should_fail_exists: false,
                policy_exists: true,
            }
        }
    }

    #[async_trait]
    impl PolicyCreator for MockPolicyCreator {
        async fn create(&self, policy: Policy) -> Result<Policy, IamError> {
            if self.should_fail_create {
                return Err(IamError::DatabaseError("Mock database error".to_string()));
            }

            let mut policies = self.policies.lock().unwrap();
            let key = policy.id.0.to_string();
            policies.insert(key, policy.clone());
            Ok(policy)
        }

        async fn exists(&self, _id: &PolicyId) -> Result<bool, IamError> {
            if self.should_fail_exists {
                return Err(IamError::DatabaseError("Mock exists check error".to_string()));
            }
            Ok(self.policy_exists)
        }
    }

    struct MockPolicyValidator {
        should_fail_syntax: bool,
        should_fail_semantics: bool,
    }

    impl MockPolicyValidator {
        fn new() -> Self {
            Self { 
                should_fail_syntax: false,
                should_fail_semantics: false,
            }
        }

        fn with_syntax_failure() -> Self {
            Self { 
                should_fail_syntax: true,
                should_fail_semantics: false,
            }
        }

        fn with_semantic_failure() -> Self {
            Self { 
                should_fail_syntax: false,
                should_fail_semantics: true,
            }
        }
    }

    #[async_trait]
    impl PolicyValidator for MockPolicyValidator {
        async fn validate_syntax(&self, _content: &str) -> Result<ValidationResult, IamError> {
            if self.should_fail_syntax {
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
            if self.should_fail_semantics {
                Err(IamError::validation_error("Mock semantic validation error".to_string()))
            } else {
                Ok(())
            }
        }
    }

    struct MockPolicyEventPublisher {
        published_events: Mutex<Vec<String>>,
        should_fail: bool,
    }

    impl MockPolicyEventPublisher {
        fn new() -> Self {
            Self {
                published_events: Mutex::new(Vec::new()),
                should_fail: false,
            }
        }

        fn with_failure() -> Self {
            Self {
                published_events: Mutex::new(Vec::new()),
                should_fail: true,
            }
        }

        fn get_published_events(&self) -> Vec<String> {
            self.published_events.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl PolicyEventPublisher for MockPolicyEventPublisher {
        async fn publish_policy_created(&self, policy: &Policy) -> Result<(), IamError> {
            if self.should_fail {
                return Err(IamError::InternalError("Mock event publish error".to_string()));
            }

            let mut events = self.published_events.lock().unwrap();
            events.push(format!("PolicyCreated: {}", policy.name));
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_create_policy_success() {
        let creator = Arc::new(MockPolicyCreator::new());
        let validator = Arc::new(MockPolicyValidator::new());
        let event_publisher = Arc::new(MockPolicyEventPublisher::new());
        
        let use_case = CreatePolicyUseCase::new(creator, validator, event_publisher.clone());
        
        let command = CreatePolicyCommand::new(
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        );

        let result = use_case.execute(command).await;
        
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policy.name, "Test Policy");
        assert_eq!(response.policy.status, PolicyStatus::Draft);
        assert_eq!(response.policy.metadata.version, 1);
        assert_eq!(response.policy.metadata.created_by, "user_123");

        // Verify event was published
        let events = event_publisher.get_published_events();
        assert_eq!(events.len(), 1);
        assert!(events[0].contains("PolicyCreated: Test Policy"));
    }

    #[tokio::test]
    async fn test_create_policy_with_description_and_tags() {
        let creator = Arc::new(MockPolicyCreator::new());
        let validator = Arc::new(MockPolicyValidator::new());
        let event_publisher = Arc::new(MockPolicyEventPublisher::new());
        
        let use_case = CreatePolicyUseCase::new(creator, validator, event_publisher);
        
        let command = CreatePolicyCommand::new(
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        )
        .with_description("A test policy".to_string())
        .with_tags(vec!["engineering".to_string(), "test".to_string()]);

        let result = use_case.execute(command).await;
        
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policy.name, "Test Policy");
        assert_eq!(response.policy.description, Some("A test policy".to_string()));
        assert_eq!(response.policy.metadata.tags, vec!["engineering", "test"]);
    }

    #[tokio::test]
    async fn test_create_policy_syntax_validation_failure() {
        let creator = Arc::new(MockPolicyCreator::new());
        let validator = Arc::new(MockPolicyValidator::with_syntax_failure());
        let event_publisher = Arc::new(MockPolicyEventPublisher::new());
        
        let use_case = CreatePolicyUseCase::new(creator, validator, event_publisher);
        
        let command = CreatePolicyCommand::new(
            "Test Policy".to_string(),
            "invalid policy content".to_string(),
            "user_123".to_string(),
        );

        let result = use_case.execute(command).await;
        
        assert!(result.is_err());
        match result.unwrap_err() {
            IamError::PolicyValidationFailed { errors } => {
                assert_eq!(errors.len(), 1);
                assert_eq!(errors[0].message, "Mock validation error");
            }
            _ => panic!("Expected PolicyValidationFailed error"),
        }
    }

    #[tokio::test]
    async fn test_create_policy_semantic_validation_failure() {
        let creator = Arc::new(MockPolicyCreator::new());
        let validator = Arc::new(MockPolicyValidator::with_semantic_failure());
        let event_publisher = Arc::new(MockPolicyEventPublisher::new());
        
        let use_case = CreatePolicyUseCase::new(creator, validator, event_publisher);
        
        let command = CreatePolicyCommand::new(
            "Test Policy".to_string(),
            "permit(principal == UnknownEntity::\"alice\", action == ReadArtifact, resource);".to_string(),
            "user_123".to_string(),
        );

        let result = use_case.execute(command).await;
        
        assert!(result.is_err());
        match result.unwrap_err() {
            IamError::PolicyValidationFailed { errors } => {
                assert_eq!(errors.len(), 1);
                assert!(errors[0].message.contains("Mock semantic validation error"));
            }
            other => panic!("Expected PolicyValidationFailed error, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_create_policy_invalid_command() {
        let creator = Arc::new(MockPolicyCreator::new());
        let validator = Arc::new(MockPolicyValidator::new());
        let event_publisher = Arc::new(MockPolicyEventPublisher::new());
        
        let use_case = CreatePolicyUseCase::new(creator, validator, event_publisher);
        
        let command = CreatePolicyCommand::new(
            "".to_string(), // Invalid empty name
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        );

        let result = use_case.execute(command).await;
        
        assert!(result.is_err());
        match result.unwrap_err() {
            IamError::InvalidInput(msg) => {
                assert!(msg.contains("Name cannot be empty"));
            }
            _ => panic!("Expected InvalidInput error"),
        }
    }

    #[tokio::test]
    async fn test_create_policy_already_exists() {
        let creator = Arc::new(MockPolicyCreator::with_existing_policy());
        let validator = Arc::new(MockPolicyValidator::new());
        let event_publisher = Arc::new(MockPolicyEventPublisher::new());
        
        let use_case = CreatePolicyUseCase::new(creator, validator, event_publisher);
        
        let command = CreatePolicyCommand::new(
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        );

        let result = use_case.execute(command).await;
        
        assert!(result.is_err());
        match result.unwrap_err() {
            IamError::PolicyAlreadyExists(_) => {
                // Expected
            }
            _ => panic!("Expected PolicyAlreadyExists error"),
        }
    }

    #[tokio::test]
    async fn test_create_policy_repository_failure() {
        let creator = Arc::new(MockPolicyCreator::with_create_failure());
        let validator = Arc::new(MockPolicyValidator::new());
        let event_publisher = Arc::new(MockPolicyEventPublisher::new());
        
        let use_case = CreatePolicyUseCase::new(creator, validator, event_publisher);
        
        let command = CreatePolicyCommand::new(
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        );

        let result = use_case.execute(command).await;
        
        assert!(result.is_err());
        match result.unwrap_err() {
            IamError::DatabaseError(msg) => {
                assert_eq!(msg, "Mock database error");
            }
            _ => panic!("Expected DatabaseError"),
        }
    }

    #[tokio::test]
    async fn test_create_policy_event_publish_failure() {
        let creator = Arc::new(MockPolicyCreator::new());
        let validator = Arc::new(MockPolicyValidator::new());
        let event_publisher = Arc::new(MockPolicyEventPublisher::with_failure());
        
        let use_case = CreatePolicyUseCase::new(creator, validator, event_publisher);
        
        let command = CreatePolicyCommand::new(
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        );

        let result = use_case.execute(command).await;
        
        assert!(result.is_err());
        match result.unwrap_err() {
            IamError::InternalError(msg) => {
                assert_eq!(msg, "Mock event publish error");
            }
            _ => panic!("Expected InternalError"),
        }
    }

    #[test]
    fn test_generate_policy_id() {
        let creator = Arc::new(MockPolicyCreator::new());
        let validator = Arc::new(MockPolicyValidator::new());
        let event_publisher = Arc::new(MockPolicyEventPublisher::new());
        
        let use_case = CreatePolicyUseCase::new(creator, validator, event_publisher);
        
        let policy_id = use_case.generate_policy_id("Test Policy");
        assert!(policy_id.is_ok());
        
        let id = policy_id.unwrap();
        let hrn_string = id.0.to_string();
        assert!(hrn_string.starts_with("hrn:hodei:iam:global:policy/"));
        assert!(hrn_string.contains("test_policy_"));
    }

    #[test]
    fn test_generate_policy_id_with_special_characters() {
        let creator = Arc::new(MockPolicyCreator::new());
        let validator = Arc::new(MockPolicyValidator::new());
        let event_publisher = Arc::new(MockPolicyEventPublisher::new());
        
        let use_case = CreatePolicyUseCase::new(creator, validator, event_publisher);
        
        let policy_id = use_case.generate_policy_id("Test Policy @#$%");
        assert!(policy_id.is_ok());
        
        let id = policy_id.unwrap();
        let hrn_string = id.0.to_string();
        assert!(hrn_string.starts_with("hrn:hodei:iam:global:policy/"));
        assert!(hrn_string.contains("test_policy_____"));
    }
}