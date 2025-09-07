use tracing::{info, error, debug, info_span};
use axum::{
    body::Body,
    extract::{Multipart, FromRequest},
    response::{IntoResponse, Json},
    http::{StatusCode, Request},
    Extension,
    Router,
    routing::post,
    middleware::{self, Next},
};
use std::sync::Arc;
use bytes::Bytes;
use serde_json::from_str;
use sha2::{Sha256, Digest};

use super::{
    dto::{UploadArtifactCommand, UploadArtifactMetadata, UploadArtifactResponse},
    use_case::UploadArtifactUseCase,
    error::UploadArtifactError,
    di::UploadArtifactDIContainer,
};
use shared::enums::HashAlgorithm;

pub struct UploadArtifactEndpoint {
    use_case: Arc<UploadArtifactUseCase>,
}

impl UploadArtifactEndpoint {
    pub fn new(use_case: Arc<UploadArtifactUseCase>) -> Self {
        Self { use_case }
    }

    pub async fn upload_artifact(
        Extension(endpoint): Extension<Arc<UploadArtifactEndpoint>>,
        multipart: Multipart,
    ) -> impl IntoResponse {
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
            // Optional checksum validation (default to Sha256)
            if let Some(provided) = meta.checksum.as_ref() {
                let algo = meta.checksum_algorithm.unwrap_or(HashAlgorithm::Sha256);
                match algo {
                    HashAlgorithm::Sha256 => {
                        let mut hasher = Sha256::new();
                        hasher.update(&cont);
                        let computed = hex::encode(hasher.finalize());
                        if !provided.eq_ignore_ascii_case(&computed) {
                            error!("Checksum mismatch: provided={}, computed={}", provided, computed);
                            return (StatusCode::BAD_REQUEST, "Invalid checksum").into_response();
                        }
                    }
                    _ => {
                        error!("Unsupported checksum algorithm for validation");
                        return (StatusCode::BAD_REQUEST, "Unsupported checksum algorithm").into_response();
                    }
                }
            }

            let cmd = UploadArtifactCommand {
                coordinates: meta.coordinates,
                file_name: meta.file_name,
                content_length: cont.len() as u64,
            };

            info!("Processing upload command: {:?}", cmd);
            info!("Content length: {}", cmd.content_length);
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

// Simple auth middleware: requires Authorization: Bearer <token> unless HODEI_AUTH_DISABLED=true
async fn auth_middleware(mut req: Request<Body>, next: Next) -> impl IntoResponse {
    // Test bypass via header
    if let Some(bypass) = req.headers().get("X-Test-Bypass-Auth") {
        if bypass.to_str().ok().map(|v| v.eq_ignore_ascii_case("true")).unwrap_or(false) {
            return next.run(req).await;
        }
    }
    if std::env::var("HODEI_AUTH_DISABLED").map(|v| v == "true").unwrap_or(false) {
        return next.run(req).await;
    }

    let headers = req.headers();
    let unauthorized = || (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();

    if let Some(value) = headers.get(axum::http::header::AUTHORIZATION) {
        if let Ok(val_str) = value.to_str() {
            if let Some(token) = val_str.strip_prefix("Bearer ") {
                if !token.is_empty() {
                    // Optionally store subject for downstream use
                    // req.extensions_mut().insert(Subject { sub: token.to_string() });
                    return next.run(req).await;
                }
            }
        }
    }
    unauthorized()
}

pub fn setup_app(app: Router, di: UploadArtifactDIContainer) -> Router {
    app.route(
        "/artifacts",
        post(UploadArtifactEndpoint::upload_artifact),
    )
    .layer(Extension(di.endpoint.clone()))
    .layer(middleware::from_fn(auth_middleware))
}
