use thiserror::Error;
use crate::shared::application::ports::account_repository::AccountRepositoryError;

#[derive(Debug, Error)]
pub enum CreateAccountError {
    #[error("Account repository error: {0}")]
    AccountRepositoryError(#[from] AccountRepositoryError),
    #[error("Invalid account name")]
    InvalidAccountName,
}
