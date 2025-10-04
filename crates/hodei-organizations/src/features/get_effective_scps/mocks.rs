use crate::features::get_effective_scps::ports::{
    AccountRepositoryPort, OuRepositoryPort, ScpRepositoryPort,
};
use crate::shared::application::ports::account_repository::AccountRepositoryError;
use crate::shared::application::ports::ou_repository::OuRepositoryError;
use crate::shared::application::ports::scp_repository::ScpRepositoryError;
use crate::shared::domain::{Account, OrganizationalUnit, ServiceControlPolicy};
use async_trait::async_trait;
use policies::shared::domain::hrn::Hrn;
use std::collections::HashMap;
use std::sync::RwLock;

/// Mock implementation of ScpRepositoryPort for testing
#[derive(Debug, Default)]
pub struct MockScpRepositoryPort {
    scps: RwLock<HashMap<String, ServiceControlPolicy>>,
}

impl MockScpRepositoryPort {
    pub fn new() -> Self {
        Self {
            scps: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_scp(self, scp: ServiceControlPolicy) -> Self {
        let hrn_string = scp.hrn.to_string();
        self.scps.write().unwrap().insert(hrn_string, scp);
        self
    }
}

#[async_trait]
impl ScpRepositoryPort for MockScpRepositoryPort {
    async fn find_scp_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError> {
        let scps = self.scps.read().unwrap();
        Ok(scps.get(&hrn.to_string()).cloned())
    }
}

/// Mock implementation of AccountRepositoryPort for testing
#[derive(Debug, Default)]
pub struct MockAccountRepositoryPort {
    accounts: RwLock<HashMap<String, Account>>,
}

impl MockAccountRepositoryPort {
    pub fn new() -> Self {
        Self {
            accounts: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_account(self, account: Account) -> Self {
        let hrn_string = account.hrn.to_string();
        self.accounts.write().unwrap().insert(hrn_string, account);
        self
    }
}

#[async_trait]
impl AccountRepositoryPort for MockAccountRepositoryPort {
    async fn find_account_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<Account>, AccountRepositoryError> {
        let accounts = self.accounts.read().unwrap();
        Ok(accounts.get(&hrn.to_string()).cloned())
    }
}

/// Mock implementation of OuRepositoryPort for testing
#[derive(Debug, Default)]
pub struct MockOuRepositoryPort {
    ous: RwLock<HashMap<String, OrganizationalUnit>>,
}

impl MockOuRepositoryPort {
    pub fn new() -> Self {
        Self {
            ous: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_ou(self, ou: OrganizationalUnit) -> Self {
        let hrn_string = ou.hrn.to_string();
        self.ous.write().unwrap().insert(hrn_string, ou);
        self
    }
}

#[async_trait]
impl OuRepositoryPort for MockOuRepositoryPort {
    async fn find_ou_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<OrganizationalUnit>, OuRepositoryError> {
        let ous = self.ous.read().unwrap();
        Ok(ous.get(&hrn.to_string()).cloned())
    }
}
