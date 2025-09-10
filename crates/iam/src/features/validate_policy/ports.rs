use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("Invalid policy syntax: {0}")]
    Syntax(String),
    #[error("Semantic validation failed: {0}")]
    Semantic(String),
    #[error("Internal validation error: {0}")]
    Internal(String),
}

#[async_trait]
pub trait PolicyValidatorPort: Send + Sync {
    async fn validate_policy(&self, policy: &str) -> Result<(), ValidationError>;
}
