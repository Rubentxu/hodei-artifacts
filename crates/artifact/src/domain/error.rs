use thiserror::Error;

#[derive(Error, Debug)]
pub enum DomainError {
    #[error("Invalid state transition: {0}")]
    InvalidStateTransition(String),
    #[error("Validation error: {0}")]
    ValidationError(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Already exists: {0}")]
    AlreadyExists(String),
    #[error("Internal error: {0}")]
    InternalError(String),
}
