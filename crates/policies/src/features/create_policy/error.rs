use std::fmt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CreatePolicyError {
    #[error("Policy already exists: {policy_id}")]
    PolicyAlreadyExists { policy_id: String },

    #[error("Policy validation failed: {reason}")]
    PolicyValidationFailed { reason: String },

    #[error("Storage error: {message}")]
    StorageError { message: String },

    #[error("Authentication failed")]
    AuthenticationFailed,

    #[error("Authorization failed: insufficient permissions")]
    AuthorizationFailed,

    #[error("Invalid policy data: {field}, {reason}")]
    InvalidPolicyData { field: String, reason: String },
}

impl CreatePolicyError {
    pub fn policy_already_exists(policy_id: impl Into<String>) -> Self {
        Self::PolicyAlreadyExists {
            policy_id: policy_id.into(),
        }
    }

    pub fn validation_failed(reason: impl Into<String>) -> Self {
        Self::PolicyValidationFailed {
            reason: reason.into(),
        }
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
}
