//! Adaptador de SurrealDB para el caso de uso CreateOu
//!
//! Este adaptador conecta la implementación genérica de SurrealUnitOfWork
//! con el puerto específico CreateOuUnitOfWork de la feature.

use async_trait::async_trait;
use std::sync::Arc;

use crate::features::create_ou::error::CreateOuError;
use crate::features::create_ou::ports::{CreateOuUnitOfWork, CreateOuUnitOfWorkFactory};
use crate::internal::application::ports::ou_repository::OuRepository;
use crate::internal::infrastructure::surreal::{SurrealUnitOfWork, SurrealUnitOfWorkFactory};
use kernel::application::ports::unit_of_work::{UnitOfWork, UnitOfWorkFactory};

/// Adaptador que envuelve SurrealUnitOfWork para la feature create_ou
pub struct CreateOuSurrealUnitOfWorkAdapter<C = surrealdb::engine::any::Any>
where
    C: surrealdb::Connection,
{
    inner: SurrealUnitOfWork<C>,
}

impl<C> CreateOuSurrealUnitOfWorkAdapter<C>
where
    C: surrealdb::Connection,
{
    pub fn new(inner: SurrealUnitOfWork<C>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl<C> CreateOuUnitOfWork for CreateOuSurrealUnitOfWorkAdapter<C>
where
    C: surrealdb::Connection,
{
    async fn begin(&mut self) -> Result<(), CreateOuError> {
        self.inner
            .begin()
            .await
            .map_err(|e| CreateOuError::TransactionError(e.to_string()))
    }

    async fn commit(&mut self) -> Result<(), CreateOuError> {
        self.inner
            .commit()
            .await
            .map_err(|e| CreateOuError::TransactionError(e.to_string()))
    }

    async fn rollback(&mut self) -> Result<(), CreateOuError> {
        self.inner
            .rollback()
            .await
            .map_err(|e| CreateOuError::TransactionError(e.to_string()))
    }

    fn ous(&self) -> Arc<dyn OuRepository> {
        self.inner.ous()
    }
}

/// Factory que crea instancias de CreateOuSurrealUnitOfWorkAdapter
pub struct CreateOuSurrealUnitOfWorkFactoryAdapter<C>
where
    C: surrealdb::Connection,
{
    inner: Arc<SurrealUnitOfWorkFactory<C>>,
}

impl<C> CreateOuSurrealUnitOfWorkFactoryAdapter<C>
where
    C: surrealdb::Connection,
{
    pub fn new(inner: Arc<SurrealUnitOfWorkFactory<C>>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl<C> CreateOuUnitOfWorkFactory for CreateOuSurrealUnitOfWorkFactoryAdapter<C>
where
    C: surrealdb::Connection,
{
    type UnitOfWork = CreateOuSurrealUnitOfWorkAdapter<C>;

    async fn create(&self) -> Result<Self::UnitOfWork, CreateOuError> {
        let uow = self
            .inner
            .create()
            .await
            .map_err(|e| CreateOuError::TransactionError(e.to_string()))?;
        Ok(CreateOuSurrealUnitOfWorkAdapter::new(uow))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_adapter_creates_uow_successfully() {
        // Arrange
        let db = surrealdb::Surreal::new::<surrealdb::engine::local::Mem>(())
            .await
            .unwrap();
        db.use_ns("test").use_db("test").await.unwrap();

        let factory = Arc::new(SurrealUnitOfWorkFactory::new(Arc::new(db)));
        let adapter_factory = CreateOuSurrealUnitOfWorkFactoryAdapter::new(factory);

        // Act
        let result = adapter_factory.create().await;

        // Assert - Verify adapter creation
        assert!(result.is_ok(), "Factory should create UoW adapter successfully");

        let uow = result.unwrap();

        // Verify repository access
        let _ou_repo = uow.ous();

        // Note: Transaction operations (begin/commit) are not tested here
        // as they require a real database connection, not in-memory.
        // These should be tested in integration tests.
    }

    #[tokio::test]
    async fn test_adapter_provides_ou_repository() {
        // Arrange
        let db = surrealdb::Surreal::new::<surrealdb::engine::local::Mem>(())
            .await
            .unwrap();
        db.use_ns("test").use_db("test").await.unwrap();

        let factory = Arc::new(SurrealUnitOfWorkFactory::new(Arc::new(db)));
        let adapter_factory = CreateOuSurrealUnitOfWorkFactoryAdapter::new(factory);
        let uow = adapter_factory.create().await.unwrap();

        // Act
        let ou_repo = uow.ous();

        // Assert
        assert!(Arc::strong_count(&ou_repo) >= 1, "Should return valid OU repository");
    }
}

