use crate::internal::application::ports::{GroupRepositoryError, UserRepositoryError};
use thiserror::Error;

/// Errors that can occur when adding a user to a group
#[derive(Debug, Error)]
pub enum AddUserToGroupError {
    #[error("Invalid user HRN: {0}")]
    InvalidUserHrn(String),

    #[error("Invalid group HRN: {0}")]
    InvalidGroupHrn(String),

    #[error("Failed to begin transaction: {0}")]
    TransactionBeginFailed(String),

    #[error("Failed to commit transaction: {0}")]
    TransactionCommitFailed(String),

    #[error("Group not found: {0}")]
    GroupNotFound(String),

    #[error("User not found: {0}")]
    UserNotFound(String),

    #[error("Failed to save user: {0}")]
    UserSaveFailed(#[from] UserRepositoryError),

    #[error("Failed to find group: {0}")]
    GroupFindFailed(#[from] GroupRepositoryError),
}

// Conversion from Box<dyn StdError> for transaction errors
impl From<Box<dyn std::error::Error + Send + Sync>> for AddUserToGroupError {
    fn from(err: Box<dyn std::error::Error + Send + Sync>) -> Self {
        // For simplicity, treat all transaction errors as begin failures
        // In a real implementation, you might want to distinguish based on context
        AddUserToGroupError::TransactionBeginFailed(err.to_string())
    }
}
