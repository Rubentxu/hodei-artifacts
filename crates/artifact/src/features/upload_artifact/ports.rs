use async_trait::async_trait;
use bytes::Bytes;
use shared::hrn::Hrn;
use std::path::{Path, PathBuf};

use super::dto::UploadArtifactCommand;
use super::error::UploadArtifactError;
use crate::domain::events::ArtifactEvent;
use crate::domain::package_version::{ArtifactDependency, PackageMetadata, PackageVersion};
use crate::domain::physical_artifact::PhysicalArtifact;

// Define a type alias for the Result type used in ports
pub type PortResult<T> = Result<T, UploadArtifactError>;

#[async_trait]
pub trait ArtifactRepository: Send + Sync {
    async fn find_physical_artifact_by_hash(
        &self,
        hash: &str,
    ) -> PortResult<Option<PhysicalArtifact>>;
    async fn save_physical_artifact(&self, artifact: &PhysicalArtifact) -> PortResult<()>;
    async fn save_package_version(&self, package_version: &PackageVersion) -> PortResult<()>;

    /// Update package metadata and dependencies for an existing package version
    async fn update_package_metadata(
        &self,
        _hrn: &Hrn,
        _metadata: PackageMetadata,
        _dependencies: Vec<ArtifactDependency>,
    ) -> PortResult<()> {
        // Default implementation that returns an error
        // Implementations should override this method
        Err(UploadArtifactError::RepositoryError(
            "update_package_metadata not implemented".to_string(),
        ))
    }
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
    async fn save_chunk(
        &self,
        upload_id: &str,
        chunk_number: usize,
        data: bytes::Bytes,
    ) -> Result<(), UploadArtifactError>;
    async fn get_received_chunks_count(
        &self,
        upload_id: &str,
    ) -> Result<usize, UploadArtifactError>;
    async fn get_received_chunk_numbers(
        &self,
        upload_id: &str,
    ) -> Result<Vec<usize>, UploadArtifactError>;
    async fn assemble_chunks(
        &self,
        upload_id: &str,
        total_chunks: usize,
        file_name: &str,
    ) -> Result<(PathBuf, String), UploadArtifactError>;
    async fn cleanup(&self, upload_id: &str) -> Result<(), UploadArtifactError>;
}

/// Hook de validación pre-commit. Devuelve Ok(()) si todo es válido; Err(vec_de_errores) si no.
#[async_trait]
pub trait ArtifactValidator: Send + Sync {
    async fn validate(
        &self,
        command: &UploadArtifactCommand,
        content: &Bytes,
    ) -> Result<(), Vec<String>>;
}

/// Validador de versiones para artefactos
#[async_trait]
pub trait VersionValidator: Send + Sync {
    async fn validate_version(&self, version_str: &str) -> Result<(), String>;
    async fn parse_version(&self, version_str: &str) -> Result<ParsedVersion, String>;
}

/// Información detallada sobre una versión parseada
#[derive(Debug, Clone)]
pub struct ParsedVersion {
    pub original: String,
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub prerelease: Option<String>,
    pub build_metadata: Option<String>,
    pub is_snapshot: bool,
}
