use crate::domain::sbom::{Sbom, SbomFormat};
use crate::features::generate_sbom::ports::{ISbomGenerator, ISbomRepository, IArtifactRetriever, SbomGenerationError};
use std::sync::Arc;
use shared::models::ArtifactReference;

pub struct GenerateSbomUseCase {
    generator: Arc<dyn ISbomGenerator>,
    repository: Arc<dyn ISbomRepository>,
    artifact_retriever: Arc<dyn IArtifactRetriever>,
}

impl GenerateSbomUseCase {
    pub fn new(
        generator: Arc<dyn ISbomGenerator>,
        repository: Arc<dyn ISbomRepository>,
        artifact_retriever: Arc<dyn IArtifactRetriever>,
    ) -> Self {
        Self { generator, repository, artifact_retriever }
    }

    pub async fn execute(&self, artifact_ref: &ArtifactReference) -> Result<Sbom, SbomGenerationError> {
        // Obtener el artefacto físico del repositorio
        let artifact = self.artifact_retriever
            .get_physical_artifact(&artifact_ref.artifact_hrn)
            .await?;
        
        // Generar el SBOM usando el generador
        let sbom = self.generator.generate(&artifact, SbomFormat::CycloneDX).await?;
        
        // Guardar el SBOM usando el repositorio
        self.repository.save(&sbom).await
            .map_err(|e| SbomGenerationError::RepositoryError(e.to_string()))?;
        
        Ok(sbom)
    }
}