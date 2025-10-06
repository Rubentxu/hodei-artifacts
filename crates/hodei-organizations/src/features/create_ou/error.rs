use crate::internal::application::ports::ou_repository::OuRepositoryError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CreateOuError {
    #[error("OU repository error: {0}")]
    OuRepositoryError(#[from] OuRepositoryError),
    #[error("Invalid OU name")]
    InvalidOuName,
    #[error("Transaction error: {0}")]
    TransactionError(String),
}
