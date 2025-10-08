use thiserror::Error;

/// Errors that can occur during group creation
#[derive(Debug, Error)]
pub enum CreateGroupError {
    #[error("Failed to save group: {0}")]
    PersistenceError(String),
    
    #[error("Invalid command data: {0}")]
    InvalidCommand(String),
}