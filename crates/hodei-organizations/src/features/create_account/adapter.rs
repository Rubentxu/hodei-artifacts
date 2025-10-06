use std::sync::Arc;
use crate::features::create_account::error::CreateAccountError;
use crate::features::create_account::ports::{
    AccountPersister, CreateAccountUnitOfWork, CreateAccountUnitOfWorkFactory,
};
use crate::internal::application::ports::account_repository::AccountRepository;
use crate::internal::domain::account::Account;
use async_trait::async_trait;

/// Adapter implementing AccountPersister over any AccountRepository.
///
/// Deprecated: Use CreateAccountSurrealUnitOfWorkAdapter instead
/// This adapter is kept for backwards compatibility but will be removed in future versions
#[allow(dead_code)]
pub(crate) struct AccountRepositoryAdapter<AR: AccountRepository> {
    repository: AR,
}

#[allow(dead_code)]
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

/// SurrealDB implementation of CreateAccountUnitOfWork
///
/// This adapter wraps the generic SurrealUnitOfWork and exposes only the
/// operations needed for the create_account feature.
pub struct CreateAccountSurrealUnitOfWorkAdapter<C = surrealdb::engine::any::Any>
where
    C: surrealdb::Connection,
{
    inner_uow: crate::internal::infrastructure::surreal::SurrealUnitOfWork<C>,
}

impl<C> CreateAccountSurrealUnitOfWorkAdapter<C>
where
    C: surrealdb::Connection,
{
    pub fn new(uow: crate::internal::infrastructure::surreal::SurrealUnitOfWork<C>) -> Self {
        Self { inner_uow: uow }
    }
}

#[async_trait]
impl<C> CreateAccountUnitOfWork for CreateAccountSurrealUnitOfWorkAdapter<C>
where
    C: surrealdb::Connection,
{
    async fn begin(&mut self) -> Result<(), CreateAccountError> {
        use kernel::application::ports::unit_of_work::UnitOfWork;
        self.inner_uow
            .begin()
            .await
            .map_err(|e| CreateAccountError::TransactionError(e.to_string()))
    }

    async fn commit(&mut self) -> Result<(), CreateAccountError> {
        use kernel::application::ports::unit_of_work::UnitOfWork;
        self.inner_uow
            .commit()
            .await
            .map_err(|e| CreateAccountError::TransactionError(e.to_string()))
    }

    async fn rollback(&mut self) -> Result<(), CreateAccountError> {
        use kernel::application::ports::unit_of_work::UnitOfWork;
        self.inner_uow
            .rollback()
            .await
            .map_err(|e| CreateAccountError::TransactionError(e.to_string()))
    }

    fn accounts(&self) -> Arc<dyn AccountRepository> {
        use kernel::application::ports::unit_of_work::UnitOfWork;
        self.inner_uow.accounts()
    }
}

/// Factory for creating CreateAccountSurrealUnitOfWork instances
pub struct CreateAccountSurrealUnitOfWorkFactoryAdapter<C>
where
    C: surrealdb::Connection,
{
    inner_factory: Arc<crate::internal::infrastructure::surreal::SurrealUnitOfWorkFactory<C>>,
}

impl<C> CreateAccountSurrealUnitOfWorkFactoryAdapter<C>
where
    C: surrealdb::Connection,
{
    pub fn new(
        factory: Arc<crate::internal::infrastructure::surreal::SurrealUnitOfWorkFactory<C>>,
    ) -> Self {
        Self {
            inner_factory: factory,
        }
    }
}

#[async_trait]
impl<C> CreateAccountUnitOfWorkFactory for CreateAccountSurrealUnitOfWorkFactoryAdapter<C>
where
    C: surrealdb::Connection,
{
    type UnitOfWork = CreateAccountSurrealUnitOfWorkAdapter<C>;

    async fn create(&self) -> Result<Self::UnitOfWork, CreateAccountError> {
        use kernel::application::ports::unit_of_work::UnitOfWorkFactory;
        let uow = self
            .inner_factory
            .create()
            .await
            .map_err(|e| CreateAccountError::TransactionError(e.to_string()))?;
        Ok(CreateAccountSurrealUnitOfWorkAdapter::new(uow))
    }
}
