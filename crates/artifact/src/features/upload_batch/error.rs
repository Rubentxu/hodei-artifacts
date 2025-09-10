use thiserror::Error;

#[derive(Error, Debug)]
pub enum BatchUploadError {
    #[error("Validation failed: {0}")]
    ValidationFailed(String),
    
    #[error("Storage error: {0}")]
    StorageError(String),
    
    #[error("Repository error: {0}")]
    RepositoryError(String),
    
    #[error("Event publish error: {0}")]
    EventPublishError(String),
    
    #[error("Transaction error: {0}")]
    TransactionError(String),
    
    #[error("Invalid batch request: {0}")]
    InvalidRequest(String),
    
    #[error("Batch processing timeout: {0}")]
    Timeout(String),
    
    #[error("Internal error: {0}")]
    InternalError(String),
}

pub type PortResult<T> = Result<T, BatchUploadError>;