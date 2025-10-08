//! Mock implementations for testing Update Policy feature

use async_trait::async_trait;
use kernel::Hrn;
use std::collections::HashMap;
use std::sync::Mutex;

use super::dto::{PolicyView, UpdatePolicyCommand};
use super::error::UpdatePolicyError;
use super::ports::{PolicyValidationError, PolicyValidator, UpdatePolicyPort, ValidationResult};
use hodei_policies::features::validate_policy::dto::ValidatePolicyCommand;

/// Mock PolicyValidator for testing
pub struct MockPolicyValidator {
    errors: Vec<String>,
    warnings: Vec<String>,
    should_fail: bool,
}

impl Default for MockPolicyValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl MockPolicyValidator {
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
            should_fail: false,
        }
    }

    pub fn with_errors(errors: Vec<String>) -> Self {
        Self {
            errors,
            warnings: Vec::new(),
            should_fail: false,
        }
    }

    pub fn with_service_error() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
            should_fail: true,
        }
    }
}

#[async_trait]
impl PolicyValidator for MockPolicyValidator {
    async fn validate(
        &self,
        _command: ValidatePolicyCommand,
    ) -> Result<ValidationResult, PolicyValidationError> {
        if self.should_fail {
            return Err(PolicyValidationError::ValidationError(
                "Mock validation service error".to_string(),
            ));
        }

        let is_valid = self.errors.is_empty();

        Ok(ValidationResult {
            is_valid,
            errors: self.errors.clone(),
        })
    }
}

/// Mock UpdatePolicyPort for testing
pub struct MockUpdatePolicyPort {
    policies: Mutex<HashMap<String, (String, Option<String>)>>, // id -> (content, description)
    should_fail: bool,
    should_return_not_found: bool,
}

impl Default for MockUpdatePolicyPort {
    fn default() -> Self {
        Self::new()
    }
}

impl MockUpdatePolicyPort {
    pub fn new() -> Self {
        let mut policies = HashMap::new();
        policies.insert(
            "test-policy".to_string(),
            (
                "permit(principal, action, resource);".to_string(),
                Some("Test policy".to_string()),
            ),
        );

        Self {
            policies: Mutex::new(policies),
            should_fail: false,
            should_return_not_found: false,
        }
    }

    pub fn with_storage_error() -> Self {
        Self {
            policies: Mutex::new(HashMap::new()),
            should_fail: true,
            should_return_not_found: false,
        }
    }

    pub fn with_not_found_error() -> Self {
        Self {
            policies: Mutex::new(HashMap::new()),
            should_fail: false,
            should_return_not_found: true,
        }
    }

    pub fn add_policy(&self, policy_id: String, content: String, description: Option<String>) {
        let mut policies = self.policies.lock().unwrap();
        policies.insert(policy_id, (content, description));
    }
}

#[async_trait]
impl UpdatePolicyPort for MockUpdatePolicyPort {
    async fn update(&self, command: UpdatePolicyCommand) -> Result<PolicyView, UpdatePolicyError> {
        if self.should_fail {
            return Err(UpdatePolicyError::StorageError(
                "Mock storage error".to_string(),
            ));
        }

        if self.should_return_not_found {
            return Err(UpdatePolicyError::PolicyNotFound(command.policy_id.clone()));
        }

        let mut policies = self.policies.lock().unwrap();

        let (content, description) = policies
            .get_mut(&command.policy_id)
            .ok_or_else(|| UpdatePolicyError::PolicyNotFound(command.policy_id.clone()))?;

        if let Some(new_content) = command.policy_content {
            *content = new_content;
        }

        if let Some(new_description) = command.description {
            *description = if new_description.is_empty() {
                None
            } else {
                Some(new_description)
            };
        }

        Ok(PolicyView {
            hrn: Hrn::new(
                "aws".to_string(),
                "iam".to_string(),
                "123456789012".to_string(),
                "Policy".to_string(),
                command.policy_id.clone(),
            ),
            name: command.policy_id.clone(),
            content: content.clone(),
            description: description.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_validator_success() {
        let validator = MockPolicyValidator::new();
        let command = ValidatePolicyCommand {
            content: "permit(...);".to_string(),
        };
        let result = validator.validate(command).await;
        assert!(result.is_ok());
        let validation = result.unwrap();
        assert!(validation.is_valid);
        assert!(validation.errors.is_empty());
    }

    #[tokio::test]
    async fn test_mock_validator_with_errors() {
        let validator = MockPolicyValidator::with_errors(vec!["Error 1".to_string()]);
        let command = ValidatePolicyCommand {
            content: "invalid".to_string(),
        };
        let result = validator.validate(command).await;
        assert!(result.is_ok());
        let validation = result.unwrap();
        assert!(!validation.is_valid);
        assert_eq!(validation.errors.len(), 1);
    }

    #[tokio::test]
    async fn test_mock_port_success() {
        let port = MockUpdatePolicyPort::new();
        let command = UpdatePolicyCommand::update_description("test-policy", "New description");
        let result = port.update(command).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_port_not_found() {
        let port = MockUpdatePolicyPort::with_not_found_error();
        let command = UpdatePolicyCommand::update_description("nonexistent", "Description");
        let result = port.update(command).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            UpdatePolicyError::PolicyNotFound(_)
        ));
    }
}
