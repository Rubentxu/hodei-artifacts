use async_trait::async_trait;
use shared::{ArtifactId, RepositoryId, UserId, IsoTimestamp};
use crate::domain::model::{Artifact, ArtifactChecksum, ArtifactVersion};
use crate::error::ArtifactError;

#[async_trait]
pub trait ArtifactRepository: Send + Sync {
    async fn save(&self, artifact: &Artifact) -> Result<(), ArtifactError>;
    async fn get(&self, id: &ArtifactId) -> Result<Option<Artifact>, ArtifactError>;
    /// Búsqueda para idempotencia: localizar artifact existente por (repository_id, checksum).
    async fn find_by_repo_and_checksum(&self, repository: &RepositoryId, checksum: &ArtifactChecksum) -> Result<Option<Artifact>, ArtifactError>;
    async fn find_by_maven_coordinates(&self, group_id: &str, artifact_id: &str, version: &str, file_name: &str) -> Result<Option<Artifact>, ArtifactError>;
    async fn find_by_npm_package_name(&self, package_name: &str) -> Result<Vec<Artifact>, ArtifactError>;
    async fn find_all_artifacts(&self) -> Result<Vec<Artifact>, ArtifactError>;
}

#[async_trait]
pub trait ArtifactStorage: Send + Sync {
    async fn put_object(&self, repository: &RepositoryId, artifact_id: &ArtifactId, bytes: &[u8]) -> Result<(), ArtifactError>;
    
    /// Obtiene un objeto como stream de bytes
    async fn get_object_stream(&self, repository: &RepositoryId, artifact_id: &ArtifactId) -> Result<Vec<u8>, ArtifactError>;
    
    /// Genera una URL presignada para descarga directa (válida por tiempo limitado)
    async fn get_presigned_download_url(&self, repository: &RepositoryId, artifact_id: &ArtifactId, expires_in_secs: u64) -> Result<String, ArtifactError>;
}

#[async_trait]
pub trait ArtifactEventPublisher: Send + Sync {
    async fn publish_created(&self, event: &shared::domain::event::ArtifactUploadedEvent) -> Result<(), ArtifactError>;
    async fn publish_download_requested(&self, event: &shared::domain::event::ArtifactDownloadRequestedEvent) -> Result<(), ArtifactError>;
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
