use crate::features::get_effective_scps::adapter::{
    AccountRepositoryAdapter, OuRepositoryAdapter, ScpRepositoryAdapter,
};
use crate::features::get_effective_scps::use_case::GetEffectiveScpsUseCase;
use crate::shared::application::ports::account_repository::AccountRepository;
use crate::shared::application::ports::ou_repository::OuRepository;
use crate::shared::application::ports::scp_repository::ScpRepository;

/// Adaptador combinado que expone tanto cuentas como OUs
pub struct OrgRepositoryAdapter<AR, OR>
where
    AR: AccountRepository + Send + Sync,
    OR: OuRepository + Send + Sync,
{
    account_adapter: AccountRepositoryAdapter<AR>,
    ou_adapter: OuRepositoryAdapter<OR>,
}

impl<AR, OR> OrgRepositoryAdapter<AR, OR>
where
    AR: AccountRepository + Send + Sync,
    OR: OuRepository + Send + Sync,
{
    pub fn new(account_repo: AR, ou_repo: OR) -> Self {
        Self {
            account_adapter: AccountRepositoryAdapter::new(account_repo),
            ou_adapter: OuRepositoryAdapter::new(ou_repo),
        }
    }
}

#[async_trait::async_trait]
impl<AR, OR> crate::features::get_effective_scps::ports::AccountRepositoryPort
    for OrgRepositoryAdapter<AR, OR>
where
    AR: AccountRepository + Send + Sync,
    OR: OuRepository + Send + Sync,
{
    async fn find_account_by_hrn(
        &self,
        hrn: &kernel::Hrn,
    ) -> Result<
        Option<crate::shared::domain::Account>,
        crate::shared::application::ports::account_repository::AccountRepositoryError,
    > {
        self.account_adapter.find_account_by_hrn(hrn).await
    }
}

#[async_trait::async_trait]
impl<AR, OR> crate::features::get_effective_scps::ports::OuRepositoryPort
    for OrgRepositoryAdapter<AR, OR>
where
    AR: AccountRepository + Send + Sync,
    OR: OuRepository + Send + Sync,
{
    async fn find_ou_by_hrn(
        &self,
        hrn: &kernel::Hrn,
    ) -> Result<
        Option<crate::shared::domain::OrganizationalUnit>,
        crate::shared::application::ports::ou_repository::OuRepositoryError,
    > {
        self.ou_adapter.find_ou_by_hrn(hrn).await
    }
}

/// Crea el caso de uso con repositorios concretos Surreal u otros
pub fn get_effective_scps_use_case<SR, AR, OR>(
    scp_repository: SR,
    account_repository: AR,
    ou_repository: OR,
) -> GetEffectiveScpsUseCase<ScpRepositoryAdapter<SR>, OrgRepositoryAdapter<AR, OR>>
where
    SR: ScpRepository + Send + Sync,
    AR: AccountRepository + Send + Sync,
    OR: OuRepository + Send + Sync,
{
    let scp_adapter = ScpRepositoryAdapter::new(scp_repository);
    let org_adapter = OrgRepositoryAdapter::new(account_repository, ou_repository);
    GetEffectiveScpsUseCase::new(scp_adapter, org_adapter)
}
