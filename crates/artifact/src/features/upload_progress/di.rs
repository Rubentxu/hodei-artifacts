use std::sync::Arc;

use super::{
    ports::{ProgressStorage, ProgressEventPublisher, RealtimeNotifier},
    service::UploadProgressService,
    api::UploadProgressApi,
    ProgressResult, UploadProgress, UpdateProgressCommand,
};
use crate::features::upload_progress::ProgressError;

/// Contenedor de inyección de dependencias para la feature de progress tracking
pub struct UploadProgressDIContainer {
    pub api: UploadProgressApi,
    pub service: UploadProgressService,
}

impl UploadProgressDIContainer {
    /// Constructor flexible que acepta cualquier implementación de los ports
    pub fn new(
        storage: Arc<dyn ProgressStorage>,
        event_publisher: Arc<dyn ProgressEventPublisher>,
        realtime_notifier: Arc<dyn RealtimeNotifier>,
    ) -> Self {
        let service = UploadProgressService::new(storage, event_publisher, realtime_notifier);
        let api = UploadProgressApi::new(service.clone());
        
        Self { api, service }
    }

    /// Método de conveniencia para producción con implementaciones reales
    #[cfg(feature = "production")]
    pub fn for_production(
        db_pool: Arc<sqlx::PgPool>,
        event_bus: Arc<dyn EventBus>,
        websocket_manager: Arc<dyn WebSocketManager>,
    ) -> Self {
        let storage: Arc<dyn ProgressStorage> = 
            Arc::new(PostgresProgressStorage::new(db_pool));
        
        let event_publisher: Arc<dyn ProgressEventPublisher> = 
            Arc::new(KafkaProgressEventPublisher::new(event_bus));
        
        let realtime_notifier: Arc<dyn RealtimeNotifier> = 
            Arc::new(WebSocketRealtimeNotifier::new(websocket_manager));
        
        Self::new(storage, event_publisher, realtime_notifier)
    }

    /// Método de conveniencia para testing
    #[cfg(test)]
    pub fn for_testing() -> Self {
        use crate::features::upload_progress::adapter::test::{MockProgressStorage, MockEventPublisher, MockRealtimeNotifier};
        
        let storage: Arc<dyn ProgressStorage> = 
            Arc::new(MockProgressStorage::default());
        
        let event_publisher: Arc<dyn ProgressEventPublisher> = 
            Arc::new(MockEventPublisher::default());
        
        let realtime_notifier: Arc<dyn RealtimeNotifier> = 
            Arc::new(MockRealtimeNotifier::default());
        
        Self::new(storage, event_publisher, realtime_notifier)
    }

    /// Método de conveniencia para desarrollo/local
    pub fn for_development() -> Self {
        use crate::features::upload_progress::adapter::memory::{MemoryProgressStorage, MemoryEventPublisher, MemoryRealtimeNotifier};
        
        let storage: Arc<dyn ProgressStorage> = 
            Arc::new(MemoryProgressStorage::default());
        
        let event_publisher: Arc<dyn ProgressEventPublisher> = 
            Arc::new(MemoryEventPublisher::default());
        
        let realtime_notifier: Arc<dyn RealtimeNotifier> = 
            Arc::new(MemoryRealtimeNotifier::default());
        
        Self::new(storage, event_publisher, realtime_notifier)
    }
}

/// Implementaciones concretas para producción
#[cfg(feature = "production")]
mod production_adapters {
    use super::*;
    use async_trait::async_trait;
    use sqlx::PgPool;
    use crate::infrastructure::event_bus::EventBus;
    use crate::infrastructure::websocket::WebSocketManager;

    /// Almacenamiento en PostgreSQL para sesiones de progreso
    pub struct PostgresProgressStorage {
        pool: Arc<PgPool>,
    }

    impl PostgresProgressStorage {
        pub fn new(pool: Arc<PgPool>) -> Self {
            Self { pool }
        }
    }

    #[async_trait]
    impl ProgressStorage for PostgresProgressStorage {
        async fn create_session(&self, progress: UploadProgress) -> ProgressResult<()> {
            sqlx::query!(
                "INSERT INTO upload_progress (upload_id, total_bytes, bytes_transferred, percentage, status, last_updated, estimated_seconds_remaining) 
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
                progress.upload_id,
                progress.total_bytes as i64,
                progress.bytes_transferred as i64,
                progress.percentage as i16,
                progress.status.to_string(),
                progress.last_updated as i64,
                progress.estimated_seconds_remaining.map(|x| x as i64)
            )
            .execute(&*self.pool)
            .await
            .map_err(|e| ProgressError::StorageError(e.to_string()))?;
            
            Ok(())
        }

        async fn get_progress(&self, upload_id: &str) -> ProgressResult<UploadProgress> {
            let record = sqlx::query!(
                "SELECT upload_id, total_bytes, bytes_transferred, percentage, status, last_updated, estimated_seconds_remaining 
                 FROM upload_progress WHERE upload_id = $1",
                upload_id
            )
            .fetch_one(&*self.pool)
            .await
            .map_err(|e| ProgressError::SessionNotFound(e.to_string()))?;

            let status = match record.status.as_str() {
                "InProgress" => UploadStatus::InProgress,
                "Completed" => UploadStatus::Completed,
                "Failed" => UploadStatus::Failed,
                "Cancelled" => UploadStatus::Cancelled,
                _ => UploadStatus::InProgress,
            };

            Ok(UploadProgress {
                upload_id: record.upload_id,
                total_bytes: record.total_bytes as u64,
                bytes_transferred: record.bytes_transferred as u64,
                percentage: record.percentage as u8,
                status,
                last_updated: record.last_updated as u64,
                estimated_seconds_remaining: record.estimated_seconds_remaining.map(|x| x as u64),
            })
        }

        async fn update_progress(&self, command: UpdateProgressCommand) -> ProgressResult<UploadProgress> {
            // Implementación similar con UPDATE SQL
            todo!()
        }

        async fn delete_session(&self, upload_id: &str) -> ProgressResult<()> {
            sqlx::query!(
                "DELETE FROM upload_progress WHERE upload_id = $1",
                upload_id
            )
            .execute(&*self.pool)
            .await
            .map_err(|e| ProgressError::StorageError(e.to_string()))?;
            
            Ok(())
        }

        async fn list_sessions(&self) -> ProgressResult<Vec<UploadProgress>> {
            let records = sqlx::query!(
                "SELECT upload_id, total_bytes, bytes_transferred, percentage, status, last_updated, estimated_seconds_remaining 
                 FROM upload_progress ORDER BY last_updated DESC"
            )
            .fetch_all(&*self.pool)
            .await
            .map_err(|e| ProgressError::StorageError(e.to_string()))?;

            let sessions = records.into_iter().map(|record| {
                let status = match record.status.as_str() {
                    "InProgress" => UploadStatus::InProgress,
                    "Completed" => UploadStatus::Completed,
                    "Failed" => UploadStatus::Failed,
                    "Cancelled" => UploadStatus::Cancelled,
                    _ => UploadStatus::InProgress,
                };

                UploadProgress {
                    upload_id: record.upload_id,
                    total_bytes: record.total_bytes as u64,
                    bytes_transferred: record.bytes_transferred as u64,
                    percentage: record.percentage as u8,
                    status,
                    last_updated: record.last_updated as u64,
                    estimated_seconds_remaining: record.estimated_seconds_remaining.map(|x| x as u64),
                }
            }).collect();

            Ok(sessions)
        }
    }

    /// Publicador de eventos para Kafka
    pub struct KafkaProgressEventPublisher {
        event_bus: Arc<dyn EventBus>,
    }

    impl KafkaProgressEventPublisher {
        pub fn new(event_bus: Arc<dyn EventBus>) -> Self {
            Self { event_bus }
        }
    }

    #[async_trait]
    impl ProgressEventPublisher for KafkaProgressEventPublisher {
        async fn publish_progress_update(&self, progress: &UploadProgress) -> ProgressResult<()> {
            self.event_bus.publish("upload-progress-updated", progress).await
                .map_err(|e| ProgressError::StorageError(e.to_string()))
        }

        async fn publish_upload_completed(&self, upload_id: &str) -> ProgressResult<()> {
            self.event_bus.publish("upload-completed", upload_id).await
                .map_err(|e| ProgressError::StorageError(e.to_string()))
        }

        async fn publish_upload_failed(&self, upload_id: &str, error: &str) -> ProgressResult<()> {
            let event = serde_json::json!({ "upload_id": upload_id, "error": error });
            self.event_bus.publish("upload-failed", event).await
                .map_err(|e| ProgressError::StorageError(e.to_string()))
        }
    }

    /// Notificador WebSocket para tiempo real
    pub struct WebSocketRealtimeNotifier {
        websocket_manager: Arc<dyn WebSocketManager>,
    }

    impl WebSocketRealtimeNotifier {
        pub fn new(websocket_manager: Arc<dyn WebSocketManager>) -> Self {
            Self { websocket_manager }
        }
    }

    #[async_trait]
    impl RealtimeNotifier for WebSocketRealtimeNotifier {
        async fn notify_progress_update(&self, progress: &UploadProgress) -> ProgressResult<()> {
            self.websocket_manager.broadcast_to_topic(
                &format!("upload-{}", progress.upload_id),
                progress
            ).await
            .map_err(|e| ProgressError::StorageError(e.to_string()))
        }

        async fn subscribe(&self, upload_id: &str, client_id: &str) -> ProgressResult<()> {
            self.websocket_manager.subscribe_to_topic(client_id, &format!("upload-{}", upload_id)).await
                .map_err(|e| ProgressError::StorageError(e.to_string()))
        }

        async fn unsubscribe(&self, client_id: &str) -> ProgressResult<()> {
            self.websocket_manager.unsubscribe_from_all_topics(client_id).await
                .map_err(|e| ProgressError::StorageError(e.to_string()))
        }
    }
}

/// Implementaciones en memoria para desarrollo
pub mod memory {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use async_trait::async_trait;

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