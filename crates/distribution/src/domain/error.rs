// crates/distribution/src/domain/error.rs

use thiserror::Error;
use shared::hrn::{Hrn, RepositoryId};

/// Errores principales del crate distribution
#[derive(Debug, Error)]
pub enum DistributionError {
    #[error("Format handler error: {0}")]
    FormatError(#[from] FormatError),
    
    #[error("Repository not found: {0}")]
    RepositoryNotFound(RepositoryId),
    
    #[error("Artifact not found: {0}")]
    ArtifactNotFound(String),
    
    #[error("Invalid package format: {0}")]
    InvalidPackageFormat(String),
    
    #[error("Metadata generation failed: {0}")]
    MetadataGenerationFailed(String),
    
    #[error("Storage backend error: {0}")]
    StorageError(String),
    
    #[error("Authorization failed for {user} on {resource}")]
    AuthorizationFailed { user: Hrn, resource: String },
    
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
    
    #[error("Format not supported: {0}")]
    FormatNotSupported(String),
}

/// Errores específicos de manejadores de formato
#[derive(Debug, Error)]
pub enum FormatError {
    #[error("Maven format error: {0}")]
    MavenError(String),
    
    #[error("npm format error: {0}")]
    NpmError(String),
    
    #[error("Docker format error: {0}")]
    DockerError(String),
    
    #[error("Invalid path format: {0}")]
    InvalidPath(String),
    
    #[error("Invalid metadata format: {0}")]
    InvalidMetadata(String),
    
    #[error("Version parsing error: {0}")]
    VersionParsingError(String),
    
    #[error("Dependency resolution error: {0}")]
    DependencyResolutionError(String),
}

/// Resultado genérico para operaciones de distribución
pub type DistributionResult<T> = Result<T, DistributionError>;