use crate::internal::application::ports::GroupRepositoryError;
use thiserror::Error;

/// Errors that can occur during group creation
#[derive(Debug, Error)]
pub enum CreateGroupError {
    #[error("Failed to begin transaction: {0}")]
    TransactionBeginFailed(String),

    #[error("Failed to commit transaction: {0}")]
    TransactionCommitFailed(String),

    #[error("Failed to save group: {0}")]
    GroupSaveFailed(#[from] GroupRepositoryError),

    #[error("Invalid command data: {0}")]
    InvalidCommand(String),
}

// Conversion from Box<dyn StdError> for transaction errors
impl From<Box<dyn std::error::Error + Send + Sync>> for CreateGroupError {
    fn from(err: Box<dyn std::error::Error + Send + Sync>) -> Self {
        // For simplicity, treat all transaction errors as begin failures
        // In a real implementation, you might want to distinguish based on context
        CreateGroupError::TransactionBeginFailed(err.to_string())
    }
}
