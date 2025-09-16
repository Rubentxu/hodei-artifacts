use axum::{extract::Multipart, response::IntoResponse, Extension};
use bytes::Bytes;
use serde_json::from_str;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tracing::{error, info, info_span};

use artifact::features::upload_artifact::dto::{UploadArtifactCommand, UploadArtifactMetadata};
use artifact::features::upload_artifact::error::UploadArtifactError;
use artifact::features::upload_artifact::use_case::UploadArtifactUseCase;
use shared::enums::HashAlgorithm;

pub async fn upload_artifact_handler(
    Extension(use_case): Extension<Arc<UploadArtifactUseCase>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let mut metadata: Option<UploadArtifactMetadata> = None;
    let mut content: Option<Bytes> = None;

    while let Some(field) = multipart.next_field().await.unwrap() {
        let name = field.name().unwrap_or("").to_string();
        let data = field.bytes().await.unwrap();
        if name == "metadata" {
            if let Ok(metadata_str) = std::str::from_utf8(&data) {
                metadata = from_str::<UploadArtifactMetadata>(metadata_str).ok();
            }
        } else if name == "file" {
            content = Some(data);
        }
    }

    if let (Some(meta), Some(cont)) = (metadata, content) {
        if let Some(provided) = meta.checksum.as_ref() {
            let algo = meta.checksum_algorithm.unwrap_or(HashAlgorithm::Sha256);
            match algo {
                HashAlgorithm::Sha256 => {
                    let mut hasher = Sha256::new();
                    hasher.update(&cont);
                    let computed = hex::encode(hasher.finalize());
                    if !provided.eq_ignore_ascii_case(&computed) {
                        return (axum::http::StatusCode::BAD_REQUEST, "Invalid checksum").into_response();
                    }
                }
                _ => {
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        "Unsupported checksum algorithm",
                    )
                        .into_response();
                }
            }
        }

        let cmd = UploadArtifactCommand {
            coordinates: meta.coordinates,
            file_name: meta.file_name,
            content_length: cont.len() as u64,
        };

        let span = info_span!(
            "upload_artifact_execution",
            coordinates = ?cmd.coordinates,
            file_name = %cmd.file_name,
            content_length = cmd.content_length
        );
        let result = span.in_scope(|| async { use_case.execute(cmd, cont).await }).await;

        match result {
            Ok(response) => {
                info!("Upload completed successfully: {}", response.hrn);
                (axum::http::StatusCode::CREATED, axum::Json(response)).into_response()
            }
            Err(e) => {
                let status_code = match e {
                    UploadArtifactError::RepositoryError(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    UploadArtifactError::StorageError(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    UploadArtifactError::EventError(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    UploadArtifactError::EventPublishError(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    UploadArtifactError::AlreadyExistsError(_) => axum::http::StatusCode::CONFLICT,
                    UploadArtifactError::BadRequest(_) => axum::http::StatusCode::BAD_REQUEST,
                    UploadArtifactError::ValidationFailed(_) => axum::http::StatusCode::BAD_REQUEST,
                    UploadArtifactError::VersioningError(_) => axum::http::StatusCode::BAD_REQUEST,
                    UploadArtifactError::NotFound(_) => axum::http::StatusCode::NOT_FOUND,
                    UploadArtifactError::Conflict(_) => axum::http::StatusCode::CONFLICT,
                };
                (status_code, e.to_string()).into_response()
            }
        }
    } else {
        error!("Missing metadata or file part in upload request");
        (axum::http::StatusCode::BAD_REQUEST, "Missing metadata or file part").into_response()
    }
}
