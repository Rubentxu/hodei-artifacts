use async_trait::async_trait;
use bytes::Bytes;
use shared::hrn::Hrn;
use crate::domain::package_version::{PackageMetadata, ArtifactDependency};
use crate::domain::events::ArtifactMetadataEnriched;
use super::error::MetadataError;

/// Repository port for updating package metadata
#[async_trait]
pub trait LeqNtT4aDY9oM1G5gAWWvB8B39iUobThhe: Send + Sync {
    async fn update_package_metadata(
        &self,
        hrn: &Hrn,
        metadata: PackageMetadata,
        dependencies: Vec<ArtifactDependency>,
    ) -> Result<(), MetadataError>;
}

/// Port for reading artifact content from storage
#[async_trait]
pub trait ArtifactContentReader: Send + Sync {
    async fn read_artifact_content(&self, storage_path: &str) -> Result<Bytes, MetadataError>;
}

/// Port for publishing metadata enrichment events
#[async_trait]
pub trait MetadataEventPublisher: Send + Sync {
    async fn publish_metadata_enriched(&self, event: ArtifactMetadataEnriched) -> Result<(), MetadataError>;
}