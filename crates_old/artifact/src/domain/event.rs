use serde::{Serialize, Deserialize};
use shared::{ArtifactId, RepositoryId, UserId, IsoTimestamp};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactCreated {
    pub artifact_id: ArtifactId,
    pub repository_id: RepositoryId,
    pub created_by: UserId,
    pub occurred_at: IsoTimestamp,
}

