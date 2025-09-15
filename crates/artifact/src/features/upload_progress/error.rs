use serde::Serialize;
use thiserror::Error;

/// Error types for upload progress tracking
#[derive(Debug, Error)]
pub enum ProgressError {
    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Access denied: {0}")]
    AccessDenied(String),

    #[error("Invalid chunk number: {0}")]
    InvalidChunkNumber(usize),

    #[error("Chunk not found: {0}")]
    ChunkNotFound(String),

    #[error("Assembly error: {0}")]
    AssemblyError(String),
}

/// Response de error estandarizado para progreso de subidas
#[derive(Debug, Serialize)]
pub struct ProgressErrorResponse {
    error: String,
    code: String,
    message: String,
}

impl ProgressErrorResponse {
    pub fn not_found() -> Self {
        Self {
            error: "NOT_FOUND".to_string(),
            code: "404".to_string(),
            message: "Upload session not found".to_string(),
        }
    }

    pub fn unauthorized() -> Self {
        Self {
            error: "UNAUTHORIZED".to_string(),
            code: "403".to_string(),
            message: "Access denied to upload progress".to_string(),
        }
    }

    pub fn internal_error() -> Self {
        Self {
            error: "INTERNAL_ERROR".to_string(),
            code: "500".to_string(),
            message: "Internal server error".to_string(),
        }
    }

    pub fn bad_request(message: &str) -> Self {
        Self {
            error: "BAD_REQUEST".to_string(),
            code: "400".to_string(),
            message: message.to_string(),
        }
    }
}
