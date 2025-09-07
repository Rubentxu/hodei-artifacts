use thiserror::Error;

#[derive(Debug, Error)]
pub enum UploadArtifactError {
    #[error("Repository error: {0}")]
    RepositoryError(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Event error: {0}")]
    EventError(String),

    #[error("Artifact already exists: {0}")]
    AlreadyExistsError(String),
}
