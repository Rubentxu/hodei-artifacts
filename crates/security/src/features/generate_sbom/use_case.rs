use crate::domain::sbom::{Sbom, SbomFormat};
use crate::features::generate_sbom::ports::{ISbomGenerator, ISbomRepository, SbomGenerationError};
use std::sync::Arc;
use artifact::domain::physical_artifact::PhysicalArtifact;
use shared::hrn::OrganizationId;
use shared::lifecycle::Lifecycle;
use shared::models::ContentHash;
use shared::enums::HashAlgorithm;
use std::collections::HashMap;

pub struct GenerateSbomUseCase {
    generator: Arc<dyn ISbomGenerator>,
    repository: Arc<dyn ISbomRepository>,
}

impl GenerateSbomUseCase {
    pub fn new(
        generator: Arc<dyn ISbomGenerator>,
        repository: Arc<dyn ISbomRepository>,
    ) -> Self {
        Self { generator, repository }
    }

    pub async fn execute(&self, _artifact_ref: &shared::models::ArtifactReference) -> Result<Sbom, SbomGenerationError> {
        // En una implementación real, aquí se obtendría el artefacto físico
        // a partir del artifact_ref. Por ahora, vamos a simularlo.
        
        // Crear un artefacto simulado
        let organization_id = OrganizationId::new("123456789012").unwrap();
        let artifact = PhysicalArtifact {
            hrn: "hrn:hodei:artifact:us-east-1:123456789012:physical-artifact/sha256-abcd1234".parse().unwrap(),
            organization_hrn: organization_id,
            content_hash: ContentHash {
                algorithm: HashAlgorithm::Sha256,
                value: "abcd1234".to_string(),
            },
            size_in_bytes: 1024,
            checksums: HashMap::new(),
            storage_location: "s3://test-bucket/artifacts/abcd1234".to_string(),
            lifecycle: Lifecycle::new(
                "hrn:hodei:iam:us-east-1:123456789012:user/test-user".parse().unwrap()
            ),
        };
        
        // Generar el SBOM usando el generador
        let sbom = self.generator.generate(&artifact, SbomFormat::CycloneDX).await?;
        
        // Guardar el SBOM usando el repositorio
        self.repository.save(&sbom).await
            .map_err(|e| SbomGenerationError::GenerationFailed(e.to_string()))?;
        
        Ok(sbom)
    }
}