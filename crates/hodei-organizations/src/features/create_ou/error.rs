use thiserror::Error;
use crate::shared::application::ports::ou_repository::OuRepositoryError;

#[derive(Debug, Error)]
pub enum CreateOuError {
    #[error("OU repository error: {0}")]
    OuRepositoryError(#[from] OuRepositoryError),
    #[error("Invalid OU name")]
    InvalidOuName,
}
