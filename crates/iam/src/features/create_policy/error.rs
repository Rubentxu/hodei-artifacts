// crates/iam/src/features/create_policy/error.rs

use crate::infrastructure::errors::ValidationError;
use cedar_policy::PolicyId;
use thiserror::Error;

/// Specific errors for the create_policy feature
#[derive(Debug, Error)]
pub enum CreatePolicyError {
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Policy validation failed: {errors:?}")]
    ValidationFailed { errors: Vec<ValidationError> },

    #[error("Policy already exists: {0}")]
    PolicyAlreadyExists(PolicyId),

    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Event publishing failed: {0}")]
    EventPublishingFailed(String),

    #[error("Internal error: {0}")]
    InternalError(String),
}

impl CreatePolicyError {
    /// Create a validation failed error from validation errors
    pub fn validation_failed(errors: Vec<ValidationError>) -> Self {
        Self::ValidationFailed { errors }
    }

    /// Create an invalid input error
    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }

    /// Create a database error
    pub fn database_error(message: impl Into<String>) -> Self {
        Self::DatabaseError(message.into())
    }
}

impl From<CreatePolicyError> for crate::infrastructure::errors::IamError {
    fn from(error: CreatePolicyError) -> Self {
        match error {
            CreatePolicyError::InvalidInput(msg) => Self::InvalidInput(msg),
            CreatePolicyError::ValidationFailed { errors } => {
                Self::PolicyValidationFailed { errors }
            }
            CreatePolicyError::PolicyAlreadyExists(id) => Self::PolicyAlreadyExists(id),
            CreatePolicyError::DatabaseError(msg) => Self::DatabaseError(msg),
            CreatePolicyError::EventPublishingFailed(msg) => Self::InternalError(msg),
            CreatePolicyError::InternalError(msg) => Self::InternalError(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_policy_error_display() {
        let error = CreatePolicyError::InvalidInput("Test message".to_string());
        assert_eq!(error.to_string(), "Invalid input: Test message");

        let error = CreatePolicyError::PolicyAlreadyExists(PolicyId::new("test_policy").unwrap());
        assert!(error.to_string().contains("Policy already exists:"));
    }

    #[test]
    fn test_create_policy_error_convenience_methods() {
        let error = CreatePolicyError::invalid_input("test message");
        assert!(matches!(error, CreatePolicyError::InvalidInput(_)));

        let error = CreatePolicyError::database_error("db error");
        assert!(matches!(error, CreatePolicyError::DatabaseError(_)));
    }
}
