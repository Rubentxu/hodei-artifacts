use std::sync::Arc;
use async_trait::async_trait;
use policies::domain::Hrn;

use crate::features::move_account::ports::{
    MoveAccountUnitOfWorkFactory, MoveAccountUnitOfWork
};
use crate::features::move_account::error::MoveAccountError;
use crate::shared::domain::account::Account;
use crate::shared::domain::ou::OrganizationalUnit;
use crate::shared::application::ports::account_repository::AccountRepository;
use crate::shared::application::ports::ou_repository::OuRepository;

/// Mock UnitOfWork for testing transactional behavior
pub struct MockMoveAccountUnitOfWork {
    pub should_fail_on_save: bool,
    pub save_calls: Arc<std::sync::Mutex<Vec<String>>>,
    pub transaction_active: bool,
}

impl Default for MockMoveAccountUnitOfWork {
    fn default() -> Self {
        Self::new()
    }
}

impl MockMoveAccountUnitOfWork {
    pub fn new() -> Self {
        Self {
            should_fail_on_save: false,
            save_calls: Arc::new(std::sync::Mutex::new(Vec::new())),
            transaction_active: false,
        }
    }
    
    pub fn with_failure(should_fail: bool) -> Self {
        Self {
            should_fail_on_save: should_fail,
            save_calls: Arc::new(std::sync::Mutex::new(Vec::new())),
            transaction_active: false,
        }
    }
}

#[async_trait]
impl MoveAccountUnitOfWork for MockMoveAccountUnitOfWork {
    async fn begin(&mut self) -> Result<(), MoveAccountError> {
        self.transaction_active = true;
        Ok(())
    }

    async fn commit(&mut self) -> Result<(), MoveAccountError> {
        if !self.transaction_active {
            return Err(MoveAccountError::OuRepositoryError(
                crate::shared::application::ports::ou_repository::OuRepositoryError::DatabaseError("No transaction in progress".to_string())
            ));
        }
        self.transaction_active = false;
        Ok(())
    }

    async fn rollback(&mut self) -> Result<(), MoveAccountError> {
        if !self.transaction_active {
            return Err(MoveAccountError::OuRepositoryError(
                crate::shared::application::ports::ou_repository::OuRepositoryError::DatabaseError("No transaction in progress".to_string())
            ));
        }
        self.transaction_active = false;
        Ok(())
    }

    fn accounts(&self) -> Arc<dyn AccountRepository> {
        Arc::new(MockAccountRepository {
            should_fail_on_save: self.should_fail_on_save,
            save_calls: self.save_calls.clone(),
        })
    }

    fn ous(&self) -> Arc<dyn OuRepository> {
        Arc::new(MockOuRepository {
            should_fail_on_save: self.should_fail_on_save,
            save_calls: self.save_calls.clone(),
        })
    }
}

/// Mock AccountRepository for testing
pub struct MockAccountRepository {
    pub should_fail_on_save: bool,
    pub save_calls: Arc<std::sync::Mutex<Vec<String>>>,
}

#[async_trait]
impl AccountRepository for MockAccountRepository {
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, crate::shared::application::ports::account_repository::AccountRepositoryError> {
        // Return a mock account for testing
        if hrn.to_string() == "account:test" {
            let account_hrn = Hrn::new("account".to_string(), "test".to_string(), "source".to_string(), "ou".to_string(), "hodei".to_string());
            Ok(Some(Account::new(
                account_hrn.clone(),
                "Test Account".to_string(),
                account_hrn
            )))
        } else {
            Ok(None)
        }
    }

    async fn save(&self, account: &Account) -> Result<(), crate::shared::application::ports::account_repository::AccountRepositoryError> {
        let mut calls = self.save_calls.lock().unwrap();
        calls.push(format!("account:{}", account.hrn));
        
        if self.should_fail_on_save {
            Err(crate::shared::application::ports::account_repository::AccountRepositoryError::DatabaseError("Mock save failure".to_string()))
        } else {
            Ok(())
        }
    }
}

/// Mock OuRepository for testing
pub struct MockOuRepository {
    pub should_fail_on_save: bool,
    pub save_calls: Arc<std::sync::Mutex<Vec<String>>>,
}

#[async_trait]
impl OuRepository for MockOuRepository {
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, crate::shared::application::ports::ou_repository::OuRepositoryError> {
        // Return mock OUs for testing
        match hrn.to_string().as_str() {
            "ou:source" => {
                let mut child_accounts = std::collections::HashSet::new();
                child_accounts.insert(Hrn::new("account".to_string(), "test".to_string(), "source".to_string(), "ou".to_string(), "hodei".to_string()));
                
                Ok(Some(OrganizationalUnit {
                    hrn: hrn.clone(),
                    parent_hrn: Hrn::new("ou".to_string(), "root".to_string(), "source".to_string(), "hodei".to_string(), "hodei".to_string()),
                    name: "Source OU".to_string(),
                    child_ous: std::collections::HashSet::new(),
                    child_accounts,
                    attached_scps: std::collections::HashSet::new(),
                }))
            },
            "ou:target" => Ok(Some(OrganizationalUnit {
                hrn: hrn.clone(),
                parent_hrn: Hrn::new("ou".to_string(), "root".to_string(), "target".to_string(), "hodei".to_string(), "hodei".to_string()),
                name: "Target OU".to_string(),
                child_ous: std::collections::HashSet::new(),
                child_accounts: std::collections::HashSet::new(),
                attached_scps: std::collections::HashSet::new(),
            })),
            _ => Ok(None),
        }
    }

    async fn save(&self, ou: &OrganizationalUnit) -> Result<(), crate::shared::application::ports::ou_repository::OuRepositoryError> {
        let mut calls = self.save_calls.lock().unwrap();
        calls.push(format!("ou:{}", ou.hrn));
        
        if self.should_fail_on_save {
            Err(crate::shared::application::ports::ou_repository::OuRepositoryError::DatabaseError("Mock save failure".to_string()))
        } else {
            Ok(())
        }
    }
}

/// Mock UnitOfWorkFactory for testing
pub struct MockMoveAccountUnitOfWorkFactory {
    pub should_fail_on_save: bool,
}

impl Default for MockMoveAccountUnitOfWorkFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl MockMoveAccountUnitOfWorkFactory {
    pub fn new() -> Self {
        Self {
            should_fail_on_save: false,
        }
    }
    
    pub fn with_failure(should_fail: bool) -> Self {
        Self {
            should_fail_on_save: should_fail,
        }
    }
}

#[async_trait]
impl MoveAccountUnitOfWorkFactory for MockMoveAccountUnitOfWorkFactory {
    type UnitOfWork = MockMoveAccountUnitOfWork;

    async fn create(&self) -> Result<Self::UnitOfWork, MoveAccountError> {
        Ok(MockMoveAccountUnitOfWork::with_failure(self.should_fail_on_save))
    }
}
