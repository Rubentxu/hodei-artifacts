//! Feature para detección automática de Content-Type usando magic numbers
//! Implementa la historia de usuario 1.10 siguiendo patrones VSA

pub mod adapter;
pub mod dto;
pub mod ports;
pub mod service;
pub mod api;
pub mod di;
pub mod error;

// Re-exportar los componentes públicos
pub use dto::{ContentTypeDetectionResult, ContentTypeMismatch, DetectContentTypeCommand, DetectionMethod, MismatchSeverity};
pub use service::ContentTypeDetectionService;
pub use api::ContentTypeDetectionApi;
pub use ports::ContentTypeDetector;
pub use error::ContentTypeDetectionError;
pub use di::ContentTypeDetectionDIContainer;