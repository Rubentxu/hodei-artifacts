//! Error types for the update_policy feature
//!
//! This module defines all error types that can occur during IAM policy
//! update operations. Following Clean Architecture principles, these
//! errors are specific to this feature and do not leak implementation details.

use thiserror::Error;

/// Errors that can occur when updating an IAM policy
///
/// This enum represents all possible failure modes during policy updates.
/// Each variant provides detailed context about what went wrong.
///
/// # Examples
///
/// ```rust,ignore
/// use hodei_iam::UpdatePolicyError;
///
/// match use_case.execute(command).await {
///     Ok(policy) => println!("Policy updated: {}", policy.id),
///     Err(UpdatePolicyError::PolicyNotFound(id)) => {
///         eprintln!("Policy not found: {}", id);
///     }
///     Err(UpdatePolicyError::InvalidPolicyContent(msg)) => {
///         eprintln!("Invalid policy: {}", msg);
///     }
///     Err(e) => eprintln!("Update failed: {}", e),
/// }
/// ```
#[derive(Debug, Error)]
pub enum UpdatePolicyError {
    /// Error occurred while updating the policy in storage
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

    /// The policy with the given ID does not exist
    ///
    /// This is returned when attempting to update a policy that doesn't exist
    /// in the system.
    #[error("Policy not found: {0}")]
    PolicyNotFound(String),

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

    /// No updates provided in the command
    ///
    /// At least one of policy_content or description must be provided.
    #[error("No updates provided: at least one field (content or description) must be specified")]
    NoUpdatesProvided,

    /// The policy content is empty or missing when update is requested
    #[error("Policy content cannot be empty")]
    EmptyPolicyContent,

    /// Authorization failure - caller doesn't have permission to update policies
    #[error("Insufficient permissions to update policy")]
    Unauthorized,

    /// The policy is a system-managed policy and cannot be updated
    ///
    /// System policies are built-in and protected from modification.
    #[error("Cannot update system-managed policy: {0}")]
    SystemPolicyProtected(String),

    /// Optimistic locking conflict - policy was modified by another process
    ///
    /// The policy version/etag doesn't match, indicating it was updated
    /// since it was last read. The caller should re-read and retry.
    #[error("Policy was modified by another process (version conflict)")]
    VersionConflict,

    /// The policy is currently in use and cannot be updated in a breaking way
    ///
    /// This is returned when the update would break existing authorization
    /// decisions for active sessions or users.
    #[error("Policy is in active use and update would break authorization: {0}")]
    PolicyInUseConflict(String),
}

impl UpdatePolicyError {
    /// Returns true if the error is retryable
    ///
    /// Some errors like storage errors or version conflicts might be transient
    /// and worth retrying. Others like validation errors are permanent and
    /// shouldn't be retried without modification.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            UpdatePolicyError::StorageError(_)
                | UpdatePolicyError::ValidationFailed(_)
                | UpdatePolicyError::VersionConflict
        )
    }

    /// Returns true if the error is a client error (4xx-like)
    ///
    /// Client errors indicate the request was invalid and shouldn't be retried
    /// without modification.
    pub fn is_client_error(&self) -> bool {
        matches!(
            self,
            UpdatePolicyError::InvalidPolicyContent(_)
                | UpdatePolicyError::PolicyNotFound(_)
                | UpdatePolicyError::InvalidHrn(_)
                | UpdatePolicyError::InvalidPolicyId(_)
                | UpdatePolicyError::NoUpdatesProvided
                | UpdatePolicyError::EmptyPolicyContent
                | UpdatePolicyError::Unauthorized
                | UpdatePolicyError::SystemPolicyProtected(_)
                | UpdatePolicyError::PolicyInUseConflict(_)
        )
    }

    /// Returns true if the error is a server error (5xx-like)
    ///
    /// Server errors indicate something went wrong on the server side
    /// and might be worth retrying.
    pub fn is_server_error(&self) -> bool {
        matches!(
            self,
            UpdatePolicyError::StorageError(_) | UpdatePolicyError::ValidationFailed(_)
        )
    }

    /// Returns true if the error indicates the resource was not found
    pub fn is_not_found(&self) -> bool {
        matches!(self, UpdatePolicyError::PolicyNotFound(_))
    }

    /// Returns true if the error is a conflict (409-like)
    pub fn is_conflict(&self) -> bool {
        matches!(
            self,
            UpdatePolicyError::VersionConflict | UpdatePolicyError::PolicyInUseConflict(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let error = UpdatePolicyError::StorageError("Database connection failed".to_string());
        assert_eq!(
            error.to_string(),
            "Policy storage error: Database connection failed"
        );
    }

    #[test]
    fn test_policy_not_found_display() {
        let error = UpdatePolicyError::PolicyNotFound("my-policy".to_string());
        assert_eq!(error.to_string(), "Policy not found: my-policy");
    }

    #[test]
    fn test_error_is_retryable() {
        assert!(UpdatePolicyError::StorageError("test".to_string()).is_retryable());
        assert!(UpdatePolicyError::VersionConflict.is_retryable());
        assert!(!UpdatePolicyError::PolicyNotFound("test".to_string()).is_retryable());
        assert!(!UpdatePolicyError::InvalidPolicyContent("test".to_string()).is_retryable());
    }

    #[test]
    fn test_error_is_client_error() {
        assert!(UpdatePolicyError::PolicyNotFound("test".to_string()).is_client_error());
        assert!(UpdatePolicyError::InvalidPolicyContent("test".to_string()).is_client_error());
        assert!(UpdatePolicyError::NoUpdatesProvided.is_client_error());
        assert!(UpdatePolicyError::Unauthorized.is_client_error());
        assert!(!UpdatePolicyError::StorageError("test".to_string()).is_client_error());
    }

    #[test]
    fn test_error_is_server_error() {
        assert!(UpdatePolicyError::StorageError("test".to_string()).is_server_error());
        assert!(UpdatePolicyError::ValidationFailed("test".to_string()).is_server_error());
        assert!(!UpdatePolicyError::PolicyNotFound("test".to_string()).is_server_error());
    }

    #[test]
    fn test_error_is_not_found() {
        assert!(UpdatePolicyError::PolicyNotFound("test".to_string()).is_not_found());
        assert!(!UpdatePolicyError::StorageError("test".to_string()).is_not_found());
    }

    #[test]
    fn test_error_is_conflict() {
        assert!(UpdatePolicyError::VersionConflict.is_conflict());
        assert!(UpdatePolicyError::PolicyInUseConflict("test".to_string()).is_conflict());
        assert!(!UpdatePolicyError::PolicyNotFound("test".to_string()).is_conflict());
    }

    #[test]
    fn test_no_updates_provided_error() {
        let error = UpdatePolicyError::NoUpdatesProvided;
        assert!(error.to_string().contains("No updates provided"));
        assert!(error.is_client_error());
        assert!(!error.is_retryable());
    }

    #[test]
    fn test_system_policy_protected_error() {
        let error = UpdatePolicyError::SystemPolicyProtected("admin-policy".to_string());
        assert!(error.to_string().contains("admin-policy"));
        assert!(error.is_client_error());
        assert!(!error.is_retryable());
    }

    #[test]
    fn test_version_conflict_error() {
        let error = UpdatePolicyError::VersionConflict;
        assert!(error.to_string().contains("version conflict"));
        assert!(error.is_retryable());
        assert!(error.is_conflict());
        assert!(!error.is_client_error());
    }

    #[test]
    fn test_empty_policy_content_error() {
        let error = UpdatePolicyError::EmptyPolicyContent;
        assert_eq!(error.to_string(), "Policy content cannot be empty");
        assert!(error.is_client_error());
        assert!(!error.is_retryable());
    }

    #[test]
    fn test_policy_in_use_conflict() {
        let error = UpdatePolicyError::PolicyInUseConflict("affects 100 users".to_string());
        assert!(error.to_string().contains("affects 100 users"));
        assert!(error.is_client_error());
        assert!(error.is_conflict());
        assert!(!error.is_retryable());
    }
}
