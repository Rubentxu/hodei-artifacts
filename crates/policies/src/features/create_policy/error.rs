//! Error types for the create_policy feature
//!
//! This module defines domain-specific errors that can occur during policy creation.
//! Following the principle of explicit error handling, each error variant provides
//! clear information about what went wrong.

use thiserror::Error;

/// Errors that can occur during policy creation
#[derive(Debug, Clone, Error)]
pub enum CreatePolicyError {
    /// Invalid input provided to the command (empty fields, duplicates, etc.)
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// The command failed validation (syntax, semantics, etc.)
    #[error("Validation error: {0}")]
    ValidationError(String),

    /// Policy ID already exists in the system (for backwards compatibility)
    #[error("Policy ID conflict: {0}")]
    IdConflict(String),

    /// Resource conflict (e.g., duplicate policy ID)
    #[error("Conflict: {0}")]
    Conflict(String),

    /// Policy document has invalid Cedar syntax
    #[error("Invalid policy syntax: {0}")]
    InvalidSyntax(String),

    /// Policy document has semantic errors (e.g., references to undefined types)
    #[error("Invalid policy semantics: {0}")]
    InvalidSemantics(String),

    /// Failed to persist the policy to storage
    #[error("Storage error: {0}")]
    StorageError(String),

    /// Failed to generate a policy ID
    #[error("ID generation error: {0}")]
    IdGenerationError(String),

    /// Internal error (unexpected conditions)
    #[error("Internal error: {0}")]
    Internal(String),
}

// Conversion helpers for common error types
impl From<std::io::Error> for CreatePolicyError {
    fn from(err: std::io::Error) -> Self {
        CreatePolicyError::Internal(format!("IO error: {}", err))
    }
}

impl From<serde_json::Error> for CreatePolicyError {
    fn from(err: serde_json::Error) -> Self {
        CreatePolicyError::Internal(format!("JSON error: {}", err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_validation() {
        let err = CreatePolicyError::ValidationError("field required".to_string());
        assert_eq!(err.to_string(), "Validation error: field required");
    }

    #[test]
    fn error_display_id_conflict() {
        let err = CreatePolicyError::IdConflict("policy-123 already exists".to_string());
        assert_eq!(
            err.to_string(),
            "Policy ID conflict: policy-123 already exists"
        );
    }

    #[test]
    fn error_display_invalid_syntax() {
        let err = CreatePolicyError::InvalidSyntax("missing semicolon".to_string());
        assert_eq!(err.to_string(), "Invalid policy syntax: missing semicolon");
    }

    #[test]
    fn error_display_storage() {
        let err = CreatePolicyError::StorageError("database unreachable".to_string());
        assert_eq!(err.to_string(), "Storage error: database unreachable");
    }

    #[test]
    fn error_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: CreatePolicyError = io_err.into();

        match err {
            CreatePolicyError::Internal(msg) => assert!(msg.contains("IO error")),
            _ => panic!("Expected Internal error variant"),
        }
    }

    #[test]
    fn error_from_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid json")
            .expect_err("Should fail parsing");
        let err: CreatePolicyError = json_err.into();

        match err {
            CreatePolicyError::Internal(msg) => assert!(msg.contains("JSON error")),
            _ => panic!("Expected Internal error variant"),
        }
    }
}
