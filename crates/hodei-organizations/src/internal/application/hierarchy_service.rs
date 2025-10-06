use crate::internal::application::ports::{
    AccountRepository, AccountRepositoryError, OuRepository, OuRepositoryError,
};
use crate::internal::domain::{Account, OrganizationalUnit};
use kernel::Hrn;
use std::sync::Arc;

/// Servicio para manejar la jerarquía organizacional
pub struct HierarchyService<AR: AccountRepository, OR: OuRepository> {
    account_repo: Arc<AR>,
    ou_repo: Arc<OR>,
}

impl<AR: AccountRepository, OR: OuRepository> HierarchyService<AR, OR> {
    /// Crea una nueva instancia del servicio
    pub fn new(account_repo: Arc<AR>, ou_repo: Arc<OR>) -> Self {
        Self {
            account_repo,
            ou_repo,
        }
    }

    /// Obtiene la cadena completa de OUs desde una cuenta hasta la raíz
    pub async fn get_parent_chain(
        &self,
        account_hrn: &Hrn,
    ) -> Result<Vec<OrganizationalUnit>, HierarchyError> {
        let mut chain = Vec::new();
        let mut current_hrn = account_hrn.clone();

        // Comenzar desde la cuenta
        let account = self
            .account_repo
            .find_account_by_hrn(&current_hrn)
            .await?
            .ok_or(HierarchyError::AccountNotFound(current_hrn.clone()))?;

        // Ascender por la jerarquía
        current_hrn = account.parent_hrn.clone();
        while let Some(ou) = self.ou_repo.find_ou_by_hrn(&current_hrn).await? {
            chain.push(ou.clone());
            current_hrn = ou.parent_hrn.clone();
        }

        Ok(chain)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HierarchyError {
    #[error("Account not found: {0}")]
    AccountNotFound(Hrn),
    #[error("OU repository error: {0}")]
    OuRepository(#[from] OuRepositoryError),
    #[error("Account repository error: {0}")]
    AccountRepository(#[from] AccountRepositoryError),
}
