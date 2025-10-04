use crate::features::create_ou::error::CreateOuError;
use crate::shared::application::ports::ou_repository::OuRepository;
use crate::shared::domain::ou::OrganizationalUnit;
use async_trait::async_trait;
use std::sync::Arc;

/// Deprecated: Use CreateOuUnitOfWorkFactory instead
/// This trait is kept for backwards compatibility but will be removed in future versions
#[async_trait]
pub trait OuPersister {
    async fn save(&self, ou: OrganizationalUnit) -> Result<(), CreateOuError>;
}

/// Unit of Work trait for CreateOu feature
///
/// This trait provides transactional boundaries for organizational unit creation operations.
/// It ensures that all operations within a transaction are atomic.
#[async_trait]
pub trait CreateOuUnitOfWork: Send + Sync {
    /// Begin a new transaction
    async fn begin(&mut self) -> Result<(), CreateOuError>;

    /// Commit the current transaction
    async fn commit(&mut self) -> Result<(), CreateOuError>;

    /// Rollback the current transaction
    async fn rollback(&mut self) -> Result<(), CreateOuError>;

    /// Get organizational unit repository for this transaction
    fn ous(&self) -> Arc<dyn OuRepository>;
}

/// Factory for creating CreateOuUnitOfWork instances
///
/// This allows dependency injection of UnitOfWork creation while keeping the
/// business logic decoupled from the specific implementation.
#[async_trait]
pub trait CreateOuUnitOfWorkFactory: Send + Sync {
    /// Type of UnitOfWork this factory creates
    type UnitOfWork: CreateOuUnitOfWork;

    /// Create a new UnitOfWork instance
    async fn create(&self) -> Result<Self::UnitOfWork, CreateOuError>;
}
