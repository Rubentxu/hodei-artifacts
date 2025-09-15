// crates/artifact/src/features/upload_artifact/upload_progress/mod.rs

//! Feature para seguimiento del progreso de subidas de artefactos
//! Implementa tanto polling (REST) como WebSocket para tracking en tiempo real

pub mod adapter;
pub mod di;
pub mod dto;
pub mod error;
pub mod mocks;
pub mod ports;
pub mod use_case;

// Re-exportar los componentes públicos
pub use di::UploadProgressDIContainer;
pub use dto::{
    ReceivedChunkInfo, ReceivedChunksResponse, UpdateProgressCommand, UploadProgress,
    UploadProgressResponse, UploadStatus,
};
pub use error::ProgressError;
pub use ports::{ProgressEventPublisher, ProgressResult, ProgressStorage, RealtimeNotifier};
pub use use_case::UploadProgressUseCase;
