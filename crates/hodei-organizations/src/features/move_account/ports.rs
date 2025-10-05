use crate::features::move_account::error::MoveAccountError;
use crate::shared::domain::account::Account;
use crate::shared::domain::ou::OrganizationalUnit;
use async_trait::async_trait;
use kernel::Hrn;
use std::sync::Arc;

/// Simplified UnitOfWork trait for MoveAccountUseCase
///
/// This trait provides direct access to the generic UnitOfWork interface
/// to avoid complex adapter patterns.
#[async_trait]
pub trait MoveAccountUnitOfWork: Send + Sync {
    /// Begin a new transaction
    async fn begin(&mut self) -> Result<(), MoveAccountError>;

    /// Commit the current transaction
    async fn commit(&mut self) -> Result<(), MoveAccountError>;

    /// Rollback the current transaction
    async fn rollback(&mut self) -> Result<(), MoveAccountError>;

    /// Get account repository for this transaction
    fn accounts(
        &self,
    ) -> Arc<dyn crate::shared::application::ports::account_repository::AccountRepository>;

    /// Get OU repository for this transaction
    fn ous(&self) -> Arc<dyn crate::shared::application::ports::ou_repository::OuRepository>;
}

/// Simplified UnitOfWorkFactory trait for MoveAccountUseCase
#[async_trait]
pub trait MoveAccountUnitOfWorkFactory: Send + Sync {
    /// Type of UnitOfWork this factory creates
    type UnitOfWork: MoveAccountUnitOfWork;

    /// Create a new UnitOfWork instance
    async fn create(&self) -> Result<Self::UnitOfWork, MoveAccountError>;
}

// Legacy traits for backward compatibility during migration
#[async_trait]
pub trait AccountRepository {
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, MoveAccountError>;
    async fn save(&self, account: Account) -> Result<(), MoveAccountError>;
}

#[async_trait]
pub trait OuRepository {
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, MoveAccountError>;
    async fn save(&self, ou: OrganizationalUnit) -> Result<(), MoveAccountError>;
}
