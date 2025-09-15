use crate::features::upload_artifact::error::UploadArtifactError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MetadataError {
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

impl From<MetadataError> for UploadArtifactError {
    fn from(error: MetadataError) -> Self {
        match error {
            MetadataError::RepositoryError(msg) => UploadArtifactError::RepositoryError(msg),
            MetadataError::StorageError(msg) => UploadArtifactError::StorageError(msg),
            MetadataError::EventError(msg) => UploadArtifactError::EventError(msg),
            MetadataError::ParseError(msg) => UploadArtifactError::BadRequest(msg),
            MetadataError::UnsupportedArtifactType(msg) => UploadArtifactError::BadRequest(msg),
            MetadataError::IoError(e) => UploadArtifactError::StorageError(e.to_string()),
        }
    }
}
