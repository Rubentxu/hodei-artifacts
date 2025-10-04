use crate::shared::domain::ou::OrganizationalUnit;
use async_trait::async_trait;
use thiserror::Error;
use policies::domain::Hrn;

#[derive(Debug, Error)]
pub enum OuRepositoryError {
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("Organizational Unit not found")]
    OuNotFound,
}

#[async_trait]
pub trait OuRepository {
    async fn save(&self, ou: &OrganizationalUnit) -> Result<(), OuRepositoryError>;
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, OuRepositoryError>;
}
