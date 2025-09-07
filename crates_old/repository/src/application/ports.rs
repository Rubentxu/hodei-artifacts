use async_trait::async_trait;
use shared::domain::event::{DomainEventEnvelope, DomainEventPayload};
use shared::{RepositoryId, UserId, IsoTimestamp};
use crate::domain::model::{Repository, RepositoryName, RepositoryDescription};
use crate::error::RepositoryError;
use serde::Serialize;

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
    async fn find_by_name(&self, name: &RepositoryName) -> Result<Option<Repository>, RepositoryError>;
    async fn delete(&self, id: &RepositoryId) -> Result<(), RepositoryError>;
}

#[async_trait]
pub trait EventBus: Send + Sync {
    async fn publish<E: DomainEventPayload + Serialize + Send + Sync>(&self, event: &DomainEventEnvelope<E>) -> Result<(), RepositoryError>;
}
