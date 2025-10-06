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
pub struct CreateOuSurrealUnitOfWorkAdapter {
    inner: SurrealUnitOfWork,
}

impl CreateOuSurrealUnitOfWorkAdapter {
    pub fn new(inner: SurrealUnitOfWork) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl CreateOuUnitOfWork for CreateOuSurrealUnitOfWorkAdapter {
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
pub struct CreateOuSurrealUnitOfWorkFactoryAdapter {
    inner: Arc<SurrealUnitOfWorkFactory>,
}

impl CreateOuSurrealUnitOfWorkFactoryAdapter {
    pub fn new(inner: Arc<SurrealUnitOfWorkFactory>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl CreateOuUnitOfWorkFactory for CreateOuSurrealUnitOfWorkFactoryAdapter {
    type UnitOfWork = CreateOuSurrealUnitOfWorkAdapter;

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
    use surrealdb::{Surreal, engine::local::Mem};

    #[tokio::test]
    async fn test_adapter_creates_uow_successfully() {
        let db = Surreal::new::<Mem>(()).await.unwrap();
        db.use_ns("test").use_db("test").await.unwrap();

        let factory = Arc::new(SurrealUnitOfWorkFactory::new(Arc::new(db)));
        let adapter_factory = CreateOuSurrealUnitOfWorkFactoryAdapter::new(factory);

        let result = adapter_factory.create().await;
        assert!(result.is_ok());

        let mut uow = result.unwrap();
        assert!(uow.begin().await.is_ok());
        assert!(uow.commit().await.is_ok());
    }
}

