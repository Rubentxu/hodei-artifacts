use thiserror::Error;
use crate::internal::application::ports::scp_repository::ScpRepositoryError;
use crate::internal::application::ports::account_repository::AccountRepositoryError;
use crate::internal::application::ports::ou_repository::OuRepositoryError;

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
