// crates/distribution/src/features/handle_docker_request/api.rs

use std::sync::Arc;
use axum::{
    extract::{Path, State, Query},
    http::{StatusCode, HeaderMap, HeaderValue},
    response::{Response, IntoResponse},
    Json,
};
use bytes::Bytes;
use tracing::{info, warn, error, instrument};
use serde::Deserialize;

use crate::features::handle_docker_request::dto::{
    DockerManifest, DockerManifestInfo, DockerBlobInfo, DockerUploadSession,
    DockerRepositoryInfo, DockerTagInfo, DockerCatalogResponse, DockerTagsListResponse,
    DockerManifestMediaType, DockerManifestV2, DockerDescriptor,
};

use super::use_case::{
    HandleDockerGetManifestUseCase, HandleDockerPutManifestUseCase, HandleDockerHeadManifestUseCase,
    HandleDockerDeleteManifestUseCase, HandleDockerGetBlobUseCase, HandleDockerPutBlobUseCase,
    HandleDockerStartBlobUploadUseCase, HandleDockerCompleteBlobUploadUseCase,
};

use super::error::DockerApiError;

/// Query parameters for catalog operations
#[derive(Deserialize)]
pub struct CatalogQuery {
    pub n: Option<usize>,
    pub last: Option<String>,
}

/// Query parameters for blob upload operations
#[derive(Deserialize)]
pub struct UploadQuery {
    pub mount: Option<String>,
    pub from: Option<String>,
}

/// Query parameters for completing blob uploads
#[derive(Deserialize)]
pub struct CompleteUploadQuery {
    pub digest: String,
}

/// Query parameters for tags listing
#[derive(Deserialize)]
pub struct TagsQuery {
    pub n: Option<usize>,
    pub last: Option<String>,
}

/// Docker Registry V2 API endpoints
pub struct DockerRegistryApi {
    get_manifest_use_case: Arc<HandleDockerGetManifestUseCase>,
    put_manifest_use_case: Arc<HandleDockerPutManifestUseCase>,
    head_manifest_use_case: Arc<HandleDockerHeadManifestUseCase>,
    delete_manifest_use_case: Arc<HandleDockerDeleteManifestUseCase>,
    get_blob_use_case: Arc<HandleDockerGetBlobUseCase>,
    put_blob_use_case: Arc<HandleDockerPutBlobUseCase>,
    start_blob_upload_use_case: Arc<HandleDockerStartBlobUploadUseCase>,
    complete_blob_upload_use_case: Arc<HandleDockerCompleteBlobUploadUseCase>,
}

impl DockerRegistryApi {
    pub fn new(
        get_manifest_use_case: Arc<HandleDockerGetManifestUseCase>,
        put_manifest_use_case: Arc<HandleDockerPutManifestUseCase>,
        head_manifest_use_case: Arc<HandleDockerHeadManifestUseCase>,
        delete_manifest_use_case: Arc<HandleDockerDeleteManifestUseCase>,
        get_blob_use_case: Arc<HandleDockerGetBlobUseCase>,
        put_blob_use_case: Arc<HandleDockerPutBlobUseCase>,
        start_blob_upload_use_case: Arc<HandleDockerStartBlobUploadUseCase>,
        complete_blob_upload_use_case: Arc<HandleDockerCompleteBlobUploadUseCase>,
    ) -> Self {
        Self {
            get_manifest_use_case,
            put_manifest_use_case,
            head_manifest_use_case,
            delete_manifest_use_case,
            get_blob_use_case,
            put_blob_use_case,
            start_blob_upload_use_case,
            complete_blob_upload_use_case,
        }
    }

    /// GET /v2/ - Check if registry supports V2 API
    #[instrument(skip(self))]
    pub async fn check_v2_support(&self) -> impl IntoResponse {
        info!("Docker Registry V2 API check requested");
        
        let mut headers = HeaderMap::new();
        headers.insert("Docker-Distribution-Api-Version", HeaderValue::from_static("registry/2.0"));
        
        (StatusCode::OK, headers)
    }

    /// GET /v2/_catalog - List repositories

    #[instrument(skip(self))]
    pub async fn list_repositories(
        &self,
        Query(query): Query<CatalogQuery>,
    ) -> Result<Json<DockerCatalogResponse>, DockerApiError> {
        info!("Listing Docker repositories");
        
        let n = query.n.unwrap_or(100);
        let last = query.last.as_deref();
        
        // This would need to be implemented in a use case
        // For now, return empty catalog
        let response = DockerCatalogResponse {
            repositories: vec![],
        };
        
        info!("Successfully listed Docker repositories");
        Ok(Json(response))
    }

    /// GET /v2/{name}/manifests/{reference} - Get manifest
    #[instrument(skip(self))]
    pub async fn get_manifest(
        &self,
        Path((name, reference)): Path<(String, String)>,
        headers: HeaderMap,
    ) -> Result<Response, DockerApiError> {
        info!("Getting Docker manifest: {}/{}", name, reference);
        
        let accept_header = headers
            .get("Accept")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("application/vnd.docker.distribution.manifest.v2+json");
        
        let manifest = self
            .get_manifest_use_case
            .execute(&name, &reference, accept_header)
            .await?;
        
        let mut response_headers = HeaderMap::new();
        response_headers.insert(
            "Content-Type",
            HeaderValue::from_static("application/vnd.docker.distribution.manifest.v2+json"),
        );
        response_headers.insert(
            "Docker-Content-Digest",
            HeaderValue::from_str(&format!("sha256:{:x}", sha2::Sha256::digest(
                serde_json::to_vec(&manifest).map_err(|e| DockerApiError::SerializationError(e.to_string()))?
            ))).map_err(|_| DockerApiError::InvalidHeader)?,
        );
        
        let body = serde_json::to_vec(&manifest)
            .map_err(|e| DockerApiError::SerializationError(e.to_string()))?;
        
        info!("Successfully retrieved Docker manifest: {}/{}", name, reference);
        Ok((StatusCode::OK, response_headers, body).into_response())
    }

    /// PUT /v2/{name}/manifests/{reference} - Put manifest
    #[instrument(skip(self, body))]
    pub async fn put_manifest(
        &self,
        Path((name, reference)): Path<(String, String)>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Result<Response, DockerApiError> {
        info!("Putting Docker manifest: {}/{}", name, reference);
        
        let content_type = headers
            .get("Content-Type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("application/vnd.docker.distribution.manifest.v2+json");
        
        let manifest: DockerManifest = serde_json::from_slice(&body)
            .map_err(|e| DockerApiError::InvalidManifest(e.to_string()))?;
        
        let digest = self
            .put_manifest_use_case
            .execute(&name, &reference, &manifest, content_type)
            .await?;
        
        let mut response_headers = HeaderMap::new();
        response_headers.insert(
            "Docker-Content-Digest",
            HeaderValue::from_str(&digest).map_err(|_| DockerApiError::InvalidHeader)?,
        );
        response_headers.insert(
            "Location",
            HeaderValue::from_str(&format!("/v2/{}/manifests/{}", name, digest))
                .map_err(|_| DockerApiError::InvalidHeader)?,
        );
        
        info!("Successfully stored Docker manifest: {}/{}", name, reference);
        Ok((StatusCode::CREATED, response_headers).into_response())
    }

    /// HEAD /v2/{name}/manifests/{reference} - Check manifest existence
    #[instrument(skip(self))]
    pub async fn head_manifest(
        &self,
        Path((name, reference)): Path<(String, String)>,
    ) -> Result<Response, DockerApiError> {
        info!("Checking Docker manifest existence: {}/{}", name, reference);
        
        let manifest_info = self
            .head_manifest_use_case
            .execute(&name, &reference)
            .await?;
        
        let mut response_headers = HeaderMap::new();
        response_headers.insert(
            "Docker-Content-Digest",
            HeaderValue::from_str(&manifest_info.digest)
                .map_err(|_| DockerApiError::InvalidHeader)?,
        );
        response_headers.insert(
            "Content-Length",
            HeaderValue::from_str(&manifest_info.size.to_string())
                .map_err(|_| DockerApiError::InvalidHeader)?,
        );
        response_headers.insert(
            "Content-Type",
            HeaderValue::from_static("application/vnd.docker.distribution.manifest.v2+json"),
        );
        
        info!("Docker manifest exists: {}/{}", name, reference);
        Ok((StatusCode::OK, response_headers).into_response())
    }

    /// DELETE /v2/{name}/manifests/{reference} - Delete manifest
    #[instrument(skip(self))]
    pub async fn delete_manifest(
        &self,
        Path((name, reference)): Path<(String, String)>,
    ) -> Result<Response, DockerApiError> {
        info!("Deleting Docker manifest: {}/{}", name, reference);
        
        self
            .delete_manifest_use_case
            .execute(&name, &reference)
            .await?;
        
        info!("Successfully deleted Docker manifest: {}/{}", name, reference);
        Ok(StatusCode::ACCEPTED.into_response())
    }

    /// GET /v2/{name}/blobs/{digest} - Get blob
    #[instrument(skip(self))]
    pub async fn get_blob(
        &self,
        Path((name, digest)): Path<(String, String)>,
    ) -> Result<Response, DockerApiError> {
        info!("Getting Docker blob: {}/{}", name, digest);
        
        let blob_data = self
            .get_blob_use_case
            .execute(&name, &digest)
            .await?;
        
        let mut response_headers = HeaderMap::new();
        response_headers.insert(
            "Content-Type",
            HeaderValue::from_static("application/octet-stream"),
        );
        response_headers.insert(
            "Docker-Content-Digest",
            HeaderValue::from_str(&digest).map_err(|_| DockerApiError::InvalidHeader)?,
        );
        response_headers.insert(
            "Content-Length",
            HeaderValue::from_str(&blob_data.len().to_string())
                .map_err(|_| DockerApiError::InvalidHeader)?,
        );
        
        info!("Successfully retrieved Docker blob: {}/{}", name, digest);
        Ok((StatusCode::OK, response_headers, blob_data).into_response())
    }

    /// POST /v2/{name}/blobs/uploads/ - Start blob upload

    #[instrument(skip(self))]
    pub async fn start_blob_upload(
        &self,
        Path(name): Path<String>,
        Query(query): Query<UploadQuery>,
    ) -> Result<Response, DockerApiError> {
        info!("Starting Docker blob upload for repository: {}", name);
        
        // Handle mount request if provided
        if let (Some(digest), Some(from)) = (&query.mount, &query.from) {
            info!("Attempting to mount blob from {} with digest {}", from, digest);
            // This would need cross-repository blob mounting logic
            // For now, proceed with normal upload
        }
        
        let session = self
            .start_blob_upload_use_case
            .execute(&name, None)
            .await?;
        
        let mut response_headers = HeaderMap::new();
        response_headers.insert(
            "Location",
            HeaderValue::from_str(&format!("/v2/{}/blobs/uploads/{}", name, session.upload_uuid))
                .map_err(|_| DockerApiError::InvalidHeader)?,
        );
        response_headers.insert(
            "Range",
            HeaderValue::from_static("0-0"),
        );
        response_headers.insert(
            "Docker-Upload-UUID",
            HeaderValue::from_str(&session.upload_uuid)
                .map_err(|_| DockerApiError::InvalidHeader)?,
        );
        
        info!("Successfully started Docker blob upload: {}", session.upload_uuid);
        Ok((StatusCode::ACCEPTED, response_headers).into_response())
    }

    /// PUT /v2/{name}/blobs/uploads/{uuid}?digest={digest} - Complete blob upload

    #[instrument(skip(self, body))]
    pub async fn complete_blob_upload(
        &self,
        Path((name, uuid)): Path<(String, String)>,
        Query(query): Query<CompleteUploadQuery>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Result<Response, DockerApiError> {
        info!("Completing Docker blob upload: {} with digest: {}", uuid, query.digest);
        
        // Check if this is a chunked upload or single request
        let data = if body.is_empty() {
            None
        } else {
            Some(body)
        };
        
        let digest = self
            .complete_blob_upload_use_case
            .execute(&name, &uuid, &query.digest, data)
            .await?;
        
        let mut response_headers = HeaderMap::new();
        response_headers.insert(
            "Docker-Content-Digest",
            HeaderValue::from_str(&digest).map_err(|_| DockerApiError::InvalidHeader)?,
        );
        response_headers.insert(
            "Location",
            HeaderValue::from_str(&format!("/v2/{}/blobs/{}", name, digest))
                .map_err(|_| DockerApiError::InvalidHeader)?,
        );
        
        info!("Successfully completed Docker blob upload: {} with digest: {}", uuid, digest);
        Ok((StatusCode::CREATED, response_headers).into_response())
    }

    /// GET /v2/{name}/tags/list - List tags

    #[instrument(skip(self))]
    pub async fn list_tags(
        &self,
        Path(name): Path<String>,
        Query(query): Query<TagsQuery>,
    ) -> Result<Json<DockerTagsListResponse>, DockerApiError> {
        info!("Listing Docker tags for repository: {}", name);
        
        let n = query.n.unwrap_or(100);
        let last = query.last.as_deref();
        
        // This would need to be implemented in a use case
        // For now, return empty tags list
        let response = DockerTagsListResponse {
            name: name.clone(),
            tags: vec![],
        };
        
        info!("Successfully listed Docker tags for repository: {}", name);
        Ok(Json(response))
    }

    /// HEAD /v2/{name}/blobs/{digest} - Check blob existence
    #[instrument(skip(self))]
    pub async fn head_blob(
        &self,
        Path((name, digest)): Path<(String, String)>,
    ) -> Result<Response, DockerApiError> {
        info!("Checking Docker blob existence: {}/{}", name, digest);
        
        let blob_info = self
            .get_blob_use_case
            .get_blob_info(&name, &digest)
            .await?;
        
        let mut response_headers = HeaderMap::new();
        response_headers.insert(
            "Docker-Content-Digest",
            HeaderValue::from_str(&blob_info.digest)
                .map_err(|_| DockerApiError::InvalidHeader)?,
        );
        response_headers.insert(
            "Content-Length",
            HeaderValue::from_str(&blob_info.size.to_string())
                .map_err(|_| DockerApiError::InvalidHeader)?,
        );
        response_headers.insert(
            "Content-Type",
            HeaderValue::from_str(&blob_info.media_type)
                .map_err(|_| DockerApiError::InvalidHeader)?,
        );
        
        info!("Docker blob exists: {}/{}", name, digest);
        Ok((StatusCode::OK, response_headers).into_response())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::handle_docker_request::ports::MockDockerManifestReader;
    use crate::features::handle_docker_request::ports::MockDockerManifestWriter;
    use crate::features::handle_docker_request::ports::MockDockerBlobReader;
    use crate::features::handle_docker_request::ports::MockDockerBlobWriter;
    use crate::features::handle_docker_request::ports::MockDockerRepositoryManager;
    use crate::features::handle_docker_request::ports::MockDockerPermissionChecker;

    fn create_test_api() -> DockerRegistryApi {
        let manifest_reader = Arc::new(MockDockerManifestReader::new());
        let manifest_writer = Arc::new(MockDockerManifestWriter::new());
        let blob_reader = Arc::new(MockDockerBlobReader::new());
        let blob_writer = Arc::new(MockDockerBlobWriter::new());
        let repository_manager = Arc::new(MockDockerRepositoryManager::new());
        let permission_checker = Arc::new(MockDockerPermissionChecker::new());

        let get_manifest_use_case = Arc::new(HandleDockerGetManifestUseCase::new(
            manifest_reader.clone(),
            permission_checker.clone(),
        ));

        let put_manifest_use_case = Arc::new(HandleDockerPutManifestUseCase::new(
            manifest_writer.clone(),
            permission_checker.clone(),
        ));

        let head_manifest_use_case = Arc::new(HandleDockerHeadManifestUseCase::new(
            manifest_reader.clone(),
            permission_checker.clone(),
        ));

        let delete_manifest_use_case = Arc::new(HandleDockerDeleteManifestUseCase::new(
            manifest_writer.clone(),
            permission_checker.clone(),
        ));

        let get_blob_use_case = Arc::new(HandleDockerGetBlobUseCase::new(
            blob_reader.clone(),
            permission_checker.clone(),
        ));

        let put_blob_use_case = Arc::new(HandleDockerPutBlobUseCase::new(
            blob_writer.clone(),
            permission_checker.clone(),
        ));

        let start_blob_upload_use_case = Arc::new(HandleDockerStartBlobUploadUseCase::new(
            blob_writer.clone(),
            permission_checker.clone(),
        ));

        let complete_blob_upload_use_case = Arc::new(HandleDockerCompleteBlobUploadUseCase::new(
            blob_writer.clone(),
            permission_checker.clone(),
        ));

        DockerRegistryApi::new(
            get_manifest_use_case,
            put_manifest_use_case,
            head_manifest_use_case,
            delete_manifest_use_case,
            get_blob_use_case,
            put_blob_use_case,
            start_blob_upload_use_case,
            complete_blob_upload_use_case,
        )
    }

    #[tokio::test]
    async fn test_check_v2_support() {
        let api = create_test_api();
        
        let response = api.check_v2_support().await;
        let (status, headers) = response.into_response().into_parts();
        
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers.get("Docker-Distribution-Api-Version").unwrap(),
            "registry/2.0"
        );
    }

    #[tokio::test]
    async fn test_list_repositories() {
        let api = create_test_api();
        
        let query = CatalogQuery { n: Some(10), last: None };
        let result = api.list_repositories(Query(query)).await;
        
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.repositories.is_empty());
    }

    #[tokio::test]
    async fn test_list_tags() {
        let api = create_test_api();
        
        let query = TagsQuery { n: Some(10), last: None };
        let result = api.list_tags(Path("test-repo".to_string()), Query(query)).await;
        
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.name, "test-repo");
        assert!(response.tags.is_empty());
    }
}