use async_trait::async_trait;
use bytes::Bytes;

use crate::domain::{
    events::ArtifactEvent,
    package_version::PackageVersion,
    physical_artifact::PhysicalArtifact,
};

use super::error::UploadArtifactError;

// Use a feature-specific error type for all port results.
pub type PortResult<T> = Result<T, UploadArtifactError>;

/// Port for database operations, segregated for the upload artifact feature.
#[async_trait]
pub trait UploadArtifactRepository: Send + Sync {
    async fn save_package_version(&self, package_version: &PackageVersion) -> PortResult<()>;
    async fn save_physical_artifact(&self, physical_artifact: &PhysicalArtifact) -> PortResult<()>;
    async fn find_physical_artifact_by_hash(&self, hash: &str) -> PortResult<Option<PhysicalArtifact>>;
}

/// Port for binary object storage operations.
#[async_trait]
pub trait ArtifactStorage: Send + Sync {
    // Upload content as Bytes.
    async fn upload(&self, content: Bytes, content_hash: &str) -> PortResult<String>;

    /// Upload desde un archivo local sin cargarlo completamente en memoria (por defecto delega a upload).
    async fn upload_from_path(&self, path: &Path, content_hash: &str) -> PortResult<String> {
        let data = tokio::fs::read(path).await.map_err(|e| UploadArtifactError::StorageError(e.to_string()))?;
        self.upload(Bytes::from(data), content_hash).await
    }
}

/// Port for publishing domain events.
#[async_trait]
pub trait EventPublisher: Send + Sync {
    async fn publish(&self, event: &ArtifactEvent) -> PortResult<()>;
}

/// Port for temporary storage of upload chunks (for resumable uploads).
#[async_trait]
pub trait ChunkedUploadStorage: Send + Sync {
    /// Guarda un chunk temporalmente asociado a un upload_id y número de chunk.
    async fn save_chunk(&self, upload_id: &str, chunk_number: u64, data: Bytes) -> PortResult<()>;
    /// Recupera un chunk específico.
    async fn get_chunk(&self, upload_id: &str, chunk_number: u64) -> PortResult<Option<Bytes>>;
    /// Ensambla todos los chunks en un archivo final y devuelve su Path.
    async fn assemble_to_path(&self, upload_id: &str, total_chunks: u64) -> PortResult<PathBuf>;
    /// Ensambla todos los chunks y devuelve Bytes (fallback).
    async fn assemble_upload(&self, upload_id: &str, total_chunks: u64) -> PortResult<Bytes> {
        let path = self.assemble_to_path(upload_id, total_chunks).await?;
        let data = tokio::fs::read(path).await.map_err(|e| UploadArtifactError::StorageError(e.to_string()))?;
        Ok(Bytes::from(data))
    }
    /// Limpia los chunks temporales de un upload (por éxito o error).
    async fn cleanup_upload(&self, upload_id: &str) -> PortResult<()>;
}
