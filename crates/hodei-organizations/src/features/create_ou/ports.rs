use crate::shared::domain::ou::OrganizationalUnit;
use crate::features::create_ou::error::CreateOuError;
use async_trait::async_trait;

#[async_trait]
pub trait OuPersister {
    async fn save(&self, ou: OrganizationalUnit) -> Result<(), CreateOuError>;
}
