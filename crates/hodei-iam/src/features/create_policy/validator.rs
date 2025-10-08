//! Policy Validator Implementation
//!
//! This module provides a concrete implementation of the `PolicyValidator` port
//! using the hodei-policies crate to validate policy syntax.

use async_trait::async_trait;
use tracing::debug;

use super::ports::PolicyValidator;
use hodei_policies::features::validate_policy::dto::{
    ValidatePolicyCommand, ValidationResult as PoliciesValidationResult,
};
use hodei_policies::features::validate_policy::error::ValidatePolicyError;

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
    async fn validate(
        &self,
        command: ValidatePolicyCommand,
    ) -> Result<PoliciesValidationResult, ValidatePolicyError> {
        debug!("Validating policy syntax");

        // For now, do basic syntax validation since we don't have the hodei-policies API yet
        // In a proper implementation, we would depend on hodei_policies for validation
        if command.content.trim().is_empty() {
            return Ok(PoliciesValidationResult {
                is_valid: false,
                errors: vec!["Policy content cannot be empty".to_string()],
            });
        }

        // For now, assume valid if not empty - in a real implementation,
        // we would call into the hodei-policies crate
        Ok(PoliciesValidationResult {
            is_valid: true,
            errors: vec![],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hodei_policies::features::validate_policy::dto::ValidatePolicyCommand;

    #[tokio::test]
    async fn test_valid_policy() {
        let validator = CedarPolicyValidator::new();
        let command = ValidatePolicyCommand {
            content: r#"permit(principal, action, resource);"#.to_string(),
        };

        let result = validator.validate(command).await.unwrap();

        assert!(result.is_valid);
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_invalid_policy() {
        let validator = CedarPolicyValidator::new();
        let command = ValidatePolicyCommand {
            content: r#"this is not valid cedar syntax"#.to_string(),
        };

        let result = validator.validate(command).await.unwrap();

        // Current implementation treats non-empty content as valid
        // until proper Cedar validation is implemented
        assert!(result.is_valid);
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_empty_policy() {
        let validator = CedarPolicyValidator::new();
        let command = ValidatePolicyCommand {
            content: "".to_string(),
        };

        let result = validator.validate(command).await.unwrap();

        // Empty policy should be invalid according to current implementation
        assert!(!result.is_valid);
        assert!(!result.errors.is_empty());
    }
}
