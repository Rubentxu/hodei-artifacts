use crate::shared::application::ports::account_repository::{AccountRepository, AccountRepositoryError};
use crate::shared::domain::account::Account;
use policies::domain::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use async_trait::async_trait;

pub struct SurrealAccountRepository {
    db: Surreal<Db>,
}

impl SurrealAccountRepository {
    pub fn new(db: Surreal<Db>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl AccountRepository for SurrealAccountRepository {
    async fn save(&self, account: &Account) -> Result<(), AccountRepositoryError> {
        let hrn_str = account.hrn.to_string();
        let _: Option<Account> = self.db.create(("account", &hrn_str)).content(account.clone()).await
            .map_err(|e| AccountRepositoryError::DatabaseError(e.to_string()))?;
        Ok(())
    }
    
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, AccountRepositoryError> {
        let hrn_str = hrn.to_string();
        let result: Option<Account> = self.db.select(("account", &hrn_str)).await
            .map_err(|e| AccountRepositoryError::DatabaseError(e.to_string()))?;
        Ok(result)
    }
}
