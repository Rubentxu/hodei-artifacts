use crate::features::create_account::ports::AccountPersister;
use crate::features::create_account::error::CreateAccountError;
use crate::shared::domain::account::Account;
use crate::shared::application::ports::account_repository::{AccountRepository};
use async_trait::async_trait;
use std::sync::Arc;

pub struct AccountPersisterAdapter<AR: AccountRepository> {
    repository: Arc<AR>,
}

impl<AR: AccountRepository> AccountPersisterAdapter<AR> {
    pub fn new(repository: Arc<AR>) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<AR: AccountRepository + Send + Sync> AccountPersister for AccountPersisterAdapter<AR> {
    async fn save(&self, account: Account) -> Result<(), CreateAccountError> {
        self.repository.save(&account).await
            .map_err(|e| CreateAccountError::AccountRepositoryError(e))
    }
}
