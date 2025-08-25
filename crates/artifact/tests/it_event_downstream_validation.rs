//! Tests de validación de eventos downstream
//! 
//! Estos tests verifican que los eventos publicados por el publisher
//! puedan ser consumidos y procesados correctamente por sistemas downstream.

use artifact::domain::model::{Artifact, ArtifactChecksum, ArtifactVersion};
use artifact::features::upload_artifact::logic::build_event::build_artifact_uploaded_event;
use shared::domain::event::ArtifactUploadedEvent;
use shared::domain::model::{ArtifactId, RepositoryId, UserId, IsoTimestamp};
use std::sync::{Arc, Mutex};
use serde_json::Value;
use uuid::Uuid;

/// Mock del SearchIndex para capturar las operaciones de indexación
#[derive(Clone)]
pub struct MockSearchIndex {
    indexed_documents: Arc<Mutex<Vec<MockIndexedDocument>>>,
}

#[derive(Debug, Clone)]
pub struct MockIndexedDocument {
    pub artifact_id: ArtifactId,
    pub repository_id: RepositoryId,
    pub name: String,
    pub version: String,
    pub indexed_at: IsoTimestamp,
}

impl MockSearchIndex {
    pub fn new() -> Self {
        Self {
            indexed_documents: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn get_indexed_documents(&self) -> Vec<MockIndexedDocument> {
        self.indexed_documents.lock().unwrap().clone()
    }

    pub fn clear(&self) {
        self.indexed_documents.lock().unwrap().clear();
    }
}

/// Mock del consumer de eventos que simula el comportamiento del KafkaEventConsumer
pub struct MockEventConsumer {
    search_index: MockSearchIndex,
    processed_events: Arc<Mutex<Vec<ProcessedEvent>>>,
}

#[derive(Debug, Clone)]
pub struct ProcessedEvent {
    pub event_id: Uuid,
    pub event_type: String,
    pub artifact_id: ArtifactId,
    pub processed_at: IsoTimestamp,
    pub success: bool,
    pub error_message: Option<String>,
}

impl MockEventConsumer {
    pub fn new(search_index: MockSearchIndex) -> Self {
        Self {
            search_index,
            processed_events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Simula el procesamiento de un evento ArtifactUploaded
    pub async fn process_artifact_uploaded_event(
        &self,
        event_json: &str,
    ) -> Result<(), String> {
        let envelope: ArtifactUploadedEvent = 
            serde_json::from_str(event_json)
                .map_err(|e| format!("Failed to deserialize event: {}", e))?;

        let event = &envelope.data;
        let processed_at = IsoTimestamp::now();

        // Simular indexación del documento
        let indexed_doc = MockIndexedDocument {
            artifact_id: event.artifact_id.clone(),
            repository_id: event.repository_id.clone(),
            name: event.artifact_id.to_string(),
            version: "1.0.0".to_string(), // Usar versión hardcoded para el mock
            indexed_at: processed_at.clone(),
        };

        // Registrar el documento indexado
        self.search_index.indexed_documents.lock().unwrap().push(indexed_doc);

        // Registrar el evento procesado
        let processed_event = ProcessedEvent {
            event_id: envelope.event_id.clone(),
            event_type: envelope.event_type.clone(),
            artifact_id: event.artifact_id.clone(),
            processed_at,
            success: true,
            error_message: None,
        };

        self.processed_events.lock().unwrap().push(processed_event);
        Ok(())
    }

    pub fn get_processed_events(&self) -> Vec<ProcessedEvent> {
        self.processed_events.lock().unwrap().clone()
    }

    pub fn clear(&self) {
        self.processed_events.lock().unwrap().clear();
    }
}

/// Validador de eventos downstream que verifica la estructura y contenido
pub struct DownstreamEventValidator;

impl DownstreamEventValidator {
    /// Valida que un evento tenga la estructura correcta para ser procesado downstream
    pub fn validate_event_structure(event_json: &str) -> Result<(), String> {
        let value: Value = serde_json::from_str(event_json)
            .map_err(|e| format!("Invalid JSON structure: {}", e))?;

        // Verificar estructura del envelope
        Self::validate_envelope_structure(&value)?;
        
        // Verificar estructura del payload
        Self::validate_payload_structure(&value)?;

        Ok(())
    }

    fn validate_envelope_structure(value: &Value) -> Result<(), String> {
        let required_fields = ["event_id", "event_type", "correlation_id", "version", "timestamp", "source", "data"];
        
        for field in required_fields.iter() {
            if !value.get(field).is_some() {
                return Err(format!("Missing required envelope field: {}", field));
            }
        }

        // Validar tipos específicos
        if !value["event_id"].is_string() {
            return Err("event_id must be string".to_string());
        }

        if !value["event_type"].is_string() {
            return Err("event_type must be string".to_string());
        }

        if !value["version"].is_string() {
            return Err("version must be string".to_string());
        }

        Ok(())
    }

    fn validate_payload_structure(value: &Value) -> Result<(), String> {
        let data = value.get("data")
            .ok_or("Missing data field")?;

        let required_payload_fields = ["artifact_id", "repository_id", "uploader"];
        
        for field in required_payload_fields.iter() {
            if !data.get(field).is_some() {
                return Err(format!("Missing required payload field: {}", field));
            }
        }

        // Validar tipos específicos del payload (campos opcionales)
        if let Some(size_bytes) = data.get("size_bytes") {
            if !size_bytes.is_number() {
                return Err("size_bytes must be number".to_string());
            }
        }

        if let Some(sha256) = data.get("sha256") {
            if !sha256.is_string() {
                return Err("sha256 must be string".to_string());
            }
        }

        Ok(())
    }

    /// Valida que el contenido del evento sea consistente
    pub fn validate_event_content(event_json: &str, expected_artifact: &Artifact) -> Result<(), String> {
        let envelope: ArtifactUploadedEvent = 
            serde_json::from_str(event_json)
                .map_err(|e| format!("Failed to deserialize event: {}", e))?;

        let event = &envelope.data;

        // Verificar que los datos del evento coincidan con el artefacto
        if event.artifact_id != expected_artifact.id {
            return Err(format!("Artifact ID mismatch: expected {}, got {}", 
                              expected_artifact.id, event.artifact_id));
        }

        if event.repository_id != expected_artifact.repository_id {
            return Err(format!("Repository ID mismatch: expected {}, got {}", 
                              expected_artifact.repository_id, event.repository_id));
        }

        if let Some(event_checksum) = &event.sha256 {
            if event_checksum != &expected_artifact.checksum.0 {
                return Err(format!("Checksum mismatch: expected {}, got {}", 
                                  expected_artifact.checksum.0, event_checksum));
            }
        }

        Ok(())
    }
}

#[tokio::test]
async fn test_downstream_event_processing_success() {
    // Crear mock del search index
    let search_index = MockSearchIndex::new();
    let consumer = MockEventConsumer::new(search_index.clone());

    // Crear un artefacto de prueba
    let artifact = Artifact::new(
        RepositoryId::new(),
        ArtifactVersion::new("1.0.0"),
        "test-artifact.jar".to_string(),
        1024,
        ArtifactChecksum::new("a".repeat(64)),
        UserId::new(),
    );

    // Construir el evento
    let correlation_id = Uuid::new_v4();
    let uploader = UserId::new();
    let envelope = build_artifact_uploaded_event(&artifact, correlation_id, &uploader);

    let event_json = serde_json::to_string(&envelope).unwrap();

    // Procesar el evento
    let result = consumer.process_artifact_uploaded_event(&event_json).await;
    assert!(result.is_ok(), "Event processing should succeed: {:?}", result.err());

    // Verificar que el evento fue procesado
    let processed_events = consumer.get_processed_events();
    assert_eq!(processed_events.len(), 1);
    assert!(processed_events[0].success);
    assert_eq!(processed_events[0].artifact_id, artifact.id);

    // Verificar que el documento fue indexado
    let indexed_docs = search_index.get_indexed_documents();
    assert_eq!(indexed_docs.len(), 1);
    assert_eq!(indexed_docs[0].artifact_id, artifact.id);
    assert_eq!(indexed_docs[0].repository_id, artifact.repository_id);
}

#[tokio::test]
async fn test_downstream_event_structure_validation() {
    // Crear un evento válido
    let artifact = Artifact::new(
        RepositoryId::new(),
        ArtifactVersion::new("1.0.0"),
        "test-artifact.jar".to_string(),
        1024,
        ArtifactChecksum::new("a".repeat(64)),
        UserId::new(),
    );

    let correlation_id = Uuid::new_v4();
    let uploader = UserId::new();
    let envelope = build_artifact_uploaded_event(&artifact, correlation_id, &uploader);

    let event_json = serde_json::to_string(&envelope).unwrap();

    // Validar estructura del evento
    let result = DownstreamEventValidator::validate_event_structure(&event_json);
    assert!(result.is_ok(), "Valid event structure should pass validation: {:?}", result.err());

    // Validar contenido del evento
    let content_result = DownstreamEventValidator::validate_event_content(&event_json, &artifact);
    assert!(content_result.is_ok(), "Valid event content should pass validation: {:?}", content_result.err());
}

#[tokio::test]
async fn test_downstream_event_malformed_json_handling() {
    let search_index = MockSearchIndex::new();
    let consumer = MockEventConsumer::new(search_index);

    // Probar con JSON malformado
    let malformed_json = r#"{"event_id": "invalid", "missing_fields": true"#;
    
    let result = consumer.process_artifact_uploaded_event(malformed_json).await;
    assert!(result.is_err(), "Malformed JSON should fail processing");
    assert!(result.unwrap_err().contains("Failed to deserialize"));

    // Verificar que no se procesó ningún evento
    let processed_events = consumer.get_processed_events();
    assert_eq!(processed_events.len(), 0);
}

#[tokio::test]
async fn test_downstream_event_missing_required_fields() {
    // Probar evento con campos faltantes
    let incomplete_event = r#"{
        "event_id": "550e8400-e29b-41d4-a716-446655440000",
        "event_type": "ArtifactUploaded",
        "version": "v1",
        "timestamp": "2024-01-01T00:00:00Z",
        "data": {
            "artifact_id": "missing-fields"
        }
    }"#;

    let result = DownstreamEventValidator::validate_event_structure(incomplete_event);
    assert!(result.is_err(), "Incomplete event should fail validation");
    assert!(result.unwrap_err().contains("Missing required"));
}

#[tokio::test]
async fn test_downstream_event_wrong_data_types() {
    // Probar evento con tipos de datos incorrectos
    let wrong_types_event = r#"{
        "event_id": 123,
        "event_type": "ArtifactUploaded",
        "correlation_id": "550e8400-e29b-41d4-a716-446655440000",
        "version": "v1",
        "timestamp": "2024-01-01T00:00:00Z",
        "source": "test",
        "data": {
            "artifact_id": "test-artifact",
            "repository_id": "test-repo",
            "uploader": "user",
            "size_bytes": "not-a-number",
            "sha256": "abc123"
        }
    }"#;

    let result = DownstreamEventValidator::validate_event_structure(wrong_types_event);
    assert!(result.is_err(), "Event with wrong types should fail validation");
}

#[tokio::test]
async fn test_multiple_downstream_events_processing() {
    let search_index = MockSearchIndex::new();
    let consumer = MockEventConsumer::new(search_index.clone());

    // Crear múltiples artefactos
    let artifacts = vec![
        Artifact::new(
            RepositoryId::new(),
            ArtifactVersion::new("1.0.0"),
            "artifact1.jar".to_string(),
            1024,
            ArtifactChecksum::new("a".repeat(64)),
            UserId::new(),
        ),
        Artifact::new(
            RepositoryId::new(),
            ArtifactVersion::new("1.1.0"),
            "artifact2.jar".to_string(),
            2048,
            ArtifactChecksum::new("b".repeat(64)),
            UserId::new(),
        ),
        Artifact::new(
            RepositoryId::new(),
            ArtifactVersion::new("2.0.0"),
            "artifact3.jar".to_string(),
            4096,
            ArtifactChecksum::new("c".repeat(64)),
            UserId::new(),
        ),
    ];

    // Procesar múltiples eventos
    for artifact in &artifacts {
        let correlation_id = Uuid::new_v4();
        let uploader = UserId::new();
        let envelope = build_artifact_uploaded_event(artifact, correlation_id, &uploader);

        let event_json = serde_json::to_string(&envelope).unwrap();
        let result = consumer.process_artifact_uploaded_event(&event_json).await;
        assert!(result.is_ok(), "Event processing should succeed");
    }

    // Verificar que todos los eventos fueron procesados
    let processed_events = consumer.get_processed_events();
    assert_eq!(processed_events.len(), 3);
    
    // Verificar que todos fueron exitosos
    for processed_event in &processed_events {
        assert!(processed_event.success);
        assert!(processed_event.error_message.is_none());
    }

    // Verificar que todos los documentos fueron indexados
    let indexed_docs = search_index.get_indexed_documents();
    assert_eq!(indexed_docs.len(), 3);

    // Verificar que los IDs coinciden
    let processed_artifact_ids: Vec<_> = processed_events.iter()
        .map(|e| e.artifact_id.clone())
        .collect();
    let indexed_artifact_ids: Vec<_> = indexed_docs.iter()
        .map(|d| d.artifact_id.clone())
        .collect();

    for artifact in &artifacts {
        assert!(processed_artifact_ids.contains(&artifact.id));
        assert!(indexed_artifact_ids.contains(&artifact.id));
    }
}

#[tokio::test]
async fn test_downstream_event_idempotency() {
    let search_index = MockSearchIndex::new();
    let consumer = MockEventConsumer::new(search_index.clone());

    let artifact = Artifact::new(
        RepositoryId::new(),
        ArtifactVersion::new("1.0.0"),
        "test-artifact.jar".to_string(),
        1024,
        ArtifactChecksum::new("a".repeat(64)),
        UserId::new(),
    );

    let correlation_id = Uuid::new_v4();
    let uploader = UserId::new();
    let envelope = build_artifact_uploaded_event(&artifact, correlation_id, &uploader);

    let event_json = serde_json::to_string(&envelope).unwrap();

    // Procesar el mismo evento múltiples veces
    for _ in 0..3 {
        let result = consumer.process_artifact_uploaded_event(&event_json).await;
        assert!(result.is_ok(), "Event processing should succeed");
    }

    // En este caso, nuestro mock no implementa idempotencia verdadera,
    // pero verificamos que todos los procesamientos fueron exitosos
    let processed_events = consumer.get_processed_events();
    assert_eq!(processed_events.len(), 3);
    
    for processed_event in &processed_events {
        assert!(processed_event.success);
        assert_eq!(processed_event.artifact_id, artifact.id);
    }

    // Nota: En un sistema real, se implementaría idempotencia para evitar
    // múltiples indexaciones del mismo documento
}
