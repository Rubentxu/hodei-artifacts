//! Eventos de dominio genéricos compartidos entre bounded contexts cuando sea seguro.
//! Eventos específicos deben vivir en el crate dueño del agregado.
use serde::{Serialize, Deserialize};
use crate::domain::model::{ArtifactId, RepositoryId, UserId, IsoTimestamp};

/// Trait base para todos los eventos de dominio
///
/// Siguiendo principios Event-Driven Architecture:
/// - Los eventos representan hechos que ocurrieron en el pasado
/// - Son inmutables y nombrados en pasado
/// - Contienen toda la información necesaria para procesamiento
pub trait DomainEvent {
    /// Tipo de evento para routing y processing
    fn event_type(&self) -> &'static str;

    /// ID del agregado que generó el evento
    fn aggregate_id(&self) -> String;

    /// Versión del esquema del evento (para evolución)
    fn schema_version(&self) -> &'static str {
        "1.0"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactUploadedEvent {
    pub artifact_id: ArtifactId,
    pub repository_id: RepositoryId,
    pub uploaded_by: UserId,
    pub occurred_at: IsoTimestamp,
}

impl DomainEvent for ArtifactUploadedEvent {
    fn event_type(&self) -> &'static str {
        "ArtifactUploaded"
    }

    fn aggregate_id(&self) -> String {
        self.artifact_id.0.to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactDownloadRequestedEvent {
    pub artifact_id: ArtifactId,
    pub requested_by: UserId,
    pub occurred_at: IsoTimestamp,
}

impl DomainEvent for ArtifactDownloadRequestedEvent {
    fn event_type(&self) -> &'static str {
        "ArtifactDownloadRequested"
    }

    fn aggregate_id(&self) -> String {
        self.artifact_id.0.to_string()
    }
}
