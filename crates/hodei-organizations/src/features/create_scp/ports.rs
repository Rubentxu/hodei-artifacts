use crate::features::create_scp::error::CreateScpError;
use crate::shared::application::ports::scp_repository::ScpRepository;
use crate::shared::domain::scp::ServiceControlPolicy;
use async_trait::async_trait;
use std::sync::Arc;

/// Deprecated: Use CreateScpUnitOfWorkFactory instead
/// This trait is kept for backwards compatibility but will be removed in future versions
#[async_trait]
pub trait ScpPersister {
    async fn save(&self, scp: ServiceControlPolicy) -> Result<(), CreateScpError>;
}

/// Unit of Work trait for CreateScp feature
///
/// This trait provides transactional boundaries for service control policy creation operations.
/// It ensures that all operations within a transaction are atomic.
#[async_trait]
pub trait CreateScpUnitOfWork: Send + Sync {
    /// Begin a new transaction
    async fn begin(&mut self) -> Result<(), CreateScpError>;

    /// Commit the current transaction
    async fn commit(&mut self) -> Result<(), CreateScpError>;

    /// Rollback the current transaction
    async fn rollback(&mut self) -> Result<(), CreateScpError>;

    /// Get service control policy repository for this transaction
    fn scps(&self) -> Arc<dyn ScpRepository>;
}

/// Factory for creating CreateScpUnitOfWork instances
///
/// This allows dependency injection of UnitOfWork creation while keeping the
/// business logic decoupled from the specific implementation.
#[async_trait]
pub trait CreateScpUnitOfWorkFactory: Send + Sync {
    /// Type of UnitOfWork this factory creates
    type UnitOfWork: CreateScpUnitOfWork;

    /// Create a new UnitOfWork instance
    async fn create(&self) -> Result<Self::UnitOfWork, CreateScpError>;
}
