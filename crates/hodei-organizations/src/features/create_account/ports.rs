use crate::features::create_account::error::CreateAccountError;
use crate::internal::application::ports::account_repository::AccountRepository;
use crate::internal::domain::account::Account;
use async_trait::async_trait;
use std::sync::Arc;

/// Deprecated: Use CreateAccountUnitOfWorkFactory instead
/// This trait is kept for backwards compatibility but will be removed in future versions
#[async_trait]
pub trait AccountPersister {
    async fn save(&self, account: Account) -> Result<(), CreateAccountError>;
}

/// Unit of Work trait for CreateAccount feature
///
/// This trait provides transactional boundaries for account creation operations.
/// It ensures that all operations within a transaction are atomic.
#[async_trait]
pub trait CreateAccountUnitOfWork: Send + Sync {
    /// Begin a new transaction
    async fn begin(&mut self) -> Result<(), CreateAccountError>;

    /// Commit the current transaction
    async fn commit(&mut self) -> Result<(), CreateAccountError>;

    /// Rollback the current transaction
    async fn rollback(&mut self) -> Result<(), CreateAccountError>;

    /// Get account repository for this transaction
    fn accounts(&self) -> Arc<dyn AccountRepository>;
}

/// Factory for creating CreateAccountUnitOfWork instances
///
/// This allows dependency injection of UnitOfWork creation while keeping the
/// business logic decoupled from the specific implementation.
#[async_trait]
pub trait CreateAccountUnitOfWorkFactory: Send + Sync {
    /// Type of UnitOfWork this factory creates
    type UnitOfWork: CreateAccountUnitOfWork;

    /// Create a new UnitOfWork instance
    async fn create(&self) -> Result<Self::UnitOfWork, CreateAccountError>;
}
