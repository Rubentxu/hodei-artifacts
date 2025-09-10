
// crates/distribution/src/features/handle_docker_request/adapter.rs

use std::sync::Arc;
use async_trait::async_trait;
use bytes::Bytes;
use tracing::{info, warn, error, instrument};
use aws_sdk_s3::Client as S3Client;
use aws_sdk_s3::types::ByteStream;
use mongodb::{Client as MongoClient, Collection};
use mongodb::bson::{doc, Document};
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

use crate::features::handle_docker_request::dto::{
    DockerManifest, DockerManifestInfo, DockerBlobInfo, DockerUploadSession,
    DockerRepositoryInfo, DockerTagInfo, DockerCatalogResponse, DockerTagsListResponse,
    DockerManifestMediaType, DockerManifestV2, DockerDescriptor,
};

use super::ports::{
    DockerManifestReader, DockerManifestWriter, DockerBlobReader, DockerBlobWriter,
    DockerRepositoryManager, DockerPermissionChecker,
    DockerManifestReaderError, DockerManifestWriterError, DockerBlobReaderError, DockerBlobWriterError,
    DockerRepositoryManagerError, DockerPermissionError,
};

/// S3-based Docker manifest reader implementation
pub struct S3DockerManifestReader {
    s3_client: Arc<S3Client>,
    bucket_name: String,
}

impl S3DockerManifestReader {
    pub fn new(s3_client: Arc<S3Client>, bucket_name: String) -> Self {
        Self {
            s3_client,
            bucket_name,
        }
    }

    fn get_manifest_key(&self, repository_name: &str, reference: &str) -> String {
        format!("docker/{}/manifests/{}", repository_name, reference)
    }
}

#[async_trait]
impl DockerManifestReader for S3DockerManifestReader {
    #[instrument(skip(self))]
    async fn get_manifest(
        &self,
        repository_name: &str,
        reference: &str,
    ) -> Result<DockerManifest, DockerManifestReaderError> {
        info!("Getting Docker manifest from S3: {}/{}", repository_name, reference);
        
        let key = self.get_manifest_key(repository_name, reference);
        
        let response = self.s3_client
            .get_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .send()
            .await
            .map_err(|e| {
                error!("Failed to get manifest from S3: {}", e);
                if e.to_string().contains("NoSuchKey") {
                    DockerManifestReaderError::NotFound {
                        repository: repository_name.to_string(),
                        reference: reference.to_string(),
                    }
                } else {
                    DockerManifestReaderError::StorageError(e.to_string())
                }
            })?;
        
        let data = response.body.collect().await
            .map_err(|e| DockerManifestReaderError::StorageError(e.to_string()))?;
        
        let manifest: DockerManifest = serde_json::from_slice(&data)
            .map_err(|e| DockerManifestReaderError::InvalidFormat(e.to_string()))?;
        
        info!("Successfully retrieved Docker manifest from S3: {}/{}", repository_name, reference);
        Ok(manifest)
    }

    #[instrument(skip(self))]
    async fn get_manifest_info(
        &self,
        repository_name: &str,
        reference: &str,
    ) -> Result<DockerManifestInfo, DockerManifestReaderError> {
        info!("Getting Docker manifest info from S3: {}/{}", repository_name, reference);
        
        let key = self.get_manifest_key(repository_name, reference);
        
        let response = self.s3_client
            .head_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .send()
            .await
            .map_err(|e| {
                error!("Failed to get manifest info from S3: {}", e);
                if e.to_string().contains("NoSuchKey") {
                    DockerManifestReaderError::NotFound {
                        repository: repository_name.to_string(),
                        reference: reference.to_string(),
                    }
                } else {
                    DockerManifestReaderError::StorageError(e.to_string())
                }
            })?;
        
        let digest = response.e_tag.ok_or_else(|| {
            DockerManifestReaderError::InvalidFormat("Missing ETag".to_string())
        })?;
        
        let size = response.content_length as u64;
        let last_modified = response.last_modified.map(|dt| {
            DateTime::from_timestamp(dt.secs(), dt.subsec_nanos() as u32)
                .unwrap_or_else(|| Utc::now())
        }).unwrap_or_else(|| Utc::now());
        
        let manifest_info = DockerManifestInfo {
            digest,
            media_type: DockerManifestMediaType::ManifestV2, // Default, should be determined from content
            size,
            created_at: last_modified,
            tags: vec![reference.to_string()],
        };
        
        info!("Successfully retrieved Docker manifest info from S3: {}/{}", repository_name, reference);
        Ok(manifest_info)
    }

    #[instrument(skip(self))]
    async fn manifest_exists(
        &self,
        repository_name: &str,
        reference: &str,
    ) -> Result<bool, DockerManifestReaderError> {
        info!("Checking Docker manifest existence in S3: {}/{}", repository_name, reference);
        
        let key = self.get_manifest_key(repository_name, reference);
        
        match self.s3_client
            .head_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .send()
            .await
        {
            Ok(_) => {
                info!("Docker manifest exists in S3: {}/{}", repository_name, reference);
                Ok(true)
            }
            Err(e) => {
                if e.to_string().contains("NoSuchKey") {
                    info!("Docker manifest does not exist in S3: {}/{}", repository_name, reference);
                    Ok(false)
                } else {
                    error!("Failed to check manifest existence in S3: {}", e);
                    Err(DockerManifestReaderError::StorageError(e.to_string()))
                }
            }
        }
    }
}

/// S3-based Docker manifest writer implementation
pub struct S3DockerManifestWriter {
    s3_client: Arc<S3Client>,
    bucket_name: String,
}

impl S3DockerManifestWriter {
    pub fn new(s3_client: Arc<S3Client>, bucket_name: String) -> Self {
        Self {
            s3_client,
            bucket_name,
        }
    }

    fn get_manifest_key(&self, repository_name: &str, reference: &str) -> String {
        format!("docker/{}/manifests/{}", repository_name, reference)
    }
}

#[async_trait]
impl DockerManifestWriter for S3DockerManifestWriter {
    #[instrument(skip(self, manifest))]
    async fn put_manifest(
        &self,
        repository_name: &str,
        reference: &str,
        manifest: &DockerManifest,
        media_type: &str,
    ) -> Result<String, DockerManifestWriterError> {
        info!("Putting Docker manifest to S3: {}/{}", repository_name, reference);
        
        let key = self.get_manifest_key(repository_name, reference);
        
        let manifest_json = serde_json::to_vec(manifest)
            .map_err(|e| DockerManifestWriterError::InvalidFormat(e.to_string()))?;
        
        let digest = format!("sha256:{:x}", sha2::Sha256::digest(&manifest_json));
        
        let byte_stream = ByteStream::from(manifest_json);
        
        self.s3_client
            .put_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .body(byte_stream)
            .content_type(media_type)
            .send()
            .await
            .map_err(|e| {
                error!("Failed to put manifest to S3: {}", e);
                DockerManifestWriterError::StorageError(e.to_string())
            })?;
        
        info!("Successfully stored Docker manifest to S3: {}/{}", repository_name, reference);
        Ok(digest)
    }

    #[instrument(skip(self))]
    async fn delete_manifest(
        &self,
        repository_name: &str,
        reference: &str,
    ) -> Result<(), DockerManifestWriterError> {
        info!("Deleting Docker manifest from S3: {}/{}", repository_name, reference);
        
        let key = self.get_manifest_key(repository_name, reference);
        
        self.s3_client
            .delete_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .send()
            .await
            .map_err(|e| {
                error!("Failed to delete manifest from S3: {}", e);
                DockerManifestWriterError::StorageError(e.to_string())
            })?;
        
        info!("Successfully deleted Docker manifest from S3: {}/{}", repository_name, reference);
        Ok(())
    }
}

/// S3-based Docker blob reader implementation
pub struct S3DockerBlobReader {
    s3_client: Arc<S3Client>,
    bucket_name: String,
}

impl S3DockerBlobReader {
    pub fn new(s3_client: Arc<S3Client>, bucket_name: String) -> Self {
        Self {
            s3_client,
            bucket_name,
        }
    }

    fn get_blob_key(&self, repository_name: &str, digest: &str) -> String {
        format!("docker/{}/blobs/{}", repository_name, digest)
    }
}

#[async_trait]
impl DockerBlobReader for S3DockerBlobReader {
    #[instrument(skip(self))]
    async fn get_blob(
        &self,
        repository_name: &str,
        digest: &str,
    ) -> Result<Bytes, DockerBlobReaderError> {
        info!("Getting Docker blob from S3: {}/{}", repository_name, digest);
        
        let key = self.get_blob_key(repository_name, digest);
        
        let response = self.s3_client
            .get_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .send()
            .await
            .map_err(|e| {
                error!("Failed to get blob from S3: {}", e);
                if e.to_string().contains("NoSuchKey") {
                    DockerBlobReaderError::NotFound {
                        repository: repository_name.to_string(),
                        digest: digest.to_string(),
                    }
                } else {
                    DockerBlobReaderError::StorageError(e.to_string())
                }
            })?;
        
        let data = response.body.collect().await
            .map_err(|e| DockerBlobReaderError::StorageError(e.to_string()))?;
        
        info!("Successfully retrieved Docker blob from S3: {}/{}", repository_name, digest);
        Ok(data.into_bytes())
    }

    #[instrument(skip(self))]
    async fn get_blob_info(
        &self,
        repository_name: &str,
        digest: &str,
    ) -> Result<DockerBlobInfo, DockerBlobReaderError> {
        info!("Getting Docker blob info from S3: {}/{}", repository_name, digest);
        
        let key = self.get_blob_key(repository_name, digest);
        
        let response = self.s3_client
            .head_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .send()
            .await
            .map_err(|e| {
                error!("Failed to get blob info from S3: {}", e);
                if e.to_string().contains("NoSuchKey") {
                    DockerBlobReaderError::NotFound {
                        repository: repository_name.to_string(),
                        digest: digest.to_string(),
                    }
                } else {
                    DockerBlobReaderError::StorageError(e.to_string())
                }
            })?;
        
        let size = response.content_length as u64;
        let content_type = response.content_type.unwrap_or_else(|| "application/octet-stream".to_string());
        let last_modified = response.last_modified.map(|dt| {
            DateTime::from_timestamp(dt.secs(), dt.subsec_nanos() as u32)
                .unwrap_or_else(|| Utc::now())
        }).unwrap_or_else(|| Utc::now());
        
        let blob_info = DockerBlobInfo {
            digest: digest.to_string(),
            size,
            media_type: content_type,
            created_at: last_modified,
        };
        
        info!("Successfully retrieved Docker blob info from S3: {}/{}", repository_name, digest);
        Ok(blob_info)
    }

    #[instrument(skip(self))]
    async fn blob_exists(
        &self,
        repository_name: &str,
        digest: &str,
    ) -> Result<bool, DockerBlobReaderError> {
        info!("Checking Docker blob existence in S3: {}/{}", repository_name, digest);
        
        let key = self.get_blob_key(repository_name, digest);
        
        match self.s3_client
            .head_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .send()
            .await
        {
            Ok(_) => {
                info!("Docker blob exists in S3: {}/{}", repository_name, digest);
                Ok(true)
            }
            Err(e) => {
                if e.to_string().contains("NoSuchKey") {
                    info!("Docker blob does not exist in S3: {}/{}", repository_name, digest);
                    Ok(false)
                } else {
                    error!("Failed to check blob existence in S3: {}", e);
                    Err(DockerBlobReaderError::StorageError(e.to_string()))
                }
            }
        }
    }
}

/// S3-based Docker blob writer implementation
pub struct S3DockerBlobWriter {
    s3_client: Arc<S3Client>,
    bucket_name: String,
}

impl S3DockerBlobWriter {
    pub fn new(s3_client: Arc<S3Client>, bucket_name: String) -> Self {
        Self {
            s3_client,
            bucket_name,
        }
    }

    fn get_blob_key(&self, repository_name: &str, digest: &str) -> String {
        format!("docker/{}/blobs/{}", repository_name, digest)
    }

    fn get_upload_key(&self, repository_name: &str, upload_uuid: &str) -> String {
        format!("docker/{}/uploads/{}", repository_name, upload_uuid)
    }
}

#[async_trait]
impl DockerBlobWriter for S3DockerBlobWriter {
    #[instrument(skip(self))]
    async fn start_blob_upload(
        &self,
        repository_name: &str,
        upload_uuid: Option<&str>,
    ) -> Result<DockerUploadSession, DockerBlobWriterError> {
        info!("Starting Docker blob upload for repository: {}", repository_name);
        
        let uuid = upload_uuid.unwrap_or(&uuid::Uuid::new_v4().to_string());
        let upload_key = self.get_upload_key(repository_name, uuid);
        
        // Create empty upload file
        let empty_data = ByteStream::from(vec![]);
        
        self.s3_client
            .put_object()
            .bucket(&self.bucket_name)
            .key(&upload_key)
            .body(empty_data)
            .send()
            .await
            .map_err(|e| {
                error!("Failed to start blob upload in S3: {}", e);
                DockerBlobWriterError::StorageError(e.to_string())
            })?;
        
        let session = DockerUploadSession {
            upload_uuid: uuid.to_string(),
            repository_name: repository_name.to_string(),
            created_at: Utc::now(),
            expires_at: Utc::now() + chrono::Duration::hours(1),
            offset: 0,
            started: true,
            completed: false,
        };
        
        info!("Successfully started Docker blob upload: {}", uuid);
        Ok(session)
    }

    #[instrument(skip(self, data))]
    async fn upload_blob(
        &self,
        repository_name: &str,
        digest: &str,
        data: Bytes,
    ) -> Result<String, DockerBlobWriterError> {
        info!("Uploading Docker blob to S3: {}/{}", repository_name, digest);
        
        // Validate digest
        if !digest.starts_with("sha256:") {
            return Err(DockerBlobWriterError::InvalidDigest(
                "Only SHA256 digests are supported".to_string()
            ));
        }
        
        let key = self.get_blob_key(repository_name, digest);
        let size = data.len() as u64;
        
        // Check size limit (100MB)
        const MAX_BLOB_SIZE: u64 = 100 * 1024 * 1024;
        if size > MAX_BLOB_SIZE {
            return Err(DockerBlobWriterError::SizeExceeded {
                size,
                max: MAX_BLOB_SIZE,
            });
        }
        
        let byte_stream = ByteStream::from(data);
        
        self.s3_client
            .put_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .body(byte_stream)
            .content_type("application/octet-stream")
            .send()
            .await
            .map_err(|e| {
                error!("Failed to upload blob to S3: {}", e);
                DockerBlobWriterError::StorageError(e.to_string())
            })?;
        
        info!("Successfully uploaded Docker blob to S3: {}/{}", repository_name, digest);
        Ok(digest.to_string())
    }

    #[instrument(skip(self, data))]
    async fn complete_blob_upload(
        &self,
        repository_name: &str,
        upload_uuid: &str,
        digest: &str,
        data: Option<Bytes>,
    ) -> Result<String, DockerBlobWriterError> {
        info!("Completing Docker blob upload: {} with digest: {}", upload_uuid, digest);
        
        if let Some(data) = data {
            // Upload the final data
            self.upload_blob(repository_name, digest, data).await
        } else {
            // Just validate the upload session exists
            let upload_key = self.get_upload_key(repository_name, upload_uuid);
            
            self.s3_client
                .head_object()
                .bucket(&self.bucket_name)
                .key(&upload_key)
                .send()
                .await
                .map_err(|e| {
                    if e.to_string().contains("NoSuchKey") {
                        DockerBlobWriterError::UploadSessionNotFound(upload_uuid.to_string())
                    } else {
                        DockerBlobWriterError::StorageError(e.to_string())
                    }
                })?;
            
            Ok(digest.to_string())
        }
    }

    #[instrument(skip(self))]
    async fn delete_blob(
        &self,
        repository_name: &str,
        digest: &str,
    ) -> Result<(), DockerBlobWriterError> {
        info!("Deleting Docker blob from S3: {}/{}", repository_name, digest);
        
        let key = self.get_blob_key(repository_name, digest);
        
        self.s3_client
            .delete_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .send()
            .await
            .map_err(|e| {
                error!("Failed to delete blob from S3: {}", e);
                DockerBlobWriterError::StorageError(e.to_string())
            })?;
        
        info!("Successfully deleted Docker blob from S3: {}/{}", repository_name, digest);
        Ok(())
    }
}

/// MongoDB-based Docker repository manager implementation
#[derive(Serialize, Deserialize)]
struct DockerRepositoryDocument {
    name: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    manifest_count: u64,
    blob_count: u64,
    total_size: u64,
}

#[derive(Serialize, Deserialize)]
struct DockerTagDocument {
    name: String,
    repository_name: String,
    digest: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    size: u64,
}

pub struct MongoDockerRepositoryManager {
    repository_collection: Collection<DockerRepositoryDocument>,
    tag_collection: Collection<DockerTagDocument>,
}

impl MongoDockerRepositoryManager {
    pub fn new(mongo_client: &MongoClient, database_name: &str) -> Self {
        let db = mongo_client.database(database_name);
        let repository_collection = db.collection("docker_repositories");
        let tag_collection = db.collection("docker_tags");
        
        Self {
            repository_collection,
            tag_collection,
        }
    }
}

#[async_trait]
impl DockerRepositoryManager for MongoDockerRepositoryManager {
    #[instrument(skip(self))]
    async fn get_repository_info(
        &self,
        repository_name: &str,
    ) -> Result<DockerRepositoryInfo, DockerRepositoryManagerError> {
        info!("Getting Docker repository info from MongoDB: {}", repository_name);
        
        let filter = doc! { "name": repository_name };
        
        let doc = self.repository_collection
            .find_one(filter)
            .await
            .map_err(|e| {
                error!("Failed to get repository info from MongoDB: {}", e);
                DockerRepositoryManagerError::StorageError(e.to_string())
            })?
            .ok_or_else(|| DockerRepositoryManagerError::RepositoryNotFound(repository_name.to_string()))?;
        
        let info = DockerRepositoryInfo {
            name: doc.name,
            created_at: doc.created_at,
            updated_at: doc.updated_at,
            manifest_count: doc.manifest_count,
            blob_count: doc.blob_count,
            total_size: doc.total_size,
        };
        
        info!("Successfully retrieved Docker repository info from MongoDB: {}", repository_name);
        Ok(info)
    }

    #[instrument(skip(self))]
    async fn list_repositories(
        &self,
        last: Option<&str>,
        n: usize,
    ) -> Result<DockerCatalogResponse, DockerRepositoryManagerError> {
        info!("Listing Docker repositories from MongoDB");
        
        let mut filter = doc! {};
        if let Some(last_repo) = last {
            filter.insert("name", doc! { "$gt": last_repo });
        }
        
        let cursor = self.repository_collection
            .find(filter)
            .limit(n as i64)
            .await
            .map_err(|e| {
                error!("Failed to list repositories from MongoDB: {}", e);
                DockerRepositoryManagerError::StorageError(e.to_string())
            })?;
        
        let mut repositories = Vec::new();
        let mut count = 0;
        
        while let Some(doc) = cursor.try_next().await.map_err(|e| {
            error!("Failed to iterate repositories cursor: {}", e);
            DockerRepositoryManagerError::StorageError(e.to_string())
        })? {
            repositories.push(doc.name);
            count += 1;
        }
        
        let response = DockerCatalogResponse {
            repositories,
            count,
        };
        
        info!("Successfully listed {} Docker repositories from MongoDB", count);
        Ok(response)
    }
}