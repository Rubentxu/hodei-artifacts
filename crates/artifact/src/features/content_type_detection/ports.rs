//! Puertos/abstracciones para la detección de Content-Type
//! Segregación de interfaces siguiendo principios SOLID

use async_trait::async_trait;
use bytes::Bytes;

use super::error::ContentTypeDetectionError;

/// Resultado de la detección de Content-Type
#[derive(Debug, Clone, PartialEq)]
pub struct ContentTypeDetectionResult {
    /// MIME type detectado mediante magic numbers
    pub detected_mime_type: String,
    /// MIME type proporcionado por el cliente (header Content-Type)
    pub client_provided_mime_type: Option<String>,
    /// Indica si hay discrepancia entre el detectado y el proporcionado
    pub has_mismatch: bool,
    /// Confianza en la detección (0.0 a 1.0)
    pub confidence: f32,
}

/// Trait para detectores de Content-Type
#[async_trait]
pub trait ContentTypeDetector: Send + Sync {
    /// Detecta el MIME type de un chunk de datos usando magic numbers
    async fn detect_from_bytes(
        &self,
        data: Bytes,
    ) -> Result<ContentTypeDetectionResult, ContentTypeDetectionError>;

    /// Detecta el MIME type de un archivo basado en su extensión (fallback)
    async fn detect_from_extension(
        &self,
        filename: &str,
    ) -> Result<ContentTypeDetectionResult, ContentTypeDetectionError>;

    /// Valida si el MIME type detectado es consistente con el proporcionado
    async fn validate_consistency(
        &self,
        detected: &str,
        provided: Option<&str>,
    ) -> Result<ContentTypeDetectionResult, ContentTypeDetectionError>;
}
