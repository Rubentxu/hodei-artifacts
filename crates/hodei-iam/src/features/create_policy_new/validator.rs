//! Cedar Policy Validator Implementation
//!
//! This module provides a concrete implementation of the `PolicyValidator` port
//! using the Cedar policy language library to validate policy syntax.

use async_trait::async_trait;
use cedar_policy::PolicySet;
use std::str::FromStr;
use tracing::{debug, warn};

use super::ports::{
    PolicyValidationError, PolicyValidator, ValidationError, ValidationResult,
};

/// Cedar-based policy validator
///
/// This validator uses the official Cedar policy library to validate
/// policy syntax and semantics.
pub struct CedarPolicyValidator;

impl CedarPolicyValidator {
    /// Create a new Cedar policy validator
    pub fn new() -> Self {
        Self
    }
}

impl Default for CedarPolicyValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PolicyValidator for CedarPolicyValidator {
    async fn validate_policy(
        &self,
        policy_content: &str,
    ) -> Result<ValidationResult, PolicyValidationError> {
        debug!("Validating Cedar policy syntax");

        // Try to parse the policy content as a Cedar PolicySet
        match PolicySet::from_str(policy_content) {
            Ok(_policy_set) => {
                debug!("Policy syntax is valid");
                Ok(ValidationResult {
                    is_valid: true,
                    errors: vec![],
                    warnings: vec![],
                })
            }
            Err(parse_errors) => {
                warn!("Policy validation failed: {:?}", parse_errors);

                // Convert Cedar parse errors into our ValidationError format
                let errors = parse_errors
                    .to_string()
                    .lines()
                    .map(|line| ValidationError::new(line.to_string()))
                    .collect();

                Ok(ValidationResult {
                    is_valid: false,
                    errors,
                    warnings: vec![],
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_valid_policy() {
        let validator = CedarPolicyValidator::new();
        let policy = r#"permit(principal, action, resource);"#;

        let result = validator.validate_policy(policy).await.unwrap();

        assert!(result.is_valid);
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_invalid_policy() {
        let validator = CedarPolicyValidator::new();
        let policy = r#"this is not valid cedar syntax"#;

        let result = validator.validate_policy(policy).await.unwrap();

        assert!(!result.is_valid);
        assert!(!result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_empty_policy() {
        let validator = CedarPolicyValidator::new();
        let policy = "";

        let result = validator.validate_policy(policy).await.unwrap();

        // Empty policy is technically valid (empty PolicySet)
        assert!(result.is_valid);
    }
}

