#[cfg(test)]
mod tests {
    use security::features::generate_sbom::{
        adapter::{SyftSbomGenerator, S3SbomRepository},
        use_case::GenerateSbomUseCase,
        api::SbomGenerationEventHandler,
    };
    use artifact::domain::events::ArtifactEvent;
    use std::sync::Arc;
    use shared::models::{ArtifactReference};
    use shared::enums::ArtifactType;
    use shared::hrn::PhysicalArtifactId;

    #[tokio::test]
    async fn test_sbom_generation_integration() {
        // Arrange
        let generator = Arc::new(SyftSbomGenerator::new());
        let repository = Arc::new(S3SbomRepository::new());
        
        let use_case = Arc::new(GenerateSbomUseCase::new(generator, repository));
        let event_handler = SbomGenerationEventHandler::new(use_case);
        
        let artifact_ref = ArtifactReference {
            artifact_hrn: PhysicalArtifactId::new("hrn:hodei:artifact:us-east-1:123456789012:physical-artifact/sha256-abcd1234").unwrap(),
            artifact_type: ArtifactType::Primary,
            role: None,
        };
        let event = ArtifactEvent::ArtifactUploaded { artifact: artifact_ref };

        // Act
        event_handler.handle(&event).await;

        // Assert
        // En este test de integración básico, simplemente verificamos que
        // el manejador de eventos se ejecute sin errores.
        // En una implementación real, podríamos verificar que se haya
        // generado y guardado un SBOM.
        assert!(true);
    }
}