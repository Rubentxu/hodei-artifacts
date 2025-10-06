use crate::features::attach_scp::ports::{
    AccountRepositoryPort, OuRepositoryPort, ScpRepositoryPort,
};
use crate::internal::application::ports::account_repository::{
    AccountRepository, AccountRepositoryError,
};
use crate::internal::application::ports::ou_repository::{OuRepository, OuRepositoryError};
use crate::internal::application::ports::scp_repository::{ScpRepository, ScpRepositoryError};
use crate::internal::domain::account::Account;
use crate::internal::domain::ou::OrganizationalUnit;
use crate::internal::domain::scp::ServiceControlPolicy;
use async_trait::async_trait;
use kernel::Hrn;

/// Adapter that implements the ScpRepositoryPort trait using the ScpRepository
pub struct ScpRepositoryAdapter<SR: ScpRepository + std::marker::Send> {
    repository: SR,
}

impl<SR: ScpRepository + std::marker::Send> ScpRepositoryAdapter<SR> {
    /// Create a new adapter instance
    pub fn new(repository: SR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<SR: ScpRepository + std::marker::Sync + std::marker::Send> ScpRepositoryPort
    for ScpRepositoryAdapter<SR>
{
    /// Find an SCP by HRN
    async fn find_scp_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError> {
        self.repository.find_by_hrn(hrn).await
    }
}

/// Adapter that implements the AccountRepositoryPort trait using the AccountRepository
pub struct AccountRepositoryAdapter<AR: AccountRepository + std::marker::Send> {
    repository: AR,
}

impl<AR: AccountRepository + std::marker::Send> AccountRepositoryAdapter<AR> {
    /// Create a new adapter instance
    pub fn new(repository: AR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<AR: AccountRepository + std::marker::Sync + std::marker::Send> AccountRepositoryPort
    for AccountRepositoryAdapter<AR>
{
    /// Find an account by HRN
    async fn find_account_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<Account>, AccountRepositoryError> {
        self.repository.find_by_hrn(hrn).await
    }

    /// Save an account
    async fn save_account(&self, account: Account) -> Result<(), AccountRepositoryError> {
        self.repository.save(&account).await
    }
}

/// Adapter that implements the OuRepositoryPort trait using the OuRepository
pub struct OuRepositoryAdapter<OR: OuRepository + std::marker::Send> {
    repository: OR,
}

impl<OR: OuRepository + std::marker::Send> OuRepositoryAdapter<OR> {
    /// Create a new adapter instance
    pub fn new(repository: OR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<OR: OuRepository + std::marker::Sync + std::marker::Send> OuRepositoryPort
    for OuRepositoryAdapter<OR>
{
    /// Find an OU by HRN
    async fn find_ou_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<OrganizationalUnit>, OuRepositoryError> {
        self.repository.find_by_hrn(hrn).await
    }

    /// Save an OU
    async fn save_ou(&self, ou: OrganizationalUnit) -> Result<(), OuRepositoryError> {
        self.repository.save(&ou).await
    }
}
