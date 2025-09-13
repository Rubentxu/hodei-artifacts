//! Errores específicos para la detección de Content-Type

use thiserror::Error;

/// Errores de detección de Content-Type
#[derive(Debug, Clone, Error)]
pub enum ContentTypeDetectionError {
    /// No se pudo detectar el tipo MIME
    #[error("No se pudo detectar el tipo MIME del contenido")]
    DetectionFailed,
    
    /// Datos insuficientes para la detección
    #[error("Datos insuficientes para la detección de tipo MIME (mínimo {0} bytes requeridos)")]
    InsufficientData(usize),
    
    /// Tipo MIME no soportado
    #[error("Tipo MIME no soportado: {0}")]
    UnsupportedMimeType(String),
    
    /// Error interno del detector
    #[error("Error interno del detector de Content-Type: {0}")]
    InternalError(String),
    
    /// Error de IO
    #[error("Error de IO durante la detección: {0}")]
    IoError(String),
}