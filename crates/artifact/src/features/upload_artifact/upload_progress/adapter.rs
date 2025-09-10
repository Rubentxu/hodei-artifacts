//! Adaptadores concretos para los ports de progress tracking
//! Incluye implementaciones para testing y desarrollo

use super::ports::{ProgressStorage, ProgressEventPublisher, RealtimeNotifier, ProgressResult, ProgressError};
use super::dto::{UploadProgress, UpdateProgressCommand, UploadStatus};
use async_trait::async_trait;
use std::sync::Mutex;

/// Adaptadores para testing (solo compilan en tests)
#[cfg(test)]
pub mod test {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    pub struct MockProgressStorage {
        pub saved_todos: Mutex<HashMap<String, UploadProgress>>,
    }

    #[async_trait]
    impl ProgressStorage for MockProgressStorage {
        async fn create_session(&self, progress: UploadProgress) -> ProgressResult<()> {
            self.saved_todos.lock().unwrap().insert(progress.upload_id.clone(), progress);
            Ok(())
        }

        async fn get_progress(&self, upload_id: &str) -> ProgressResult<UploadProgress> {
            self.saved_todos.lock().unwrap().get(upload_id)
                .cloned()
                .ok_or_else(|| ProgressError::SessionNotFound(upload_id.to_string()))
        }

        async fn update_progress(&self, command: UpdateProgressCommand) -> ProgressResult<UploadProgress> {
            let mut sessions = self.saved_todos.lock().unwrap();
            let progress = sessions.get_mut(&command.upload_id)
                .ok_or_else(|| ProgressError::SessionNotFound(command.upload_id.clone()))?;

            progress.update(command.bytes_transferred, command.total_bytes);
            progress.status = command.status;
            
            Ok(progress.clone())
        }

        async fn delete_session(&self, upload_id: &str) -> ProgressResult<()> {
            self.saved_todos.lock().unwrap().remove(upload_id);
            Ok(())
        }

        async fn list_sessions(&self) -> ProgressResult<Vec<UploadProgress>> {
            Ok(self.saved_todos.lock().unwrap().values().cloned().collect())
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
}

/// Adaptadores en memoria para desarrollo
pub mod memory {
    use super::*;
    use std::collections::HashMap;

    /// Almacenamiento en memoria para desarrollo
    #[derive(Default)]
    pub struct MemoryProgressStorage {
        sessions: Mutex<HashMap<String, UploadProgress>>,
    }

    #[async_trait]
    impl ProgressStorage for MemoryProgressStorage {
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

    /// Publicador de eventos en memoria para desarrollo
    #[derive(Default)]
    pub struct MemoryEventPublisher {
        published_events: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl ProgressEventPublisher for MemoryEventPublisher {
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

    /// Notificador en memoria para desarrollo
    #[derive(Default)]
    pub struct MemoryRealtimeNotifier {
        notifications: Mutex<Vec<String>>,
        subscriptions: Mutex<Vec<(String, String)>>,
    }

    #[async_trait]
    impl RealtimeNotifier for MemoryRealtimeNotifier {
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
}