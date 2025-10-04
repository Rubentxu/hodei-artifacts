use thiserror::Error;
use crate::shared::application::ports::account_repository::AccountRepositoryError;
use crate::shared::application::ports::ou_repository::OuRepositoryError;

#[derive(Debug, Error)]
pub enum MoveAccountError {
    #[error("Account repository error: {0}")]
    AccountRepositoryError(#[from] AccountRepositoryError),
    #[error("OU repository error: {0}")]
    OuRepositoryError(#[from] OuRepositoryError),
    #[error("Account not found")]
    AccountNotFound,
    #[error("Source OU not found")]
    SourceOuNotFound,
    #[error("Target OU not found")]
    TargetOuNotFound,
}
