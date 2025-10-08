//! Error types for the playground_evaluate feature
//!
//! This module defines the errors that can occur during ad-hoc policy evaluation
//! in the playground environment.

use thiserror::Error;

/// Errors that can occur during playground policy evaluation
#[derive(Debug, Clone, Error)]
pub enum PlaygroundEvaluateError {
    /// Invalid command parameters
    #[error("Invalid command: {0}")]
    InvalidCommand(String),

    /// Schema loading or parsing error
    #[error("Schema error: {0}")]
    SchemaError(String),

    /// Policy parsing or validation error
    #[error("Policy error: {0}")]
    PolicyError(String),

    /// Error during policy evaluation
    #[error("Evaluation error: {0}")]
    EvaluationError(String),

    /// Schema validation failed
    #[error("Schema validation failed: {0}")]
    SchemaValidationError(String),

    /// Policy validation failed against schema
    #[error("Policy validation failed: {0}")]
    PolicyValidationError(String),

    /// Authorization request is invalid
    #[error("Invalid authorization request: {0}")]
    InvalidRequest(String),

    /// Context attribute error
    #[error("Invalid context attribute: {0}")]
    InvalidContextAttribute(String),

    /// Storage error when loading stored schema
    #[error("Schema storage error: {0}")]
    SchemaStorageError(String),

    /// Schema not found in storage
    #[error("Schema version '{0}' not found in storage")]
    SchemaNotFound(String),

    /// Internal error
    #[error("Internal playground error: {0}")]
    InternalError(String),
}

// Implement conversion from build_schema errors for schema loading
impl From<crate::features::build_schema::error::BuildSchemaError> for PlaygroundEvaluateError {
    fn from(err: crate::features::build_schema::error::BuildSchemaError) -> Self {
        match err {
            crate::features::build_schema::error::BuildSchemaError::SchemaStorageError(msg) => {
                PlaygroundEvaluateError::SchemaStorageError(msg)
            }
            crate::features::build_schema::error::BuildSchemaError::SchemaBuildError(msg) => {
                PlaygroundEvaluateError::SchemaError(msg)
            }
            crate::features::build_schema::error::BuildSchemaError::SchemaValidationError(msg) => {
                PlaygroundEvaluateError::SchemaValidationError(msg)
            }
            other => PlaygroundEvaluateError::InternalError(other.to_string()),
        }
    }
}

// Implement conversion from validate_policy errors
impl From<crate::features::validate_policy::error::ValidatePolicyError>
    for PlaygroundEvaluateError
{
    fn from(err: crate::features::validate_policy::error::ValidatePolicyError) -> Self {
        match err {
            crate::features::validate_policy::error::ValidatePolicyError::ValidationError(msg) => {
                PlaygroundEvaluateError::PolicyValidationError(msg)
            }
            crate::features::validate_policy::error::ValidatePolicyError::InternalError(msg) => {
                PlaygroundEvaluateError::InternalError(msg)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = PlaygroundEvaluateError::InvalidCommand("missing schema".to_string());
        assert_eq!(err.to_string(), "Invalid command: missing schema");
    }

    #[test]
    fn test_schema_error() {
        let err = PlaygroundEvaluateError::SchemaError("parse failed".to_string());
        assert!(err.to_string().contains("Schema error"));
    }

    #[test]
    fn test_policy_error() {
        let err = PlaygroundEvaluateError::PolicyError("syntax error".to_string());
        assert!(err.to_string().contains("Policy error"));
    }
}
