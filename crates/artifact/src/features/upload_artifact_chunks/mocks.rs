use async_trait::async_trait;
use std::sync::Mutex;
use std::collections::HashMap;
use bytes::Bytes;

use super::{
    dto::*,
    error::ChunkedUploadError,
    ports::*,
};

/// Mock para ChunkedUploadSessionRepository
pub struct MockChunkedUploadSessionRepository {
    pub sessions: Mutex<HashMap<String, UploadSession>>,
}

impl MockChunkedUploadSessionRepository {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl ChunkedUploadSessionRepository for MockChunkedUploadSessionRepository {
    async fn create_session(&self, session: &UploadSession) -> PortResult<()> {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.insert(session.session_id.clone(), session.clone());
        Ok(())
    }
    
    async fn get_session(&self, session_id: &str) -> PortResult<Option<UploadSession>> {
        let sessions = self.sessions.lock().unwrap();
        Ok(sessions.get(session_id).cloned())
    }
    
    async fn update_session(&self, session: &UploadSession) -> PortResult<()> {
        let mut sessions = self.sessions.lock().unwrap();
        if sessions.contains_key(&session.session_id) {
            sessions.insert(session.session_id.clone(), session.clone());
            Ok(())
        } else {
            Err(ChunkedUploadError::SessionNotFound(session.session_id.clone()))
        }
    }
    
    async fn delete_session(&self, session_id: &str) -> PortResult<()> {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.remove(session_id);
        Ok(())
    }
    
    async fn list_active_sessions(&self, repository_hrn: &shared::hrn::Hrn) -> PortResult<Vec<UploadSession>> {
        let sessions = self.sessions.lock().unwrap();
        let active_sessions: Vec<UploadSession> = sessions
            .values()
            .filter(|s| s.repository_hrn == *repository_hrn && s.status == UploadSessionStatus::InProgress)
            .cloned()
            .collect();
        Ok(active_sessions)
    }
    
    async fn cleanup_expired_sessions(&self) -> PortResult<u32> {
        let mut sessions = self.sessions.lock().unwrap();
        let now = time::OffsetDateTime::now_utc();
        let expired_ids: Vec<String> = sessions
            .iter()
            .filter(|(_, session)| session.expires_at < now)
            .map(|(id, _)| id.clone())
            .collect();
        
        let count = expired_ids.len() as u32;
        for id in expired_ids {
            sessions.remove(&id);
        }
        
        Ok(count)
    }
}

/// Mock para ChunkStorage
pub struct MockChunkStorage {
    pub chunks: Mutex<HashMap<String, HashMap<u32, Bytes>>>,
}

impl MockChunkStorage {
    pub fn new() -> Self {
        Self {
            chunks: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl ChunkStorage for MockChunkStorage {
    async fn store_chunk(&self, session_id: &str, chunk_number: u32, data: Bytes) -> PortResult<String> {
        let mut chunks = self.chunks.lock().unwrap();
        let session_chunks = chunks.entry(session_id.to_string()).or_insert_with(HashMap::new);
        session_chunks.insert(chunk_number, data);
        Ok(format!("mock://{}/{}", session_id, chunk_number))
    }
    
    async fn retrieve_chunk(&self, session_id: &str, chunk_number: u32) -> PortResult<Option<Bytes>> {
        let chunks = self.chunks.lock().unwrap();
        Ok(chunks.get(session_id).and_then(|session| session.get(&chunk_number)).cloned())
    }
    
    async fn chunk_exists(&self, session_id: &str, chunk_number: u32) -> PortResult<bool> {
        let chunks = self.chunks.lock().unwrap();
        Ok(chunks.get(session_id).map_or(false, |session| session.contains_key(&chunk_number)))
    }
    
    async fn delete_session_chunks(&self, session_id: &str) -> PortResult<()> {
        let mut chunks = self.chunks.lock().unwrap();
        chunks.remove(session_id);
        Ok(())
    }
    
    async fn assemble_chunks(&self, session_id: &str, output_path: &str) -> PortResult<String> {
        let chunks = self.chunks.lock().unwrap();
        if let Some(session_chunks) = chunks.get(session_id) {
            // Simular ensamblaje
            let assembled_path = format!("{}/{}", output_path, session_id);
            Ok(assembled_path)
        } else {
            Err(ChunkedUploadError::StorageError("No chunks found for session".to_string()))
        }
    }
    
    async fn get_session_chunks_size(&self, session_id: &str) -> PortResult<u64> {
        let chunks = self.chunks.lock().unwrap();
        if let Some(session_chunks) = chunks.get(session_id) {
            let total_size: usize = session_chunks.values().map(|chunk| chunk.len()).sum();
            Ok(total_size as u64)
        } else {
            Ok(0)
        }
    }
}

/// Mock para ChunkedUploadEventPublisher
pub struct MockChunkedUploadEventPublisher {
    pub published_events: Mutex<Vec<ChunkedUploadEvent>>,
}

impl MockChunkedUploadEventPublisher {
    pub fn new() -> Self {
        Self {
            published_events: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl ChunkedUploadEventPublisher for MockChunkedUploadEventPublisher {
    async fn publish_upload_initiated(&self, event: ChunkedUploadEvent) -> PortResult<()> {
        let mut events = self.published_events.lock().unwrap();
        events.push(event);
        Ok(())
    }
    
    async fn publish_chunk_uploaded(&self, event: ChunkedUploadEvent) -> PortResult<()> {
        let mut events = self.published_events.lock().unwrap();
        events.push(event);
        Ok(())
    }
    
    async fn publish_upload_completed(&self, event: ChunkedUploadEvent) -> PortResult<()> {
        let mut events = self.published_events.lock().unwrap();
        events.push(event);
        Ok(())
    }
    
    async fn publish_upload_aborted(&self, event: ChunkedUploadEvent) -> PortResult<()> {
        let mut events = self.published_events.lock().unwrap();
        events.push(event);
        Ok(())
    }
    
    async fn publish_upload_failed(&self, event: ChunkedUploadEvent) -> PortResult<()> {
        let mut events = self.published_events.lock().unwrap();
        events.push(event);
        Ok(())
    }
}

/// Mock para ChunkedUploadProgressTracker
pub struct MockChunkedUploadProgressTracker {
    pub progress: Mutex<HashMap<String, ChunkedUploadProgress>>,
}

impl MockChunkedUploadProgressTracker {
    pub fn new() -> Self {
        Self {
            progress: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl ChunkedUploadProgressTracker for MockChunkedUploadProgressTracker {
    async fn update_chunk_progress(&self, session_id: &str, chunk_number: u32, bytes_uploaded: u64) -> PortResult<()> {
        let mut progress = self.progress.lock().unwrap();
        if let Some(session_progress) = progress.get_mut(session_id) {
            session_progress.chunks_uploaded += 1;
            session_progress.bytes_uploaded += bytes_uploaded;
            session_progress.progress_percentage = 
                (session_progress.chunks_uploaded as f64 / session_progress.total_chunks as f64) * 100.0;
            session_progress.last_updated = time::OffsetDateTime::now_utc();
        }
        Ok(())
    }
    
    async fn get_session_progress(&self, session_id: &str) -> PortResult<ChunkedUploadProgress> {
        let progress = self.progress.lock().unwrap();
        progress.get(session_id)
            .cloned()
            .ok_or_else(|| ChunkedUploadError::SessionNotFound(session_id.to_string()))
    }
    
    async fn complete_session_progress(&self, session_id: &str, final_path: &str) -> PortResult<()> {
        let mut progress = self.progress.lock().unwrap();
        if let Some(session_progress) = progress.get_mut(session_id) {
            session_progress.progress_percentage = 100.0;
            session_progress.last_updated = time::OffsetDateTime::now_utc();
        }
        Ok(())
    }
    
    async fn reset_session_progress(&self, session_id: &str) -> PortResult<()> {
        let mut progress = self.progress.lock().unwrap();
        progress.remove(session_id);
        Ok(())
    }
}