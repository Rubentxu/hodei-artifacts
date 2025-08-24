use async_trait::async_trait;
use shared::{RepositoryId, UserId, IsoTimestamp};
use crate::domain::model::{Repository, RepositoryName, RepositoryDescription};
use crate::error::RepositoryError;

pub struct NewRepositoryParams {
    pub id: RepositoryId,
    pub name: RepositoryName,
    pub description: Option<RepositoryDescription>,
    pub created_by: UserId,
    pub occurred_at: IsoTimestamp,
}

#[async_trait]
pub trait RepositoryStore: Send + Sync {
    async fn save(&self, repo: &Repository) -> Result<(), RepositoryError>;
    async fn get(&self, id: &RepositoryId) -> Result<Option<Repository>, RepositoryError>;
}

#[async_trait]
pub trait RepositoryEventPublisher: Send + Sync {
    async fn publish_created(&self, repo: &Repository) -> Result<(), RepositoryError>;
}

