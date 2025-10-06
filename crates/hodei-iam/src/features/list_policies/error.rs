//! Error types for List Policies feature

use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum ListPoliciesError {
    #[error("Repository error: {0}")]
    RepositoryError(String),

    #[error("Invalid pagination parameters: {0}")]
    InvalidPagination(String),

    #[error("Internal error: {0}")]
    InternalError(String),
}

impl ListPoliciesError {
    pub fn is_client_error(&self) -> bool {
        matches!(self, ListPoliciesError::InvalidPagination(_))
    }

    pub fn is_server_error(&self) -> bool {
        matches!(
            self,
            ListPoliciesError::RepositoryError(_) | ListPoliciesError::InternalError(_)
        )
    }

    pub fn is_retryable(&self) -> bool {
        matches!(self, ListPoliciesError::RepositoryError(_))
    }
}

