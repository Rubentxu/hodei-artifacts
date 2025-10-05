use crate::shared::application::ports::account_repository::AccountRepositoryError;
use crate::shared::application::ports::ou_repository::OuRepositoryError;
use crate::shared::application::ports::scp_repository::ScpRepositoryError;
use crate::shared::domain::{Account, OrganizationalUnit, ServiceControlPolicy};
use kernel::Hrn;

/// Port for retrieving service control policies
#[async_trait::async_trait]
pub trait ScpRepositoryPort: Send + Sync {
    /// Find an SCP by HRN
    async fn find_scp_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError>;
}

/// Port for retrieving accounts
#[async_trait::async_trait]
pub trait AccountRepositoryPort: Send + Sync {
    /// Find an account by HRN
    async fn find_account_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<Account>, AccountRepositoryError>;
}

/// Port for retrieving organizational units
#[async_trait::async_trait]
pub trait OuRepositoryPort: Send + Sync {
    /// Find an OU by HRN
    async fn find_ou_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<OrganizationalUnit>, OuRepositoryError>;
}
