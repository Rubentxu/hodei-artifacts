use crate::shared::domain::account::Account;
use async_trait::async_trait;
use kernel::Hrn;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AccountRepositoryError {
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("Account not found")]
    AccountNotFound,
}

#[async_trait]
pub trait AccountRepository {
    async fn save(&self, account: &Account) -> Result<(), AccountRepositoryError>;
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, AccountRepositoryError>;
}
