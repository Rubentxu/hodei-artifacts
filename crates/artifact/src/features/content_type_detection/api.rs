//! API endpoints para detección de Content-Type

use axum::{
    extract::{Json, Multipart, State},
    http::StatusCode,
    response::IntoResponse,
};
use bytes::Bytes;
use std::sync::Arc;
use tracing::{debug, info, warn};

use super::{
    dto::{ContentTypeDetectionResult, DetectContentTypeCommand},
    error::ContentTypeDetectionError,
    use_case::ContentTypeDetectionUseCase,
};

/// API endpoints para detección de Content-Type
#[derive(Clone)]
pub struct ContentTypeDetectionApi {
    pub use_case: Arc<ContentTypeDetectionUseCase>,
}

impl ContentTypeDetectionApi {
    pub fn new(use_case: Arc<ContentTypeDetectionUseCase>) -> Self {
        Self { use_case }
    }

    /// Endpoint para detectar Content-Type desde multipart form data
    pub async fn detect_from_multipart(
        State(api): State<Self>,
        mut multipart: Multipart,
    ) -> impl IntoResponse {
        let mut data = Bytes::new();
        let mut filename = None;
        let mut client_content_type = None;

        while let Some(field) = multipart.next_field().await.unwrap() {
            let field_name = field.name().unwrap_or("");

            match field_name {
                "file" => {
                    filename = field.file_name().map(|s| s.to_string());
                    client_content_type = field.content_type().map(|s| s.to_string());

                    if let Ok(field_data) = field.bytes().await {
                        // Tomar solo los primeros 4KB para detección (suficiente para magic numbers)
                        data = field_data.slice(0..std::cmp::min(4096, field_data.len()));
                    }
                }
                _ => {}
            }
        }

        if data.is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "CONTENT_TYPE_DETECTION_ERROR".to_string(),
                    message: "No se proporcionaron datos para análisis".to_string(),
                }),
            )
                .into_response();
        }

        match api
            .use_case
            .detect_content_type(data, filename.as_deref(), client_content_type.as_deref())
            .await
        {
            Ok(result) => {
                info!(
                    detected = %result.detected_mime_type,
                    provided = ?client_content_type,
                    has_mismatch = result.has_mismatch,
                    "Content-Type detectado exitosamente"
                );

                let api_result = ContentTypeDetectionResult {
                    detected_mime_type: result.detected_mime_type,
                    client_provided_mime_type: client_content_type,
                    has_mismatch: result.has_mismatch,
                    confidence: result.confidence,
                };

                Json(api_result).into_response()
            }
            Err(error) => {
                warn!(error = %error, "Error en detección de Content-Type");

                let status_code = match error {
                    ContentTypeDetectionError::InsufficientData(_) => StatusCode::BAD_REQUEST,
                    ContentTypeDetectionError::DetectionFailed => StatusCode::UNPROCESSABLE_ENTITY,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };

                (
                    status_code,
                    Json(ErrorResponse {
                        error: "CONTENT_TYPE_DETECTION_ERROR".to_string(),
                        message: error.to_string(),
                    }),
                )
                    .into_response()
            }
        }
    }

    /// Endpoint para detectar Content-Type desde JSON command
    pub async fn detect_from_json(
        State(api): State<Self>,
        Json(command): Json<DetectContentTypeCommand>,
    ) -> impl IntoResponse {
        if command.data.is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "CONTENT_TYPE_DETECTION_ERROR".to_string(),
                    message: "Datos vacíos para análisis".to_string(),
                }),
            )
                .into_response();
        }

        let data = Bytes::from(command.data);

        match api
            .use_case
            .detect_content_type(
                data,
                command.filename.as_deref(),
                command.client_content_type.as_deref(),
            )
            .await
        {
            Ok(result) => {
                debug!(
                    detected = %result.detected_mime_type,
                    provided = ?command.client_content_type,
                    "Content-Type detectado desde JSON"
                );

                let api_result = ContentTypeDetectionResult {
                    detected_mime_type: result.detected_mime_type,
                    client_provided_mime_type: command.client_content_type,
                    has_mismatch: result.has_mismatch,
                    confidence: result.confidence,
                };

                Json(api_result).into_response()
            }
            Err(error) => {
                warn!(error = %error, "Error en detección de Content-Type desde JSON");

                let status_code = match error {
                    ContentTypeDetectionError::InsufficientData(_) => StatusCode::BAD_REQUEST,
                    ContentTypeDetectionError::DetectionFailed => StatusCode::UNPROCESSABLE_ENTITY,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };

                (
                    status_code,
                    Json(ErrorResponse {
                        error: "CONTENT_TYPE_DETECTION_ERROR".to_string(),
                        message: error.to_string(),
                    }),
                )
                    .into_response()
            }
        }
    }
}

/// Response de error estandarizado
#[derive(Debug, serde::Serialize)]
struct ErrorResponse {
    error: String,
    message: String,
}
