use thiserror::Error;
use crate::features::upload_artifact::error::UploadArtifactError;

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("Validation rule error: {0}")]
    RuleError(String),

    #[error("Repository error: {0}")]
    RepositoryError(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Event error: {0}")]
    EventError(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Unsupported artifact type: {0}")]
    UnsupportedArtifactType(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

impl From<ValidationError> for UploadArtifactError {
    fn from(error: ValidationError) -> Self {
        match error {
            ValidationError::RuleError(msg) => UploadArtifactError::BadRequest(msg),
            ValidationError::RepositoryError(msg) => UploadArtifactError::RepositoryError(msg),
            ValidationError::StorageError(msg) => UploadArtifactError::StorageError(msg),
            ValidationError::EventError(msg) => UploadArtifactError::EventError(msg),
            ValidationError::ParseError(msg) => UploadArtifactError::BadRequest(msg),
            ValidationError::UnsupportedArtifactType(msg) => UploadArtifactError::BadRequest(msg),
            ValidationError::IoError(e) => UploadArtifactError::StorageError(e.to_string()),
        }
    }
}