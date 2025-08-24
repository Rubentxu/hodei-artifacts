use async_trait::async_trait;
use shared::{ArtifactId, RepositoryId, UserId, IsoTimestamp};
use crate::domain::model::{Artifact, ArtifactChecksum, ArtifactVersion};
use crate::error::ArtifactError;

#[async_trait]
pub trait ArtifactRepository: Send + Sync {
    async fn save(&self, artifact: &Artifact) -> Result<(), ArtifactError>;
    async fn get(&self, id: &ArtifactId) -> Result<Option<Artifact>, ArtifactError>;
}

#[async_trait]
pub trait ArtifactStorage: Send + Sync {
    async fn put_object(&self, repository: &RepositoryId, artifact_id: &ArtifactId, bytes: &[u8]) -> Result<(), ArtifactError>;
}

#[async_trait]
pub trait ArtifactEventPublisher: Send + Sync {
    async fn publish_created(&self, artifact: &Artifact) -> Result<(), ArtifactError>;
}

pub struct NewArtifactParams {
    pub repository_id: RepositoryId,
    pub version: ArtifactVersion,
    pub file_name: String,
    pub size_bytes: u64,
    pub checksum: ArtifactChecksum,
    pub created_by: UserId,
    pub occurred_at: IsoTimestamp,
}

