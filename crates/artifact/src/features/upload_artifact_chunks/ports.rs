use super::{dto::UploadSession, error::ChunkedUploadError};
use async_trait::async_trait;
use bytes::Bytes;
use shared::hrn::Hrn;

// Define a type alias for the Result type used in ports
pub type PortResult<T> = Result<T, ChunkedUploadError>;

/// Repository port for managing upload sessions
#[async_trait]
pub trait ChunkedUploadSessionRepository: Send + Sync {
    /// Create a new upload session
    async fn create_session(&self, session: &UploadSession) -> PortResult<()>;

    /// Get an upload session by ID
    async fn get_session(&self, session_id: &str) -> PortResult<Option<UploadSession>>;

    /// Update an upload session
    async fn update_session(&self, session: &UploadSession) -> PortResult<()>;

    /// Delete an upload session
    async fn delete_session(&self, session_id: &str) -> PortResult<()>;

    /// List active sessions for a repository
    async fn list_active_sessions(&self, repository_hrn: &Hrn) -> PortResult<Vec<UploadSession>>;

    /// Cleanup expired sessions
    async fn cleanup_expired_sessions(&self) -> PortResult<u32>;
}

/// Port for storing chunk data
#[async_trait]
pub trait ChunkStorage: Send + Sync {
    /// Store a chunk of data
    async fn store_chunk(
        &self,
        session_id: &str,
        chunk_number: u32,
        data: Bytes,
    ) -> PortResult<String>;

    /// Retrieve a chunk of data
    async fn retrieve_chunk(
        &self,
        session_id: &str,
        chunk_number: u32,
    ) -> PortResult<Option<Bytes>>;

    /// Check if a chunk exists
    async fn chunk_exists(&self, session_id: &str, chunk_number: u32) -> PortResult<bool>;

    /// Delete all chunks for a session
    async fn delete_session_chunks(&self, session_id: &str) -> PortResult<()>;

    /// Assemble chunks into final file
    async fn assemble_chunks(&self, session_id: &str, output_path: &str) -> PortResult<String>;

    /// Get total size of stored chunks for a session
    async fn get_session_chunks_size(&self, session_id: &str) -> PortResult<u64>;
}

/// Port for publishing upload events
#[async_trait]
pub trait ChunkedUploadEventPublisher: Send + Sync {
    /// Publish chunk upload initiated event
    async fn publish_upload_initiated(&self, event: ChunkedUploadEvent) -> PortResult<()>;

    /// Publish chunk uploaded event
    async fn publish_chunk_uploaded(&self, event: ChunkedUploadEvent) -> PortResult<()>;

    /// Publish chunk upload completed event
    async fn publish_upload_completed(&self, event: ChunkedUploadEvent) -> PortResult<()>;

    /// Publish chunk upload aborted event
    async fn publish_upload_aborted(&self, event: ChunkedUploadEvent) -> PortResult<()>;

    /// Publish chunk upload failed event
    async fn publish_upload_failed(&self, event: ChunkedUploadEvent) -> PortResult<()>;
}

/// Port for upload progress tracking
#[async_trait]
pub trait ChunkedUploadProgressTracker: Send + Sync {
    /// Initialize progress for a new session
    async fn init_session_progress(
        &self,
        session_id: &str,
        total_chunks: u32,
        total_size: u64,
    ) -> PortResult<()>;

    /// Update progress for a chunk upload
    async fn update_chunk_progress(
        &self,
        session_id: &str,
        chunk_number: u32,
        bytes_uploaded: u64,
    ) -> PortResult<()>;

    /// Get current progress for a session
    async fn get_session_progress(&self, session_id: &str) -> PortResult<ChunkedUploadProgress>;

    /// Complete progress tracking for a session
    async fn complete_session_progress(&self, session_id: &str, final_path: &str)
    -> PortResult<()>;

    /// Reset progress tracking for a session
    async fn reset_session_progress(&self, session_id: &str) -> PortResult<()>;
}

/// Chunked upload progress information
#[derive(Debug, Clone)]
pub struct ChunkedUploadProgress {
    /// Upload session ID
    pub session_id: String,

    /// Total number of chunks
    pub total_chunks: u32,

    /// Number of chunks uploaded so far
    pub chunks_uploaded: u32,

    /// Total bytes uploaded so far
    pub bytes_uploaded: u64,

    /// Total size of the file
    pub total_size: u64,

    /// Upload progress percentage (0.0 to 100.0)
    pub progress_percentage: f64,

    /// Current upload speed in bytes per second
    pub current_speed_bps: f64,

    /// Estimated time remaining in seconds
    pub estimated_time_remaining_secs: Option<u64>,

    /// Last updated timestamp
    pub last_updated: time::OffsetDateTime,
}

/// Chunked upload events
#[derive(Debug, Clone)]
pub enum ChunkedUploadEvent {
    /// Event for upload initiation
    UploadInitiated {
        session_id: String,
        package_hrn: Hrn,
        total_size: u64,
        chunk_count: u32,
        initiated_at: time::OffsetDateTime,
    },

    /// Event for chunk upload
    ChunkUploaded {
        session_id: String,
        chunk_number: u32,
        chunk_size: u64,
        bytes_uploaded: u64,
        uploaded_at: time::OffsetDateTime,
    },

    /// Event for upload completion
    UploadCompleted {
        session_id: String,
        package_hrn: Hrn,
        total_bytes: u64,
        chunks_uploaded: u32,
        storage_path: String,
        duration_ms: u64,
        completed_at: time::OffsetDateTime,
    },

    /// Event for upload abortion
    UploadAborted {
        session_id: String,
        package_hrn: Hrn,
        reason: Option<String>,
        aborted_at: time::OffsetDateTime,
    },

    /// Event for upload failure
    UploadFailed {
        session_id: String,
        package_hrn: Hrn,
        error: String,
        failed_at: time::OffsetDateTime,
    },
}
