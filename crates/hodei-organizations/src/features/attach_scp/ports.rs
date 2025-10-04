use crate::shared::domain::scp::ServiceControlPolicy;
use crate::shared::domain::account::Account;
use crate::shared::domain::ou::OrganizationalUnit;
use crate::shared::application::ports::scp_repository::ScpRepositoryError;
use crate::shared::application::ports::account_repository::AccountRepositoryError;
use crate::shared::application::ports::ou_repository::OuRepositoryError;
use policies::domain::Hrn;

/// Port for retrieving service control policies
#[async_trait::async_trait]
pub trait ScpRepositoryPort: Send + Sync {
    /// Find an SCP by HRN
    async fn find_scp_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError>;
}

/// Port for retrieving and updating accounts
#[async_trait::async_trait]
pub trait AccountRepositoryPort: Send + Sync {
    /// Find an account by HRN
    async fn find_account_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, AccountRepositoryError>;
    
    /// Save an account
    async fn save_account(&self, account: Account) -> Result<(), AccountRepositoryError>;
}

/// Port for retrieving and updating organizational units
#[async_trait::async_trait]
pub trait OuRepositoryPort: Send + Sync {
    /// Find an OU by HRN
    async fn find_ou_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, OuRepositoryError>;
    
    /// Save an OU
    async fn save_ou(&self, ou: OrganizationalUnit) -> Result<(), OuRepositoryError>;
}
