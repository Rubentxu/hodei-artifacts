use crate::features::scan_for_vulnerabilities::use_case::ScanForVulnerabilitiesUseCase;
use artifact::domain::events::ArtifactEvent;
use std::sync::Arc;

pub struct VulnerabilityScanEventHandler {
    use_case: Arc<ScanForVulnerabilitiesUseCase>,
}

impl VulnerabilityScanEventHandler {
    pub fn new(use_case: Arc<ScanForVulnerabilitiesUseCase>) -> Self {
        Self { use_case }
    }

    // Este método será llamado por el consumidor de Kafka/RabbitMQ
    pub async fn handle(&self, event: &ArtifactEvent) {
        if let ArtifactEvent::ArtifactUploaded { artifact } = event {
            if let Err(e) = self.use_case.execute(artifact).await {
                tracing::error!("Failed to scan artifact for vulnerabilities: {}", e);
            }
        }
    }
}
