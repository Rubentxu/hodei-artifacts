use crate::features::create_account::error::CreateAccountError;
use crate::features::create_account::ports::AccountPersister;
use crate::shared::application::ports::account_repository::AccountRepository;
use crate::shared::domain::account::Account;
use async_trait::async_trait;

/// Adapter implementing AccountPersister over any AccountRepository.
/// Creation is done via `AccountRepositoryAdapter::new(repo)`. The previous
/// `account_persister` helper was removed to avoid dead_code warnings and
/// simplify DI wiring.
pub(crate) struct AccountRepositoryAdapter<AR: AccountRepository> {
    repository: AR,
}

impl<AR: AccountRepository> AccountRepositoryAdapter<AR> {
    pub(crate) fn new(repository: AR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<AR: AccountRepository + Send + Sync> AccountPersister for AccountRepositoryAdapter<AR> {
    async fn save(&self, account: Account) -> Result<(), CreateAccountError> {
        <AR as AccountRepository>::save(&self.repository, &account)
            .await
            .map_err(CreateAccountError::AccountRepositoryError)
    }
}
