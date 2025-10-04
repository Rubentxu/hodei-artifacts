use crate::features::create_scp::error::CreateScpError;
use crate::features::create_scp::ports::{
    CreateScpUnitOfWork, CreateScpUnitOfWorkFactory, ScpPersister,
};
use crate::shared::application::ports::scp_repository::ScpRepository;
use crate::shared::domain::scp::ServiceControlPolicy;
use async_trait::async_trait;
use std::sync::Arc;

/// Deprecated: Use CreateScpSurrealUnitOfWorkAdapter instead
/// This adapter is kept for backwards compatibility but will be removed in future versions
pub struct ScpRepositoryAdapter<SR: ScpRepository> {
    repository: SR,
}

impl<SR: ScpRepository> ScpRepositoryAdapter<SR> {
    pub fn new(repository: SR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<SR: ScpRepository> ScpPersister for ScpRepositoryAdapter<SR> {
    async fn save(&self, scp: ServiceControlPolicy) -> Result<(), CreateScpError> {
        self.repository
            .save(&scp)
            .await
            .map_err(CreateScpError::ScpRepositoryError)
    }
}

/// SurrealDB implementation of CreateScpUnitOfWork
///
/// This adapter wraps the generic SurrealUnitOfWork and exposes only the
/// operations needed for the create_scp feature.
pub struct CreateScpSurrealUnitOfWorkAdapter {
    inner_uow: crate::shared::infrastructure::surreal::SurrealUnitOfWork,
}

impl CreateScpSurrealUnitOfWorkAdapter {
    pub fn new(uow: crate::shared::infrastructure::surreal::SurrealUnitOfWork) -> Self {
        Self { inner_uow: uow }
    }
}

#[async_trait]
impl CreateScpUnitOfWork for CreateScpSurrealUnitOfWorkAdapter {
    async fn begin(&mut self) -> Result<(), CreateScpError> {
        use shared::application::ports::unit_of_work::UnitOfWork;
        self.inner_uow
            .begin()
            .await
            .map_err(|e| CreateScpError::TransactionError(e.to_string()))
    }

    async fn commit(&mut self) -> Result<(), CreateScpError> {
        use shared::application::ports::unit_of_work::UnitOfWork;
        self.inner_uow
            .commit()
            .await
            .map_err(|e| CreateScpError::TransactionError(e.to_string()))
    }

    async fn rollback(&mut self) -> Result<(), CreateScpError> {
        use shared::application::ports::unit_of_work::UnitOfWork;
        self.inner_uow
            .rollback()
            .await
            .map_err(|e| CreateScpError::TransactionError(e.to_string()))
    }

    fn scps(&self) -> Arc<dyn ScpRepository> {
        use shared::application::ports::unit_of_work::UnitOfWork;
        self.inner_uow.scps()
    }
}

/// Factory for creating CreateScpSurrealUnitOfWork instances
pub struct CreateScpSurrealUnitOfWorkFactoryAdapter {
    inner_factory: Arc<crate::shared::infrastructure::surreal::SurrealUnitOfWorkFactory>,
}

impl CreateScpSurrealUnitOfWorkFactoryAdapter {
    pub fn new(
        factory: Arc<crate::shared::infrastructure::surreal::SurrealUnitOfWorkFactory>,
    ) -> Self {
        Self {
            inner_factory: factory,
        }
    }
}

#[async_trait]
impl CreateScpUnitOfWorkFactory for CreateScpSurrealUnitOfWorkFactoryAdapter {
    type UnitOfWork = CreateScpSurrealUnitOfWorkAdapter;

    async fn create(&self) -> Result<Self::UnitOfWork, CreateScpError> {
        use shared::application::ports::unit_of_work::UnitOfWorkFactory;
        let uow = self
            .inner_factory
            .create()
            .await
            .map_err(|e| CreateScpError::TransactionError(e.to_string()))?;
        Ok(CreateScpSurrealUnitOfWorkAdapter::new(uow))
    }
}
