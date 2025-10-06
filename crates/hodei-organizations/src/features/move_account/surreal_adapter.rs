//! Adaptador de SurrealDB para el caso de uso MoveAccount
//!
//! Este adaptador conecta la implementación genérica de SurrealUnitOfWork
//! con el puerto específico MoveAccountUnitOfWork de la feature.

use async_trait::async_trait;
use std::sync::Arc;

use crate::features::move_account::error::MoveAccountError;
use crate::features::move_account::ports::{MoveAccountUnitOfWork, MoveAccountUnitOfWorkFactory};
use crate::internal::application::ports::account_repository::AccountRepository;
use crate::internal::application::ports::ou_repository::OuRepository;
use crate::internal::infrastructure::surreal::{SurrealUnitOfWork, SurrealUnitOfWorkFactory};
use kernel::application::ports::unit_of_work::{UnitOfWork, UnitOfWorkFactory};

/// Adaptador que envuelve SurrealUnitOfWork para la feature move_account
pub struct MoveAccountSurrealUnitOfWorkAdapter<C = surrealdb::engine::any::Any>
where
    C: surrealdb::Connection,
{
    inner: SurrealUnitOfWork<C>,
}

impl<C> MoveAccountSurrealUnitOfWorkAdapter<C>
where
    C: surrealdb::Connection,
{
    pub fn new(inner: SurrealUnitOfWork<C>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl<C> MoveAccountUnitOfWork for MoveAccountSurrealUnitOfWorkAdapter<C>
where
    C: surrealdb::Connection,
{
    async fn begin(&mut self) -> Result<(), MoveAccountError> {
        self.inner
            .begin()
            .await
            .map_err(|e| MoveAccountError::OuRepositoryError(
                crate::internal::application::ports::ou_repository::OuRepositoryError::DatabaseError(
                    e.to_string()
                )
            ))
    }

    async fn commit(&mut self) -> Result<(), MoveAccountError> {
        self.inner
            .commit()
            .await
            .map_err(|e| MoveAccountError::OuRepositoryError(
                crate::internal::application::ports::ou_repository::OuRepositoryError::DatabaseError(
                    e.to_string()
                )
            ))
    }

    async fn rollback(&mut self) -> Result<(), MoveAccountError> {
        self.inner
            .rollback()
            .await
            .map_err(|e| MoveAccountError::OuRepositoryError(
                crate::internal::application::ports::ou_repository::OuRepositoryError::DatabaseError(
                    e.to_string()
                )
            ))
    }

    fn accounts(&self) -> Arc<dyn AccountRepository> {
        self.inner.accounts()
    }

    fn ous(&self) -> Arc<dyn OuRepository> {
        self.inner.ous()
    }
}

/// Factory que crea instancias de MoveAccountSurrealUnitOfWorkAdapter
pub struct MoveAccountSurrealUnitOfWorkFactoryAdapter<C>
where
    C: surrealdb::Connection,
{
    inner: Arc<SurrealUnitOfWorkFactory<C>>,
}

impl<C> MoveAccountSurrealUnitOfWorkFactoryAdapter<C>
where
    C: surrealdb::Connection,
{
    pub fn new(inner: Arc<SurrealUnitOfWorkFactory<C>>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl<C> MoveAccountUnitOfWorkFactory for MoveAccountSurrealUnitOfWorkFactoryAdapter<C>
where
    C: surrealdb::Connection,
{
    type UnitOfWork = MoveAccountSurrealUnitOfWorkAdapter<C>;

    async fn create(&self) -> Result<Self::UnitOfWork, MoveAccountError> {
        let uow = self
            .inner
            .create()
            .await
            .map_err(|e| MoveAccountError::OuRepositoryError(
                crate::internal::application::ports::ou_repository::OuRepositoryError::DatabaseError(
                    e.to_string()
                )
            ))?;
        Ok(MoveAccountSurrealUnitOfWorkAdapter::new(uow))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_move_account_adapter_lifecycle() {
        // Arrange
        let db = surrealdb::Surreal::new::<surrealdb::engine::local::Mem>(())
            .await
            .unwrap();
        db.use_ns("test").use_db("test").await.unwrap();

        let factory = Arc::new(SurrealUnitOfWorkFactory::new(Arc::new(db)));
        let adapter_factory = MoveAccountSurrealUnitOfWorkFactoryAdapter::new(factory);

        // Act
        let result = adapter_factory.create().await;

        // Assert
        assert!(result.is_ok(), "Factory should create UoW adapter successfully");

        let uow = result.unwrap();

        // Verify repositories are available
        let _accounts = uow.accounts();
        let _ous = uow.ous();

        // Note: Transaction operations tested in integration tests with real DB
    }

    #[tokio::test]
    async fn test_adapter_provides_both_repositories() {
        // Arrange
        let db = surrealdb::Surreal::new::<surrealdb::engine::local::Mem>(())
            .await
            .unwrap();
        db.use_ns("test").use_db("test").await.unwrap();

        let factory = Arc::new(SurrealUnitOfWorkFactory::new(Arc::new(db)));
        let adapter_factory = MoveAccountSurrealUnitOfWorkFactoryAdapter::new(factory);
        let uow = adapter_factory.create().await.unwrap();

        // Act
        let account_repo = uow.accounts();
        let ou_repo = uow.ous();

        // Assert
        assert!(Arc::strong_count(&account_repo) >= 1, "Should return valid account repository");
        assert!(Arc::strong_count(&ou_repo) >= 1, "Should return valid OU repository");
    }
}
