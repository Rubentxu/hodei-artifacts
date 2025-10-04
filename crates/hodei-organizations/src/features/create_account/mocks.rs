use crate::features::create_account::ports::AccountPersister;
use crate::features::create_account::error::CreateAccountError;
use crate::shared::domain::account::Account;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::Arc;

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
