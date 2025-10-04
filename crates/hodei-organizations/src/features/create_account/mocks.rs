use crate::features::create_account::error::CreateAccountError;
use crate::features::create_account::ports::{
    AccountPersister, CreateAccountUnitOfWork, CreateAccountUnitOfWorkFactory,
};
use crate::shared::application::ports::account_repository::AccountRepository;
use crate::shared::domain::account::Account;
use async_trait::async_trait;
use policies::domain::Hrn;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Deprecated: Use MockCreateAccountUnitOfWork instead
pub struct MockAccountPersister {
    accounts: Arc<Mutex<HashMap<String, Account>>>,
}

impl MockAccountPersister {
    pub fn new() -> Self {
        Self {
            accounts: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl AccountPersister for MockAccountPersister {
    async fn save(&self, account: Account) -> Result<(), CreateAccountError> {
        let mut accounts = self.accounts.lock().unwrap();
        accounts.insert(account.hrn.to_string(), account);
        Ok(())
    }
}

/// Mock Account Repository for testing
pub struct MockAccountRepository {
    accounts: Arc<Mutex<HashMap<String, Account>>>,
    should_fail: bool,
}

impl MockAccountRepository {
    pub fn new() -> Self {
        Self {
            accounts: Arc::new(Mutex::new(HashMap::new())),
            should_fail: false,
        }
    }

    pub fn with_failure(should_fail: bool) -> Self {
        Self {
            accounts: Arc::new(Mutex::new(HashMap::new())),
            should_fail,
        }
    }

    pub fn get_saved_accounts(&self) -> Vec<Account> {
        self.accounts.lock().unwrap().values().cloned().collect()
    }
}

#[async_trait]
impl AccountRepository for MockAccountRepository {
    async fn save(
        &self,
        account: &Account,
    ) -> Result<(), crate::shared::application::ports::account_repository::AccountRepositoryError>
    {
        if self.should_fail {
            return Err(
                crate::shared::application::ports::account_repository::AccountRepositoryError::DatabaseError(
                    "Mock failure".to_string(),
                ),
            );
        }

        let mut accounts = self.accounts.lock().unwrap();
        accounts.insert(account.hrn.to_string(), account.clone());
        Ok(())
    }

    async fn find_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<
        Option<Account>,
        crate::shared::application::ports::account_repository::AccountRepositoryError,
    > {
        let accounts = self.accounts.lock().unwrap();
        Ok(accounts.get(&hrn.to_string()).cloned())
    }
}

/// Mock UnitOfWork for testing transactional behavior
pub struct MockCreateAccountUnitOfWork {
    pub should_fail_on_save: bool,
    pub save_calls: Arc<Mutex<Vec<String>>>,
    pub transaction_active: bool,
    account_repo: Arc<MockAccountRepository>,
}

impl Default for MockCreateAccountUnitOfWork {
    fn default() -> Self {
        Self::new()
    }
}

impl MockCreateAccountUnitOfWork {
    pub fn new() -> Self {
        Self {
            should_fail_on_save: false,
            save_calls: Arc::new(Mutex::new(Vec::new())),
            transaction_active: false,
            account_repo: Arc::new(MockAccountRepository::new()),
        }
    }

    pub fn with_failure(should_fail: bool) -> Self {
        Self {
            should_fail_on_save: should_fail,
            save_calls: Arc::new(Mutex::new(Vec::new())),
            transaction_active: false,
            account_repo: Arc::new(MockAccountRepository::with_failure(should_fail)),
        }
    }

    pub fn get_saved_accounts(&self) -> Vec<Account> {
        self.account_repo.get_saved_accounts()
    }
}

#[async_trait]
impl CreateAccountUnitOfWork for MockCreateAccountUnitOfWork {
    async fn begin(&mut self) -> Result<(), CreateAccountError> {
        self.transaction_active = true;
        Ok(())
    }

    async fn commit(&mut self) -> Result<(), CreateAccountError> {
        if !self.transaction_active {
            return Err(CreateAccountError::TransactionError(
                "No transaction in progress".to_string(),
            ));
        }
        self.transaction_active = false;
        Ok(())
    }

    async fn rollback(&mut self) -> Result<(), CreateAccountError> {
        if !self.transaction_active {
            return Err(CreateAccountError::TransactionError(
                "No transaction in progress".to_string(),
            ));
        }
        self.transaction_active = false;
        Ok(())
    }

    fn accounts(&self) -> Arc<dyn AccountRepository> {
        self.account_repo.clone()
    }
}

/// Mock UnitOfWorkFactory for testing
pub struct MockCreateAccountUnitOfWorkFactory {
    pub should_fail_on_save: bool,
}

impl Default for MockCreateAccountUnitOfWorkFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl MockCreateAccountUnitOfWorkFactory {
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
impl CreateAccountUnitOfWorkFactory for MockCreateAccountUnitOfWorkFactory {
    type UnitOfWork = MockCreateAccountUnitOfWork;

    async fn create(&self) -> Result<Self::UnitOfWork, CreateAccountError> {
        Ok(MockCreateAccountUnitOfWork::with_failure(
            self.should_fail_on_save,
        ))
    }
}
