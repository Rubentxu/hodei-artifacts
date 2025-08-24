use async_trait::async_trait;
use crate::domain::model::UserAccount;
use shared::UserId;
use crate::error::IamError;

#[async_trait]
pub trait UserAccountRepository: Send + Sync {
    async fn save(&self, user: &UserAccount) -> Result<(), IamError>;
    async fn get(&self, id: &UserId) -> Result<Option<UserAccount>, IamError>;
}

