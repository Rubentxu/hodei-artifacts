use crate::domain::model::{RepositoryDescription, RepositoryName};
use serde::{Deserialize, Serialize};
use shared::domain::event::DomainEventPayload;
use shared::{IsoTimestamp, RepositoryId, UserId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryCreatedEvent {
    pub repository_id: RepositoryId,
    pub name: RepositoryName,
    pub description: Option<RepositoryDescription>,
    pub created_by: UserId,
    pub occurred_at: IsoTimestamp,
}

impl DomainEventPayload for RepositoryCreatedEvent {
    fn base_type(&self) -> &'static str {
        "RepositoryCreated"
    }

    fn aggregate_id(&self) -> String {
        self.repository_id.0.to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryDeletedEvent {
    pub repository_id: RepositoryId,
    pub deleted_by: UserId,
    pub occurred_at: IsoTimestamp,
}

impl DomainEventPayload for RepositoryDeletedEvent {
    fn base_type(&self) -> &'static str {
        "RepositoryDeleted"
    }

    fn aggregate_id(&self) -> String {
        self.repository_id.0.to_string()
    }
}
