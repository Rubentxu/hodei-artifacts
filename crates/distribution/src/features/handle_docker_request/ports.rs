// crates/distribution/src/features/handle_docker_request/ports.rs

use async_trait::async_trait;
use bytes::Bytes;
use crate::features::handle_docker_request::dto::{
    DockerManifest, DockerManifestInfo, DockerBlobInfo, DockerUploadSession,
    DockerRepositoryInfo, DockerTagInfo, DockerCatalogResponse, DockerTagsListResponse,
};

/// Docker manifest reader port - segregated interface for reading manifests
#[async_trait]
pub trait DockerManifestReader: Send + Sync {
    /// Get manifest by repository name and reference (tag or digest)
    async fn get_manifest(
        &self,
        repository_name: &str,
        reference: &str,
    ) -> Result<DockerManifest, DockerManifestReaderError>;
    
    /// Get manifest info (metadata) by repository name and reference
    async fn get_manifest_info(
        &self,
        repository_name: &str,
        reference: &str,
    ) -> Result<DockerManifestInfo, DockerManifestReaderError>;
    
    /// Check if manifest exists
    async fn manifest_exists(
        &self,
        repository_name: &str,
        reference: &str,
    ) -> Result<bool, DockerManifestReaderError>;
}

/// Docker manifest writer port - segregated interface for writing manifests
#[async_trait]
pub trait DockerManifestWriter: Send + Sync {
    /// Put manifest by repository name and reference
    async fn put_manifest(
        &self,
        repository_name: &str,
        reference: &str,
        manifest: &DockerManifest,
        media_type: &str,
    ) -> Result<String, DockerManifestWriterError>;
    
    /// Delete manifest by repository name and reference
    async fn delete_manifest(
        &self,
        repository_name: &str,
        reference: &str,
    ) -> Result<(), DockerManifestWriterError>;
}

/// Docker blob reader port - segregated interface for reading blobs
#[async_trait]
pub trait DockerBlobReader: Send + Sync {
    /// Get blob data by repository name and digest
    async fn get_blob(
        &self,
        repository_name: &str,
        digest: &str,
    ) -> Result<Bytes, DockerBlobReaderError>;
    
    /// Get blob info (metadata) by repository name and digest
    async fn get_blob_info(
        &self,
        repository_name: &str,
        digest: &str,
    ) -> Result<DockerBlobInfo, DockerBlobReaderError>;
    
    /// Check if blob exists
    async fn blob_exists(
        &self,
        repository_name: &str,
        digest: &str,
    ) -> Result<bool, DockerBlobReaderError>;
}

/// Docker blob writer port - segregated interface for writing blobs
#[async_trait]
pub trait DockerBlobWriter: Send + Sync {
    /// Start blob upload session
    async fn start_blob_upload(
        &self,
        repository_name: &str,
        upload_uuid: Option<&str>,
    ) -> Result<DockerUploadSession, DockerBlobWriterError>;
    
    /// Upload blob data
    async fn upload_blob(
        &self,
        repository_name: &str,
        digest: &str,
        data: Bytes,
    ) -> Result<String, DockerBlobWriterError>;
    
    /// Complete blob upload
    async fn complete_blob_upload(
        &self,
        repository_name: &str,
        upload_uuid: &str,
        digest: &str,
        data: Option<Bytes>,
    ) -> Result<String, DockerBlobWriterError>;
    
    /// Delete blob by repository name and digest
    async fn delete_blob(
        &self,
        repository_name: &str,
        digest: &str,
    ) -> Result<(), DockerBlobWriterError>;
}

/// Docker repository manager port - segregated interface for repository operations
#[async_trait]
pub trait DockerRepositoryManager: Send + Sync {
    /// Get repository information
    async fn get_repository_info(
        &self,
        repository_name: &str,
    ) -> Result<DockerRepositoryInfo, DockerRepositoryManagerError>;
    
    /// List repositories with pagination
    async fn list_repositories(
        &self,
        last: Option<&str>,
        n: usize,
    ) -> Result<DockerCatalogResponse, DockerRepositoryManagerError>;
    
    /// List tags for repository
    async fn list_tags(
        &self,
        repository_name: &str,
        last: Option<&str>,
        n: usize,
    ) -> Result<DockerTagsListResponse, DockerRepositoryManagerError>;
    
    /// Get tag information
    async fn get_tag_info(
        &self,
        repository_name: &str,
        tag: &str,
    ) -> Result<DockerTagInfo, DockerRepositoryManagerError>;
    
    /// Check if repository exists
    async fn repository_exists(
        &self,
        repository_name: &str,
    ) -> Result<bool, DockerRepositoryManagerError>;
}

/// Docker permission checker port - segregated interface for permission checking
#[async_trait]
pub trait DockerPermissionChecker: Send + Sync {
    /// Check if user can read from repository
    async fn can_read(
        &self,
        user_id: &str,
        repository_name: &str,
    ) -> Result<bool, DockerPermissionError>;
    
    /// Check if user can write to repository
    async fn can_write(
        &self,
        user_id: &str,
        repository_name: &str,
    ) -> Result<bool, DockerPermissionError>;
    
    /// Check if user can delete from repository
    async fn can_delete(
        &self,
        user_id: &str,
        repository_name: &str,
    ) -> Result<bool, DockerPermissionError>;
    
    /// Check if user can list repositories
    async fn can_list(
        &self,
        user_id: &str,
    ) -> Result<bool, DockerPermissionError>;
}

/// Error types for Docker manifest reader
#[derive(Debug, thiserror::Error)]
pub enum DockerManifestReaderError {
    #[error("Manifest not found: {repository}/{reference}")]
    NotFound { repository: String, reference: String },
    
    #[error("Invalid manifest format: {0}")]
    InvalidFormat(String),
    
    #[error("Repository not found: {0}")]
    RepositoryNotFound(String),
    
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    
    #[error("Storage error: {0}")]
    StorageError(String),
    
    #[error("Internal error: {0}")]
    InternalError(String),
}

/// Error types for Docker manifest writer
#[derive(Debug, thiserror::Error)]
pub enum DockerManifestWriterError {
    #[error("Manifest already exists: {repository}/{reference}")]
    AlreadyExists { repository: String, reference: String },
    
    #[error("Invalid manifest format: {0}")]
    InvalidFormat(String),
    
    #[error("Repository not found: {0}")]
    RepositoryNotFound(String),
    
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    
    #[error("Storage error: {0}")]
    StorageError(String),
    
    #[error("Internal error: {0}")]
    InternalError(String),
}

/// Error types for Docker blob reader
#[derive(Debug, thiserror::Error)]
pub enum DockerBlobReaderError {
    #[error("Blob not found: {repository}/{digest}")]
    NotFound { repository: String, digest: String },
    
    #[error("Invalid digest format: {0}")]
    InvalidDigest(String),
    
    #[error("Repository not found: {0}")]
    RepositoryNotFound(String),
    
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    
    #[error("Storage error: {0}")]
    StorageError(String),
    
    #[error("Internal error: {0}")]
    InternalError(String),
}

/// Error types for Docker blob writer
#[derive(Debug, thiserror::Error)]
pub enum DockerBlobWriterError {
    #[error("Blob already exists: {repository}/{digest}")]
    AlreadyExists { repository: String, digest: String },
    
    #[error("Upload session not found: {0}")]
    UploadSessionNotFound(String),
    
    #[error("Invalid digest format: {0}")]
    InvalidDigest(String),
    
    #[error("Blob size exceeded limit: {size} bytes (max: {max})")]
    SizeExceeded { size: u64, max: u64 },
    
    #[error("Repository not found: {0}")]
    RepositoryNotFound(String),
    
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    
    #[error("Storage error: {0}")]
    StorageError(String),
    
    #[error("Internal error: {0}")]
    InternalError(String),
}

/// Error types for Docker repository manager
#[derive(Debug, thiserror::Error)]
pub enum DockerRepositoryManagerError {
    #[error("Repository not found: {0}")]
    RepositoryNotFound(String),
    
    #[error("Invalid repository name: {0}")]
    InvalidRepositoryName(String),
    
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    
    #[error("Storage error: {0}")]
    StorageError(String),
    
    #[error("Internal error: {0}")]
    InternalError(String),
}

/// Error types for Docker permission checker
#[derive(Debug, thiserror::Error)]
pub enum DockerPermissionError {
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    
    #[error("User not found: {0}")]
    UserNotFound(String),
    
    #[error("Repository not found: {0}")]
    RepositoryNotFound(String),
    
    #[error("Policy evaluation error: {0}")]
    PolicyEvaluationError(String),
    
    #[error("Internal error: {0}")]
    InternalError(String),
}

/// Mock implementations for testing
#[cfg(test)]
pub mod test {
    use super::*;
    use std::sync::Mutex;
    use std::collections::HashMap;
    use crate::features::handle_docker_request::dto::{
        DockerManifestV2, DockerDescriptor, DockerManifestMediaType,
        DockerBlobInfo, DockerRepositoryInfo, DockerTagInfo,
        DockerCatalogResponse, DockerTagsListResponse, DockerUploadSession,
    };
    use chrono::Utc;

    /// Mock Docker manifest reader
    pub struct MockDockerManifestReader {
        pub manifests: Mutex<HashMap<String, DockerManifest>>,
        pub manifest_infos: Mutex<HashMap<String, DockerManifestInfo>>,
    }

    impl MockDockerManifestReader {
        pub fn new() -> Self {
            Self {
                manifests: Mutex::new(HashMap::new()),
                manifest_infos: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl DockerManifestReader for MockDockerManifestReader {
        async fn get_manifest(
            &self,
            repository_name: &str,
            reference: &str,
        ) -> Result<DockerManifest, DockerManifestReaderError> {
            let key = format!("{}/{}", repository_name, reference);
            self.manifests.lock().unwrap()
                .get(&key)
                .cloned()
                .ok_or_else(|| DockerManifestReaderError::NotFound {
                    repository: repository_name.to_string(),
                    reference: reference.to_string(),
                })
        }

        async fn get_manifest_info(
            &self,
            repository_name: &str,
            reference: &str,
        ) -> Result<DockerManifestInfo, DockerManifestReaderError> {
            let key = format!("{}/{}", repository_name, reference);
            self.manifest_infos.lock().unwrap()
                .get(&key)
                .cloned()
                .ok_or_else(|| DockerManifestReaderError::NotFound {
                    repository: repository_name.to_string(),
                    reference: reference.to_string(),
                })
        }

        async fn manifest_exists(
            &self,
            repository_name: &str,
            reference: &str,
        ) -> Result<bool, DockerManifestReaderError> {
            let key = format!("{}/{}", repository_name, reference);
            Ok(self.manifests.lock().unwrap().contains_key(&key))
        }
    }

    /// Mock Docker manifest writer
    pub struct MockDockerManifestWriter {
        pub manifests: Mutex<HashMap<String, DockerManifest>>,
        pub manifest_infos: Mutex<HashMap<String, DockerManifestInfo>>,
    }

    impl MockDockerManifestWriter {
        pub fn new() -> Self {
            Self {
                manifests: Mutex::new(HashMap::new()),
                manifest_infos: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl DockerManifestWriter for MockDockerManifestWriter {
        async fn put_manifest(
            &self,
            repository_name: &str,
            reference: &str,
            manifest: &DockerManifest,
            media_type: &str,
        ) -> Result<String, DockerManifestWriterError> {
            let key = format!("{}/{}", repository_name, reference);
            let digest = format!("sha256:{}", reference); // Simplified for testing
            
            self.manifests.lock().unwrap().insert(key.clone(), manifest.clone());
            
            let manifest_info = DockerManifestInfo {
                digest: digest.clone(),
                media_type: DockerManifestMediaType::ManifestV2,
                size: 1024, // Simplified
                created_at: Utc::now(),
                tags: vec![reference.to_string()],
            };
            
            self.manifest_infos.lock().unwrap().insert(key, manifest_info);
            
            Ok(digest)
        }

        async fn delete_manifest(
            &self,
            repository_name: &str,
            reference: &str,
        ) -> Result<(), DockerManifestWriterError> {
            let key = format!("{}/{}", repository_name, reference);
            self.manifests.lock().unwrap().remove(&key);
            self.manifest_infos.lock().unwrap().remove(&key);
            Ok(())
        }
    }

    /// Mock Docker blob reader
    pub struct MockDockerBlobReader {
        pub blobs: Mutex<HashMap<String, Bytes>>,
        pub blob_infos: Mutex<HashMap<String, DockerBlobInfo>>,
    }

    impl MockDockerBlobReader {
        pub fn new() -> Self {
            Self {
                blobs: Mutex::new(HashMap::new()),
                blob_infos: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl DockerBlobReader for MockDockerBlobReader {
        async fn get_blob(
            &self,
            repository_name: &str,
            digest: &str,
        ) -> Result<Bytes, DockerBlobReaderError> {
            let key = format!("{}/{}", repository_name, digest);
            self.blobs.lock().unwrap()
                .get(&key)
                .cloned()
                .ok_or_else(|| DockerBlobReaderError::NotFound {
                    repository: repository_name.to_string(),
                    digest: digest.to_string(),
                })
        }

        async fn get_blob_info(
            &self,
            repository_name: &str,
            digest: &str,
        ) -> Result<DockerBlobInfo, DockerBlobReaderError> {
            let key = format!("{}/{}", repository_name, digest);
            self.blob_infos.lock().unwrap()
                .get(&key)
                .cloned()
                .ok_or_else(|| DockerBlobReaderError::NotFound {
                    repository: repository_name.to_string(),
                    digest: digest.to_string(),
                })
        }

        async fn blob_exists(
            &self,
            repository_name: &str,
            digest: &str,
        ) -> Result<bool, DockerBlobReaderError> {
            let key = format!("{}/{}", repository_name, digest);
            Ok(self.blobs.lock().unwrap().contains_key(&key))
        }
    }

    /// Mock Docker blob writer
    pub struct MockDockerBlobWriter {
        pub blobs: Mutex<HashMap<String, Bytes>>,
        pub blob_infos: Mutex<HashMap<String, DockerBlobInfo>>,
        pub upload_sessions: Mutex<HashMap<String, DockerUploadSession>>,
    }

    impl MockDockerBlobWriter {
        pub fn new() -> Self {
            Self {
                blobs: Mutex::new(HashMap::new()),
                blob_infos: Mutex::new(HashMap::new()),
                upload_sessions: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl DockerBlobWriter for MockDockerBlobWriter {
        async fn start_blob_upload(
            &self,
            repository_name: &str,
            upload_uuid: Option<&str>,
        ) -> Result<DockerUploadSession, DockerBlobWriterError> {
            let uuid = upload_uuid.unwrap_or(&format!("upload-{}", uuid::Uuid::new_v4()));
            let session = DockerUploadSession {
                upload_uuid: uuid.to_string(),
                repository_name: repository_name.to_string(),
                created_at: Utc::now(),
                expires_at: Utc::now() + chrono::Duration::hours(1),
                offset: 0,
                started: true,
                completed: false,
            };
            
            self.upload_sessions.lock().unwrap().insert(uuid.to_string(), session.clone());
            Ok(session)
        }

        async fn upload_blob(
            &self,
            repository_name: &str,
            digest: &str,
            data: Bytes,
        ) -> Result<String, DockerBlobWriterError> {
            let key = format!("{}/{}", repository_name, digest);
            let size = data.len() as u64;
            
            self.blobs.lock().unwrap().insert(key.clone(), data);
            
            let blob_info = DockerBlobInfo {
                digest: digest.to_string(),
                size,
                media_type: "application/octet-stream".to_string(),
                created_at: Utc::now(),
            };
            
            self.blob_infos.lock().unwrap().insert(key, blob_info);
            
            Ok(digest.to_string())
        }

        async fn complete_blob_upload(
            &self,
            repository_name: &str,
            upload_uuid: &str,
            digest: &str,
            data: Option<Bytes>,
        ) -> Result<String, DockerBlobWriterError> {
            if let Some(data) = data {
                self.upload_blob(repository_name, digest, data).await
            } else {
                Ok(digest.to_string())
            }
        }

        async fn delete_blob(
            &self,
            repository_name: &str,
            digest: &str,
        ) -> Result<(), DockerBlobWriterError> {
            let key = format!("{}/{}", repository_name, digest);
            self.blobs.lock().unwrap().remove(&key);
            self.blob_infos.lock().unwrap().remove(&key);
            Ok(())
        }
    }

    /// Mock Docker repository manager
    pub struct MockDockerRepositoryManager {
        pub repositories: Mutex<HashMap<String, DockerRepositoryInfo>>,
        pub tags: Mutex<HashMap<String, Vec<DockerTagInfo>>>,
    }

    impl MockDockerRepositoryManager {
        pub fn new() -> Self {
            Self {
                repositories: Mutex::new(HashMap::new()),
                tags: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl DockerRepositoryManager for MockDockerRepositoryManager {
        async fn get_repository_info(
            &self,
            repository_name: &str,
        ) -> Result<DockerRepositoryInfo, DockerRepositoryManagerError> {
            self.repositories.lock().unwrap()
                .get(repository_name)
                .cloned()
                .ok_or_else(|| DockerRepositoryManagerError::RepositoryNotFound(repository_name.to_string()))
        }

        async fn list_repositories(
            &self,
            last: Option<&str>,
            n: usize,
        ) -> Result<DockerCatalogResponse, DockerRepositoryManagerError> {
            let repos: Vec<String> = self.repositories.lock().unwrap()
                .keys()
                .filter(|name| last.map_or(true, |last| *name > last))
                .take(n)
                .cloned()
                .collect();
            
            Ok(DockerCatalogResponse {
                repositories: repos,
                next: None,
            })
        }

        async fn list_tags(
            &self,
            repository_name: &str,
            last: Option<&str>,
            n: usize,
        ) -> Result<DockerTagsListResponse, DockerRepositoryManagerError> {
            let tags = self.tags.lock().unwrap()
                .get(repository_name)
                .map(|tags| {
                    tags.iter()
                        .filter(|tag| last.map_or(true, |last| tag.name > last))
                        .take(n)
                        .map(|tag| tag.name.clone())
                        .collect()
                })
                .unwrap_or_default();
            
            Ok(DockerTagsListResponse {
                name: repository_name.to_string(),
                tags,
            })
        }

        async fn get_tag_info(
            &self,
            repository_name: &str,
            tag: &str,
        ) -> Result<DockerTagInfo, DockerRepositoryManagerError> {
            let tags = self.tags.lock().unwrap();
            let tag_info = tags.get(repository_name)
                .and_then(|tags| tags.iter().find(|t| t.name == tag))
                .cloned()
                .ok_or_else(|| DockerRepositoryManagerError::RepositoryNotFound(format!("{}/{}", repository_name, tag)))?;
            
            Ok(tag_info)
        }

        async fn repository_exists(
            &self,
            repository_name: &str,
        ) -> Result<bool, DockerRepositoryManagerError> {
            Ok(self.repositories.lock().unwrap().contains_key(repository_name))
        }
    }

    /// Mock Docker permission checker
    pub struct MockDockerPermissionChecker {
        pub permissions: Mutex<HashMap<String, bool>>,
    }

    impl MockDockerPermissionChecker {
        pub fn new() -> Self {
            Self {
                permissions: Mutex::new(HashMap::new()),
            }
        }
        
        pub fn allow_all(&self) {
            let mut perms = self.permissions.lock().unwrap();
            perms.insert("read".to_string(), true);
            perms.insert("write".to_string(), true);
            perms.insert("delete".to_string(), true);
            perms.insert("list".to_string(), true);
        }
        
        pub fn deny_all(&self) {
            let mut perms = self.permissions.lock().unwrap();
            perms.insert("read".to_string(), false);
            perms.insert("write".to_string(), false);
            perms.insert("delete".to_string(), false);
            perms.insert("list".to_string(), false);
        }
    }

    #[async_trait]
    impl DockerPermissionChecker for MockDockerPermissionChecker {
        async fn can_read(
            &self,
            _user_id: &str,
            _repository_name: &str,
        ) -> Result<bool, DockerPermissionError> {
            Ok(self.permissions.lock().unwrap().get("read").copied().unwrap_or(true))
        }

        async fn can_write(
            &self,
            _user_id: &str,
            _repository_name: &str,
        ) -> Result<bool, DockerPermissionError> {
            Ok(self.permissions.lock().unwrap().get("write").copied().unwrap_or(true))
        }

        async fn can_delete(
            &self,
            _user_id: &str,
            _repository_name: &str,
        ) -> Result<bool, DockerPermissionError> {
            Ok(self.permissions.lock().unwrap().get("delete").copied().unwrap_or(true))
        }

        async fn can_list(
            &self,
            _user_id: &str,
        ) -> Result<bool, DockerPermissionError> {
            Ok(self.permissions.lock().unwrap().get("list").copied().unwrap_or(true))
        }
    }
}