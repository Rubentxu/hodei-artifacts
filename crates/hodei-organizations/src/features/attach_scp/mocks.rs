use crate::features::attach_scp::ports::{
    AccountRepositoryPort, OuRepositoryPort, ScpRepositoryPort,
};
use crate::internal::application::ports::account_repository::AccountRepositoryError;
use crate::internal::application::ports::ou_repository::OuRepositoryError;
use crate::internal::application::ports::scp_repository::ScpRepositoryError;
use crate::internal::domain::account::Account;
use crate::internal::domain::ou::OrganizationalUnit;
use crate::internal::domain::scp::ServiceControlPolicy;
use kernel::Hrn;

use async_trait::async_trait;
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

    pub fn update_account(&self, account: Account) {
        let hrn_string = account.hrn.to_string();
        self.accounts.write().unwrap().insert(hrn_string, account);
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

    async fn save_account(&self, account: Account) -> Result<(), AccountRepositoryError> {
        let mut accounts = self.accounts.write().unwrap();
        accounts.insert(account.hrn.to_string(), account);
        Ok(())
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

    pub fn update_ou(&self, ou: OrganizationalUnit) {
        let hrn_string = ou.hrn.to_string();
        self.ous.write().unwrap().insert(hrn_string, ou);
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

    async fn save_ou(&self, ou: OrganizationalUnit) -> Result<(), OuRepositoryError> {
        let mut ous = self.ous.write().unwrap();
        ous.insert(ou.hrn.to_string(), ou);
        Ok(())
    }
}
