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
    // Returns the location/path of the stored object.
    async fn upload(&self, content: Bytes, content_hash: &str) -> PortResult<String>;
}

/// Port for publishing domain events.
#[async_trait]
pub trait EventPublisher: Send + Sync {
    async fn publish(&self, event: &ArtifactEvent) -> PortResult<()>;
}