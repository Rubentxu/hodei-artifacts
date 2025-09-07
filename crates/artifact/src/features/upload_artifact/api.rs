use axum::{
    extract::Multipart,
    response::{IntoResponse, Json},
    http::StatusCode,
};
use std::sync::Arc;
use bytes::Bytes;
use futures_util::stream::StreamExt;
use serde_json::from_str;

use super::{
    dto::{UploadArtifactCommand, UploadArtifactResponse},
    use_case::UploadArtifactUseCase,
    error::UploadArtifactError,
};

/// The API entry point for the Upload Artifact feature.
pub struct UploadArtifactEndpoint {
    use_case: Arc<UploadArtifactUseCase>,
}

impl UploadArtifactEndpoint {
    pub fn new(use_case: Arc<UploadArtifactUseCase>) -> Self {
        Self { use_case }
    }

    /// Axum handler for the artifact upload request.
    pub async fn handle_request(
        &self,
        mut multipart: Multipart,
    ) -> impl IntoResponse {
        let mut command: Option<UploadArtifactCommand> = None;
        let mut content: Option<Bytes> = None;

        while let Some(field) = multipart.next_field().await.unwrap() {
            let name = field.name().unwrap_or("").to_string();
            let data = field.bytes().await.unwrap();

            if name == "metadata" {
                if let Ok(metadata_str) = std::str::from_utf8(&data) {
                    command = from_str(metadata_str).ok();
                }
            } else if name == "file" {
                content = Some(data);
            }
        }

        if let (Some(mut cmd), Some(cont)) = (command, content) {
            cmd.content_length = cont.len() as u64;
            match self.use_case.execute(cmd, cont).await {
                Ok(response) => (StatusCode::CREATED, Json(response)).into_response(),
                Err(e) => {
                    // Map domain errors to http status codes
                    let status_code = match e {
                        UploadArtifactError::RepositoryError(_) => StatusCode::INTERNAL_SERVER_ERROR,
                        UploadArtifactError::StorageError(_) => StatusCode::INTERNAL_SERVER_ERROR,
                        UploadArtifactError::EventError(_) => StatusCode::INTERNAL_SERVER_ERROR,
                        UploadArtifactError::AlreadyExistsError(_) => StatusCode::CONFLICT,
                    };
                    (status_code, e.to_string()).into_response()
                }
            }
        } else {
            (StatusCode::BAD_REQUEST, "Missing metadata or file part").into_response()
        }
    }
}
