use async_trait::async_trait;
use bytes::Bytes;
use std::path::{Path, PathBuf};

use super::error::UploadArtifactError;
use crate::domain::physical_artifact::PhysicalArtifact;
use crate::domain::package_version::PackageVersion;
use crate::domain::events::ArtifactEvent;

// Define a type alias for the Result type used in ports
pub type PortResult<T> = Result<T, UploadArtifactError>;

#[async_trait]
pub trait ArtifactRepository: Send + Sync {
    async fn find_physical_artifact_by_hash(&self, hash: &str) -> PortResult<Option<PhysicalArtifact>>;
    async fn save_physical_artifact(&self, artifact: &PhysicalArtifact) -> PortResult<()>;
    async fn save_package_version(&self, package_version: &PackageVersion) -> PortResult<()>;
}

#[async_trait]
pub trait ArtifactStorage: Send + Sync {
    async fn upload(&self, content: Bytes, content_hash: &str) -> PortResult<String>;
    async fn upload_from_path(&self, path: &Path, content_hash: &str) -> PortResult<String>;
}

#[async_trait]
pub trait EventPublisher: Send + Sync {
    async fn publish(&self, event: &ArtifactEvent) -> PortResult<()>;
}

#[async_trait]
pub trait ChunkedUploadStorage: Send + Sync {
    async fn save_chunk(&self, upload_id: &str, chunk_number: usize, data: bytes::Bytes) -> Result<(), UploadArtifactError>;
    async fn get_received_chunks_count(&self, upload_id: &str) -> Result<usize, UploadArtifactError>;
    async fn assemble_chunks(&self, upload_id: &str, total_chunks: usize, file_name: &str) -> Result<(PathBuf, String), UploadArtifactError>;
    async fn cleanup(&self, upload_id: &str) -> Result<(), UploadArtifactError>;
}
