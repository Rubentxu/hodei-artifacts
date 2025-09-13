use thiserror::Error;
use crate::features::upload_artifact::error::UploadArtifactError;

#[derive(Debug, Error)]
pub enum ChunkedUploadError {
    #[error("Invalid upload session: {0}")]
    InvalidSession(String),
    
    #[error("Session not found: {0}")]
    SessionNotFound(String),
    
    #[error("Session expired: {0}")]
    SessionExpired(String),
    
    #[error("Invalid chunk number: {0}")]
    InvalidChunkNumber(String),
    
    #[error("Chunk size mismatch: expected {expected}, got {actual}")]
    ChunkSizeMismatch { expected: u64, actual: u64 },
    
    #[error("Chunk checksum verification failed: {0}")]
    ChunkChecksumFailed(String),
    
    #[error("Final checksum verification failed: expected {expected}, got {actual}")]
    FinalChecksumFailed { expected: String, actual: String },
    
    #[error("Upload session already completed: {0}")]
    SessionAlreadyCompleted(String),
    
    #[error("Upload session aborted: {0}")]
    SessionAborted(String),
    
    #[error("Storage error: {0}")]
    StorageError(String),
    
    #[error("Repository error: {0}")]
    RepositoryError(String),
    
    #[error("Event error: {0}")]
    EventError(String),
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    #[error("Validation error: {0}")]
    ValidationError(String),
}

impl From<ChunkedUploadError> for UploadArtifactError {
    fn from(error: ChunkedUploadError) -> Self {
        match error {
            ChunkedUploadError::InvalidSession(msg) => UploadArtifactError::BadRequest(msg),
            ChunkedUploadError::SessionNotFound(msg) => UploadArtifactError::NotFound(msg),
            ChunkedUploadError::SessionExpired(msg) => UploadArtifactError::BadRequest(msg),
            ChunkedUploadError::InvalidChunkNumber(msg) => UploadArtifactError::BadRequest(msg),
            ChunkedUploadError::ChunkSizeMismatch { .. } => UploadArtifactError::BadRequest(error.to_string()),
            ChunkedUploadError::ChunkChecksumFailed(msg) => UploadArtifactError::BadRequest(msg),
            ChunkedUploadError::FinalChecksumFailed { .. } => UploadArtifactError::BadRequest(error.to_string()),
            ChunkedUploadError::SessionAlreadyCompleted(msg) => UploadArtifactError::Conflict(msg),
            ChunkedUploadError::SessionAborted(msg) => UploadArtifactError::Conflict(msg),
            ChunkedUploadError::StorageError(msg) => UploadArtifactError::StorageError(msg),
            ChunkedUploadError::RepositoryError(msg) => UploadArtifactError::RepositoryError(msg),
            ChunkedUploadError::EventError(msg) => UploadArtifactError::EventError(msg),
            ChunkedUploadError::IoError(e) => UploadArtifactError::StorageError(e.to_string()),
            ChunkedUploadError::SerializationError(msg) => UploadArtifactError::BadRequest(msg),
            ChunkedUploadError::ValidationError(msg) => UploadArtifactError::BadRequest(msg),
        }
    }
}