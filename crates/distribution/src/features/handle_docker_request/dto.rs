// crates/distribution/src/features/handle_docker_request/dto.rs

use serde::{Deserialize, Serialize};
use bytes::Bytes;
use thiserror::Error;
use axum::http::StatusCode;
use std::collections::HashMap;

/// Docker API error types
#[derive(Debug, Error)]
pub enum DockerApiError {
    #[error("Repository not found: {0}")]
    RepositoryNotFound(String),
    
    #[error("Manifest not found: {0}")]
    ManifestNotFound(String),
    
    #[error("Blob not found: {0}")]
    BlobNotFound(String),
    
    #[error("Invalid digest: {0}")]
    InvalidDigest(String),
    
    #[error("Invalid manifest: {0}")]
    InvalidManifest(String),
    
    #[error("Blob size exceeds limit: {0} bytes")]
    BlobSizeExceeded(u64),
    
    #[error("Forbidden: {0}")]
    Forbidden(String),
    
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    
    #[error("Upload session not found: {0}")]
    UploadSessionNotFound(String),
    
    #[error("Internal server error: {0}")]
    InternalServerError(String),
    
    #[error("Repository error: {0}")]
    RepositoryError(String),
}

impl DockerApiError {
    pub fn to_http_status(&self) -> StatusCode {
        match self {
            DockerApiError::RepositoryNotFound(_) => StatusCode::NOT_FOUND,
            DockerApiError::ManifestNotFound(_) => StatusCode::NOT_FOUND,
            DockerApiError::BlobNotFound(_) => StatusCode::NOT_FOUND,
            DockerApiError::InvalidDigest(_) => StatusCode::BAD_REQUEST,
            DockerApiError::InvalidManifest(_) => StatusCode::BAD_REQUEST,
            DockerApiError::BlobSizeExceeded(_) => StatusCode::PAYLOAD_TOO_LARGE,
            DockerApiError::Forbidden(_) => StatusCode::FORBIDDEN,
            DockerApiError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            DockerApiError::UploadSessionNotFound(_) => StatusCode::NOT_FOUND,
            DockerApiError::InternalServerError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            DockerApiError::RepositoryError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

/// Docker manifest media types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DockerManifestMediaType {
    #[serde(rename = "application/vnd.docker.distribution.manifest.v2+json")]
    ManifestV2,
    
    #[serde(rename = "application/vnd.docker.distribution.manifest.list.v2+json")]
    ManifestListV2,
    
    #[serde(rename = "application/vnd.docker.distribution.manifest.v1+json")]
    ManifestV1,
    
    #[serde(rename = "application/vnd.docker.distribution.manifest.v1+prettyjws")]
    ManifestV1Signed,
}

impl std::fmt::Display for DockerManifestMediaType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DockerManifestMediaType::ManifestV2 => write!(f, "application/vnd.docker.distribution.manifest.v2+json"),
            DockerManifestMediaType::ManifestListV2 => write!(f, "application/vnd.docker.distribution.manifest.list.v2+json"),
            DockerManifestMediaType::ManifestV1 => write!(f, "application/vnd.docker.distribution.manifest.v1+json"),
            DockerManifestMediaType::ManifestV1Signed => write!(f, "application/vnd.docker.distribution.manifest.v1+prettyjws"),
        }
    }
}

/// Docker blob media types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DockerBlobMediaType {
    #[serde(rename = "application/vnd.docker.image.rootfs.diff.tar.gzip")]
    LayerGzip,
    
    #[serde(rename = "application/vnd.docker.container.image.v1+json")]
    Config,
    
    #[serde(rename = "application/vnd.docker.plugin.v1+json")]
    PluginConfig,
}

/// Get manifest request
#[derive(Debug, Clone)]
pub struct GetManifestRequest {
    pub repository_name: String,
    pub reference: String, // can be tag or digest
    pub user_id: String,
    pub accept_headers: Vec<String>,
}

/// Get manifest response
#[derive(Debug, Clone)]
pub struct GetManifestResponse {
    pub manifest: DockerManifest,
    pub content_type: DockerManifestMediaType,
    pub digest: String,
    pub content_length: u64,
    pub docker_content_digest: String,
}

/// Put manifest request
#[derive(Debug, Clone)]
pub struct PutManifestRequest {
    pub repository_name: String,
    pub reference: String,
    pub manifest: DockerManifest,
    pub content_type: DockerManifestMediaType,
    pub user_id: String,
    pub content_length: u64,
}

/// Put manifest response
#[derive(Debug, Clone)]
pub struct PutManifestResponse {
    pub location: String,
    pub docker_content_digest: String,
}

/// Head manifest request
#[derive(Debug, Clone)]
pub struct HeadManifestRequest {
    pub repository_name: String,
    pub reference: String,
    pub user_id: String,
}

/// Head manifest response
#[derive(Debug, Clone)]
pub struct HeadManifestResponse {
    pub content_length: u64,
    pub docker_content_digest: String,
    pub content_type: DockerManifestMediaType,
}

/// Delete manifest request
#[derive(Debug, Clone)]
pub struct DeleteManifestRequest {
    pub repository_name: String,
    pub reference: String,
    pub user_id: String,
}

/// Delete manifest response
#[derive(Debug, Clone)]
pub struct DeleteManifestResponse {
    pub success: bool,
}

/// Get blob request
#[derive(Debug, Clone)]
pub struct GetBlobRequest {
    pub repository_name: String,
    pub digest: String,
    pub user_id: String,
}

/// Get blob response
#[derive(Debug, Clone)]
pub struct GetBlobResponse {
    pub data: Bytes,
    pub digest: String,
    pub content_length: u64,
    pub content_type: String,
}

/// Put blob request
#[derive(Debug, Clone)]
pub struct PutBlobRequest {
    pub repository_name: String,
    pub digest: String,
    pub data: Bytes,
    pub user_id: String,
    pub content_length: u64,
}

/// Put blob response
#[derive(Debug, Clone)]
pub struct PutBlobResponse {
    pub location: String,
    pub docker_content_digest: String,
}

/// Head blob request
#[derive(Debug, Clone)]
pub struct HeadBlobRequest {
    pub repository_name: String,
    pub digest: String,
    pub user_id: String,
}

/// Head blob response
#[derive(Debug, Clone)]
pub struct HeadBlobResponse {
    pub content_length: u64,
    pub docker_content_digest: String,
    pub content_type: String,
}

/// Delete blob request
#[derive(Debug, Clone)]
pub struct DeleteBlobRequest {
    pub repository_name: String,
    pub digest: String,
    pub user_id: String,
}

/// Delete blob response
#[derive(Debug, Clone)]
pub struct DeleteBlobResponse {
    pub success: bool,
}

/// Start blob upload request
#[derive(Debug, Clone)]
pub struct StartBlobUploadRequest {
    pub repository_name: String,
    pub user_id: String,
    pub upload_uuid: Option<String>,
}

/// Start blob upload response
#[derive(Debug, Clone)]
pub struct StartBlobUploadResponse {
    pub upload_location: String,
    pub upload_uuid: String,
    pub range: String,
}

/// Complete blob upload request
#[derive(Debug, Clone)]
pub struct CompleteBlobUploadRequest {
    pub repository_name: String,
    pub upload_uuid: String,
    pub digest: String,
    pub data: Option<Bytes>,
    pub user_id: String,
}

/// Complete blob upload response
#[derive(Debug, Clone)]
pub struct CompleteBlobUploadResponse {
    pub location: String,
    pub docker_content_digest: String,
    pub upload_uuid: String,
}

/// Docker manifest structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "schemaVersion")]
pub enum DockerManifest {
    #[serde(rename = "2")]
    V2(DockerManifestV2),
    
    #[serde(rename = "1")]
    V1(DockerManifestV1),
}

/// Docker manifest v2
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerManifestV2 {
    pub schema_version: u32,
    pub media_type: DockerManifestMediaType,
    pub config: DockerDescriptor,
    pub layers: Vec<DockerDescriptor>,
}

/// Docker manifest v1
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerManifestV1 {
    pub schema_version: u32,
    pub name: String,
    pub tag: String,
    pub architecture: String,
    pub fs_layers: Vec<DockerFsLayer>,
    pub history: Vec<DockerHistory>,
    pub signatures: Option<Vec<DockerSignature>>,
}

/// Docker descriptor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerDescriptor {
    pub media_type: String,
    pub size: u64,
    pub digest: String,
    pub urls: Option<Vec<String>>,
}

/// Docker filesystem layer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerFsLayer {
    pub blob_sum: String,
}

/// Docker history entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerHistory {
    pub v1_compatibility: String,
}

/// Docker signature
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerSignature {
    pub header: DockerSignatureHeader,
    pub signature: String,
    pub protected: String,
}

/// Docker signature header
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerSignatureHeader {
    pub jwk: DockerJwk,
    pub alg: String,
}

/// Docker JSON Web Key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerJwk {
    pub crv: String,
    pub kid: String,
    pub kty: String,
    pub x: String,
    pub y: String,
}

/// Docker repository information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerRepositoryInfo {
    pub name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub manifest_count: u64,
    pub blob_count: u64,
    pub total_size: u64,
}

/// Docker tag information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerTagInfo {
    pub name: String,
    pub digest: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub size: u64,
}

/// Docker catalog response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerCatalogResponse {
    pub repositories: Vec<String>,
    pub next: Option<String>,
}

/// Docker tags list response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerTagsListResponse {
    pub name: String,
    pub tags: Vec<String>,
}

/// Docker upload session
#[derive(Debug, Clone)]
pub struct DockerUploadSession {
    pub upload_uuid: String,
    pub repository_name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub offset: u64,
    pub started: bool,
    pub completed: bool,
}

/// Docker blob info
#[derive(Debug, Clone)]
pub struct DockerBlobInfo {
    pub digest: String,
    pub size: u64,
    pub media_type: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Docker manifest info
#[derive(Debug, Clone)]
pub struct DockerManifestInfo {
    pub digest: String,
    pub media_type: DockerManifestMediaType,
    pub size: u64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub tags: Vec<String>,
}

/// Helper functions for validation
pub fn validate_docker_repository_name(name: &str) -> Result<(), DockerApiError> {
    if name.is_empty() {
        return Err(DockerApiError::InvalidManifest("Repository name cannot be empty".to_string()));
    }
    
    if name.len() > 255 {
        return Err(DockerApiError::InvalidManifest("Repository name too long".to_string()));
    }
    
    // Docker repository name validation rules
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '/') {
        return Err(DockerApiError::InvalidManifest("Repository name contains invalid characters".to_string()));
    }
    
    if name.starts_with('-') || name.ends_with('-') {
        return Err(DockerApiError::InvalidManifest("Repository name cannot start or end with hyphen".to_string()));
    }
    
    if name.starts_with('_') || name.ends_with('_') {
        return Err(DockerApiError::InvalidManifest("Repository name cannot start or end with underscore".to_string()));
    }
    
    Ok(())
}

pub fn validate_docker_tag(tag: &str) -> Result<(), DockerApiError> {
    if tag.is_empty() {
        return Err(DockerApiError::InvalidManifest("Tag cannot be empty".to_string()));
    }
    
    if tag.len() > 128 {
        return Err(DockerApiError::InvalidManifest("Tag too long".to_string()));
    }
    
    // Docker tag validation rules
    if !tag.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.') {
        return Err(DockerApiError::InvalidManifest("Tag contains invalid characters".to_string()));
    }
    
    Ok(())
}

pub fn validate_docker_digest(digest: &str) -> Result<(), DockerApiError> {
    if digest.is_empty() {
        return Err(DockerApiError::InvalidDigest("Digest cannot be empty".to_string()));
    }
    
    // Basic digest validation - should be algorithm:hex
    let parts: Vec<&str> = digest.split(':').collect();
    if parts.len() != 2 {
        return Err(DockerApiError::InvalidDigest("Invalid digest format".to_string()));
    }
    
    let algorithm = parts[0];
    let hash = parts[1];
    
    // Supported algorithms
    match algorithm {
        "sha256" => {
            if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(DockerApiError::InvalidDigest("Invalid SHA256 hash".to_string()));
            }
        }
        "sha512" => {
            if hash.len() != 128 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(DockerApiError::InvalidDigest("Invalid SHA512 hash".to_string()));
            }
        }
        _ => return Err(DockerApiError::InvalidDigest(format!("Unsupported digest algorithm: {}", algorithm))),
    }
    
    Ok(())
}