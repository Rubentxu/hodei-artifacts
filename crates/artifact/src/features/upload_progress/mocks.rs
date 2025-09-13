use std::path::PathBuf;
use std::sync::Mutex;
use async_trait::async_trait;
use std::collections::HashMap;

use super::ports::{ProgressStorage, ProgressEventPublisher, RealtimeNotifier, ChunkedUploadStorage, ProgressResult};
use super::dto::{UploadProgress, UploadStatus, UpdateProgressCommand, ReceivedChunksResponse, ReceivedChunkInfo};
use crate::features::upload_progress::ProgressError;

#[derive(Default)]
pub struct MockProgressStorage {
    pub sessions: Mutex<HashMap<String, UploadProgress>>,
}

#[async_trait]
impl ProgressStorage for MockProgressStorage {
    async fn create_session(&self, progress: UploadProgress) -> ProgressResult<()> {
        self.sessions.lock().unwrap().insert(progress.upload_id.clone(), progress);
        Ok(())
    }

    async fn get_progress(&self, upload_id: &str) -> ProgressResult<UploadProgress> {
        self.sessions.lock().unwrap().get(upload_id)
            .cloned()
            .ok_or_else(|| ProgressError::SessionNotFound(upload_id.to_string()))
    }

    async fn update_progress(&self, command: UpdateProgressCommand) -> ProgressResult<UploadProgress> {
        let mut sessions = self.sessions.lock().unwrap();
        let progress = sessions.get_mut(&command.upload_id)
            .ok_or_else(|| ProgressError::SessionNotFound(command.upload_id.clone()))?;

        progress.update(command.bytes_transferred, command.total_bytes);
        progress.status = command.status;
        
        Ok(progress.clone())
    }

    async fn delete_session(&self, upload_id: &str) -> ProgressResult<()> {
        self.sessions.lock().unwrap().remove(upload_id);
        Ok(())
    }

    async fn list_sessions(&self) -> ProgressResult<Vec<UploadProgress>> {
        Ok(self.sessions.lock().unwrap().values().cloned().collect())
    }
}

#[derive(Default)]
pub struct MockEventPublisher {
    pub published_events: Mutex<Vec<String>>,
}

#[async_trait]
impl ProgressEventPublisher for MockEventPublisher {
    async fn publish_progress_update(&self, _progress: &UploadProgress) -> ProgressResult<()> {
        self.published_events.lock().unwrap().push("progress_update".to_string());
        Ok(())
    }

    async fn publish_upload_completed(&self, upload_id: &str) -> ProgressResult<()> {
        self.published_events.lock().unwrap().push(format!("completed_{}", upload_id));
        Ok(())
    }

    async fn publish_upload_failed(&self, upload_id: &str, _error: &str) -> ProgressResult<()> {
        self.published_events.lock().unwrap().push(format!("failed_{}", upload_id));
        Ok(())
    }
}

#[derive(Default)]
pub struct MockRealtimeNotifier {
    pub notifications: Mutex<Vec<String>>,
    pub subscriptions: Mutex<Vec<(String, String)>>,
}

#[async_trait]
impl RealtimeNotifier for MockRealtimeNotifier {
    async fn notify_progress_update(&self, progress: &UploadProgress) -> ProgressResult<()> {
        self.notifications.lock().unwrap().push(format!("notify_{}_{}", progress.upload_id, progress.percentage));
        Ok(())
    }

    async fn subscribe(&self, upload_id: &str, client_id: &str) -> ProgressResult<()> {
        self.subscriptions.lock().unwrap().push((upload_id.to_string(), client_id.to_string()));
        Ok(())
    }

    async fn unsubscribe(&self, client_id: &str) -> ProgressResult<()> {
        let mut subscriptions = self.subscriptions.lock().unwrap();
        subscriptions.retain(|(_, cid)| cid != client_id);
        Ok(())
    }
}

#[derive(Default)]
pub struct MockChunkedUploadStorage {
    pub chunks: Mutex<HashMap<String, Vec<usize>>>,
}

#[async_trait]
impl ChunkedUploadStorage for MockChunkedUploadStorage {
    async fn save_chunk(&self, upload_id: &str, chunk_number: usize, _data: bytes::Bytes) -> Result<(), ProgressError> {
        let mut chunks = self.chunks.lock().unwrap();
        let chunk_list = chunks.entry(upload_id.to_string()).or_insert_with(Vec::new);
        if !chunk_list.contains(&chunk_number) {
            chunk_list.push(chunk_number);
        }
        Ok(())
    }

    async fn get_received_chunks_count(&self, upload_id: &str) -> Result<usize, ProgressError> {
        let chunks = self.chunks.lock().unwrap();
        Ok(chunks.get(upload_id).map_or(0, |c| c.len()))
    }

    async fn get_received_chunk_numbers(&self, upload_id: &str) -> Result<Vec<usize>, ProgressError> {
        let chunks = self.chunks.lock().unwrap();
        Ok(chunks.get(upload_id).cloned().unwrap_or_default())
    }

    async fn get_received_chunks_info(&self, upload_id: &str) -> Result<Vec<ReceivedChunkInfo>, ProgressError> {
        let chunks = self.chunks.lock().unwrap();
        let chunk_numbers = chunks.get(upload_id).cloned().unwrap_or_default();
        let chunk_info = chunk_numbers.into_iter().map(|num| ReceivedChunkInfo {
            chunk_number: num,
            size: 1024, // Mock size
        }).collect();
        Ok(chunk_info)
    }

    async fn assemble_chunks(&self, _upload_id: &str, _total_chunks: usize, _file_name: &str) -> Result<(PathBuf, String), ProgressError> {
        Ok((PathBuf::from("/tmp/test"), "test-hash".to_string()))
    }

    async fn cleanup(&self, _upload_id: &str) -> Result<(), ProgressError> {
        Ok(())
    }
    
  }
