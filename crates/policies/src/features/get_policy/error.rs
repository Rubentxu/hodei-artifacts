use std::fmt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetPolicyError {
    #[error("Policy not found: {policy_id}")]
    PolicyNotFound { policy_id: String },

    #[error("Storage error: {message}")]
    StorageError { message: String },

    #[error("Authentication failed")]
    AuthenticationFailed,

    #[error("Authorization failed: insufficient permissions")]
    AuthorizationFailed,

    #[error("Invalid policy ID: {policy_id}")]
    InvalidPolicyId { policy_id: String },
}

impl GetPolicyError {
    pub fn policy_not_found(policy_id: impl Into<String>) -> Self {
        Self::PolicyNotFound {
            policy_id: policy_id.into(),
        }
    }

    pub fn storage_error(message: impl Into<String>) -> Self {
        Self::StorageError {
            message: message.into(),
        }
    }

    pub fn invalid_policy_id(policy_id: impl Into<String>) -> Self {
        Self::InvalidPolicyId {
            policy_id: policy_id.into(),
        }
    }
}
