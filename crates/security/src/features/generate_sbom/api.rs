use crate::features::generate_sbom::use_case::GenerateSbomUseCase;
use artifact::domain::events::ArtifactEvent;
use std::sync::Arc;

pub struct SbomGenerationEventHandler {
    use_case: Arc<GenerateSbomUseCase>,
}

impl SbomGenerationEventHandler {
    pub fn new(use_case: Arc<GenerateSbomUseCase>) -> Self {
        Self { use_case }
    }

    // Este método será llamado por el consumidor de Kafka/RabbitMQ
    pub async fn handle(&self, event: &ArtifactEvent) {
        if let ArtifactEvent::ArtifactUploaded { artifact } = event {
            match self.use_case.execute(artifact).await {
                Ok(sbom) => {
                    tracing::info!("Successfully generated SBOM {} for artifact {}", sbom.id, sbom.artifact_id);
                }
                Err(e) => {
                    tracing::error!("Failed to generate SBOM for artifact: {}", e);
                }
            }
        }
    }
}