//! Configuración de Dependency Injection para detección de Content-Type

use super::adapter::InferContentTypeDetector;
use super::ports::ContentTypeDetector;
use super::use_case::ContentTypeDetectionUseCase;
use std::sync::Arc;

/// Contenedor DI para la feature de detección de Content-Type
#[derive(Clone)]
pub struct ContentTypeDetectionDIContainer {
    pub use_case: Arc<ContentTypeDetectionUseCase>,
}

impl ContentTypeDetectionDIContainer {
    /// Crea un nuevo contenedor DI con las implementaciones por defecto
    pub fn new() -> Self {
        let detector = Arc::new(InferContentTypeDetector::new()) as Arc<dyn ContentTypeDetector>;
        let use_case = Arc::new(ContentTypeDetectionUseCase::new(detector));

        Self { use_case }
    }

    /// Crea un contenedor DI con un detector personalizado
    pub fn with_detector(detector: Arc<dyn ContentTypeDetector>) -> Self {
        let use_case = Arc::new(ContentTypeDetectionUseCase::new(detector));

        Self { use_case }
    }
}

impl Default for ContentTypeDetectionDIContainer {
    fn default() -> Self {
        Self::new()
    }
}
