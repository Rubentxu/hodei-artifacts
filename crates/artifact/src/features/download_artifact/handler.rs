use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use shared::{ArtifactId, UserId};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    application::ports::{ArtifactRepository, ArtifactStorage, ArtifactEventPublisher},
    error::ArtifactError,
};
use super::{
    query::{GetArtifactQuery, GetArtifactResponse, DownloadMethod},
    logic::handle_get_artifact,
};

#[derive(Debug, Deserialize)]
pub struct DownloadQueryParams {
    #[serde(default)]
    pub presigned: bool,
    #[serde(default = "default_expires_secs")]
    pub expires_secs: u64,
}

fn default_expires_secs() -> u64 {
    3600 // 1 hora por defecto
}

#[derive(Debug, Serialize)]
pub struct DownloadResponse {
    pub artifact_id: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub media_type: Option<String>,
    pub checksum: String,
    #[serde(flatten)]
    pub download_info: DownloadInfo,
}

#[derive(Debug, Serialize)]
#[serde(tag = "download_type")]
pub enum DownloadInfo {
    #[serde(rename = "presigned")]
    PresignedUrl { url: String, expires_at: String },
    #[serde(rename = "direct")]
    Direct { content_base64: String },
}

pub async fn download_artifact_handler(
    Path(artifact_id): Path<String>,
    Query(params): Query<DownloadQueryParams>,
    headers: HeaderMap,
    State(app_state): State<Arc<AppState>>,
) -> Result<Response, ArtifactError> {
    // Extraer información del usuario desde headers (placeholder para ABAC)
    let user_id = headers
        .get("x-user-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("anonymous")
        .to_string();

    let user_agent = headers
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    // Nota: client_ip se obtendría normalmente de proxy headers
    let client_ip = headers
        .get("x-forwarded-for")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    // Parse UUID strings to domain types
    let artifact_uuid = Uuid::parse_str(&artifact_id)
        .map_err(|_| ArtifactError::NotFound)?;
    let user_uuid = Uuid::parse_str(&user_id)
        .map_err(|_| ArtifactError::NotFound)?;

    let artifact_id = ArtifactId(artifact_uuid);
    let user_id = UserId(user_uuid);

    let query = GetArtifactQuery::new(artifact_id, user_id)
        .with_presigned(params.expires_secs)
        .with_user_agent(user_agent.unwrap_or_default())
        .with_client_ip(client_ip.unwrap_or_default());

    let query = if params.presigned {
        query.with_presigned(params.expires_secs)
    } else {
        query
    };

    let response = handle_get_artifact(
        query,
        app_state.artifact_repository.as_ref(),
        app_state.artifact_storage.as_ref(),
        app_state.event_publisher.as_ref(),
    )
    .await?;

    let download_response = map_to_http_response(response);

    Ok(Json(download_response).into_response())
}

fn map_to_http_response(response: GetArtifactResponse) -> DownloadResponse {
    let download_info = match response.download_method {
        DownloadMethod::PresignedUrl { url, expires_at } => {
            DownloadInfo::PresignedUrl { url, expires_at }
        }
        DownloadMethod::Direct { content } => {
            let content_base64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, content);
            DownloadInfo::Direct { content_base64 }
        }
    };

    DownloadResponse {
        artifact_id: response.artifact_id.0.to_string(),
        file_name: response.file_name,
        size_bytes: response.size_bytes,
        media_type: response.media_type,
        checksum: response.checksum,
        download_info,
    }
}

// Placeholder para el AppState - esto se definiría en el nivel de aplicación
pub struct AppState {
    pub artifact_repository: Arc<dyn ArtifactRepository>,
    pub artifact_storage: Arc<dyn ArtifactStorage>,
    pub event_publisher: Arc<dyn ArtifactEventPublisher>,
}

impl ArtifactError {
    pub fn into_response(self) -> Response {
        let (status, message) = match self {
            ArtifactError::NotFound => (StatusCode::NOT_FOUND, "Artifact no encontrado".to_string()),
            ArtifactError::Storage(msg) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Error de almacenamiento: {}", msg)),
            ArtifactError::Event(msg) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Error de evento: {}", msg)),
            ArtifactError::Repository(msg) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Error de repositorio: {}", msg)),
            ArtifactError::Duplicate => (StatusCode::CONFLICT, "Artifact duplicado".to_string()),
        };

        let body = serde_json::json!({
            "error": message,
            "status": status.as_u16()
        });

        (status, Json(body)).into_response()
    }
}

impl IntoResponse for ArtifactError {
    fn into_response(self) -> Response {
        self.into_response()
    }
}
