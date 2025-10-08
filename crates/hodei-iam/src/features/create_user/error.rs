use thiserror::Error;

/// Errors that can occur during user creation
#[derive(Debug, Error)]
pub enum CreateUserError {
    #[error("Failed to save user: {0}")]
    PersistenceError(String),
    
    #[error("Invalid command data: {0}")]
    InvalidCommand(String),
    
    #[error("Storage error: {0}")]
    StorageError(String),
}

// Conversion from Box<dyn StdError> for storage errors
impl From<Box<dyn std::error::Error + Send + Sync>> for CreateUserError {
    fn from(err: Box<dyn std::error::Error + Send + Sync>) -> Self {
        CreateUserError::StorageError(err.to_string())
    }
}