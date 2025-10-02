use crate::shared::domain::ports::StorageError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HodeiPoliciesError {
    #[error("Invalid command: {0}")]
    InvalidCommand(String),

    #[error("Policy with ID '{0}' was not found")]
    NotFound(String),

    #[error("Storage error")]
    Storage(#[from] StorageError), // Automatic conversion from StorageError

    #[error("Error parsing policy: {0}")]
    PolicyParse(String),

    #[error("Policy is invalid according to schema: {0}")]
    PolicyValidation(String),

    #[error("Internal engine error: {0}")]
    Engine(String),
}