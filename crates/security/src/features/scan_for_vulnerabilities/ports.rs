use async_trait::async_trait;
use crate::domain::vulnerability::{Vulnerability, VulnerabilityReport};
use artifact::domain::physical_artifact::PhysicalArtifact;
use std::sync::Arc;
use shared::hrn::PhysicalArtifactId;

// Puerto para un escáner individual.
#[async_trait]
pub trait IScanner: Send + Sync {
    async fn scan(&self, artifact: &PhysicalArtifact) -> Result<Vec<Vulnerability>, ScanError>;
    fn name(&self) -> &'static str;
}

// Puerto para un proveedor que determina qué escáneres aplicar.
#[async_trait]
pub trait IScannerProvider: Send + Sync {
    fn scanners_for(&self, artifact: &PhysicalArtifact) -> Vec<Arc<dyn IScanner>>;
}

// Puerto para persistir el informe de vulnerabilidades.
#[async_trait]
pub trait IVulnerabilityRepository: Send + Sync {
    async fn save_report(&self, report: &VulnerabilityReport) -> Result<(), RepositoryError>;
}

// Puerto para obtener un PhysicalArtifact.
#[async_trait]
pub trait IPhysicalArtifactRepository: Send + Sync {
    async fn get_by_hrn(&self, hrn: &PhysicalArtifactId) -> Result<PhysicalArtifact, RepositoryError>;
}

// Errores específicos de los puertos
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("Scanner execution failed: {0}")]
    ExecutionError(String),
}

#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    #[error("Failed to save report: {0}")]
    SaveError(String),
    #[error("Artifact not found: {0}")]
    NotFound(String),
}
