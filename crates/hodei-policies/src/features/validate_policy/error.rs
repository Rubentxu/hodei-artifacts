use thiserror::Error;

#[derive(Debug, Error)]
pub enum ValidatePolicyError {
    #[error("An unexpected internal error occurred: {0}")]
    InternalError(String),
}
