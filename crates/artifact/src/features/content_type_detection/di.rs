//! Configuración de Dependency Injection para detección de Content-Type

use super::adapter::InferContentTypeDetector;
use super::ports::ContentTypeDetector;
use super::service::ContentTypeDetectionService;
use std::sync::Arc;

/// Contenedor DI para la feature de detección de Content-Type
#[derive(Clone)]
pub struct ContentTypeDetectionDIContainer {
    pub service: Arc<ContentTypeDetectionService>,
}

impl ContentTypeDetectionDIContainer {
    /// Crea un nuevo contenedor DI con las implementaciones por defecto
    pub fn new() -> Self {
        let detector = Arc::new(InferContentTypeDetector::new()) as Arc<dyn ContentTypeDetector>;
        let service = Arc::new(ContentTypeDetectionService::new(detector));
        
        Self { service }
    }
    
    /// Crea un contenedor DI con un detector personalizado
    pub fn with_detector(detector: Arc<dyn ContentTypeDetector>) -> Self {
        let service = Arc::new(ContentTypeDetectionService::new(detector));
        
        Self { service }
    }
}

impl Default for ContentTypeDetectionDIContainer {
    fn default() -> Self {
        Self::new()
    }
}