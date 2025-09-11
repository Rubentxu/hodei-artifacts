// crates/artifact/src/features/upload_artifact/upload_progress/mod.rs

//! Feature para seguimiento del progreso de subidas de artefactos
//! Implementa tanto polling (REST) como WebSocket para tracking en tiempo real

pub mod dto;
pub mod ports;
pub mod service;
pub mod api;

// Re-exportar los componentes públicos
pub use dto::{UploadProgress, UploadStatus, UpdateProgressCommand, UploadProgressResponse, ReceivedChunksResponse, ReceivedChunkInfo};
pub use service::UploadProgressService;
pub use api::UploadProgressApi;
pub use ports::{ProgressStorage, ProgressEventPublisher, RealtimeNotifier, ProgressError, ProgressResult};