use async_trait::async_trait;
use crate::domain::sbom::{Sbom, SbomFormat};
use artifact::domain::physical_artifact::PhysicalArtifact;
use shared::hrn::PhysicalArtifactId;

// Puerto para un generador de SBOM.
#[async_trait]
pub trait ISbomGenerator: Send + Sync {
    async fn generate(&self, artifact: &PhysicalArtifact, format: SbomFormat) -> Result<Sbom, SbomGenerationError>;
}

// Puerto para persistir el SBOM.
#[async_trait]
pub trait ISbomRepository: Send + Sync {
    async fn save(&self, sbom: &Sbom) -> Result<(), SbomRepositoryError>;
    async fn get_by_artifact_id(&self, artifact_id: &PhysicalArtifactId) -> Result<Option<Sbom>, SbomRepositoryError>;
}

// Puerto para obtener artefactos físicos.
#[async_trait]
pub trait IArtifactRetriever: Send + Sync {
    async fn get_physical_artifact(&self, artifact_id: &PhysicalArtifactId) -> Result<PhysicalArtifact, SbomGenerationError>;
}

// Errores específicos de generación de SBOM
#[derive(Debug, thiserror::Error)]
pub enum SbomGenerationError {
    #[error("Failed to generate SBOM: {0}")]
    GenerationFailed(String),
    
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),
    
    #[error("Artifact not found: {0}")]
    ArtifactNotFound(String),
    
    #[error("Repository error: {0}")]
    RepositoryError(String),
}

// Errores específicos del repositorio de SBOM
#[derive(Debug, thiserror::Error)]
pub enum SbomRepositoryError {
    #[error("Failed to save SBOM: {0}")]
    SaveError(String),
    
    #[error("Failed to retrieve SBOM: {0}")]
    RetrieveError(String),
    
    #[error("SBOM not found for artifact: {0}")]
    NotFound(String),
}