//! Mock implementations for testing the create_policy feature
//!
//! This module provides mock implementations of the ports used by
//! CreatePolicyUseCase, allowing for isolated unit testing without
//! requiring real infrastructure (databases, validation services, etc.)

use crate::features::create_policy_new::dto::CreatePolicyCommand;
use crate::features::create_policy_new::error::CreatePolicyError;
use crate::features::create_policy_new::ports::{
    CreatePolicyPort, PolicyValidationError, PolicyValidator, ValidationError, ValidationResult,
    ValidationWarning,
};
use async_trait::async_trait;
use policies::shared::domain::Policy;
use std::sync::{Arc, Mutex};

/// Mock implementation of PolicyValidator for testing
///
/// This mock allows tests to configure validation behavior:
/// - Return success/failure
/// - Inject specific validation errors
/// - Simulate service failures
#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct MockPolicyValidator {
    /// If true, the validate_policy call will fail with a service error
    pub should_fail_service: bool,

    /// List of validation error messages to return
    /// If empty, validation is considered successful
    pub validation_errors: Vec<String>,

    /// List of validation warnings to return
    pub validation_warnings: Vec<(String, String)>, // (message, severity)

    /// Counter tracking how many times validate_policy was called
    pub call_count: Arc<Mutex<usize>>,
}

impl MockPolicyValidator {
    /// Create a new mock validator that will succeed
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a mock that will fail validation with the given errors
    #[allow(dead_code)]
    pub fn with_errors(errors: Vec<String>) -> Self {
        Self {
            validation_errors: errors,
            ..Default::default()
        }
    }

    /// Create a mock that will fail with a service error
    #[allow(dead_code)]
    pub fn with_service_error() -> Self {
        Self {
            should_fail_service: true,
            ..Default::default()
        }
    }

    /// Add a validation warning
    #[allow(dead_code)]
    pub fn add_warning(&mut self, message: String, severity: String) {
        self.validation_warnings.push((message, severity));
    }

    /// Get the number of times validate_policy was called
    #[allow(dead_code)]
    pub fn get_call_count(&self) -> usize {
        *self.call_count.lock().unwrap()
    }
}

#[async_trait]
impl PolicyValidator for MockPolicyValidator {
    async fn validate_policy(
        &self,
        _policy_content: &str,
    ) -> Result<ValidationResult, PolicyValidationError> {
        // Increment call counter
        *self.call_count.lock().unwrap() += 1;

        // Simulate service failure if configured
        if self.should_fail_service {
            return Err(PolicyValidationError::ServiceError(
                "Mock validation service error".to_string(),
            ));
        }

        // Determine if validation is successful
        let is_valid = self.validation_errors.is_empty();

        // Build validation errors
        let errors = self
            .validation_errors
            .iter()
            .map(|msg| ValidationError {
                message: msg.clone(),
                line: None,
                column: None,
            })
            .collect();

        // Build validation warnings
        let warnings = self
            .validation_warnings
            .iter()
            .map(|(msg, severity)| ValidationWarning {
                message: msg.clone(),
                severity: severity.clone(),
            })
            .collect();

        Ok(ValidationResult {
            is_valid,
            errors,
            warnings,
        })
    }
}

/// Mock implementation of CreatePolicyPort for testing
///
/// This mock allows tests to:
/// - Configure success/failure scenarios
/// - Track created policies
/// - Simulate storage errors
/// - Simulate duplicate policy errors
#[allow(dead_code)]
#[derive(Debug)]
pub struct MockCreatePolicyPort {
    /// If true, create() will fail with a storage error
    pub should_fail_storage: bool,

    /// If true, create() will fail with PolicyAlreadyExists error
    pub should_fail_duplicate: bool,

    /// Policy IDs that should be considered as "already existing"
    pub existing_policy_ids: Vec<String>,

    /// List of policies that were successfully created
    pub created_policies: Arc<Mutex<Vec<Policy>>>,

    /// Counter tracking how many times create was called
    pub call_count: Arc<Mutex<usize>>,
}

impl Default for MockCreatePolicyPort {
    fn default() -> Self {
        Self {
            should_fail_storage: false,
            should_fail_duplicate: false,
            existing_policy_ids: vec![],
            created_policies: Arc::new(Mutex::new(vec![])),
            call_count: Arc::new(Mutex::new(0)),
        }
    }
}

impl MockCreatePolicyPort {
    /// Create a new mock port that will succeed
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a mock that will fail with a storage error
    #[allow(dead_code)]
    pub fn with_storage_error() -> Self {
        Self {
            should_fail_storage: true,
            ..Default::default()
        }
    }

    /// Create a mock that will fail with PolicyAlreadyExists error
    #[allow(dead_code)]
    pub fn with_duplicate_error() -> Self {
        Self {
            should_fail_duplicate: true,
            ..Default::default()
        }
    }

    /// Create a mock with pre-existing policy IDs
    #[allow(dead_code)]
    pub fn with_existing_policies(policy_ids: Vec<String>) -> Self {
        Self {
            existing_policy_ids: policy_ids,
            ..Default::default()
        }
    }

    /// Get the number of successfully created policies
    #[allow(dead_code)]
    pub fn get_created_count(&self) -> usize {
        self.created_policies.lock().unwrap().len()
    }

    /// Get the number of times create was called
    #[allow(dead_code)]
    pub fn get_call_count(&self) -> usize {
        *self.call_count.lock().unwrap()
    }

    /// Get a clone of all created policies
    #[allow(dead_code)]
    pub fn get_created_policies(&self) -> Vec<Policy> {
        self.created_policies.lock().unwrap().clone()
    }

    /// Check if a specific policy ID was created
    #[allow(dead_code)]
    pub fn has_policy(&self, policy_id: &str) -> bool {
        self.created_policies
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.id().to_string().contains(policy_id))
    }
}

#[async_trait]
impl CreatePolicyPort for MockCreatePolicyPort {
    async fn create(&self, command: CreatePolicyCommand) -> Result<Policy, CreatePolicyError> {
        // Increment call counter
        *self.call_count.lock().unwrap() += 1;

        // Simulate storage error if configured
        if self.should_fail_storage {
            return Err(CreatePolicyError::StorageError(
                "Mock storage error: database connection failed".to_string(),
            ));
        }

        // Simulate duplicate error if configured
        if self.should_fail_duplicate {
            return Err(CreatePolicyError::PolicyAlreadyExists(
                command.policy_id.clone(),
            ));
        }

        // Check if policy ID already exists in the "existing" list
        if self.existing_policy_ids.contains(&command.policy_id) {
            return Err(CreatePolicyError::PolicyAlreadyExists(
                command.policy_id.clone(),
            ));
        }

        // Create a mock policy with the command data using domain constructors (no private field access)
        let policy_id_str = format!("hrn:hodei:iam::test-account:policy/{}", command.policy_id);
        let policy_id = policies::shared::domain::policy::PolicyId::new(policy_id_str);
        let metadata = policies::shared::domain::policy::PolicyMetadata::new(
            command.description.clone(),
            vec![],
        );
        let policy = Policy::new(policy_id, command.policy_content, metadata);

        // Store the created policy
        self.created_policies.lock().unwrap().push(policy.clone());

        Ok(policy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_validator_success() {
        let validator = MockPolicyValidator::new();
        let result = validator.validate_policy("permit(...)").await.unwrap();
        assert!(result.is_valid);
        assert_eq!(validator.get_call_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_validator_with_errors() {
        let validator = MockPolicyValidator::with_errors(vec!["Syntax error".to_string()]);
        let result = validator.validate_policy("invalid").await.unwrap();
        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
    }

    #[tokio::test]
    async fn test_mock_validator_service_error() {
        let validator = MockPolicyValidator::with_service_error();
        let result = validator.validate_policy("permit(...)").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_port_success() {
        let port = MockCreatePolicyPort::new();
        let command = CreatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: "permit(...)".to_string(),
            description: Some("Test".to_string()),
        };

        let result = port.create(command).await;
        assert!(result.is_ok());
        assert_eq!(port.get_created_count(), 1);
        assert_eq!(port.get_call_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_port_storage_error() {
        let port = MockCreatePolicyPort::with_storage_error();
        let command = CreatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: "permit(...)".to_string(),
            description: None,
        };

        let result = port.create(command).await;
        assert!(result.is_err());
        matches!(result.unwrap_err(), CreatePolicyError::StorageError(_));
    }

    #[tokio::test]
    async fn test_mock_port_duplicate_error() {
        let port =
            MockCreatePolicyPort::with_existing_policies(vec!["existing-policy".to_string()]);
        let command = CreatePolicyCommand {
            policy_id: "existing-policy".to_string(),
            policy_content: "permit(...)".to_string(),
            description: None,
        };

        let result = port.create(command).await;
        assert!(result.is_err());
        matches!(
            result.unwrap_err(),
            CreatePolicyError::PolicyAlreadyExists(_)
        );
    }

    #[tokio::test]
    async fn test_mock_port_has_policy() {
        let port = MockCreatePolicyPort::new();
        let command = CreatePolicyCommand {
            policy_id: "my-policy".to_string(),
            policy_content: "permit(...)".to_string(),
            description: None,
        };

        port.create(command).await.unwrap();
        assert!(port.has_policy("my-policy"));
        assert!(!port.has_policy("other-policy"));
    }
}
