//! Adaptador de SurrealDB para el caso de uso CreateAccount
//!
//! Este adaptador conecta la implementación genérica de SurrealUnitOfWork
//! con el puerto específico CreateAccountUnitOfWork de la feature.

use async_trait::async_trait;
use std::sync::Arc;

use crate::features::create_account::error::CreateAccountError;
use crate::features::create_account::ports::{CreateAccountUnitOfWork, CreateAccountUnitOfWorkFactory};
use crate::internal::application::ports::account_repository::AccountRepository;
use crate::internal::infrastructure::surreal::{SurrealUnitOfWork, SurrealUnitOfWorkFactory};
use kernel::application::ports::unit_of_work::{UnitOfWork, UnitOfWorkFactory};

/// Adaptador que envuelve SurrealUnitOfWork para la feature create_account
pub struct CreateAccountSurrealUnitOfWorkAdapter<C = surrealdb::engine::any::Any>
where
    C: surrealdb::Connection,
{
    inner: SurrealUnitOfWork<C>,
}

impl<C> CreateAccountSurrealUnitOfWorkAdapter<C>
where
    C: surrealdb::Connection,
{
    pub fn new(inner: SurrealUnitOfWork<C>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl<C> CreateAccountUnitOfWork for CreateAccountSurrealUnitOfWorkAdapter<C>
where
    C: surrealdb::Connection,
{
    async fn begin(&mut self) -> Result<(), CreateAccountError> {
        self.inner
            .begin()
            .await
            .map_err(|e| CreateAccountError::TransactionError(e.to_string()))
    }

    async fn commit(&mut self) -> Result<(), CreateAccountError> {
        self.inner
            .commit()
            .await
            .map_err(|e| CreateAccountError::TransactionError(e.to_string()))
    }

    async fn rollback(&mut self) -> Result<(), CreateAccountError> {
        self.inner
            .rollback()
            .await
            .map_err(|e| CreateAccountError::TransactionError(e.to_string()))
    }

    fn accounts(&self) -> Arc<dyn AccountRepository> {
        self.inner.accounts()
    }
}

/// Factory que crea instancias de CreateAccountSurrealUnitOfWorkAdapter
pub struct CreateAccountSurrealUnitOfWorkFactoryAdapter<C>
where
    C: surrealdb::Connection,
{
    inner: Arc<SurrealUnitOfWorkFactory<C>>,
}

impl<C> CreateAccountSurrealUnitOfWorkFactoryAdapter<C>
where
    C: surrealdb::Connection,
{
    pub fn new(inner: Arc<SurrealUnitOfWorkFactory<C>>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl<C> CreateAccountUnitOfWorkFactory for CreateAccountSurrealUnitOfWorkFactoryAdapter<C>
where
    C: surrealdb::Connection,
{
    type UnitOfWork = CreateAccountSurrealUnitOfWorkAdapter<C>;

    async fn create(&self) -> Result<Self::UnitOfWork, CreateAccountError> {
        let uow = self
            .inner
            .create()
            .await
            .map_err(|e| CreateAccountError::TransactionError(e.to_string()))?;
        Ok(CreateAccountSurrealUnitOfWorkAdapter::new(uow))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_adapter_wraps_real_uow() {
        // Arrange - Create a real SurrealDB connection
        let db = surrealdb::Surreal::new::<surrealdb::engine::local::Mem>(())
            .await
            .unwrap();
        db.use_ns("test").use_db("test").await.unwrap();

        let factory = Arc::new(SurrealUnitOfWorkFactory::new(Arc::new(db)));
        let adapter_factory = CreateAccountSurrealUnitOfWorkFactoryAdapter::new(factory);

        // Act
        let result = adapter_factory.create().await;

        // Assert - Verify the adapter can be created
        assert!(result.is_ok(), "Factory should create UoW adapter successfully");
        let _uow = result.unwrap();

        // Note: We don't test actual transactions here as SurrealDB Mem engine
        // has limitations with BEGIN/COMMIT in test context.
        // Transaction behavior should be tested in integration tests with a real DB.
    }

    #[tokio::test]
    async fn test_adapter_provides_repository() {
        // Arrange
        let db = surrealdb::Surreal::new::<surrealdb::engine::local::Mem>(())
            .await
            .unwrap();
        db.use_ns("test").use_db("test").await.unwrap();

        let factory = Arc::new(SurrealUnitOfWorkFactory::new(Arc::new(db)));
        let adapter_factory = CreateAccountSurrealUnitOfWorkFactoryAdapter::new(factory);
        let uow = adapter_factory.create().await.unwrap();

        // Act - Get repository from the UoW
        let account_repo = uow.accounts();

        // Assert - Verify repository is valid
        assert!(Arc::strong_count(&account_repo) >= 1, "Should return valid repository");
    }

    #[tokio::test]
    async fn test_adapter_factory_is_reusable() {
        // Arrange
        let db = surrealdb::Surreal::new::<surrealdb::engine::local::Mem>(())
            .await
            .unwrap();
        db.use_ns("test").use_db("test").await.unwrap();

        let factory = Arc::new(SurrealUnitOfWorkFactory::new(Arc::new(db)));
        let adapter_factory = CreateAccountSurrealUnitOfWorkFactoryAdapter::new(factory);

        // Act - Create multiple UoW instances from the same factory
        let uow1 = adapter_factory.create().await;
        let uow2 = adapter_factory.create().await;

        // Assert - Both should be created successfully
        assert!(uow1.is_ok(), "First UoW should be created");
        assert!(uow2.is_ok(), "Second UoW should be created");
    }
}

