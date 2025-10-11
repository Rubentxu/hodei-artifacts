//! DTOs y estructuras de datos para la detección de Content-Type

use serde::{Deserialize, Serialize};
use kernel::domain::entity::ActionTrait;
use kernel::domain::value_objects::ServiceName;

/// Resultado de la detección de Content-Type para uso en APIs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentTypeDetectionResult {
    /// MIME type detectado mediante magic numbers
    pub detected_mime_type: String,

    /// MIME type proporcionado por el cliente (header Content-Type)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_provided_mime_type: Option<String>,

    /// Indica si hay discrepancia entre el detectado y el proporcionado
    pub has_mismatch: bool,

    /// Confianza en la detección (0.0 a 1.0)
    pub confidence: f32,
}

/// Información sobre discrepancia de Content-Type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentTypeMismatch {
    /// MIME type detectado
    pub detected: String,

    /// MIME type proporcionado
    pub provided: String,

    /// Nivel de severidad de la discrepancia
    pub severity: MismatchSeverity,

    /// Recomendación de acción
    pub recommendation: String,
}

/// Nivel de severidad de la discrepancia
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum MismatchSeverity {
    /// Discrepancia menor (ej: text/plain vs application/octet-stream)
    Low,
    /// Discrepancia moderada (ej: image/jpeg vs application/pdf)
    Medium,
    /// Discrepancia crítica (ej: application/java-archive vs text/plain)
    High,
}

/// Comando para forzar la detección de Content-Type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectContentTypeCommand {
    /// Datos del artefacto (primeros bytes)
    pub data: Vec<u8>,

    /// Nombre del archivo (opcional, para detección por extensión)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,

    /// Content-Type proporcionado por el cliente (opcional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_content_type: Option<String>,
}

impl ActionTrait for DetectContentTypeCommand {
    fn name() -> &'static str {
        "DetectContentType"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("artifact").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Artifact::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Artifact::Package".to_string()
    }
}
