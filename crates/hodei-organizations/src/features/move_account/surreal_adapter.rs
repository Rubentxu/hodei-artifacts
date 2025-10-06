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
pub struct MoveAccountSurrealUnitOfWorkAdapter {
    inner: SurrealUnitOfWork,
}

impl MoveAccountSurrealUnitOfWorkAdapter {
    pub fn new(inner: SurrealUnitOfWork) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl MoveAccountUnitOfWork for MoveAccountSurrealUnitOfWorkAdapter {
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
pub struct MoveAccountSurrealUnitOfWorkFactoryAdapter {
    inner: Arc<SurrealUnitOfWorkFactory>,
}

impl MoveAccountSurrealUnitOfWorkFactoryAdapter {
    pub fn new(inner: Arc<SurrealUnitOfWorkFactory>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl MoveAccountUnitOfWorkFactory for MoveAccountSurrealUnitOfWorkFactoryAdapter {
    type UnitOfWork = MoveAccountSurrealUnitOfWorkAdapter;

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
    use surrealdb::{Surreal, engine::local::Mem};

    #[tokio::test]
    async fn test_move_account_adapter_lifecycle() {
        let db = Surreal::new::<Mem>(()).await.unwrap();
        db.use_ns("test").use_db("test").await.unwrap();

        let factory = Arc::new(SurrealUnitOfWorkFactory::new(Arc::new(db)));
        let adapter_factory = MoveAccountSurrealUnitOfWorkFactoryAdapter::new(factory);

        let mut uow = adapter_factory.create().await.unwrap();
        assert!(uow.begin().await.is_ok());

        // Verify repositories are available
        let _accounts = uow.accounts();
        let _ous = uow.ous();

        assert!(uow.commit().await.is_ok());
    }
}

