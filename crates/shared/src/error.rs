use thiserror::Error;

#[derive(Debug, Error)]
pub enum SharedError {
    #[error("Error publicando evento: {0}")]
    PublishError(String),
}

