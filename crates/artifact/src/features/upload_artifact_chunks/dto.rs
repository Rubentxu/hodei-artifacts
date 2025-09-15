use crate::domain::package_version::PackageCoordinates;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use shared::hrn::Hrn;

/// Command to initiate a chunked upload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitiateChunkedUploadCommand {
    /// Package HRN for the artifact being uploaded
    pub package_hrn: Hrn,

    /// Repository HRN where the artifact will be stored
    pub repository_hrn: Hrn,

    /// Artifact coordinates
    pub coordinates: PackageCoordinates,

    /// Total size of the artifact in bytes
    pub total_size: u64,

    /// Number of chunks the file will be split into
    pub chunk_count: u32,

    /// Artifact type (maven, npm, etc.)
    pub artifact_type: String,

    /// Content type of the artifact
    pub content_type: String,

    /// Checksum of the complete file (for verification)
    pub checksum: String,

    /// Metadata about the upload
    pub metadata: std::collections::HashMap<String, String>,
}

/// Command to upload a chunk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadChunkCommand {
    /// Upload session ID
    pub session_id: String,

    /// Chunk sequence number (0-based)
    pub chunk_number: u32,

    /// Chunk data
    pub chunk_data: Bytes,

    /// Checksum of this chunk
    pub chunk_checksum: String,
}

/// Command to complete a chunked upload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteChunkedUploadCommand {
    /// Upload session ID
    pub session_id: String,

    /// Final checksum verification
    pub final_checksum: String,
}

/// Command to abort a chunked upload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbortChunkedUploadCommand {
    /// Upload session ID
    pub session_id: String,

    /// Reason for aborting
    pub reason: Option<String>,
}

/// Result of initiating a chunked upload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitiateChunkedUploadResult {
    /// Upload session ID
    pub session_id: String,

    /// Package HRN
    pub package_hrn: Hrn,

    /// Expected chunk size (if applicable)
    pub chunk_size: Option<u32>,

    /// Upload URL template for chunks
    pub upload_url_template: String,

    /// Expiration time for the session
    pub expires_at: time::OffsetDateTime,
}

/// Result of uploading a chunk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadChunkResult {
    /// Upload session ID
    pub session_id: String,

    /// Chunk number
    pub chunk_number: u32,

    /// Whether the chunk was successfully uploaded
    pub success: bool,

    /// Bytes received
    pub bytes_received: u64,

    /// Upload status message
    pub message: Option<String>,
}

/// Result of completing a chunked upload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteChunkedUploadResult {
    /// Upload session ID
    pub session_id: String,

    /// Package HRN
    pub package_hrn: Hrn,

    /// Whether the upload was successfully completed
    pub success: bool,

    /// Final artifact storage path
    pub storage_path: Option<String>,

    /// Upload statistics
    pub stats: UploadStats,
}

/// Upload statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadStats {
    /// Total bytes uploaded
    pub total_bytes: u64,

    /// Number of chunks uploaded
    pub chunks_uploaded: u32,

    /// Total upload time in milliseconds
    pub upload_duration_ms: u64,

    /// Average upload speed in bytes per second
    pub avg_speed_bps: f64,
}

/// Upload session information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadSession {
    /// Session ID
    pub session_id: String,

    /// Package HRN
    pub package_hrn: Hrn,

    /// Repository HRN
    pub repository_hrn: Hrn,

    /// Artifact coordinates
    pub coordinates: PackageCoordinates,

    /// Total size of the artifact
    pub total_size: u64,

    /// Number of chunks
    pub chunk_count: u32,

    /// Artifact type
    pub artifact_type: String,

    /// Content type
    pub content_type: String,

    /// Expected checksum
    pub expected_checksum: String,

    /// Upload status
    pub status: UploadSessionStatus,

    /// Created at
    pub created_at: time::OffsetDateTime,

    /// Expires at
    pub expires_at: time::OffsetDateTime,

    /// Metadata
    pub metadata: std::collections::HashMap<String, String>,
}

/// Upload session status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum UploadSessionStatus {
    /// Upload is in progress
    InProgress,

    /// Upload is completed
    Completed,

    /// Upload is aborted
    Aborted,

    /// Upload is expired
    Expired,

    /// Upload failed
    Failed,
}
