use std::fmt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ManagePolicyVersionsError {
    #[error("Policy not found: {policy_id}")]
    PolicyNotFound { policy_id: String },

    #[error("Version not found: {version}")]
    VersionNotFound { version: i64 },

    #[error("Version already exists: {version}")]
    VersionAlreadyExists { version: i64 },

    #[error("Invalid version number: {version}")]
    InvalidVersionNumber { version: i64 },

    #[error("Version conflict: expected {expected}, got {actual}")]
    VersionConflict { expected: i64, actual: i64 },

    #[error("Storage error: {message}")]
    StorageError { message: String },

    #[error("Authentication failed")]
    AuthenticationFailed,

    #[error("Authorization failed: insufficient permissions")]
    AuthorizationFailed,

    #[error("Cannot rollback to version: {reason}")]
    CannotRollback { reason: String },

    #[error("Version history error: {message}")]
    VersionHistoryError { message: String },
}

impl ManagePolicyVersionsError {
    pub fn policy_not_found(policy_id: impl Into<String>) -> Self {
        Self::PolicyNotFound {
            policy_id: policy_id.into(),
        }
    }

    pub fn version_not_found(version: i64) -> Self {
        Self::VersionNotFound { version }
    }

    pub fn version_already_exists(version: i64) -> Self {
        Self::VersionAlreadyExists { version }
    }

    pub fn invalid_version(version: i64) -> Self {
        Self::InvalidVersionNumber { version }
    }

    pub fn version_conflict(expected: i64, actual: i64) -> Self {
        Self::VersionConflict { expected, actual }
    }

    pub fn storage_error(message: impl Into<String>) -> Self {
        Self::StorageError {
            message: message.into(),
        }
    }

    pub fn cannot_rollback(reason: impl Into<String>) -> Self {
        Self::CannotRollback {
            reason: reason.into(),
        }
    }

    pub fn history_error(message: impl Into<String>) -> Self {
        Self::VersionHistoryError {
            message: message.into(),
        }
    }
}
