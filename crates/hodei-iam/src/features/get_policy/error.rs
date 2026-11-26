//! Error types for Get Policy feature

use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum GetPolicyError {
    /// The policy was not found
    #[error("Policy not found: {0}")]
    PolicyNotFound(String),

    /// Error accessing the repository
    #[error("Repository error: {0}")]
    RepositoryError(String),

    /// HRN validation error
    #[error("Invalid HRN: {0}")]
    InvalidHrn(String),
}
