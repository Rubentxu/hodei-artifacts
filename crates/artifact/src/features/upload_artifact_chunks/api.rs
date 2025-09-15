use axum::{http::StatusCode, response::IntoResponse, Json};
use bytes::Bytes;
use std::sync::Arc;
use tracing::{error, info};

use super::{
    dto::*, error::ChunkedUploadError, ports::ChunkedUploadProgress, use_case::ChunkedUploadUseCase,
};

/// Punto de entrada API para chunked uploads
pub struct ChunkedUploadEndpoint {
    use_case: Arc<ChunkedUploadUseCase>,
}

impl ChunkedUploadEndpoint {
    pub fn new(use_case: Arc<ChunkedUploadUseCase>) -> Self {
        Self { use_case }
    }

    /// Iniciar un nuevo upload por chunks
    pub async fn initiate_upload(
        &self,
        command: InitiateChunkedUploadCommand,
    ) -> Result<InitiateChunkedUploadResult, ChunkedUploadError> {
        info!(
            "Initiating chunked upload for package: {}",
            command.package_hrn
        );

        let result = self.use_case.initiate_upload(command).await;

        result.map_err(|e| {
            error!("Failed to initiate chunked upload: {}", e);
            e
        })
    }

    /// Subir un chunk
    pub async fn upload_chunk(
        &self,
        session_id: String,
        chunk_number: u32,
        chunk_data: Bytes,
        chunk_checksum: String,
    ) -> Result<UploadChunkResult, ChunkedUploadError> {
        info!(
            "Uploading chunk {} for session {}",
            chunk_number, session_id
        );

        let command = UploadChunkCommand {
            session_id,
            chunk_number,
            chunk_data,
            chunk_checksum,
        };

        let result = self.use_case.upload_chunk(command).await;

        result.map_err(|e| {
            error!("Failed to upload chunk: {}", e);
            e
        })
    }

    /// Completar un upload por chunks
    pub async fn complete_upload(
        &self,
        session_id: String,
        final_checksum: String,
    ) -> Result<CompleteChunkedUploadResult, ChunkedUploadError> {
        info!("Completing chunked upload for session {}", session_id);

        let command = CompleteChunkedUploadCommand {
            session_id,
            final_checksum,
        };

        let result = self.use_case.complete_upload(command).await;

        result.map_err(|e| {
            error!("Failed to complete chunked upload: {}", e);
            e
        })
    }

    /// Abortar un upload por chunks
    pub async fn abort_upload(
        &self,
        session_id: String,
        reason: Option<String>,
    ) -> Result<(), ChunkedUploadError> {
        info!("Aborting chunked upload for session {}", session_id);

        let command = AbortChunkedUploadCommand { session_id, reason };

        let result = self.use_case.abort_upload(command).await;

        result.map_err(|e| {
            error!("Failed to abort chunked upload: {}", e);
            e
        })
    }

    /// Obtener progreso de upload
    pub async fn get_upload_progress(
        &self,
        session_id: &str,
    ) -> Result<ChunkedUploadProgress, ChunkedUploadError> {
        self.use_case.get_upload_progress(session_id).await
    }

    /// Limpiar sesiones expiradas
    pub async fn cleanup_expired_sessions(&self) -> Result<u32, ChunkedUploadError> {
        self.use_case.cleanup_expired_sessions().await
    }
}

/// Implementación de IntoResponse para ChunkedUploadError
impl IntoResponse for ChunkedUploadError {
    fn into_response(self) -> axum::response::Response {
        let (status, error_message) = match self {
            ChunkedUploadError::InvalidSession(msg) => (StatusCode::BAD_REQUEST, msg),
            ChunkedUploadError::SessionNotFound(msg) => (StatusCode::NOT_FOUND, msg),
            ChunkedUploadError::SessionExpired(msg) => (StatusCode::BAD_REQUEST, msg),
            ChunkedUploadError::InvalidChunkNumber(msg) => (StatusCode::BAD_REQUEST, msg),
            ChunkedUploadError::ChunkSizeMismatch { .. } => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            ChunkedUploadError::ChunkChecksumFailed(msg) => (StatusCode::BAD_REQUEST, msg),
            ChunkedUploadError::FinalChecksumFailed { .. } => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            ChunkedUploadError::SessionAlreadyCompleted(msg) => (StatusCode::CONFLICT, msg),
            ChunkedUploadError::SessionAborted(msg) => (StatusCode::CONFLICT, msg),
            ChunkedUploadError::StorageError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            ChunkedUploadError::RepositoryError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            ChunkedUploadError::EventError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            ChunkedUploadError::IoError(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            ChunkedUploadError::SerializationError(msg) => (StatusCode::BAD_REQUEST, msg),
            ChunkedUploadError::ValidationError(msg) => (StatusCode::BAD_REQUEST, msg),
        };

        let body = Json(serde_json::json!({
            "error": error_message,
        }));

        (status, body).into_response()
    }
}
