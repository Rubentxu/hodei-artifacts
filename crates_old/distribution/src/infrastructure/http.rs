use std::sync::Arc;
use axum::{Extension, extract::{Json, Path}, http::StatusCode, routing::{get, put}, Router};
use axum::body::Bytes;
use serde::Serialize;

use artifact::application::ports::{ArtifactStorage, ArtifactRepository, ArtifactEventPublisher};
use iam::application::ports::Authorization;
use crate::error::DistributionError;
use crate::features::npm::package_meta::handler::handle_npm_package_meta;
use crate::features::npm::package_meta::publish_handler::{handle_npm_publish, NpmPublishResponse};
use crate::features::npm::tarball::handler::handle_npm_tarball_download;
use shared::RepositoryId;

#[derive(Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    InvalidInput,
    PackageConflict,
    Unauthorized,
    NotFound,
    InternalError,
}

#[derive(Serialize)]
pub struct ErrorResponseBody {
    error: String,
    code: ErrorCode,
}

/// Handler GET /v2/npm/:package_name
/// Returns npm package metadata
pub async fn get_npm_package_metadata_handler(
    Extension(artifact_repository): Extension<Arc<dyn ArtifactRepository>>,
    Extension(authorization): Extension<Arc<dyn Authorization>>,
    Path(package_name): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponseBody>)> {
    match handle_npm_package_meta(artifact_repository, authorization, package_name).await {
        Ok(metadata) => Ok(Json(serde_json::to_value(metadata).unwrap())),
        Err(DistributionError::NotFound) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponseBody {
                error: "Package not found".to_string(),
                code: ErrorCode::NotFound,
            }),
        )),
        Err(DistributionError::Iam(_)) => Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponseBody {
                error: "Unauthorized to access package".to_string(),
                code: ErrorCode::Unauthorized,
            }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponseBody {
                error: format!("Internal server error: {}", e),
                code: ErrorCode::InternalError,
            }),
        )),
    }
}

/// Handler PUT /v2/npm/:package_name
/// Handles npm package publish
pub async fn publish_npm_package_handler(
    Extension(artifact_storage): Extension<Arc<dyn ArtifactStorage>>,
    Extension(artifact_repository): Extension<Arc<dyn ArtifactRepository>>,
    Extension(artifact_event_publisher): Extension<Arc<dyn ArtifactEventPublisher>>,
    Extension(authorization): Extension<Arc<dyn Authorization>>,
    Path((repository_id, package_name)): Path<(RepositoryId, String)>,
    bytes: Bytes,
) -> Result<Json<NpmPublishResponse>, (StatusCode, Json<ErrorResponseBody>)> {
    match handle_npm_publish(
        artifact_storage,
        artifact_repository,
        artifact_event_publisher,
        authorization,
        repository_id,
        package_name,
        bytes.to_vec(),
    )
    .await
    {
        Ok(response) => Ok(Json(response)),
        Err(DistributionError::Iam(_)) => Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponseBody {
                error: "Unauthorized to publish package".to_string(),
                code: ErrorCode::Unauthorized,
            }),
        )),
        Err(DistributionError::Artifact(_)) => Err((
            StatusCode::CONFLICT,
            Json(ErrorResponseBody {
                error: "Package already exists or conflict occurred".to_string(),
                code: ErrorCode::PackageConflict,
            }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponseBody {
                error: format!("Internal server error: {}", e),
                code: ErrorCode::InternalError,
            }),
        )),
    }
}

/// Handler GET /v2/npm/:package_name/-/:file_name
/// Handles npm tarball download
pub async fn download_npm_tarball_handler(
    Extension(artifact_storage): Extension<Arc<dyn ArtifactStorage>>,
    Extension(artifact_repository): Extension<Arc<dyn ArtifactRepository>>,
    Extension(authorization): Extension<Arc<dyn Authorization>>,
    Path((package_name, file_name)): Path<(String, String)>,
) -> Result<Vec<u8>, (StatusCode, Json<ErrorResponseBody>)> {
    match handle_npm_tarball_download(
        artifact_storage,
        artifact_repository,
        authorization,
        package_name,
        file_name,
    )
    .await
    {
        Ok(bytes) => Ok(bytes),
        Err(DistributionError::NotFound) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponseBody {
                error: "Package or tarball not found".to_string(),
                code: ErrorCode::NotFound,
            }),
        )),
        Err(DistributionError::Iam(_)) => Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponseBody {
                error: "Unauthorized to download package".to_string(),
                code: ErrorCode::Unauthorized,
            }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponseBody {
                error: format!("Internal server error: {}", e),
                code: ErrorCode::InternalError,
            }),
        )),
    }
}

pub fn create_distribution_router(
    artifact_storage: Arc<dyn ArtifactStorage>,
    artifact_repository: Arc<dyn ArtifactRepository>,
    artifact_event_publisher: Arc<dyn ArtifactEventPublisher>,
    authorization: Arc<dyn Authorization>,
) -> Router {
    Router::new()
        .route("/v2/npm/:package_name", get(get_npm_package_metadata_handler))
        .route("/v2/npm/:repository_id/:package_name", put(publish_npm_package_handler))
        .route("/v2/npm/:package_name/-/:file_name", get(download_npm_tarball_handler))
        .layer(Extension(artifact_storage))
        .layer(Extension(artifact_repository))
        .layer(Extension(artifact_event_publisher))
        .layer(Extension(authorization))
}