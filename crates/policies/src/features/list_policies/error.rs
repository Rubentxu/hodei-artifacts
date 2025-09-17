use std::fmt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ListPoliciesError {
    #[error("Storage error: {message}")]
    StorageError { message: String },

    #[error("Authentication failed")]
    AuthenticationFailed,

    #[error("Authorization failed: insufficient permissions")]
    AuthorizationFailed,

    #[error("Invalid query parameters: {field}, {reason}")]
    InvalidQueryParameters { field: String, reason: String },

    #[error("Query limit exceeded: max {max}, requested {requested}")]
    QueryLimitExceeded { max: usize, requested: usize },
}

impl ListPoliciesError {
    pub fn storage_error(message: impl Into<String>) -> Self {
        Self::StorageError {
            message: message.into(),
        }
    }

    pub fn invalid_query(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidQueryParameters {
            field: field.into(),
            reason: reason.into(),
        }
    }

    pub fn limit_exceeded(max: usize, requested: usize) -> Self {
        Self::QueryLimitExceeded { max, requested }
    }
}
