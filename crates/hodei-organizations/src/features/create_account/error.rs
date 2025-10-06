use crate::internal::application::ports::account_repository::AccountRepositoryError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CreateAccountError {
    #[error("Account repository error: {0}")]
    AccountRepositoryError(#[from] AccountRepositoryError),
    #[error("Invalid account name")]
    InvalidAccountName,
    #[error("Transaction error: {0}")]
    TransactionError(String),
}
