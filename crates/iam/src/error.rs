use thiserror::Error;

#[derive(Debug, Error)]
pub enum IamError {
    #[error("Validation error: {0}")]
    ValidationError(String),
    #[error("Repository error: {0}")]
    MongoError(String),
    #[error("User not found")]
    NotFound,
    #[error("Internal error: {0}")]
    InternalError(String),
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    #[error("JWT error: {0}")]
    JwtError(String),
    #[error("Bcrypt error: {0}")]
    BcryptError(String),
}
