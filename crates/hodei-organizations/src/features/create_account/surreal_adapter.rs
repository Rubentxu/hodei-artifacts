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
pub struct CreateAccountSurrealUnitOfWorkAdapter {
    inner: SurrealUnitOfWork,
}

impl CreateAccountSurrealUnitOfWorkAdapter {
    pub fn new(inner: SurrealUnitOfWork) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl CreateAccountUnitOfWork for CreateAccountSurrealUnitOfWorkAdapter {
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
pub struct CreateAccountSurrealUnitOfWorkFactoryAdapter {
    inner: Arc<SurrealUnitOfWorkFactory>,
}

impl CreateAccountSurrealUnitOfWorkFactoryAdapter {
    pub fn new(inner: Arc<SurrealUnitOfWorkFactory>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl CreateAccountUnitOfWorkFactory for CreateAccountSurrealUnitOfWorkFactoryAdapter {
    type UnitOfWork = CreateAccountSurrealUnitOfWorkAdapter;

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
    use surrealdb::{Surreal, engine::local::Mem};

    #[tokio::test]
    async fn test_adapter_wraps_real_uow() {
        // Arrange
        let db = Surreal::new::<Mem>(()).await.unwrap();
        db.use_ns("test").use_db("test").await.unwrap();

        let factory = Arc::new(SurrealUnitOfWorkFactory::new(Arc::new(db)));
        let adapter_factory = CreateAccountSurrealUnitOfWorkFactoryAdapter::new(factory);

        // Act
        let result = adapter_factory.create().await;

        // Assert
        assert!(result.is_ok(), "Factory should create UoW successfully");
        let mut uow = result.unwrap();

        // Verify transaction lifecycle
        assert!(uow.begin().await.is_ok(), "Should begin transaction");
        assert!(uow.commit().await.is_ok(), "Should commit transaction");
    }

    #[tokio::test]
    async fn test_adapter_rollback() {
        // Arrange
        let db = Surreal::new::<Mem>(()).await.unwrap();
        db.use_ns("test").use_db("test").await.unwrap();

        let factory = Arc::new(SurrealUnitOfWorkFactory::new(Arc::new(db)));
        let adapter_factory = CreateAccountSurrealUnitOfWorkFactoryAdapter::new(factory);
        let mut uow = adapter_factory.create().await.unwrap();

        // Act
        uow.begin().await.unwrap();
        let rollback_result = uow.rollback().await;

        // Assert
        assert!(rollback_result.is_ok(), "Should rollback transaction successfully");
    }

    #[tokio::test]
    async fn test_adapter_provides_repository() {
        // Arrange
        let db = Surreal::new::<Mem>(()).await.unwrap();
        db.use_ns("test").use_db("test").await.unwrap();

        let factory = Arc::new(SurrealUnitOfWorkFactory::new(Arc::new(db)));
        let adapter_factory = CreateAccountSurrealUnitOfWorkFactoryAdapter::new(factory);
        let uow = adapter_factory.create().await.unwrap();

        // Act
        let account_repo = uow.accounts();

        // Assert
        assert!(Arc::strong_count(&account_repo) >= 1, "Should return valid repository");
    }
}

