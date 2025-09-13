use tracing::{info, error, debug, info_span};
use axum::{
    extract::{Multipart, State},
    response::{IntoResponse, Json},
    http::StatusCode,
};
use std::sync::Arc;
use serde_json::json;

use super::{
    dto::{BatchUploadRequest, BatchUploadArtifactMetadata, BatchUploadArtifactCommand},
    error::BatchUploadError,
    use_case::BatchUploadUseCase,
};

pub struct BatchUploadEndpoint {
    use_case: Arc<BatchUploadUseCase>,
}

impl BatchUploadEndpoint {
    pub fn new(use_case: Arc<BatchUploadUseCase>) -> Self {
        Self { use_case }
    }

    pub async fn upload_batch(
        State(endpoint): State<Arc<BatchUploadEndpoint>>,
        mut multipart: Multipart,
    ) -> impl IntoResponse {
        let span = info_span!("batch_upload_request");
        let _enter = span.enter();

        info!("Processing batch upload request");

        let mut metadata: Option<BatchUploadRequest> = None;
        let mut artifacts: Vec<(BatchUploadArtifactMetadata, Vec<u8>)> = Vec::new();

        while let Some(field) = multipart.next_field().await.map_err(|e| {
            error!("Failed to read multipart field: {}", e);
            BatchUploadError::InvalidRequest(e.to_string())
        })? {
            let name = field.name().unwrap_or("").to_string();
            let data = field.bytes().await.map_err(|e| {
                error!("Failed to read field bytes: {}", e);
                BatchUploadError::InvalidRequest(e.to_string())
            })?;

            if name == "metadata" {
                metadata = serde_json::from_slice(&data).map_err(|e| {
                    error!("Failed to parse metadata: {}", e);
                    BatchUploadError::InvalidRequest(format!("Invalid metadata: {}", e))
                })?;
            } else if name.starts_with("artifact_") {
                if let Some(metadata) = &metadata {
                    let index = name.trim_start_matches("artifact_").parse::<usize>().map_err(|_| {
                        BatchUploadError::InvalidRequest(format!("Invalid artifact field name: {}", name))
                    })?;

                    if index < metadata.artifacts.len() {
                        let artifact_metadata = metadata.artifacts[index].clone();
                        artifacts.push((artifact_metadata, data.to_vec()));
                    }
                }
            }
        }

        let metadata = metadata.ok_or_else(|| {
            error!("Missing metadata in batch request");
            BatchUploadError::InvalidRequest("Missing metadata".to_string())
        })?;

        if artifacts.len() != metadata.artifacts.len() {
            return Err(BatchUploadError::InvalidRequest(
                "Number of artifacts in metadata does not match uploaded files".to_string(),
            ));
        }

        debug!("Processing batch with {} artifacts", artifacts.len());

        let commands: Vec<BatchUploadArtifactCommand> = artifacts
            .iter()
            .map(|(meta, data)| BatchUploadArtifactCommand {
                coordinates: meta.coordinates.clone(),
                file_name: meta.file_name.clone(),
                content_length: data.len() as u64,
            })
            .collect();

        let contents: Vec<Vec<u8>> = artifacts.into_iter().map(|(_, data)| data).collect();

        match endpoint.use_case.execute_batch(commands, contents).await {
            Ok(response) => {
                info!("Batch upload completed successfully");
                Ok((StatusCode::OK, Json(response)))
            }
            Err(e) => {
                error!("Batch upload failed: {}", e);
                Err(e)
            }
        }
    }
}

impl IntoResponse for BatchUploadError {
    fn into_response(self) -> axum::response::Response {
        let status_code = match self {
            BatchUploadError::ValidationFailed(_) => StatusCode::BAD_REQUEST,
            BatchUploadError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
            BatchUploadError::Timeout(_) => StatusCode::REQUEST_TIMEOUT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };

        let body = json!({
            "error": self.to_string(),
            "code": status_code.as_u16(),
        });

        (status_code, Json(body)).into_response()
    }
}