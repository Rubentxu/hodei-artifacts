use crate::features::create_ou::ports::OuPersister;
use crate::features::create_ou::error::CreateOuError;
use crate::shared::domain::ou::OrganizationalUnit;
use crate::shared::application::ports::ou_repository::OuRepository;
use async_trait::async_trait;
use std::sync::Arc;

pub struct OuPersisterAdapter<OR: OuRepository> {
    repository: Arc<OR>,
}

impl<OR: OuRepository> OuPersisterAdapter<OR> {
    pub fn new(repository: Arc<OR>) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<OR: OuRepository> OuPersister for OuPersisterAdapter<OR> {
    async fn save(&self, ou: OrganizationalUnit) -> Result<(), CreateOuError> {
        self.repository.save(&ou).await?;
        Ok(())
    }
}
