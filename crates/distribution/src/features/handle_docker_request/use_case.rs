// crates/distribution/src/features/handle_docker_request/use_case.rs

use std::sync::Arc;
use async_trait::async_trait;
use tracing::{info, warn, error, instrument};
use bytes::Bytes;

use crate::features::handle_docker_request::dto::{
    GetManifestRequest, GetManifestResponse, PutManifestRequest, PutManifestResponse,
    HeadManifestRequest, HeadManifestResponse, DeleteManifestRequest, DeleteManifestResponse,
    GetBlobRequest, GetBlobResponse, PutBlobRequest, PutBlobResponse,
    HeadBlobRequest, HeadBlobResponse, DeleteBlobRequest, DeleteBlobResponse,
    StartBlobUploadRequest, StartBlobUploadResponse, CompleteBlobUploadRequest, CompleteBlobUploadResponse,
    DockerApiError, validate_docker_repository_name, validate_docker_tag, validate_docker_digest,
};

use super::ports::{
    DockerManifestReader, DockerManifestWriter, DockerBlobReader, DockerBlobWriter,
    DockerRepositoryManager, DockerPermissionChecker,
    DockerManifestReaderError, DockerManifestWriterError, DockerBlobReaderError, DockerBlobWriterError,
    DockerRepositoryManagerError, DockerPermissionError,
};

/// Use case for getting Docker manifests
pub struct HandleDockerGetManifestUseCase {
    manifest_reader: Arc<dyn DockerManifestReader>,
    permission_checker: Arc<dyn DockerPermissionChecker>,
}

impl HandleDockerGetManifestUseCase {
    pub fn new(
        manifest_reader: Arc<dyn DockerManifestReader>,
        permission_checker: Arc<dyn DockerPermissionChecker>,
    ) -> Self {
        Self {
            manifest_reader,
            permission_checker,
        }
    }

    #[instrument(skip(self, request))]
    pub async fn execute(&self, request: GetManifestRequest) -> Result<GetManifestResponse, DockerApiError> {
        info!("Getting Docker manifest: {}/{}", request.repository_name, request.reference);
        
        // Validate repository name
        validate_docker_repository_name(&request.repository_name)?;
        
        // Validate reference (tag or digest)
        if request.reference.starts_with("sha256:") {
            validate_docker_digest(&request.reference)?;
        } else {
            validate_docker_tag(&request.reference)?;
        }
        
        // Check permissions
        let can_read = self.permission_checker
            .can_read(&request.user_id, &request.repository_name)
            .await
            .map_err(|e| DockerApiError::Forbidden(e.to_string()))?;
        
        if !can_read {
            return Err(DockerApiError::Forbidden(
                format!("User {} cannot read from repository {}", request.user_id, request.repository_name)
            ));
        }
        
        // Get manifest
        let manifest = self.manifest_reader
            .get_manifest(&request.repository_name, &request.reference)
            .await
            .map_err(|e| match e {
                DockerManifestReaderError::NotFound { .. } => DockerApiError::ManifestNotFound(
                    format!("{}/{}", request.repository_name, request.reference)
                ),
                DockerManifestReaderError::RepositoryNotFound(_) => DockerApiError::RepositoryNotFound(
                    request.repository_name.clone()
                ),
                DockerManifestReaderError::PermissionDenied(_) => DockerApiError::Forbidden(
                    "Permission denied".to_string()
                ),
                _ => DockerApiError::InternalServerError(e.to_string()),
            })?;
        
        // Calculate digest (simplified for this example)
        let digest = format!("sha256:{}", request.reference);
        
        // Determine content type based on manifest
        let content_type = match &manifest {
            DockerManifest::V2(_) => crate::features::handle_docker_request::dto::DockerManifestMediaType::ManifestV2,
            DockerManifest::V1(_) => crate::features::handle_docker_request::dto::DockerManifestMediaType::ManifestV1,
        };
        
        // Serialize manifest to get content length
        let manifest_json = serde_json::to_vec(&manifest)
            .map_err(|e| DockerApiError::InternalServerError(e.to_string()))?;
        
        let content_length = manifest_json.len() as u64;
        
        info!("Successfully retrieved Docker manifest: {}/{}", request.repository_name, request.reference);
        
        Ok(GetManifestResponse {
            manifest,
            content_type,
            digest: digest.clone(),
            content_length,
            docker_content_digest: digest,
        })
    }
}

/// Use case for putting Docker manifests
pub struct HandleDockerPutManifestUseCase {
    manifest_writer: Arc<dyn DockerManifestWriter>,
    permission_checker: Arc<dyn DockerPermissionChecker>,
}

impl HandleDockerPutManifestUseCase {
    pub fn new(
        manifest_writer: Arc<dyn DockerManifestWriter>,
        permission_checker: Arc<dyn DockerPermissionChecker>,
    ) -> Self {
        Self {
            manifest_writer,
            permission_checker,
        }
    }

    #[instrument(skip(self, request))]
    pub async fn execute(&self, request: PutManifestRequest) -> Result<PutManifestResponse, DockerApiError> {
        info!("Putting Docker manifest: {}/{}", request.repository_name, request.reference);
        
        // Validate repository name
        validate_docker_repository_name(&request.repository_name)?;
        
        // Validate reference (tag or digest)
        if request.reference.starts_with("sha256:") {
            validate_docker_digest(&request.reference)?;
        } else {
            validate_docker_tag(&request.reference)?;
        }
        
        // Check permissions
        let can_write = self.permission_checker
            .can_write(&request.user_id, &request.repository_name)
            .await
            .map_err(|e| DockerApiError::Forbidden(e.to_string()))?;
        
        if !can_write {
            return Err(DockerApiError::Forbidden(
                format!("User {} cannot write to repository {}", request.user_id, request.repository_name)
            ));
        }
        
        // Put manifest
        let digest = self.manifest_writer
            .put_manifest(
                &request.repository_name,
                &request.reference,
                &request.manifest,
                &request.content_type.to_string(),
            )
            .await
            .map_err(|e| match e {
                DockerManifestWriterError::AlreadyExists { .. } => DockerApiError::InternalServerError(
                    "Manifest already exists".to_string()
                ),
                DockerManifestWriterError::RepositoryNotFound(_) => DockerApiError::RepositoryNotFound(
                    request.repository_name.clone()
                ),
                DockerManifestWriterError::PermissionDenied(_) => DockerApiError::Forbidden(
                    "Permission denied".to_string()
                ),
                DockerManifestWriterError::InvalidFormat(_) => DockerApiError::InvalidManifest(
                    "Invalid manifest format".to_string()
                ),
                _ => DockerApiError::InternalServerError(e.to_string()),
            })?;
        
        let location = format!("/v2/{}/manifests/{}", request.repository_name, request.reference);
        
        info!("Successfully stored Docker manifest: {}/{}", request.repository_name, request.reference);
        
        Ok(PutManifestResponse {
            location,
            docker_content_digest: digest,
        })
    }
}

/// Use case for head Docker manifests
pub struct HandleDockerHeadManifestUseCase {
    manifest_reader: Arc<dyn DockerManifestReader>,
    permission_checker: Arc<dyn DockerPermissionChecker>,
}

impl HandleDockerHeadManifestUseCase {
    pub fn new(
        manifest_reader: Arc<dyn DockerManifestReader>,
        permission_checker: Arc<dyn DockerPermissionChecker>,
    ) -> Self {
        Self {
            manifest_reader,
            permission_checker,
        }
    }

    #[instrument(skip(self, request))]
    pub async fn execute(&self, request: HeadManifestRequest) -> Result<HeadManifestResponse, DockerApiError> {
        info!("Checking Docker manifest: {}/{}", request.repository_name, request.reference);
        
        // Validate repository name
        validate_docker_repository_name(&request.repository_name)?;
        
        // Validate reference (tag or digest)
        if request.reference.starts_with("sha256:") {
            validate_docker_digest(&request.reference)?;
        } else {
            validate_docker_tag(&request.reference)?;
        }
        
        // Check permissions
        let can_read = self.permission_checker
            .can_read(&request.user_id, &request.repository_name)
            .await
            .map_err(|e| DockerApiError::Forbidden(e.to_string()))?;
        
        if !can_read {
            return Err(DockerApiError::Forbidden(
                format!("User {} cannot read from repository {}", request.user_id, request.repository_name)
            ));
        }
        
        // Get manifest info
        let manifest_info = self.manifest_reader
            .get_manifest_info(&request.repository_name, &request.reference)
            .await
            .map_err(|e| match e {
                DockerManifestReaderError::NotFound { .. } => DockerApiError::ManifestNotFound(
                    format!("{}/{}", request.repository_name, request.reference)
                ),
                DockerManifestReaderError::RepositoryNotFound(_) => DockerApiError::RepositoryNotFound(
                    request.repository_name.clone()
                ),
                DockerManifestReaderError::PermissionDenied(_) => DockerApiError::Forbidden(
                    "Permission denied".to_string()
                ),
                _ => DockerApiError::InternalServerError(e.to_string()),
            })?;
        
        info!("Successfully checked Docker manifest: {}/{}", request.repository_name, request.reference);
        
        Ok(HeadManifestResponse {
            content_length: manifest_info.size,
            docker_content_digest: manifest_info.digest,
            content_type: manifest_info.media_type,
        })
    }
}

/// Use case for deleting Docker manifests
pub struct HandleDockerDeleteManifestUseCase {
    manifest_writer: Arc<dyn DockerManifestWriter>,
    permission_checker: Arc<dyn DockerPermissionChecker>,
}

impl HandleDockerDeleteManifestUseCase {
    pub fn new(
        manifest_writer: Arc<dyn DockerManifestWriter>,
        permission_checker: Arc<dyn DockerPermissionChecker>,
    ) -> Self {
        Self {
            manifest_writer,
            permission_checker,
        }
    }

    #[instrument(skip(self, request))]
    pub async fn execute(&self, request: DeleteManifestRequest) -> Result<DeleteManifestResponse, DockerApiError> {
        info!("Deleting Docker manifest: {}/{}", request.repository_name, request.reference);
        
        // Validate repository name
        validate_docker_repository_name(&request.repository_name)?;
        
        // Validate reference (tag or digest)
        if request.reference.starts_with("sha256:") {
            validate_docker_digest(&request.reference)?;
        } else {
            validate_docker_tag(&request.reference)?;
        }
        
        // Check permissions
        let can_delete = self.permission_checker
            .can_delete(&request.user_id, &request.repository_name)
            .await
            .map_err(|e| DockerApiError::Forbidden(e.to_string()))?;
        
        if !can_delete {
            return Err(DockerApiError::Forbidden(
                format!("User {} cannot delete from repository {}", request.user_id, request.repository_name)
            ));
        }
        
        // Delete manifest
        self.manifest_writer
            .delete_manifest(&request.repository_name, &request.reference)
            .await
            .map_err(|e| match e {
                DockerManifestWriterError::RepositoryNotFound(_) => DockerApiError::RepositoryNotFound(
                    request.repository_name.clone()
                ),
                DockerManifestWriterError::PermissionDenied(_) => DockerApiError::Forbidden(
                    "Permission denied".to_string()
                ),
                _ => DockerApiError::InternalServerError(e.to_string()),
            })?;
        
        info!("Successfully deleted Docker manifest: {}/{}", request.repository_name, request.reference);
        
        Ok(DeleteManifestResponse {
            success: true,
        })
    }
}

/// Use case for getting Docker blobs
pub struct HandleDockerGetBlobUseCase {
    blob_reader: Arc<dyn DockerBlobReader>,
    permission_checker: Arc<dyn DockerPermissionChecker>,
}

impl HandleDockerGetBlobUseCase {
    pub fn new(
        blob_reader: Arc<dyn DockerBlobReader>,
        permission_checker: Arc<dyn DockerPermissionChecker>,
    ) -> Self {
        Self {
            blob_reader,
            permission_checker,
        }
    }

    #[instrument(skip(self, request))]
    pub async fn execute(&self, request: GetBlobRequest) -> Result<GetBlobResponse, DockerApiError> {
        info!("Getting Docker blob: {}/{}", request.repository_name, request.digest);
        
        // Validate repository name
        validate_docker_repository_name(&request.repository_name)?;
        
        // Validate digest
        validate_docker_digest(&request.digest)?;
        
        // Check permissions
        let can_read = self.permission_checker
            .can_read(&request.user_id, &request.repository_name)
            .await
            .map_err(|e| DockerApiError::Forbidden(e.to_string()))?;
        
        if !can_read {
            return Err(DockerApiError::Forbidden(
                format!("User {} cannot read from repository {}", request.user_id, request.repository_name)
            ));
        }
        
        // Get blob info first to get metadata
        let blob_info = self.blob_reader
            .get_blob_info(&request.repository_name, &request.digest)
            .await
            .map_err(|e| match e {
                DockerBlobReaderError::NotFound { .. } => DockerApiError::BlobNotFound(
                    format!("{}/{}", request.repository_name, request.digest)
                ),
                DockerBlobReaderError::RepositoryNotFound(_) => DockerApiError::RepositoryNotFound(
                    request.repository_name.clone()
                ),
                DockerBlobReaderError::PermissionDenied(_) => DockerApiError::Forbidden(
                    "Permission denied".to_string()
                ),
                _ => DockerApiError::InternalServerError(e.to_string()),
            })?;
        
        // Get blob data
        let data = self.blob_reader
            .get_blob(&request.repository_name, &request.digest)
            .await
            .map_err(|e| match e {
                DockerBlobReaderError::NotFound { .. } => DockerApiError::BlobNotFound(
                    format!("{}/{}", request.repository_name, request.digest)
                ),
                DockerBlobReaderError::RepositoryNotFound(_) => DockerApiError::RepositoryNotFound(
                    request.repository_name.clone()
                ),
                DockerBlobReaderError::PermissionDenied(_) => DockerApiError::Forbidden(
                    "Permission denied".to_string()
                ),
                _ => DockerApiError::InternalServerError(e.to_string()),
            })?;
        
        info!("Successfully retrieved Docker blob: {}/{}", request.repository_name, request.digest);
        
        Ok(GetBlobResponse {
            data,
            digest: blob_info.digest,
            content_length: blob_info.size,
            content_type: blob_info.media_type,
        })
    }
}

/// Use case for putting Docker blobs
pub struct HandleDockerPutBlobUseCase {
    blob_writer: Arc<dyn DockerBlobWriter>,
    permission_checker: Arc<dyn DockerPermissionChecker>,
}

impl HandleDockerPutBlobUseCase {
    pub fn new(
        blob_writer: Arc<dyn DockerBlobWriter>,
        permission_checker: Arc<dyn DockerPermissionChecker>,
    ) -> Self {
        Self {
            blob_writer,
            permission_checker,
        }
    }

    #[instrument(skip(self, request))]
    pub async fn execute(&self, request: PutBlobRequest) -> Result<PutBlobResponse, DockerApiError> {
        info!("Putting Docker blob: {}/{}", request.repository_name, request.digest);
        
        // Validate repository name
        validate_docker_repository_name(&request.repository_name)?;
        
        // Validate digest
        validate_docker_digest(&request.digest)?;
        
        // Check blob size limit (100MB for this example)
        const MAX_BLOB_SIZE: u64 = 100 * 1024 * 1024;
        if request.content_length > MAX_BLOB_SIZE {
            return Err(DockerApiError::BlobSizeExceeded(request.content_length));
        }
        
        // Check permissions
        let can_write = self.permission_checker
            .can_write(&request.user_id, &request.repository_name)
            .await
            .map_err(|e| DockerApiError::Forbidden(e.to_string()))?;
        
        if !can_write {
            return Err(DockerApiError::Forbidden(
                format!("User {} cannot write to repository {}", request.user_id, request.repository_name)
            ));
        }
        
        // Upload blob
        let digest = self.blob_writer
            .upload_blob(&request.repository_name, &request.digest, request.data)
            .await
            .map_err(|e| match e {
                DockerBlobWriterError::AlreadyExists { .. } => DockerApiError::InternalServerError(
                    "Blob already exists".to_string()
                ),
                DockerBlobWriterError::RepositoryNotFound(_) => DockerApiError::RepositoryNotFound(
                    request.repository_name.clone()
                ),
                DockerBlobWriterError::PermissionDenied(_) => DockerApiError::Forbidden(
                    "Permission denied".to_string()
                ),
                DockerBlobWriterError::InvalidDigest(_) => DockerApiError::InvalidDigest(
                    "Invalid digest format".to_string()
                ),
                DockerBlobWriterError::SizeExceeded { .. } => DockerApiError::BlobSizeExceeded(
                    request.content_length
                ),
                _ => DockerApiError::InternalServerError(e.to_string()),
            })?;
        
        let location = format!("/v2/{}/blobs/{}", request.repository_name, request.digest);
        
        info!("Successfully stored Docker blob: {}/{}", request.repository_name, request.digest);
        
        Ok(PutBlobResponse {
            location,
            docker_content_digest: digest,
        })
    }
}

/// Use case for starting Docker blob uploads
pub struct HandleDockerStartBlobUploadUseCase {
    blob_writer: Arc<dyn DockerBlobWriter>,
    permission_checker: Arc<dyn DockerPermissionChecker>,
}

impl HandleDockerStartBlobUploadUseCase {
    pub fn new(
        blob_writer: Arc<dyn DockerBlobWriter>,
        permission_checker: Arc<dyn DockerPermissionChecker>,
    ) -> Self {
        Self {
            blob_writer,
            permission_checker,
        }
    }

    #[instrument(skip(self, request))]
    pub async fn execute(&self, request: StartBlobUploadRequest) -> Result<StartBlobUploadResponse, DockerApiError> {
        info!("Starting Docker blob upload for repository: {}", request.repository_name);
        
        // Validate repository name
        validate_docker_repository_name(&request.repository_name)?;
        
        // Check permissions
        let can_write = self.permission_checker
            .can_write(&request.user_id, &request.repository_name)
            .await
            .map_err(|e| DockerApiError::Forbidden(e.to_string()))?;
        
        if !can_write {
            return Err(DockerApiError::Forbidden(
                format!("User {} cannot write to repository {}", request.user_id, request.repository_name)
            ));
        }
        
        // Start upload session
        let session = self.blob_writer
            .start_blob_upload(&request.repository_name, request.upload_uuid.as_deref())
            .await
            .map_err(|e| match e {
                DockerBlobWriterError::RepositoryNotFound(_) => DockerApiError::RepositoryNotFound(
                    request.repository_name.clone()
                ),
                DockerBlobWriterError::PermissionDenied(_) => DockerApiError::Forbidden(
                    "Permission denied".to_string()
                ),
                _ => DockerApiError::InternalServerError(e.to_string()),
            })?;
        
        let upload_location = format!("/v2/{}/blobs/uploads/{}", request.repository_name, session.upload_uuid);
        
        info!("Successfully started Docker blob upload: {}", session.upload_uuid);
        
        Ok(StartBlobUploadResponse {
            upload_location,
            upload_uuid: session.upload_uuid,
            range: format!("0-{}", session.offset),
        })
    }
}

/// Use case for completing Docker blob uploads
pub struct HandleDockerCompleteBlobUploadUseCase {
    blob_writer: Arc<dyn DockerBlobWriter>,
    permission_checker: Arc<dyn DockerPermissionChecker>,
}

impl HandleDockerCompleteBlobUploadUseCase {
    pub fn new(
        blob_writer: Arc<dyn DockerBlobWriter>,
        permission_checker: Arc<dyn DockerPermissionChecker>,
    ) -> Self {
        Self {
            blob_writer,
            permission_checker,
        }
    }

    #[instrument(skip(self, request))]
    pub async fn execute(&self, request: CompleteBlobUploadRequest) -> Result<CompleteBlobUploadResponse, DockerApiError> {
        info!("Completing Docker blob upload: {} for digest: {}", request.upload_uuid, request.digest);
        
        // Validate repository name
        validate_docker_repository_name(&request.repository_name)?;
        
        // Validate digest
        validate_docker_digest(&request.digest)?;
        
        // Check permissions
        let can_write = self.permission_checker
            .can_write(&request.user_id, &request.repository_name)
            .await
            .map_err(|e| DockerApiError::Forbidden(e.to_string()))?;
        
        if !can_write {
            return Err(DockerApiError::Forbidden(
                format!("User {} cannot write to repository {}", request.user_id, request.repository_name)
            ));
        }
        
        // Complete upload
        let digest = self.blob_writer
            .complete_blob_upload(
                &request.repository_name,
                &request.upload_uuid,
                &request.digest,
                request.data,
            )
            .await
            .map_err(|e| match e {
                DockerBlobWriterError::UploadSessionNotFound(_) => DockerApiError::UploadSessionNotFound(
                    request.upload_uuid.clone()
                ),
                DockerBlobWriterError::RepositoryNotFound(_) => DockerApiError::RepositoryNotFound(
                    request.repository_name.clone()
                ),
                DockerBlobWriterError::PermissionDenied(_) => DockerApiError::Forbidden(
                    "Permission denied".to_string()
                ),
                DockerBlobWriterError::InvalidDigest(_) => DockerApiError::InvalidDigest(
                    "Invalid digest format".to_string()
                ),
                _ => DockerApiError::InternalServerError(e.to_string()),
            })?;
        
        let location = format!("/v2/{}/blobs/{}", request.repository_name, request.digest);
        
        info!("Successfully completed Docker blob upload: {} with digest: {}", request.upload_uuid, request.digest);
        
        Ok(CompleteBlobUploadResponse {
            location,
            docker_content_digest: digest,
            upload_uuid: request.upload_uuid,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::handle_docker_request::ports::test::{
        MockDockerManifestReader, MockDockerManifestWriter, MockDockerBlobReader, MockDockerBlobWriter,
        MockDockerPermissionChecker,
    };
    use crate::features::handle_docker_request::dto::{
        DockerManifestV2, DockerDescriptor, DockerManifestMediaType,
    };

    #[tokio::test]
    async fn test_get_manifest_use_case_success() {
        let manifest_reader = Arc::new(MockDockerManifestReader::new());
        let permission_checker = Arc::new(MockDockerPermissionChecker::new());
        permission_checker.allow_all();
        
        let use_case = HandleDockerGetManifestUseCase::new(manifest_reader.clone(), permission_checker);
        
        let manifest = DockerManifest::V2(DockerManifestV2 {
            schema_version: 2,
            media_type: DockerManifestMediaType::ManifestV2,
            config: DockerDescriptor {
                media_type: "application/vnd.docker.container.image.v1+json".to_string(),
                size: 1234,
                digest: "sha256:abc123".to_string(),
                urls: None,
            },
            layers: vec![],
        });
        
        // Add test manifest
        {
            let mut manifests = manifest_reader.manifests.lock().unwrap();
            manifests.insert("test-repo/latest".to_string(), manifest.clone());
        }
        
        let request = GetManifestRequest {
            repository_name: "test-repo".to_string(),
            reference: "latest".to_string(),
            user_id: "test-user".to_string(),
            accept_headers: vec![],
        };
        
        let result = use_case.execute(request).await;
        assert!(result.is_ok());
        
        let response = result.unwrap();
        assert_eq!(response.content_length, 2); // Simplified JSON length
    }

    #[tokio::test]
    async fn test_put_manifest_use_case_success() {
        let manifest_writer = Arc::new(MockDockerManifestWriter::new());
        let permission_checker = Arc::new(MockDockerPermissionChecker::new());
        permission_checker.allow_all();
        
        let use_case = HandleDockerPutManifestUseCase::new(manifest_writer, permission_checker);
        
        let manifest = DockerManifest::V2(DockerManifestV2 {
            schema_version: 2,
            media_type: DockerManifestMediaType::ManifestV2,
            config: DockerDescriptor {
                media_type: "application/vnd.docker.container.image.v1+json".to_string(),
                size: 1234,
                digest: "sha256:abc123".to_string(),
                urls: None,
            },
            layers: vec![],
        });
        
        let request = PutManifestRequest {
            repository_name: "test-repo".to_string(),
            reference: "latest".to_string(),
            manifest,
            content_type: DockerManifestMediaType::ManifestV2,
            user_id: "test-user".to_string(),
            content_length: 1024,
        };
        
        let result = use_case.execute(request).await;
        assert!(result.is_ok());
        
        let response = result.unwrap();
        assert!(response.location.contains("test-repo"));
        assert!(response.location.contains("latest"));
    }

    #[tokio::test]
    async fn test_get_manifest_use_case_permission_denied() {
        let manifest_reader = Arc::new(MockDockerManifestReader::new());
        let permission_checker = Arc::new(MockDockerPermissionChecker::new());
        permission_checker.deny_all();
        
        let use_case = HandleDockerGetManifestUseCase::new(manifest_reader, permission_checker);
        
        let request = GetManifestRequest {
            repository_name: "test-repo".to_string(),
            reference: "latest".to_string(),
            user_id: "test-user".to_string(),
            accept_headers: vec![],
        };
        
        let result = use_case.execute(request).await;
        assert!(result.is_err());
        
        let error = result.unwrap_err();
        assert!(matches!(error, DockerApiError::Forbidden(_)));
    }

    #[tokio::test]
    async fn test_get_manifest_use_case_invalid_repository_name() {
        let manifest_reader = Arc::new(MockDockerManifestReader::new());
        let permission_checker = Arc::new(MockDockerPermissionChecker::new());
        permission_checker.allow_all();
        
        let use_case = HandleDockerGetManifestUseCase::new(manifest_reader, permission_checker);
        
        let request = GetManifestRequest {
            repository_name: "".to_string(), // Invalid empty name
            reference: "latest".to_string(),
            user_id: "test-user".to_string(),
            accept_headers: vec![],
        };
        
        let result = use_case.execute(request).await;
        assert!(result.is_err());
        
        let error = result.unwrap_err();
        assert!(matches!(error, DockerApiError::InvalidManifest(_)));
    }
}
