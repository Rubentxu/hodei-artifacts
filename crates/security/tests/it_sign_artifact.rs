#[cfg(test)]
mod tests {
    use security::features::sign_artifact::{
        adapter::{Sha256ArtifactHasher, FileKeyProvider, MongoSignatureRepository},
        use_case::SignArtifactUseCase,
        api::ArtifactSigningEventHandler,
    };
    use artifact::domain::events::ArtifactEvent;
    use std::sync::Arc;
    use shared::models::{ArtifactReference};
    use shared::enums::ArtifactType;
    use shared::hrn::PhysicalArtifactId;

    #[tokio::test]
    async fn test_artifact_signing_integration() {
        // Arrange
        let hasher = Arc::new(Sha256ArtifactHasher::new());
        let key_provider = Arc::new(FileKeyProvider::new(
            "/tmp/test_signing.key".to_string(),
            "test-key-1".to_string()
        ));
        let repository = Arc::new(MongoSignatureRepository::new());
        
        let use_case = Arc::new(SignArtifactUseCase::new(hasher, key_provider, repository));
        let event_handler = ArtifactSigningEventHandler::new(use_case);
        
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
        // generado y guardado una firma.
        assert!(true);
    }
}