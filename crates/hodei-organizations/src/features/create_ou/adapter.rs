use crate::features::create_ou::error::CreateOuError;
use crate::features::create_ou::ports::{
    CreateOuUnitOfWork, CreateOuUnitOfWorkFactory, OuPersister,
};
use crate::shared::application::ports::ou_repository::OuRepository;
use crate::shared::domain::ou::OrganizationalUnit;
use async_trait::async_trait;
use std::sync::Arc;

/// Deprecated: Use CreateOuSurrealUnitOfWorkAdapter instead
/// This adapter is kept for backwards compatibility but will be removed in future versions
pub struct OuPersisterAdapter<OR: OuRepository> {
    repository: Arc<OR>,
}

impl<OR: OuRepository> OuPersisterAdapter<OR> {
    pub fn new(repository: Arc<OR>) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<OR: OuRepository> OuPersister for OuPersisterAdapter<OR> {
    async fn save(&self, ou: OrganizationalUnit) -> Result<(), CreateOuError> {
        self.repository.save(&ou).await?;
        Ok(())
    }
}

/// SurrealDB implementation of CreateOuUnitOfWork
///
/// This adapter wraps the generic SurrealUnitOfWork and exposes only the
/// operations needed for the create_ou feature.
pub struct CreateOuSurrealUnitOfWorkAdapter {
    inner_uow: crate::shared::infrastructure::surreal::SurrealUnitOfWork,
}

impl CreateOuSurrealUnitOfWorkAdapter {
    pub fn new(uow: crate::shared::infrastructure::surreal::SurrealUnitOfWork) -> Self {
        Self { inner_uow: uow }
    }
}

#[async_trait]
impl CreateOuUnitOfWork for CreateOuSurrealUnitOfWorkAdapter {
    async fn begin(&mut self) -> Result<(), CreateOuError> {
        use shared::application::ports::unit_of_work::UnitOfWork;
        self.inner_uow
            .begin()
            .await
            .map_err(|e| CreateOuError::TransactionError(e.to_string()))
    }

    async fn commit(&mut self) -> Result<(), CreateOuError> {
        use shared::application::ports::unit_of_work::UnitOfWork;
        self.inner_uow
            .commit()
            .await
            .map_err(|e| CreateOuError::TransactionError(e.to_string()))
    }

    async fn rollback(&mut self) -> Result<(), CreateOuError> {
        use shared::application::ports::unit_of_work::UnitOfWork;
        self.inner_uow
            .rollback()
            .await
            .map_err(|e| CreateOuError::TransactionError(e.to_string()))
    }

    fn ous(&self) -> Arc<dyn OuRepository> {
        use shared::application::ports::unit_of_work::UnitOfWork;
        self.inner_uow.ous()
    }
}

/// Factory for creating CreateOuSurrealUnitOfWork instances
pub struct CreateOuSurrealUnitOfWorkFactoryAdapter {
    inner_factory: Arc<crate::shared::infrastructure::surreal::SurrealUnitOfWorkFactory>,
}

impl CreateOuSurrealUnitOfWorkFactoryAdapter {
    pub fn new(
        factory: Arc<crate::shared::infrastructure::surreal::SurrealUnitOfWorkFactory>,
    ) -> Self {
        Self {
            inner_factory: factory,
        }
    }
}

#[async_trait]
impl CreateOuUnitOfWorkFactory for CreateOuSurrealUnitOfWorkFactoryAdapter {
    type UnitOfWork = CreateOuSurrealUnitOfWorkAdapter;

    async fn create(&self) -> Result<Self::UnitOfWork, CreateOuError> {
        use shared::application::ports::unit_of_work::UnitOfWorkFactory;
        let uow = self
            .inner_factory
            .create()
            .await
            .map_err(|e| CreateOuError::TransactionError(e.to_string()))?;
        Ok(CreateOuSurrealUnitOfWorkAdapter::new(uow))
    }
}
