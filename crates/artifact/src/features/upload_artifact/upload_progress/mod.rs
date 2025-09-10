//! Feature para seguimiento del progreso de subidas de artefactos
//! Implementa tanto polling (REST) como WebSocket para tracking en tiempo real

pub mod ports;
pub mod dto;
pub mod service;
pub mod api;
pub mod di;
pub mod adapter;

// Re-exportar los componentes públicos
pub use dto::{UploadProgress, UploadProgressResponse, UpdateProgressCommand, UploadStatus};
pub use service::UploadProgressService;
pub use api::UploadProgressApi;
pub use di::UploadProgressDIContainer;
pub use ports::{ProgressStorage, ProgressEventPublisher, RealtimeNotifier, ProgressError, ProgressResult};