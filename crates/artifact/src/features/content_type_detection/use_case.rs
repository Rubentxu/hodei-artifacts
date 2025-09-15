//! Caso de uso principal para detección de Content-Type
//! Cumple con VSA: orquesta la lógica de la feature y delega a puertos segregados.
//! No contiene lógica de infraestructura ni lógica transversal.
//! Este archivo implementa el caso de uso de la feature content_type_detection.

use super::error::ContentTypeDetectionError;
use super::ports::{ContentTypeDetectionResult, ContentTypeDetector};
use bytes::Bytes;
use std::sync::Arc;

/// Caso de uso principal de detección de Content-Type
/// - Orquesta la detección y validación de Content-Type.
/// - Depende solo de puertos definidos en la feature (no acopla a infraestructura).
/// - No expone detalles de implementación ni lógica transversal.
#[derive(Clone)]
pub struct ContentTypeDetectionUseCase {
    detector: Arc<dyn ContentTypeDetector>,
}

impl ContentTypeDetectionUseCase {
    /// Inyección de dependencia por trait (puerto segregado).
    pub fn new(detector: Arc<dyn ContentTypeDetector>) -> Self {
        Self { detector }
    }

    /// Caso de uso principal: Detecta el Content-Type de un artefacto usando magic numbers como primario
    /// y extensión como fallback. Valida consistencia con el header proporcionado.
    pub async fn detect_content_type(
        &self,
        data: Bytes,
        filename: Option<&str>,
        client_provided_mime_type: Option<&str>,
    ) -> Result<ContentTypeDetectionResult, ContentTypeDetectionError> {
        // Intentar detección mediante magic numbers (método preferido)
        let magic_result = self.detector.detect_from_bytes(data.clone()).await;

        let detected_mime_type = match magic_result {
            Ok(result) => result.detected_mime_type,
            Err(_) => {
                // Fallback a detección por extensión si magic numbers falla
                if let Some(filename) = filename {
                    let extension_result = self.detector.detect_from_extension(filename).await?;
                    extension_result.detected_mime_type
                } else {
                    return Err(ContentTypeDetectionError::DetectionFailed);
                }
            }
        };

        // Validar consistencia con el header proporcionado
        self.detector
            .validate_consistency(&detected_mime_type, client_provided_mime_type)
            .await
    }

    /// Detecta Content-Type solo mediante magic numbers (sin fallback)
    pub async fn detect_from_magic_numbers(
        &self,
        data: Bytes,
        client_provided_mime_type: Option<&str>,
    ) -> Result<ContentTypeDetectionResult, ContentTypeDetectionError> {
        let result = self.detector.detect_from_bytes(data).await?;

        // Validar consistencia
        self.detector
            .validate_consistency(&result.detected_mime_type, client_provided_mime_type)
            .await
    }

    /// Detecta Content-Type solo mediante extensión de archivo
    pub async fn detect_from_extension_only(
        &self,
        filename: &str,
        client_provided_mime_type: Option<&str>,
    ) -> Result<ContentTypeDetectionResult, ContentTypeDetectionError> {
        let result = self.detector.detect_from_extension(filename).await?;

        // Validar consistencia
        self.detector
            .validate_consistency(&result.detected_mime_type, client_provided_mime_type)
            .await
    }
}
