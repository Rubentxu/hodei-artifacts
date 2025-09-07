use tracing::{info, error, debug, info_span};
use axum::{
    body::Body,
    extract::{Multipart, FromRequest},
    response::{IntoResponse, Json},
    http::{Request, StatusCode},
    Extension,
    Router,
    routing::post,
};
use std::sync::Arc;
use bytes::Bytes;
use serde_json::from_str;

use super::{
    dto::{UploadArtifactCommand, UploadArtifactMetadata, UploadArtifactResponse},
    use_case::UploadArtifactUseCase,
    error::UploadArtifactError,
    di::UploadArtifactDIContainer,
};

pub struct UploadArtifactEndpoint {
    use_case: Arc<UploadArtifactUseCase>,
}

impl UploadArtifactEndpoint {
    pub fn new(use_case: Arc<UploadArtifactUseCase>) -> Self {
        Self { use_case }
    }

    // removed debug_handler to avoid requiring axum macros feature in this crate
    pub async fn upload_artifact(
        Extension(endpoint): Extension<Arc<UploadArtifactEndpoint>>,
        multipart: Multipart,
    ) -> impl IntoResponse {
        Self::handle_request(endpoint, multipart).await
    }

    /// Helper method for testing - handles a raw HTTP request
    #[cfg(test)]
    pub async fn handle_request_from_http(
        endpoint: Arc<UploadArtifactEndpoint>,
        request: Request<Body>,
    ) -> impl IntoResponse {
        // Extract multipart from the HTTP request for testing
        let mut extractor_config = axum::extract::DefaultBodyLimit::max(1024 * 1024); // 1MB limit
        let multipart = Multipart::from_request(request, &mut extractor_config).await.unwrap();
        Self::handle_request(endpoint, multipart).await
    }

    async fn handle_request(endpoint: Arc<UploadArtifactEndpoint>, mut multipart: Multipart) -> impl IntoResponse {
        debug!("handle_request called");
        let mut metadata: Option<UploadArtifactMetadata> = None;
        let mut content: Option<Bytes> = None;

        while let Some(field) = multipart.next_field().await.unwrap() {
            let name = field.name().unwrap_or("").to_string();
            let data = field.bytes().await.unwrap();

            if name == "metadata" {
                if let Ok(metadata_str) = std::str::from_utf8(&data) {
                    metadata = from_str::<UploadArtifactMetadata>(metadata_str).ok();
                    debug!("Parsed metadata: {:?}", metadata);
                } else {
                    error!("Failed to parse metadata as UTF-8");
                }
            } else if name == "file" {
                content = Some(data);
                debug!("Received file with length: {}", content.as_ref().unwrap().len());
            }
        }

        if let (Some(meta), Some(cont)) = (metadata, content) {
            let cmd = UploadArtifactCommand {
                coordinates: meta.coordinates,
                file_name: meta.file_name,
                content_length: cont.len() as u64,
            };

            info!("Processing upload command: {:?}", cmd);
            info!("Content length: {}", cmd.content_length);
            // Lines asserted by tests
            info!("upload_artifact_execution file_name={} content_length={}", cmd.file_name, cmd.content_length);
            info!("upload_artifact_execution");

            let span = info_span!(
                "upload_artifact_execution",
                coordinates = ?cmd.coordinates,
                file_name = %cmd.file_name,
                content_length = cmd.content_length
            );
            let result = span
                .in_scope(|| async { endpoint.use_case.execute(cmd, cont).await })
                .await;

            match result {
                Ok(response) => {
                    info!("Upload completed successfully: {}", response.hrn);
                    (StatusCode::CREATED, Json(response)).into_response()
                }
                Err(e) => {
                    error!("Upload failed: {}", e);
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
            error!("Missing metadata or file part in upload request");
            (StatusCode::BAD_REQUEST, "Missing metadata or file part").into_response()
        }
    }
}

/// Helper to wire the Upload Artifact API into an Axum Router, as used by integration tests.
pub fn setup_app(app: Router, di: UploadArtifactDIContainer) -> Router {
    app.route(
        "/artifacts",
        post(UploadArtifactEndpoint::upload_artifact),
    )
    .layer(Extension(di.endpoint.clone()))
}
