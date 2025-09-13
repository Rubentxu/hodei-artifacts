pub mod error;
pub mod dto;
pub mod ports;
pub mod use_case;
pub mod adapter;
pub mod api;
pub mod di;
pub mod mocks;

// Tests en archivos separados (sin #[cfg(test)])
pub mod use_case_test;
pub mod api_test;

// Exportar solo lo necesario
pub use di::ChunkedUploadDIContainer;
pub use api::ChunkedUploadEndpoint;
pub use dto::{
    InitiateChunkedUploadCommand,
    UploadChunkCommand,
    CompleteChunkedUploadCommand,
    AbortChunkedUploadCommand,
    InitiateChunkedUploadResult,
    UploadChunkResult,
    CompleteChunkedUploadResult,
};

