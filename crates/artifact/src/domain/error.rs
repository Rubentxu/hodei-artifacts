use thiserror::Error;

#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("Storage error: {0}")]
    StorageError(String),
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
}
