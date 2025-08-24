use async_trait::async_trait;
use shared::ArtifactId;
use crate::domain::model::{SbomId, SbomSummary};
use crate::error::SupplyChainError;

#[async_trait]
pub trait SbomRepository: Send + Sync {
    async fn save_summary(&self, summary: &SbomSummary) -> Result<(), SupplyChainError>;
    async fn get_summary(&self, sbom_id: &SbomId) -> Result<Option<SbomSummary>, SupplyChainError>;
}

#[async_trait]
pub trait SbomGenerationService: Send + Sync {
    async fn generate_for_artifact(&self, artifact_id: &ArtifactId) -> Result<SbomSummary, SupplyChainError>;
}

