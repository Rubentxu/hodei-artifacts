use thiserror::Error;
use crate::shared::application::ports::scp_repository::ScpRepositoryError;
use crate::shared::application::ports::account_repository::AccountRepositoryError;
use crate::shared::application::ports::ou_repository::OuRepositoryError;

/// Error type for attach SCP use case
#[derive(Debug, Error)]
pub enum AttachScpError {
    #[error("SCP repository error: {0}")]
    ScpRepository(#[from] ScpRepositoryError),
    #[error("Account repository error: {0}")]
    AccountRepository(#[from] AccountRepositoryError),
    #[error("OU repository error: {0}")]
    OuRepository(#[from] OuRepositoryError),
    #[error("SCP not found: {0}")]
    ScpNotFound(String),
    #[error("Target entity not found: {0}")]
    TargetNotFound(String),
    #[error("Invalid target entity type: {0}")]
    InvalidTargetType(String),
}
