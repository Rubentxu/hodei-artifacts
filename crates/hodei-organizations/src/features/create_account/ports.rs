use crate::shared::domain::account::Account;
use crate::features::create_account::error::CreateAccountError;
use async_trait::async_trait;

#[async_trait]
pub trait AccountPersister {
    async fn save(&self, account: Account) -> Result<(), CreateAccountError>;
}
