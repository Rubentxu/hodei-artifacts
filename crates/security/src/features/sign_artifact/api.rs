use crate::features::sign_artifact::use_case::SignArtifactUseCase;
use artifact::domain::events::ArtifactEvent;
use std::sync::Arc;

pub struct ArtifactSigningEventHandler {
    use_case: Arc<SignArtifactUseCase>,
}

impl ArtifactSigningEventHandler {
    pub fn new(use_case: Arc<SignArtifactUseCase>) -> Self {
        Self { use_case }
    }

    // Este método será llamado por el consumidor de Kafka/RabbitMQ
    pub async fn handle(&self, event: &ArtifactEvent) {
        if let ArtifactEvent::ArtifactUploaded { artifact } = event {
            tracing::info!("Processing artifact signing for artifact: {:?}", artifact.artifact_hrn);
            
            match self.use_case.execute(artifact).await {
                Ok(signature) => {
                    tracing::info!("Successfully signed artifact {:?} with signature {}", artifact.artifact_hrn, signature.id);
                }
                Err(e) => {
                    tracing::error!("Failed to sign artifact {:?}: {}", artifact.artifact_hrn, e);
                }
            }
        }
    }
}