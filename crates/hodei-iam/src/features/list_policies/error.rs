use thiserror::Error;

/// Errors that can occur during policy listing operations
#[derive(Debug, Error)]
pub enum ListPoliciesError {
    /// Database-related error
    #[error("Database error: {0}")]
    Database(String),
    /// Invalid query parameters
    #[error("Invalid query parameters: {0}")]
    InvalidQuery(String),
    /// Invalid pagination parameters
    #[error("Invalid pagination parameters: {0}")]
    InvalidPagination(String),
    /// Repository error
    #[error("Repository error: {0}")]
    RepositoryError(String),
    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),
}
