use serde::{Serialize, Deserialize};
use shared::{RepositoryId, UserId, IsoTimestamp};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryCreatedEvent {
    pub repository_id: RepositoryId,
    pub created_by: UserId,
    pub occurred_at: IsoTimestamp,
}

