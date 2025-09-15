//! Feature para detección automática de Content-Type usando magic numbers
//! Implementa la historia de usuario 1.10 siguiendo patrones VSA

pub mod adapter;
pub mod api;
pub mod di;
pub mod dto;
pub mod error;
pub mod ports;

pub mod mocks;
mod use_case;

// Re-exportar los componentes públicos
pub use api::ContentTypeDetectionApi;
pub use di::ContentTypeDetectionDIContainer;
pub use dto::{
    ContentTypeDetectionResult, ContentTypeMismatch, DetectContentTypeCommand, MismatchSeverity,
};
pub use error::ContentTypeDetectionError;
pub use ports::ContentTypeDetector;
pub use use_case::ContentTypeDetectionUseCase;

#[cfg(test)]
mod use_case_test;
