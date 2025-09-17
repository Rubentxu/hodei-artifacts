use std::fmt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DeletePolicyError {
    #[error("Policy not found: {policy_id}")]
    PolicyNotFound { policy_id: String },

    #[error("Policy deletion failed: {reason}")]
    PolicyDeletionFailed { reason: String },

    #[error("Storage error: {message}")]
    StorageError { message: String },

    #[error("Authentication failed")]
    AuthenticationFailed,

    #[error("Authorization failed: insufficient permissions")]
    AuthorizationFailed,

    #[error("Policy is in immutable state")]
    PolicyImmutable,

    #[error("Policy deletion not allowed: {reason}")]
    PolicyDeletionNotAllowed { reason: String },

    #[error("Policy has dependencies: {dependencies}")]
    PolicyHasDependencies { dependencies: String },
}

impl DeletePolicyError {
    pub fn policy_not_found(policy_id: impl Into<String>) -> Self {
        Self::PolicyNotFound {
            policy_id: policy_id.into(),
        }
    }

    pub fn deletion_failed(reason: impl Into<String>) -> Self {
        Self::PolicyDeletionFailed {
            reason: reason.into(),
        }
    }

    pub fn storage_error(message: impl Into<String>) -> Self {
        Self::StorageError {
            message: message.into(),
        }
    }

    pub fn deletion_not_allowed(reason: impl Into<String>) -> Self {
        Self::PolicyDeletionNotAllowed {
            reason: reason.into(),
        }
    }

    pub fn has_dependencies(dependencies: impl Into<String>) -> Self {
        Self::PolicyHasDependencies {
            dependencies: dependencies.into(),
        }
    }
}
