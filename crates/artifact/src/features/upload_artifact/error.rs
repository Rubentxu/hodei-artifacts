use thiserror::Error;

#[derive(Debug, Error)]
pub enum UploadArtifactError {
    #[error("Repository error: {0}")]
    RepositoryError(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Event error: {0}")]
    EventError(String),

    #[error("Event publish error: {0}")]
    EventPublishError(String),

    #[error("Artifact already exists: {0}")]
    AlreadyExistsError(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Validation failed: {0}")]
    ValidationFailed(String),

    #[error("Versioning error: {0}")]
    VersioningError(String),
}
