use std::sync::Arc;
use tracing::{info, warn, error};
use time::OffsetDateTime;
use uuid::Uuid;
use super::{
    dto::{
        InitiateChunkedUploadCommand,
        UploadChunkCommand,
        CompleteChunkedUploadCommand,
        AbortChunkedUploadCommand,
        InitiateChunkedUploadResult,
        UploadChunkResult,
        CompleteChunkedUploadResult,
        UploadSession,
        UploadSessionStatus,
        UploadStats,
    },
    error::ChunkedUploadError,
    ports::{
        ChunkedUploadSessionRepository,
        ChunkStorage,
        ChunkedUploadEventPublisher,
        ChunkedUploadProgressTracker,
        ChunkedUploadProgress,
    },
};

/// Use case for handling chunked artifact uploads
pub struct ChunkedUploadUseCase {
    session_repository: Arc<dyn ChunkedUploadSessionRepository>,
    chunk_storage: Arc<dyn ChunkStorage>,
    event_publisher: Arc<dyn ChunkedUploadEventPublisher>,
    progress_tracker: Arc<dyn ChunkedUploadProgressTracker>,
}

impl ChunkedUploadUseCase {
    pub fn new(
        session_repository: Arc<dyn ChunkedUploadSessionRepository>,
        chunk_storage: Arc<dyn ChunkStorage>,
        event_publisher: Arc<dyn ChunkedUploadEventPublisher>,
        progress_tracker: Arc<dyn ChunkedUploadProgressTracker>,
    ) -> Self {
        Self {
            session_repository,
            chunk_storage,
            event_publisher,
            progress_tracker,
        }
    }
    
    /// Initiate a new chunked upload session
    pub async fn initiate_upload(&self, command: InitiateChunkedUploadCommand) -> Result<InitiateChunkedUploadResult, ChunkedUploadError> {
        info!("Initiating chunked upload for package: {}", command.package_hrn);
        
        // Generate session ID
        let session_id = Uuid::new_v4().to_string();
        
        // Calculate expiration time (24 hours from now)
        let expires_at = OffsetDateTime::now_utc() + time::Duration::hours(24);
        
        // Calculate chunk size if not provided
        let chunk_size = if command.total_size > 0 && command.chunk_count > 0 {
            Some((command.total_size / command.chunk_count as u64) as u32)
        } else {
            None
        };
        
        // Create upload session
        let session = UploadSession {
            session_id: session_id.clone(),
            package_hrn: command.package_hrn.clone(),
            repository_hrn: command.repository_hrn,
            coordinates: command.coordinates,
            total_size: command.total_size,
            chunk_count: command.chunk_count,
            artifact_type: command.artifact_type,
            content_type: command.content_type,
            expected_checksum: command.checksum,
            status: UploadSessionStatus::InProgress,
            created_at: OffsetDateTime::now_utc(),
            expires_at,
            metadata: command.metadata,
        };
        
        // Store session
        self.session_repository.create_session(&session).await?;
        
        // Publish event
        let event = super::ports::ChunkedUploadEvent::UploadInitiated {
            session_id: session_id.clone(),
            package_hrn: command.package_hrn,
            total_size: command.total_size,
            chunk_count: command.chunk_count,
            initiated_at: OffsetDateTime::now_utc(),
        };
        
        if let Err(e) = self.event_publisher.publish_upload_initiated(event).await {
            error!("Failed to publish upload initiated event: {}", e);
        }
        
        info!("Chunked upload initiated with session ID: {}", session_id);
        
        Ok(InitiateChunkedUploadResult {
            session_id,
            package_hrn: command.package_hrn,
            chunk_size,
            upload_url_template: format!("/api/v1/uploads/{}/chunks/{{chunk_number}}", session_id),
            expires_at,
        })
    }
    
    /// Upload a chunk
    pub async fn upload_chunk(&self, command: UploadChunkCommand) -> Result<UploadChunkResult, ChunkedUploadError> {
        info!("Uploading chunk {} for session {}", command.chunk_number, command.session_id);
        
        // Get session
        let mut session = self.session_repository.get_session(&command.session_id).await?
            .ok_or_else(|| ChunkedUploadError::SessionNotFound(command.session_id.clone()))?;
        
        // Validate session status
        if session.status != UploadSessionStatus::InProgress {
            return Err(ChunkedUploadError::SessionAlreadyCompleted(command.session_id));
        }
        
        // Validate session expiration
        if session.expires_at < OffsetDateTime::now_utc() {
            session.status = UploadSessionStatus::Expired;
            self.session_repository.update_session(&session).await?;
            return Err(ChunkedUploadError::SessionExpired(command.session_id));
        }
        
        // Validate chunk number
        if command.chunk_number >= session.chunk_count {
            return Err(ChunkedUploadError::InvalidChunkNumber(
                format!("Chunk number {} is out of range (0-{})", command.chunk_number, session.chunk_count - 1)
            ));
        }
        
        // Store chunk
        let storage_path = self.chunk_storage.store_chunk(
            &command.session_id,
            command.chunk_number,
            command.chunk_data.clone(),
        ).await?;
        
        // Update progress
        self.progress_tracker.update_chunk_progress(
            &command.session_id,
            command.chunk_number,
            command.chunk_data.len() as u64,
        ).await?;
        
        // Publish event
        let event = super::ports::ChunkedUploadEvent::ChunkUploaded {
            session_id: command.session_id.clone(),
            chunk_number: command.chunk_number,
            chunk_size: command.chunk_data.len() as u64,
            bytes_uploaded: command.chunk_data.len() as u64,
            uploaded_at: OffsetDateTime::now_utc(),
        };
        
        if let Err(e) = self.event_publisher.publish_chunk_uploaded(event).await {
            error!("Failed to publish chunk uploaded event: {}", e);
        }
        
        info!("Chunk {} uploaded successfully for session {}", command.chunk_number, command.session_id);
        
        Ok(UploadChunkResult {
            session_id: command.session_id,
            chunk_number: command.chunk_number,
            success: true,
            bytes_received: command.chunk_data.len() as u64,
            message: Some("Chunk uploaded successfully".to_string()),
        })
    }
    
    /// Complete a chunked upload
    pub async fn complete_upload(&self, command: CompleteChunkedUploadCommand) -> Result<CompleteChunkedUploadResult, ChunkedUploadError> {
        info!("Completing chunked upload for session {}", command.session_id);
        
        // Get session
        let mut session = self.session_repository.get_session(&command.session_id).await?
            .ok_or_else(|| ChunkedUploadError::SessionNotFound(command.session_id.clone()))?;
        
        // Validate session status
        if session.status != UploadSessionStatus::InProgress {
            return Err(ChunkedUploadError::SessionAlreadyCompleted(command.session_id));
        }
        
        // Validate session expiration
        if session.expires_at < OffsetDateTime::now_utc() {
            session.status = UploadSessionStatus::Expired;
            self.session_repository.update_session(&session).await?;
            return Err(ChunkedUploadError::SessionExpired(command.session_id));
        }
        
        // Get upload progress
        let progress = self.progress_tracker.get_session_progress(&command.session_id).await?;
        
        // Validate that all chunks were uploaded
        if progress.chunks_uploaded != session.total_chunks {
            return Err(ChunkedUploadError::ValidationError(
                format!("Expected {} chunks, but only {} were uploaded", session.total_chunks, progress.chunks_uploaded)
            ));
        }
        
        // Assemble chunks into final file
        let storage_path = self.chunk_storage.assemble_chunks(
            &command.session_id,
            &format!("artifacts/{}/{}", session.repository_hrn, session.package_hrn),
        ).await?;
        
        // Update session status
        session.status = UploadSessionStatus::Completed;
        self.session_repository.update_session(&session).await?;
        
        // Complete progress tracking
        self.progress_tracker.complete_session_progress(&command.session_id, &storage_path).await?;
        
        // Calculate upload statistics
        let stats = UploadStats {
            total_bytes: progress.bytes_uploaded,
            chunks_uploaded: progress.chunks_uploaded,
            upload_duration_ms: (OffsetDateTime::now_utc() - session.created_at).whole_milliseconds() as u64,
            avg_speed_bps: if progress.bytes_uploaded > 0 {
                progress.bytes_uploaded as f64 / ((OffsetDateTime::now_utc() - session.created_at).whole_seconds() as f64)
            } else {
                0.0
            },
        };
        
        // Publish event
        let event = super::ports::ChunkedUploadEvent::UploadCompleted {
            session_id: command.session_id.clone(),
            package_hrn: session.package_hrn.clone(),
            total_bytes: progress.bytes_uploaded,
            chunks_uploaded: progress.chunks_uploaded,
            storage_path: storage_path.clone(),
            duration_ms: stats.upload_duration_ms,
            completed_at: OffsetDateTime::now_utc(),
        };
        
        if let Err(e) = self.event_publisher.publish_upload_completed(event).await {
            error!("Failed to publish upload completed event: {}", e);
        }
        
        info!("Chunked upload completed successfully for session {}", command.session_id);
        
        Ok(CompleteChunkedUploadResult {
            session_id: command.session_id,
            package_hrn: session.package_hrn,
            success: true,
            storage_path: Some(storage_path),
            stats,
        })
    }
    
    /// Abort a chunked upload
    pub async fn abort_upload(&self, command: AbortChunkedUploadCommand) -> Result<(), ChunkedUploadError> {
        info!("Aborting chunked upload for session {}", command.session_id);
        
        // Get session
        let mut session = self.session_repository.get_session(&command.session_id).await?
            .ok_or_else(|| ChunkedUploadError::SessionNotFound(command.session_id.clone()))?;
        
        // Validate session status
        if session.status == UploadSessionStatus::Completed || session.status == UploadSessionStatus::Aborted {
            return Err(ChunkedUploadError::SessionAlreadyCompleted(command.session_id));
        }
        
        // Update session status
        session.status = UploadSessionStatus::Aborted;
        self.session_repository.update_session(&session).await?;
        
        // Delete stored chunks
        if let Err(e) = self.chunk_storage.delete_session_chunks(&command.session_id).await {
            warn!("Failed to delete session chunks: {}", e);
        }
        
        // Reset progress tracking
        if let Err(e) = self.progress_tracker.reset_session_progress(&command.session_id).await {
            warn!("Failed to reset progress tracking: {}", e);
        }
        
        // Publish event
        let event = super::ports::ChunkedUploadEvent::UploadAborted {
            session_id: command.session_id.clone(),
            package_hrn: session.package_hrn,
            reason: command.reason,
            aborted_at: OffsetDateTime::now_utc(),
        };
        
        if let Err(e) = self.event_publisher.publish_upload_aborted(event).await {
            error!("Failed to publish upload aborted event: {}", e);
        }
        
        info!("Chunked upload aborted for session {}", command.session_id);
        
        Ok(())
    }
    
    /// Get upload progress for a session
    pub async fn get_upload_progress(&self, session_id: &str) -> Result<ChunkedUploadProgress, ChunkedUploadError> {
        self.progress_tracker.get_session_progress(session_id).await
    }
    
    /// Cleanup expired sessions
    pub async fn cleanup_expired_sessions(&self) -> Result<u32, ChunkedUploadError> {
        let count = self.session_repository.cleanup_expired_sessions().await?;
        info!("Cleaned up {} expired upload sessions", count);
        Ok(count)
    }
}

