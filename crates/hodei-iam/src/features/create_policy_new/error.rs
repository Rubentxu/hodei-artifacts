//! Error types for the create_policy feature
//!
//! This module defines all error types that can occur during IAM policy
//! creation operations. Following Clean Architecture principles, these
//! errors are specific to this feature and do not leak implementation details.

use thiserror::Error;

/// Errors that can occur when creating an IAM policy
///
/// This enum represents all possible failure modes during policy creation.
/// Each variant provides detailed context about what went wrong.
///
/// # Examples
///
/// ```rust,ignore
/// use hodei_iam::CreatePolicyError;
///
/// match use_case.execute(command).await {
///     Ok(policy) => println!("Policy created: {}", policy.id),
///     Err(CreatePolicyError::InvalidPolicyContent(msg)) => {
///         eprintln!("Invalid policy syntax: {}", msg);
///     }
///     Err(CreatePolicyError::PolicyAlreadyExists) => {
///         eprintln!("A policy with this ID already exists");
///     }
///     Err(e) => eprintln!("Creation failed: {}", e),
/// }
/// ```
#[derive(Debug, Error)]
pub enum CreatePolicyError {
    /// Error occurred while storing the policy
    ///
    /// This indicates a problem with the persistence layer (database, file system, etc.)
    #[error("Policy storage error: {0}")]
    StorageError(String),

    /// The policy content is syntactically or semantically invalid
    ///
    /// This is returned when the Cedar policy text cannot be parsed
    /// or contains invalid constructs.
    #[error("Invalid policy content: {0}")]
    InvalidPolicyContent(String),

    /// Policy validation service failed
    ///
    /// This indicates that the validation service itself encountered an error,
    /// not that the policy is invalid.
    #[error("Policy validation failed: {0}")]
    ValidationFailed(String),

    /// A policy with the same ID already exists
    ///
    /// Policy IDs must be unique within an account. This error is returned
    /// when attempting to create a policy with an ID that's already in use.
    #[error("Policy already exists with id: {0}")]
    PolicyAlreadyExists(String),

    /// The provided HRN format is invalid
    ///
    /// This is returned when the policy ID cannot be converted to a valid HRN.
    #[error("Invalid HRN format: {0}")]
    InvalidHrn(String),

    /// The policy ID is invalid or empty
    ///
    /// Policy IDs must follow specific format rules (alphanumeric, hyphens, etc.)
    #[error("Invalid policy ID: {0}")]
    InvalidPolicyId(String),

    /// The policy content is empty or missing
    #[error("Policy content cannot be empty")]
    EmptyPolicyContent,

    /// Authorization failure - caller doesn't have permission to create policies
    #[error("Insufficient permissions to create policy")]
    Unauthorized,
}

impl CreatePolicyError {
    /// Returns true if the error is retryable
    ///
    /// Some errors like storage errors might be transient and worth retrying.
    /// Others like validation errors are permanent and shouldn't be retried.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            CreatePolicyError::StorageError(_) | CreatePolicyError::ValidationFailed(_)
        )
    }

    /// Returns true if the error is a client error (4xx-like)
    ///
    /// Client errors indicate the request was invalid and shouldn't be retried
    /// without modification.
    pub fn is_client_error(&self) -> bool {
        matches!(
            self,
            CreatePolicyError::InvalidPolicyContent(_)
                | CreatePolicyError::PolicyAlreadyExists(_)
                | CreatePolicyError::InvalidHrn(_)
                | CreatePolicyError::InvalidPolicyId(_)
                | CreatePolicyError::EmptyPolicyContent
                | CreatePolicyError::Unauthorized
        )
    }

    /// Returns true if the error is a server error (5xx-like)
    ///
    /// Server errors indicate something went wrong on the server side
    /// and might be worth retrying.
    pub fn is_server_error(&self) -> bool {
        matches!(
            self,
            CreatePolicyError::StorageError(_) | CreatePolicyError::ValidationFailed(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let error = CreatePolicyError::StorageError("Database connection failed".to_string());
        assert_eq!(
            error.to_string(),
            "Policy storage error: Database connection failed"
        );
    }

    #[test]
    fn test_error_is_retryable() {
        assert!(CreatePolicyError::StorageError("test".to_string()).is_retryable());
        assert!(!CreatePolicyError::InvalidPolicyContent("test".to_string()).is_retryable());
        assert!(!CreatePolicyError::PolicyAlreadyExists("test".to_string()).is_retryable());
    }

    #[test]
    fn test_error_is_client_error() {
        assert!(CreatePolicyError::InvalidPolicyContent("test".to_string()).is_client_error());
        assert!(CreatePolicyError::PolicyAlreadyExists("test".to_string()).is_client_error());
        assert!(CreatePolicyError::Unauthorized.is_client_error());
        assert!(!CreatePolicyError::StorageError("test".to_string()).is_client_error());
    }

    #[test]
    fn test_error_is_server_error() {
        assert!(CreatePolicyError::StorageError("test".to_string()).is_server_error());
        assert!(CreatePolicyError::ValidationFailed("test".to_string()).is_server_error());
        assert!(!CreatePolicyError::InvalidPolicyContent("test".to_string()).is_server_error());
    }

    #[test]
    fn test_empty_policy_content_error() {
        let error = CreatePolicyError::EmptyPolicyContent;
        assert_eq!(error.to_string(), "Policy content cannot be empty");
        assert!(error.is_client_error());
        assert!(!error.is_retryable());
    }

    #[test]
    fn test_policy_already_exists_error() {
        let error = CreatePolicyError::PolicyAlreadyExists("my-policy".to_string());
        assert!(error.to_string().contains("my-policy"));
        assert!(error.is_client_error());
        assert!(!error.is_retryable());
    }
}
