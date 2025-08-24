//! Eventos de dominio alineados con catálogo (docs/evento-catalog.md) y modelo (docs/domain.md).
//!
//! Objetivos MVP:
//! - Envelope estándar con: eventType (incl. versión), eventId, correlationId, causationId?, timestamp, source, metadata, data.
//! - Payloads mínimos necesarios: ArtifactUploaded, ArtifactDownloadRequested.
//! - Versionado simple v1 (string "v1" en eventType: p.ej. "ArtifactUploaded.v1").
//! - Event-Carried State: incluir campos críticos que evitan lecturas adicionales inmediatas.
//!
//! Evolución futura:
//! - Ampliar metadata (traceId/spanId/tenantId) y otros eventos del catálogo.
//! - Registro de esquemas (avro/json schema) para validación en borde.

use serde::{Serialize, Deserialize};
use uuid::Uuid;
use std::collections::HashMap;
use crate::domain::model::{ArtifactId, RepositoryId, UserId, IsoTimestamp};

/// Versión de schema actual (se concatenate en eventType como sufijo .v1).
pub const EVENT_VERSION_V1: &str = "v1";
/// Fuente estándar (podrá parametrizarse por slice al publicar).
pub const DEFAULT_EVENT_SOURCE: &str = "hodei-artifacts.core";

/// Trait para payloads de eventos (solo datos de negocio).
pub trait DomainEventPayload {
    /// Nombre base sin sufijo de versión (ej: "ArtifactUploaded").
    fn base_type(&self) -> &'static str;
    /// Id lógico del agregado raíz productor (para particionamiento/routing).
    fn aggregate_id(&self) -> String;
    /// Versión semántica del payload (sin prefijo "v").
    fn version(&self) -> &'static str { EVENT_VERSION_V1 }
    /// eventType final (ej: ArtifactUploaded.v1).
    fn full_event_type(&self) -> String {
        format!("{}.{}", self.base_type(), self.version())
    }
}

/// Legacy compatibility trait (mantiene compilación de crates que aún usan DomainEvent).
/// Implementado para cada payload. Nuevo código debería usar DomainEventPayload + DomainEventEnvelope.
pub trait DomainEvent {
    fn event_type(&self) -> &'static str;
    fn aggregate_id(&self) -> String;
    fn schema_version(&self) -> &'static str { "1.0" }
}

/// Envelope genérico para publicación / serialización / trazabilidad.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainEventEnvelope<T: DomainEventPayload> {
    pub event_type: String,
    pub event_id: Uuid,
    pub correlation_id: Uuid,
    pub causation_id: Option<Uuid>,
    pub timestamp: IsoTimestamp,
    pub version: String,
    pub source: String,
    pub data: T,
    /// Metadata técnica / cross-cutting (trace, span, userId, tenantId, etc.).
    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub metadata: HashMap<String, String>,
}

impl<T: DomainEventPayload> DomainEventEnvelope<T> {
    /// Crea un envelope raíz (no derivado de otro evento).
    pub fn new_root(data: T, source: Option<String>) -> Self {
        let event_id = Uuid::new_v4();
        let correlation_id = event_id; // correlación inicia igual que event_id raíz
        Self {
            event_type: data.full_event_type(),
            event_id,
            correlation_id,
            causation_id: None,
            timestamp: IsoTimestamp::now(),
            version: data.version().to_string(),
            source: source.unwrap_or_else(|| DEFAULT_EVENT_SOURCE.to_string()),
            data,
            metadata: HashMap::new(),
        }
    }

    /// Crea envelope correlacionado (derivado de flujo existente).
    pub fn from_correlation(data: T, correlation_id: Uuid, causation_id: Option<Uuid>, source: Option<String>) -> Self {
        Self {
            event_type: data.full_event_type(),
            event_id: Uuid::new_v4(),
            correlation_id,
            causation_id,
            timestamp: IsoTimestamp::now(),
            version: data.version().to_string(),
            source: source.unwrap_or_else(|| DEFAULT_EVENT_SOURCE.to_string()),
            data,
            metadata: HashMap::new(),
        }
    }

    /// Añade un par metadata (sobrescribe si existe).
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

// ===================== Payloads =========================

/// Payload para evento ArtifactUploaded.v1
///
/// Campos alineados con catálogo (subset):
/// - repository: path o identificador lógico (usamos repository_id por ahora, path vendrá del agregado Repository).
/// - name/version vendrán en fase posterior tras modelar ArtifactCoordinates; MVP: artifact_id + repository_id suficientes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactUploaded {
    pub artifact_id: ArtifactId,
    pub repository_id: RepositoryId,
    pub uploader: UserId,
    pub sha256: Option<String>,
    pub size_bytes: Option<u64>,
    pub media_type: Option<String>,
    pub upload_time_ms: Option<u32>,
}

impl DomainEventPayload for ArtifactUploaded {
    fn base_type(&self) -> &'static str { "ArtifactUploaded" }
    fn aggregate_id(&self) -> String { self.artifact_id.0.to_string() }
}

impl DomainEvent for ArtifactUploaded {
    fn event_type(&self) -> &'static str { "ArtifactUploaded" }
    fn aggregate_id(&self) -> String { self.artifact_id.0.to_string() }
}

/// Payload para evento ArtifactDownloadRequested.v1
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactDownloadRequested {
    pub artifact_id: ArtifactId,
    pub user_id: UserId,
    pub user_agent: Option<String>,
    pub client_ip: Option<String>,
    pub requested_range: Option<String>,
}

impl DomainEventPayload for ArtifactDownloadRequested {
    fn base_type(&self) -> &'static str { "ArtifactDownloadRequested" }
    fn aggregate_id(&self) -> String { self.artifact_id.0.to_string() }
}

impl DomainEvent for ArtifactDownloadRequested {
    fn event_type(&self) -> &'static str { "ArtifactDownloadRequested" }
    fn aggregate_id(&self) -> String { self.artifact_id.0.to_string() }
}

// ===================== Helpers específicos =========================

/// Atajo: construir envelope raíz para ArtifactUploaded.
pub fn new_artifact_uploaded_root(payload: ArtifactUploaded) -> DomainEventEnvelope<ArtifactUploaded> {
    DomainEventEnvelope::new_root(payload, Some("hodei-artifacts.artifact-ingest".to_string()))
}

/// Atajo: construir envelope raíz para ArtifactDownloadRequested.
pub fn new_artifact_download_requested_root(payload: ArtifactDownloadRequested) -> DomainEventEnvelope<ArtifactDownloadRequested> {
    DomainEventEnvelope::new_root(payload, Some("hodei-artifacts.artifact-retrieve".to_string()))
}

// ===================== Aliases de compatibilidad =========================
/// Alias para mantener referencias existentes en documentación y puertos.
pub type ArtifactUploadedEvent = DomainEventEnvelope<ArtifactUploaded>;
pub type ArtifactDownloadRequestedEvent = DomainEventEnvelope<ArtifactDownloadRequested>;
