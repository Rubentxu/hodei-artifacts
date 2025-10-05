use crate::features::get_effective_scps::ports::{
    AccountRepositoryPort, OuRepositoryPort, ScpRepositoryPort,
};
use crate::shared::application::ports::account_repository::{
    AccountRepository, AccountRepositoryError,
};
use crate::shared::application::ports::ou_repository::{OuRepository, OuRepositoryError};
use crate::shared::application::ports::scp_repository::{ScpRepository, ScpRepositoryError};
use crate::shared::domain::{Account, OrganizationalUnit, ServiceControlPolicy};
use async_trait::async_trait;
use kernel::Hrn;

/// Adapter that implements the ScpRepositoryPort trait using the ScpRepository
pub struct ScpRepositoryAdapter<SR: ScpRepository> {
    repository: SR,
}

impl<SR: ScpRepository> ScpRepositoryAdapter<SR> {
    /// Create a new adapter instance
    pub fn new(repository: SR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<SR: ScpRepository> ScpRepositoryPort for ScpRepositoryAdapter<SR> {
    /// Find an SCP by HRN
    async fn find_scp_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError> {
        self.repository.find_by_hrn(hrn).await
    }
}

/// Adapter that implements the AccountRepositoryPort trait using the AccountRepository
pub struct AccountRepositoryAdapter<AR: AccountRepository + Send + Sync> {
    repository: AR,
}

impl<AR: AccountRepository + Send + Sync> AccountRepositoryAdapter<AR> {
    /// Create a new adapter instance
    pub fn new(repository: AR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<AR: AccountRepository + Send + Sync> AccountRepositoryPort for AccountRepositoryAdapter<AR> {
    /// Find an account by HRN
    async fn find_account_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<Account>, AccountRepositoryError> {
        self.repository.find_by_hrn(hrn).await
    }
}

/// Adapter that implements the OuRepositoryPort trait using the OuRepository
pub struct OuRepositoryAdapter<OR: OuRepository + Send + Sync> {
    repository: OR,
}

impl<OR: OuRepository + Send + Sync> OuRepositoryAdapter<OR> {
    /// Create a new adapter instance
    pub fn new(repository: OR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<OR: OuRepository + Send + Sync> OuRepositoryPort for OuRepositoryAdapter<OR> {
    /// Find an OU by HRN
    async fn find_ou_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<OrganizationalUnit>, OuRepositoryError> {
        self.repository.find_by_hrn(hrn).await
    }
}
