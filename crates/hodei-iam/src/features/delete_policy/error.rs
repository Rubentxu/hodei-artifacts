//! Error types for the delete_policy feature
//!
//! This module defines all error types that can occur during IAM policy
//! deletion operations. Following Clean Architecture principles, these
//! errors are specific to this feature and do not leak implementation details.

use thiserror::Error;

/// Errors that can occur when deleting an IAM policy
///
/// This enum represents all possible failure modes during policy deletion.
/// Each variant provides detailed context about what went wrong.
///
/// # Examples
///
/// ```rust,ignore
/// use hodei_iam::DeletePolicyError;
///
/// match use_case.execute(command).await {
///     Ok(()) => println!("Policy deleted successfully"),
///     Err(DeletePolicyError::PolicyNotFound(id)) => {
///         eprintln!("Policy not found: {}", id);
///     }
///     Err(DeletePolicyError::PolicyInUse(msg)) => {
///         eprintln!("Cannot delete: {}", msg);
///     }
///     Err(e) => eprintln!("Deletion failed: {}", e),
/// }
/// ```
#[derive(Debug, Error)]
pub enum DeletePolicyError {
    /// Error occurred while deleting the policy from storage
    ///
    /// This indicates a problem with the persistence layer (database, file system, etc.)
    #[error("Policy storage error: {0}")]
    StorageError(String),

    /// The policy with the given ID does not exist
    ///
    /// This is returned when attempting to delete a policy that doesn't exist
    /// in the system.
    #[error("Policy not found: {0}")]
    PolicyNotFound(String),

    /// The provided policy ID is invalid or empty
    ///
    /// Policy IDs must follow specific format rules (alphanumeric, hyphens, etc.)
    #[error("Invalid policy ID: {0}")]
    InvalidPolicyId(String),

    /// The policy cannot be deleted because it is currently in use
    ///
    /// This is returned when the policy is attached to users, groups, or roles
    /// and cannot be safely deleted without breaking authorization.
    #[error("Policy is in use and cannot be deleted: {0}")]
    PolicyInUse(String),

    /// The provided HRN format is invalid
    ///
    /// This is returned when the policy ID cannot be converted to a valid HRN.
    #[error("Invalid HRN format: {0}")]
    InvalidHrn(String),

    /// Authorization failure - caller doesn't have permission to delete policies
    #[error("Insufficient permissions to delete policy")]
    Unauthorized,

    /// The policy is a system-managed policy and cannot be deleted
    ///
    /// System policies are built-in and protected from deletion.
    #[error("Cannot delete system-managed policy: {0}")]
    SystemPolicyProtected(String),
}

impl DeletePolicyError {
    /// Returns true if the error is retryable
    ///
    /// Some errors like storage errors might be transient and worth retrying.
    /// Others like "not found" or "policy in use" are permanent and shouldn't be retried.
    pub fn is_retryable(&self) -> bool {
        matches!(self, DeletePolicyError::StorageError(_))
    }

    /// Returns true if the error is a client error (4xx-like)
    ///
    /// Client errors indicate the request was invalid and shouldn't be retried
    /// without modification.
    pub fn is_client_error(&self) -> bool {
        matches!(
            self,
            DeletePolicyError::PolicyNotFound(_)
                | DeletePolicyError::InvalidPolicyId(_)
                | DeletePolicyError::PolicyInUse(_)
                | DeletePolicyError::InvalidHrn(_)
                | DeletePolicyError::Unauthorized
                | DeletePolicyError::SystemPolicyProtected(_)
        )
    }

    /// Returns true if the error is a server error (5xx-like)
    ///
    /// Server errors indicate something went wrong on the server side
    /// and might be worth retrying.
    pub fn is_server_error(&self) -> bool {
        matches!(self, DeletePolicyError::StorageError(_))
    }

    /// Returns true if the error indicates the resource was not found
    pub fn is_not_found(&self) -> bool {
        matches!(self, DeletePolicyError::PolicyNotFound(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let error = DeletePolicyError::StorageError("Database connection failed".to_string());
        assert_eq!(
            error.to_string(),
            "Policy storage error: Database connection failed"
        );
    }

    #[test]
    fn test_policy_not_found_display() {
        let error = DeletePolicyError::PolicyNotFound("my-policy".to_string());
        assert_eq!(error.to_string(), "Policy not found: my-policy");
    }

    #[test]
    fn test_error_is_retryable() {
        assert!(DeletePolicyError::StorageError("test".to_string()).is_retryable());
        assert!(!DeletePolicyError::PolicyNotFound("test".to_string()).is_retryable());
        assert!(!DeletePolicyError::PolicyInUse("test".to_string()).is_retryable());
    }

    #[test]
    fn test_error_is_client_error() {
        assert!(DeletePolicyError::PolicyNotFound("test".to_string()).is_client_error());
        assert!(DeletePolicyError::PolicyInUse("test".to_string()).is_client_error());
        assert!(DeletePolicyError::Unauthorized.is_client_error());
        assert!(!DeletePolicyError::StorageError("test".to_string()).is_client_error());
    }

    #[test]
    fn test_error_is_server_error() {
        assert!(DeletePolicyError::StorageError("test".to_string()).is_server_error());
        assert!(!DeletePolicyError::PolicyNotFound("test".to_string()).is_server_error());
    }

    #[test]
    fn test_error_is_not_found() {
        assert!(DeletePolicyError::PolicyNotFound("test".to_string()).is_not_found());
        assert!(!DeletePolicyError::StorageError("test".to_string()).is_not_found());
        assert!(!DeletePolicyError::PolicyInUse("test".to_string()).is_not_found());
    }

    #[test]
    fn test_policy_in_use_error() {
        let error = DeletePolicyError::PolicyInUse("Attached to 5 users".to_string());
        assert!(error.to_string().contains("Attached to 5 users"));
        assert!(error.is_client_error());
        assert!(!error.is_retryable());
    }

    #[test]
    fn test_system_policy_protected_error() {
        let error = DeletePolicyError::SystemPolicyProtected("admin-policy".to_string());
        assert!(error.to_string().contains("admin-policy"));
        assert!(error.is_client_error());
        assert!(!error.is_retryable());
    }

    #[test]
    fn test_invalid_policy_id_error() {
        let error = DeletePolicyError::InvalidPolicyId("".to_string());
        assert_eq!(error.to_string(), "Invalid policy ID: ");
        assert!(error.is_client_error());
        assert!(!error.is_retryable());
    }
}
