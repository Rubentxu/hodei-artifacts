//! Tests de integración EVENT-2: Capturar JSON del publisher y validar esquema mínimo.
//!
//! Objetivo:
//! - Crear un mock publisher que capture los JSON enviados
//! - Validar que los eventos tengan el esquema mínimo requerido
//! - Verificar conformidad con DomainEventEnvelope y payloads específicos

use std::sync::{Arc, Mutex};
use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

use artifact::application::ports::ArtifactEventPublisher;
use artifact::domain::model::Artifact;
use artifact::error::ArtifactError;
use shared::domain::event::ArtifactDownloadRequestedEvent;
use shared::{ArtifactId, RepositoryId, UserId, IsoTimestamp};
use artifact::domain::model::{ArtifactChecksum, ArtifactVersion};

/// Mock publisher que captura los JSON enviados para validación de esquemas
#[derive(Debug, Clone)]
pub struct JsonCapturingPublisher {
    captured_events: Arc<Mutex<Vec<CapturedEvent>>>,
}

#[derive(Debug, Clone)]
pub struct CapturedEvent {
    pub topic: String,
    pub json_payload: String,
    pub parsed_value: Value,
}

impl JsonCapturingPublisher {
    pub fn new() -> Self {
        Self {
            captured_events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn get_captured_events(&self) -> Vec<CapturedEvent> {
        self.captured_events.lock().unwrap().clone()
    }

    pub fn clear_captured_events(&self) {
        self.captured_events.lock().unwrap().clear();
    }

    fn capture_event(&self, topic: &str, json_payload: &str) -> Result<(), ArtifactError> {
        let parsed_value: Value = serde_json::from_str(json_payload)
            .map_err(|e| ArtifactError::Event(format!("Invalid JSON: {}", e)))?;

        let event = CapturedEvent {
            topic: topic.to_string(),
            json_payload: json_payload.to_string(),
            parsed_value,
        };

        self.captured_events.lock().unwrap().push(event);
        Ok(())
    }
}

#[async_trait]
impl ArtifactEventPublisher for JsonCapturingPublisher {
    async fn publish_created(&self, artifact: &Artifact) -> Result<(), ArtifactError> {
        let json_payload = serde_json::to_string(artifact)
            .map_err(|e| ArtifactError::Event(format!("Serialization failed: {}", e)))?;
        
        self.capture_event("artifact_created", &json_payload)
    }

    async fn publish_download_requested(&self, event: &ArtifactDownloadRequestedEvent) -> Result<(), ArtifactError> {
        let json_payload = serde_json::to_string(event)
            .map_err(|e| ArtifactError::Event(format!("Serialization failed: {}", e)))?;
        
        self.capture_event("artifact_download_requested", &json_payload)
    }
}

/// Validadores de esquema para los diferentes tipos de eventos
pub struct EventSchemaValidator;

impl EventSchemaValidator {
    /// Valida esquema mínimo para DomainEventEnvelope
    pub fn validate_domain_event_envelope(json: &Value) -> Result<(), String> {
        // Campos obligatorios del envelope
        let required_fields = [
            "event_type", "event_id", "correlation_id", "timestamp", 
            "version", "source", "data"
        ];

        for field in &required_fields {
            if !json.get(field).is_some() {
                return Err(format!("Campo requerido faltante: {}", field));
            }
        }

        // Validaciones específicas de tipo
        if let Some(event_type) = json.get("event_type").and_then(|v| v.as_str()) {
            if !event_type.contains(".v") {
                return Err("event_type debe incluir versión (ej: ArtifactUploaded.v1)".to_string());
            }
        }

        if let Some(event_id) = json.get("event_id").and_then(|v| v.as_str()) {
            Uuid::parse_str(event_id)
                .map_err(|_| "event_id debe ser un UUID válido".to_string())?;
        }

        if let Some(correlation_id) = json.get("correlation_id").and_then(|v| v.as_str()) {
            Uuid::parse_str(correlation_id)
                .map_err(|_| "correlation_id debe ser un UUID válido".to_string())?;
        }

        Ok(())
    }

    /// Valida esquema específico para ArtifactUploaded
    pub fn validate_artifact_uploaded_payload(data: &Value) -> Result<(), String> {
        let required_fields = ["artifact_id", "repository_id", "uploader"];

        for field in &required_fields {
            if !data.get(field).is_some() {
                return Err(format!("Campo requerido en payload ArtifactUploaded: {}", field));
            }
        }

        // Validaciones opcionales de formato si están presentes
        if let Some(size_bytes) = data.get("size_bytes") {
            if !size_bytes.is_number() {
                return Err("size_bytes debe ser un número".to_string());
            }
        }

        if let Some(sha256) = data.get("sha256").and_then(|v| v.as_str()) {
            if sha256.len() != 64 {
                return Err("sha256 debe tener 64 caracteres hexadecimales".to_string());
            }
        }

        Ok(())
    }

    /// Valida esquema específico para ArtifactDownloadRequested
    pub fn validate_artifact_download_requested_payload(data: &Value) -> Result<(), String> {
        let required_fields = ["artifact_id", "user_id"];

        for field in &required_fields {
            if !data.get(field).is_some() {
                return Err(format!("Campo requerido en payload ArtifactDownloadRequested: {}", field));
            }
        }

        Ok(())
    }
}

// ===================== Tests =========================

#[tokio::test]
async fn test_artifact_created_json_schema_validation() {
    let publisher = JsonCapturingPublisher::new();
    
    // Crear artifact de prueba
    let artifact = Artifact::new(
        RepositoryId::new(),
        ArtifactVersion::new("1.0.0"),
        "test-artifact.jar".to_string(),
        1024,
        ArtifactChecksum::new("a".repeat(64)),
        UserId::new(),
    );

    // Publicar evento
    publisher.publish_created(&artifact).await.unwrap();

    // Verificar captura
    let captured = publisher.get_captured_events();
    assert_eq!(captured.len(), 1);
    
    let event = &captured[0];
    assert_eq!(event.topic, "artifact_created");

    // Validar que sea JSON válido y tenga estructura de Artifact
    let json = &event.parsed_value;
    
    // Verificar campos de Artifact
    assert!(json.get("id").is_some(), "Artifact debe tener id");
    assert!(json.get("repository_id").is_some(), "Artifact debe tener repository_id");
    assert!(json.get("version").is_some(), "Artifact debe tener version");
    assert!(json.get("file_name").is_some(), "Artifact debe tener file_name");
    assert!(json.get("size_bytes").is_some(), "Artifact debe tener size_bytes");
    assert!(json.get("checksum").is_some(), "Artifact debe tener checksum");
    assert!(json.get("created_by").is_some(), "Artifact debe tener created_by");
    assert!(json.get("created_at").is_some(), "Artifact debe tener created_at");

    println!("✓ Artifact JSON schema válido: {}", event.json_payload);
}

#[tokio::test]
async fn test_download_requested_event_json_schema_validation() {
    let publisher = JsonCapturingPublisher::new();
    
    // Crear evento de descarga de prueba usando las funciones build_event del dominio
    let artifact_id = ArtifactId::new();
    let user_id = UserId::new();
    let correlation_id = Uuid::new_v4();
    
    // Crear payload de download requested
    let payload = shared::domain::event::ArtifactDownloadRequested {
        artifact_id,
        user_id,
        user_agent: Some("test-client/1.0".to_string()),
        client_ip: Some("127.0.0.1".to_string()),
        requested_range: None,
    };

    // Crear envelope
    let event = shared::domain::event::DomainEventEnvelope::from_correlation(
        payload,
        correlation_id,
        None,
        Some("hodei-artifacts.artifact-retrieve".to_string()),
    );

    // Publicar evento
    publisher.publish_download_requested(&event).await.unwrap();

    // Verificar captura
    let captured = publisher.get_captured_events();
    assert_eq!(captured.len(), 1);
    
    let captured_event = &captured[0];
    assert_eq!(captured_event.topic, "artifact_download_requested");

    // Validar esquema DomainEventEnvelope
    let json = &captured_event.parsed_value;
    EventSchemaValidator::validate_domain_event_envelope(json)
        .expect("Event envelope debe tener esquema válido");

    // Verificar event_type específico
    let event_type = json.get("event_type").unwrap().as_str().unwrap();
    assert_eq!(event_type, "ArtifactDownloadRequested.v1");

    // Validar payload específico
    let data = json.get("data").expect("Event debe tener data");
    EventSchemaValidator::validate_artifact_download_requested_payload(data)
        .expect("Payload ArtifactDownloadRequested debe ser válido");

    println!("✓ ArtifactDownloadRequested JSON schema válido: {}", captured_event.json_payload);
}

#[tokio::test]
async fn test_multiple_events_capture_and_validation() {
    let publisher = JsonCapturingPublisher::new();
    
    // Crear múltiples eventos
    let artifact = Artifact::new(
        RepositoryId::new(),
        ArtifactVersion::new("2.0.0"),
        "multi-test.jar".to_string(),
        2048,
        ArtifactChecksum::new("b".repeat(64)),
        UserId::new(),
    );

    let download_payload = shared::domain::event::ArtifactDownloadRequested {
        artifact_id: ArtifactId::new(),
        user_id: UserId::new(),
        user_agent: None,
        client_ip: None,
        requested_range: Some("bytes=0-1023".to_string()),
    };

    let download_event = shared::domain::event::DomainEventEnvelope::new_root(
        download_payload,
        None,
    );

    // Publicar ambos eventos
    publisher.publish_created(&artifact).await.unwrap();
    publisher.publish_download_requested(&download_event).await.unwrap();

    // Verificar captura de múltiples eventos
    let captured = publisher.get_captured_events();
    assert_eq!(captured.len(), 2);

    // Verificar que cada evento tenga el topic correcto
    assert_eq!(captured[0].topic, "artifact_created");
    assert_eq!(captured[1].topic, "artifact_download_requested");

    // Verificar que ambos sean JSON válidos
    for event in &captured {
        assert!(event.parsed_value.is_object(), "Cada evento debe ser un objeto JSON válido");
    }

    println!("✓ Múltiples eventos capturados y validados correctamente");
}

#[tokio::test]
async fn test_json_schema_validation_detects_missing_fields() {
    // Test que verifica que el validador detecta campos faltantes
    let incomplete_envelope = serde_json::json!({
        "event_type": "ArtifactUploaded.v1",
        "event_id": "123e4567-e89b-12d3-a456-426614174000",
        // Falta correlation_id, timestamp, version, source, data
    });

    let result = EventSchemaValidator::validate_domain_event_envelope(&incomplete_envelope);
    assert!(result.is_err(), "Debe detectar campos faltantes");
    
    let error_msg = result.unwrap_err();
    assert!(error_msg.contains("correlation_id"), "Debe detectar correlation_id faltante");

    println!("✓ Validación detecta campos faltantes: {}", error_msg);
}

#[tokio::test]
async fn test_invalid_event_type_format_detection() {
    // Test que verifica validación de formato event_type
    let invalid_envelope = serde_json::json!({
        "event_type": "ArtifactUploaded", // Sin versión
        "event_id": "123e4567-e89b-12d3-a456-426614174000",
        "correlation_id": "123e4567-e89b-12d3-a456-426614174001",
        "timestamp": "2025-01-25T10:00:00Z",
        "version": "v1",
        "source": "test",
        "data": {}
    });

    let result = EventSchemaValidator::validate_domain_event_envelope(&invalid_envelope);
    assert!(result.is_err(), "Debe detectar event_type sin versión");
    
    let error_msg = result.unwrap_err();
    assert!(error_msg.contains("versión"), "Debe mencionar problema de versión");

    println!("✓ Validación detecta event_type inválido: {}", error_msg);
}

#[tokio::test]
async fn test_clear_captured_events_functionality() {
    let publisher = JsonCapturingPublisher::new();
    
    // Crear y publicar un evento
    let artifact = Artifact::new(
        RepositoryId::new(),
        ArtifactVersion::new("1.0.0"),
        "clear-test.jar".to_string(),
        512,
        ArtifactChecksum::new("c".repeat(64)),
        UserId::new(),
    );

    publisher.publish_created(&artifact).await.unwrap();
    assert_eq!(publisher.get_captured_events().len(), 1);

    // Limpiar eventos capturados
    publisher.clear_captured_events();
    assert_eq!(publisher.get_captured_events().len(), 0);

    println!("✓ Funcionalidad de limpiar eventos funciona correctamente");
}
