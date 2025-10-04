use crate::features::create_account::error::CreateAccountError;
use crate::features::create_account::ports::AccountPersister;
use crate::shared::application::ports::account_repository::AccountRepository;
use crate::shared::domain::account::Account;
use async_trait::async_trait;

/// Adapter that implements the AccountPersister trait using the AccountRepository
pub struct AccountRepositoryAdapter<AR: AccountRepository> {
    repository: AR,
}

impl<AR: AccountRepository> AccountRepositoryAdapter<AR> {
    /// Create a new adapter instance
    pub fn new(repository: AR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<AR: AccountRepository + Send + Sync> AccountPersister for AccountRepositoryAdapter<AR> {
    /// Save an account using the repository
    async fn save(&self, account: Account) -> Result<(), CreateAccountError> {
        self.repository
            .save(&account)
            .await
            .map_err(CreateAccountError::AccountRepositoryError)
    }
}
