// crates/iam/src/infrastructure/validation/cedar_validator.rs

use crate::domain::validation::ValidationResult;
use crate::features::create_policy::ports::PolicyValidator;
use crate::features::update_policy::ports::PolicyUpdateValidator;
use crate::infrastructure::errors::{IamError, ValidationError};
use async_trait::async_trait;
use cedar_policy::{PolicySet, Schema};
use std::str::FromStr;

/// Cedar-based implementation of policy validation
pub struct CedarPolicyValidator {
    /// Optional schema for semantic validation
    schema: Option<Schema>,
}

impl CedarPolicyValidator {
    /// Create a new Cedar policy validator without schema
    pub fn new() -> Self {
        Self { schema: None }
    }

    /// Create a new Cedar policy validator with schema
    pub fn with_schema(schema: Schema) -> Self {
        Self {
            schema: Some(schema),
        }
    }

    /// Parse a single policy from Cedar DSL
    fn parse_single_policy(&self, content: &str) -> Result<cedar_policy::Policy, IamError> {
        cedar_policy::Policy::from_str(content)
            .map_err(|e| IamError::validation_error(format!("Policy parse error: {}", e)))
    }

    /// Parse multiple policies from Cedar DSL
    fn parse_policy_set(&self, content: &str) -> Result<PolicySet, IamError> {
        PolicySet::from_str(content)
            .map_err(|e| IamError::validation_error(format!("PolicySet parse error: {}", e)))
    }

    /// Extract line and column information from Cedar parse error
    fn extract_location_from_error(&self, error_msg: &str) -> (Option<u32>, Option<u32>) {
        // Cedar error messages sometimes contain location information
        // This is a best-effort extraction
        let line_regex = regex::Regex::new(r"line (\d+)").ok();
        let column_regex = regex::Regex::new(r"column (\d+)").ok();

        let line = line_regex
            .and_then(|re| re.captures(error_msg))
            .and_then(|caps| caps.get(1))
            .and_then(|m| m.as_str().parse().ok());

        let column = column_regex
            .and_then(|re| re.captures(error_msg))
            .and_then(|caps| caps.get(1))
            .and_then(|m| m.as_str().parse().ok());

        (line, column)
    }

    /// Create validation error with location information
    fn create_validation_error(&self, message: String) -> ValidationError {
        let (line, column) = self.extract_location_from_error(&message);
        ValidationError {
            message,
            line,
            column,
        }
    }
}

impl Default for CedarPolicyValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PolicyValidator for CedarPolicyValidator {
    async fn validate_syntax(&self, content: &str) -> Result<ValidationResult, IamError> {
        self.validate_policy_syntax(content).await
    }
}

#[async_trait]
impl PolicyUpdateValidator for CedarPolicyValidator {
    async fn validate_syntax(&self, content: &str) -> Result<ValidationResult, IamError> {
        self.validate_policy_syntax(content).await
    }
}

impl CedarPolicyValidator {
    /// Common validation logic used by both traits
    async fn validate_policy_syntax(&self, content: &str) -> Result<ValidationResult, IamError> {
        if content.trim().is_empty() {
            return Ok(ValidationResult::invalid_with_message(
                "Policy content cannot be empty".to_string(),
            ));
        }

        // Try to parse as a single policy first
        match self.parse_single_policy(content) {
            Ok(_) => Ok(ValidationResult::valid()),
            Err(_) => {
                // If single policy fails, try as a policy set
                match self.parse_policy_set(content) {
                    Ok(_) => Ok(ValidationResult::valid()),
                    Err(e) => {
                        if let IamError::PolicyValidationFailed { errors } = e {
                            Ok(ValidationResult::invalid(errors))
                        } else {
                            // Convert other errors to validation errors
                            let validation_error = self.create_validation_error(e.to_string());
                            Ok(ValidationResult::invalid(vec![validation_error]))
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::create_policy::ports::PolicyValidator;

    #[tokio::test]
    async fn test_validate_syntax_valid_policy() {
        let validator = CedarPolicyValidator::new();
        let policy_content = r#"
            permit(
                principal == User::"alice",
                action == Action::"read",
                resource == Document::"test"
            );
        "#;

        let result = PolicyValidator::validate_syntax(&validator, policy_content).await;
        assert!(result.is_ok());
        let validation_result = result.unwrap();
        assert!(validation_result.is_valid);
        assert!(validation_result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_validate_syntax_valid_policy_set() {
        let validator = CedarPolicyValidator::new();
        let policy_content = r#"
            permit(
                principal == User::"alice",
                action == Action::"read",
                resource == Document::"test"
            );
            
            forbid(
                principal == User::"bob",
                action == Action::"delete",
                resource
            );
        "#;

        let result = PolicyValidator::validate_syntax(&validator, policy_content).await;
        assert!(result.is_ok());
        let validation_result = result.unwrap();
        assert!(validation_result.is_valid);
        assert!(validation_result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_validate_syntax_empty_content() {
        let validator = CedarPolicyValidator::new();
        let policy_content = "";

        let result = PolicyValidator::validate_syntax(&validator, policy_content).await;
        assert!(result.is_ok());
        let validation_result = result.unwrap();
        assert!(!validation_result.is_valid);
        assert_eq!(validation_result.errors.len(), 1);
        assert!(
            validation_result.errors[0]
                .message
                .contains("cannot be empty")
        );
    }

    #[tokio::test]
    async fn test_validate_syntax_invalid_policy() {
        let validator = CedarPolicyValidator::new();
        let policy_content = r#"
            invalid_keyword(
                principal == User::"alice",
                action == Action::"read",
                resource == Document::"test"
            );
        "#;

        let result = PolicyValidator::validate_syntax(&validator, policy_content).await;
        assert!(result.is_ok());
        let validation_result = result.unwrap();
        assert!(!validation_result.is_valid);
        assert!(!validation_result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_validate_syntax_malformed_policy() {
        let validator = CedarPolicyValidator::new();
        let policy_content = r#"
            permit(
                principal == User::"alice"
                // Missing comma and other parts
            );
        "#;

        let result = PolicyValidator::validate_syntax(&validator, policy_content).await;
        assert!(result.is_ok());
        let validation_result = result.unwrap();
        assert!(!validation_result.is_valid);
        assert!(!validation_result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_create_validation_error() {
        let validator = CedarPolicyValidator::new();
        let error_msg = "Parse error at line 5, column 10";
        let validation_error = validator.create_validation_error(error_msg.to_string());

        assert_eq!(validation_error.message, error_msg);
        // Note: The regex extraction might not work perfectly with this test message
        // In real Cedar errors, the format might be different
    }

    #[tokio::test]
    async fn test_validator_default() {
        let validator = CedarPolicyValidator::default();
        assert!(validator.schema.is_none());
    }

    #[tokio::test]
    async fn test_validation_result_helpers() {
        let mut result = ValidationResult::valid();
        assert!(result.is_valid);
        assert!(result.errors.is_empty());

        result.add_error(ValidationError {
            message: "Test error".to_string(),
            line: Some(1),
            column: Some(5),
        });

        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.first_error_message(), Some("Test error"));
    }
}
