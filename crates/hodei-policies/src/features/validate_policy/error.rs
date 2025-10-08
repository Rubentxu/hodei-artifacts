use thiserror::Error;

#[derive(Debug, Error)]
pub enum ValidatePolicyError {
    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("An unexpected internal error occurred: {0}")]
    InternalError(String),
}
