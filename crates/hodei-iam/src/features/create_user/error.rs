use crate::internal::application::ports::UserRepositoryError;
use thiserror::Error;

/// Errors that can occur during user creation
#[derive(Debug, Error)]
pub enum CreateUserError {
    #[error("Failed to begin transaction: {0}")]
    TransactionBeginFailed(String),

    #[error("Failed to commit transaction: {0}")]
    TransactionCommitFailed(String),

    #[error("Failed to save user: {0}")]
    UserSaveFailed(#[from] UserRepositoryError),

    #[error("Invalid command data: {0}")]
    InvalidCommand(String),
}

// Conversion from Box<dyn StdError> for transaction errors
impl From<Box<dyn std::error::Error + Send + Sync>> for CreateUserError {
    fn from(err: Box<dyn std::error::Error + Send + Sync>) -> Self {
        // For simplicity, treat all transaction errors as begin failures
        // In a real implementation, you might want to distinguish based on context
        CreateUserError::TransactionBeginFailed(err.to_string())
    }
}
