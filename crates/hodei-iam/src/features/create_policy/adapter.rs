//! Adapter implementations for the create_policy feature
use std::str::FromStr;
//!
//! This module provides concrete implementations of the ports defined for
//! policy management, including validation and persistence adapters.
//!
//! # TODO: REFACTOR
//! This is a temporary stub implementation. In Phase 2 of the refactoring,
//! this feature will be:
//! 1. Split into separate features (create, delete, update, get, list)
//! 2. Moved to the appropriate bounded context
//! 3. Properly integrated with the policies crate validation

use crate::features::create_policy::ports::{
    PolicyValidationError, PolicyValidator, ValidationError, ValidationResult, ValidationWarning,
};
use async_trait::async_trait;

/// Temporary stub adapter for policy validation
///
/// This is a placeholder implementation that allows the code to compile.
/// It performs basic Cedar syntax validation.
///
/// # TODO
/// Replace with proper integration to policies crate validation once
/// the policies crate features are refactored.
pub struct StubPolicyValidatorAdapter;

impl StubPolicyValidatorAdapter {
    /// Create a new stub adapter instance
    pub fn new() -> Self {
        Self
    }
}

impl Default for StubPolicyValidatorAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PolicyValidator for StubPolicyValidatorAdapter {
    async fn validate_policy(
        &self,
        policy_content: &str,
    ) -> Result<ValidationResult, PolicyValidationError> {
        // Basic validation: check if the policy content is not empty
        // and contains basic Cedar policy structure

        if policy_content.trim().is_empty() {
            return Ok(ValidationResult {
                is_valid: false,
                errors: vec![ValidationError {
                    message: "Policy content cannot be empty".to_string(),
                    line: None,
                    column: None,
                }],
                warnings: vec![],
            });
        }

        // Try to parse as Cedar PolicySet to validate syntax
        match cedar_policy::PolicySet::from_str(policy_content) {
            Ok(_) => Ok(ValidationResult {
                is_valid: true,
                errors: vec![],
                warnings: vec![],
            }),
            Err(parse_errors) => {
                let errors: Vec<ValidationError> = parse_errors
                    .errors()
                    .iter()
                    .map(|e| ValidationError {
                        message: e.to_string(),
                        line: None,
                        column: None,
                    })
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
    async fn test_stub_validator_rejects_empty_policy() {
        let validator = StubPolicyValidatorAdapter::new();
        let result = validator.validate_policy("").await.unwrap();

        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].message.contains("empty"));
    }

    #[tokio::test]
    async fn test_stub_validator_accepts_valid_cedar_syntax() {
        let validator = StubPolicyValidatorAdapter::new();
        let valid_policy = r#"
            permit(
                principal,
                action,
                resource
            );
        "#;

        let result = validator.validate_policy(valid_policy).await.unwrap();

        assert!(
            result.is_valid,
            "Expected valid policy, got errors: {:?}",
            result.errors
        );
        assert_eq!(result.errors.len(), 0);
    }

    #[tokio::test]
    async fn test_stub_validator_rejects_invalid_cedar_syntax() {
        let validator = StubPolicyValidatorAdapter::new();
        let invalid_policy = "this is not valid cedar syntax";

        let result = validator.validate_policy(invalid_policy).await.unwrap();

        assert!(!result.is_valid);
        assert!(!result.errors.is_empty());
    }
}
