// crates/iam/src/features/validate_policy/error.rs

use crate::infrastructure::errors::ValidationError;
use thiserror::Error;

/// Specific errors for the validate_policy feature
#[derive(Debug, Error)]
pub enum ValidatePolicyError {
    #[error("Invalid policy syntax: {0}")]
    InvalidSyntax(String),

    #[error("Policy validation failed: {errors:?}")]
    ValidationFailed { errors: Vec<ValidationError> },

    #[error("Schema validation failed: {0}")]
    SchemaValidationFailed(String),

    #[error("Internal validation error: {0}")]
    InternalError(String),

    #[error("Cedar policy error: {0}")]
    CedarError(String),
}

impl ValidatePolicyError {
    /// Create a validation failed error from validation errors
    pub fn validation_failed(errors: Vec<ValidationError>) -> Self {
        Self::ValidationFailed { errors }
    }

    /// Create an invalid syntax error
    pub fn invalid_syntax(message: impl Into<String>) -> Self {
        Self::InvalidSyntax(message.into())
    }

    /// Create a schema validation failed error
    pub fn schema_validation_failed(message: impl Into<String>) -> Self {
        Self::SchemaValidationFailed(message.into())
    }

    /// Create an internal error
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::InternalError(message.into())
    }
}

impl From<ValidatePolicyError> for crate::infrastructure::errors::IamError {
    fn from(error: ValidatePolicyError) -> Self {
        match error {
            ValidatePolicyError::InvalidSyntax(msg) => Self::PolicyValidationFailed {
                errors: vec![crate::infrastructure::errors::ValidationError {
                    message: msg,
                    line: None,
                    column: None,
                }],
            },
            ValidatePolicyError::ValidationFailed { errors } => {
                Self::PolicyValidationFailed { errors }
            }
            ValidatePolicyError::SchemaValidationFailed(msg) => Self::PolicyValidationFailed {
                errors: vec![crate::infrastructure::errors::ValidationError {
                    message: msg,
                    line: None,
                    column: None,
                }],
            },
            ValidatePolicyError::InternalError(msg) => Self::InternalError(msg),
            ValidatePolicyError::CedarError(msg) => Self::InternalError(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_policy_error_display() {
        let error = ValidatePolicyError::InvalidSyntax("Test syntax error".to_string());
        assert_eq!(
            error.to_string(),
            "Invalid policy syntax: Test syntax error"
        );

        let error = ValidatePolicyError::SchemaValidationFailed("Schema error".to_string());
        assert_eq!(error.to_string(), "Schema validation failed: Schema error");
    }

    #[test]
    fn test_validate_policy_error_convenience_methods() {
        let error = ValidatePolicyError::invalid_syntax("test syntax");
        assert!(matches!(error, ValidatePolicyError::InvalidSyntax(_)));

        let error = ValidatePolicyError::schema_validation_failed("test schema");
        assert!(matches!(
            error,
            ValidatePolicyError::SchemaValidationFailed(_)
        ));
    }
}
