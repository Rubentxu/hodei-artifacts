//! Tests unitarios para funciones build_event
//! EVENT-1: Verificar payloads completos, correlation_id y versiones

#[cfg(test)]
mod tests {
    use crate::{
        features::upload_artifact::logic::build_event::{
            build_artifact_uploaded_event, 
            build_artifact_upload_idempotent_event, 
            validate_artifact_for_events
        },
        features::download_artifact::logic::build_event::{
            build_download_requested_event,
            DownloadEventContext
        },
        domain::model::{Artifact, ArtifactChecksum, ArtifactVersion},
        features::download_artifact::query::GetArtifactQuery,
    };
    use shared::{
        ArtifactId, RepositoryId, UserId, IsoTimestamp,
        domain::event::DomainEventPayload
    };
    use uuid::Uuid;

    #[test]
    fn test_build_artifact_uploaded_event_payload_completo() {
        // Arrange
        let artifact_id = ArtifactId(Uuid::new_v4());
        let repository_id = RepositoryId(Uuid::new_v4());
        let uploader = UserId(Uuid::new_v4());
        let correlation_id = Uuid::new_v4();
        
        let artifact = Artifact {
            id: artifact_id,
            repository_id: repository_id.clone(),
            version: ArtifactVersion("1.2.3".to_string()),
            file_name: "test-artifact.jar".to_string(),
            size_bytes: 4096,
            checksum: ArtifactChecksum("a1b2c3d4e5f6".repeat(11)), // 64 chars
            created_at: IsoTimestamp::now(),
            created_by: uploader.clone(),
            coordinates: None,
        };

        // Act
        let event_envelope = build_artifact_uploaded_event(&artifact, correlation_id, &uploader);

        // Assert - Envelope structure
        assert_eq!(event_envelope.event_type, "ArtifactUploaded.v1");
        assert_eq!(event_envelope.correlation_id, correlation_id);
        assert_eq!(event_envelope.version, "v1");
        assert_eq!(event_envelope.source, "hodei-artifacts.artifact-upload");
        assert!(event_envelope.causation_id.is_none());
        assert!(!event_envelope.event_id.to_string().is_empty());
        
        // Assert - Payload completo (Event-Carried State)
        let payload = &event_envelope.data;
        assert_eq!(payload.artifact_id, artifact_id);
        assert_eq!(payload.repository_id, repository_id);
        assert_eq!(payload.uploader, uploader);
        assert_eq!(payload.sha256.as_ref().unwrap(), &artifact.checksum.0);
        assert_eq!(payload.size_bytes.unwrap(), artifact.size_bytes);
        
        // Media type y upload_time_ms no implementados aún (como se espera)
        assert!(payload.media_type.is_none());
        assert!(payload.upload_time_ms.is_none());
    }

    #[test]
    fn test_build_artifact_uploaded_event_correlation_id() {
        let artifact = create_test_artifact();
        let uploader = UserId(Uuid::new_v4());
        let correlation_id = Uuid::new_v4();

        let event_envelope = build_artifact_uploaded_event(&artifact, correlation_id, &uploader);

        // Verifica que el correlation_id se preserva correctamente
        assert_eq!(event_envelope.correlation_id, correlation_id);
        
        // El event_id debe ser diferente al correlation_id (nuevo evento)
        assert_ne!(event_envelope.event_id, correlation_id);
    }

    #[test]
    fn test_build_artifact_uploaded_event_version() {
        let artifact = create_test_artifact();
        let uploader = UserId(Uuid::new_v4());
        let correlation_id = Uuid::new_v4();

        let event_envelope = build_artifact_uploaded_event(&artifact, correlation_id, &uploader);

        // Verifica versionado correcto
        assert_eq!(event_envelope.version, "v1");
        assert_eq!(event_envelope.event_type, "ArtifactUploaded.v1");
        
        // Verifica que el payload implementa DomainEventPayload correctamente
        let payload = &event_envelope.data;
        assert_eq!(payload.base_type(), "ArtifactUploaded");
        assert_eq!(payload.version(), "v1");
        assert_eq!(payload.full_event_type(), "ArtifactUploaded.v1");
        assert_eq!(payload.aggregate_id(), artifact.id.0.to_string());
    }

    #[test]
    fn test_build_artifact_upload_idempotent_event() {
        let artifact = create_test_artifact();
        let uploader = UserId(Uuid::new_v4());
        let correlation_id = Uuid::new_v4();

        let event_envelope = build_artifact_upload_idempotent_event(&artifact, correlation_id, &uploader);

        // Verifica estructura básica
        assert_eq!(event_envelope.event_type, "ArtifactUploaded.v1");
        assert_eq!(event_envelope.correlation_id, correlation_id);
        
        // Verifica metadata específica de idempotencia
        assert_eq!(
            event_envelope.metadata.get("operation_type").unwrap(),
            "idempotent_upload"
        );
        assert_eq!(
            event_envelope.metadata.get("reason").unwrap(),
            "artifact_already_exists"
        );
        
        // Payload debe ser idéntico al evento normal
        let payload = &event_envelope.data;
        assert_eq!(payload.artifact_id, artifact.id);
        assert_eq!(payload.repository_id, artifact.repository_id);
        assert_eq!(payload.uploader, uploader);
    }

    #[test]
    fn test_build_download_requested_event_payload_completo() {
        // Arrange
        let artifact_id = ArtifactId(Uuid::new_v4());
        let user_id = UserId(Uuid::new_v4());
        
        let query = GetArtifactQuery {
            artifact_id,
            user_id,
            user_agent: Some("Mozilla/5.0 Test".to_string()),
            client_ip: Some("192.168.1.100".to_string()),
            use_presigned_url: false,
            presigned_expires_secs: None,
        };

        // Act
        let result = build_download_requested_event(&query);

        // Assert
        assert!(result.is_ok());
        let event_envelope = result.unwrap();
        
        // Envelope structure
        assert_eq!(event_envelope.event_type, "ArtifactDownloadRequested.v1");
        assert_eq!(event_envelope.version, "v1");
        assert_eq!(event_envelope.source, "hodei-artifacts.artifact-retrieve");
        
        // Payload completo
        let payload = &event_envelope.data;
        assert_eq!(payload.artifact_id, artifact_id);
        assert_eq!(payload.user_id, user_id);
        assert_eq!(payload.user_agent.as_ref().unwrap(), "Mozilla/5.0 Test");
        assert_eq!(payload.client_ip.as_ref().unwrap(), "192.168.1.100");
        assert!(payload.requested_range.is_none());
    }

    #[test]
    fn test_build_download_requested_event_version() {
        let query = create_test_download_query();
        let result = build_download_requested_event(&query);
        
        assert!(result.is_ok());
        let event_envelope = result.unwrap();
        
        // Verifica versionado
        assert_eq!(event_envelope.version, "v1");
        assert_eq!(event_envelope.event_type, "ArtifactDownloadRequested.v1");
        
        // Verifica payload traits
        let payload = &event_envelope.data;
        assert_eq!(payload.base_type(), "ArtifactDownloadRequested");
        assert_eq!(payload.version(), "v1");
        assert_eq!(payload.full_event_type(), "ArtifactDownloadRequested.v1");
        assert_eq!(payload.aggregate_id(), query.artifact_id.0.to_string());
    }

    #[test]
    fn test_download_event_context_from_query() {
        let query = GetArtifactQuery {
            artifact_id: ArtifactId(Uuid::new_v4()),
            user_id: UserId(Uuid::new_v4()),
            user_agent: Some("Custom-Agent/1.0".to_string()),
            client_ip: Some("10.0.0.1".to_string()),
            use_presigned_url: true,
            presigned_expires_secs: Some(3600),
        };
        
        let correlation_id = Uuid::new_v4().to_string();
        let context = DownloadEventContext::from_query(&query, correlation_id.clone());
        
        assert_eq!(context.correlation_id, correlation_id);
        assert_eq!(context.user_agent, "Custom-Agent/1.0");
        assert_eq!(context.client_ip, "10.0.0.1");
        assert_eq!(context.download_method, "presigned");
    }

    #[test]
    fn test_download_event_context_defaults() {
        let query = GetArtifactQuery {
            artifact_id: ArtifactId(Uuid::new_v4()),
            user_id: UserId(Uuid::new_v4()),
            user_agent: None,
            client_ip: None,
            use_presigned_url: false,
            presigned_expires_secs: None,
        };
        
        let correlation_id = Uuid::new_v4().to_string();
        let context = DownloadEventContext::from_query(&query, correlation_id.clone());
        
        assert_eq!(context.correlation_id, correlation_id);
        assert_eq!(context.user_agent, "unknown");
        assert_eq!(context.client_ip, "unknown");
        assert_eq!(context.download_method, "direct");
    }

    #[test]
    fn test_validate_artifact_for_events_valid() {
        let artifact = create_test_artifact();
        let result = validate_artifact_for_events(&artifact);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_artifact_for_events_empty_filename() {
        let mut artifact = create_test_artifact();
        artifact.file_name = "".to_string();
        
        let result = validate_artifact_for_events(&artifact);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Artifact file_name cannot be empty");
    }

    #[test]
    fn test_validate_artifact_for_events_zero_size() {
        let mut artifact = create_test_artifact();
        artifact.size_bytes = 0;
        
        let result = validate_artifact_for_events(&artifact);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Artifact size must be greater than 0");
    }

    #[test]
    fn test_validate_artifact_for_events_empty_checksum() {
        let mut artifact = create_test_artifact();
        artifact.checksum = ArtifactChecksum("".to_string());
        
        let result = validate_artifact_for_events(&artifact);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Artifact checksum cannot be empty");
    }

    // Helper functions
    fn create_test_artifact() -> Artifact {
        Artifact {
            id: ArtifactId(Uuid::new_v4()),
            repository_id: RepositoryId(Uuid::new_v4()),
            version: ArtifactVersion("1.0.0".to_string()),
            file_name: "test.jar".to_string(),
            size_bytes: 1024,
            checksum: ArtifactChecksum("a".repeat(64)),
            created_at: IsoTimestamp::now(),
            created_by: UserId(Uuid::new_v4()),
            coordinates: None,
        }
    }

    fn create_test_download_query() -> GetArtifactQuery {
        GetArtifactQuery {
            artifact_id: ArtifactId(Uuid::new_v4()),
            user_id: UserId(Uuid::new_v4()),
            user_agent: Some("Test-Agent".to_string()),
            client_ip: Some("127.0.0.1".to_string()),
            use_presigned_url: false,
            presigned_expires_secs: None,
        }
    }
}
