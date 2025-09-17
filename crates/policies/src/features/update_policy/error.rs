use std::fmt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum UpdatePolicyError {
    #[error("Policy not found: {policy_id}")]
    PolicyNotFound { policy_id: String },

    #[error("Policy validation failed: {reason}")]
    PolicyValidationFailed { reason: String },

    #[error("Policy version conflict: expected {expected}, got {actual}")]
    PolicyVersionConflict { expected: i64, actual: i64 },

    #[error("Storage error: {message}")]
    StorageError { message: String },

    #[error("Authentication failed")]
    AuthenticationFailed,

    #[error("Authorization failed: insufficient permissions")]
    AuthorizationFailed,

    #[error("Invalid policy data: {field}, {reason}")]
    InvalidPolicyData { field: String, reason: String },

    #[error("Policy is in immutable state")]
    PolicyImmutable,

    #[error("Policy update not allowed: {reason}")]
    PolicyUpdateNotAllowed { reason: String },
}

impl UpdatePolicyError {
    pub fn policy_not_found(policy_id: impl Into<String>) -> Self {
        Self::PolicyNotFound {
            policy_id: policy_id.into(),
        }
    }

    pub fn validation_failed(reason: impl Into<String>) -> Self {
        Self::PolicyValidationFailed {
            reason: reason.into(),
        }
    }

    pub fn version_conflict(expected: i64, actual: i64) -> Self {
        Self::PolicyVersionConflict { expected, actual }
    }

    pub fn storage_error(message: impl Into<String>) -> Self {
        Self::StorageError {
            message: message.into(),
        }
    }

    pub fn invalid_data(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidPolicyData {
            field: field.into(),
            reason: reason.into(),
        }
    }

    pub fn update_not_allowed(reason: impl Into<String>) -> Self {
        Self::PolicyUpdateNotAllowed {
            reason: reason.into(),
        }
    }
}
