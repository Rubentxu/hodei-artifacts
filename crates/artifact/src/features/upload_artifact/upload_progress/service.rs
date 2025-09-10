use std::sync::Arc;
use super::ports::{ProgressStorage, ProgressEventPublisher, RealtimeNotifier, ProgressResult};
use super::dto::{UploadProgress, UploadStatus, UpdateProgressCommand};
use tracing::{info, warn};

/// Servicio principal para gestión de progreso de subidas
#[derive(Clone)]
pub struct UploadProgressService {
    storage: Arc<dyn ProgressStorage>,
    event_publisher: Arc<dyn ProgressEventPublisher>,
    realtime_notifier: Arc<dyn RealtimeNotifier>,
}

impl UploadProgressService {
    pub fn new(
        storage: Arc<dyn ProgressStorage>,
        event_publisher: Arc<dyn ProgressEventPublisher>,
        realtime_notifier: Arc<dyn RealtimeNotifier>,
    ) -> Self {
        Self {
            storage,
            event_publisher,
            realtime_notifier,
        }
    }

    /// Crear una nueva sesión de progreso
    pub async fn create_session(&self, upload_id: String, total_bytes: u64) -> ProgressResult<()> {
        info!(upload_id = %upload_id, total_bytes, "Creating new upload progress session");
        
        let progress = UploadProgress::new(upload_id.clone(), total_bytes);
        
        self.storage.create_session(progress).await?;
        
        info!(upload_id = %upload_id, "Upload progress session created successfully");
        Ok(())
    }

    /// Obtener el progreso actual de una sesión
    pub async fn get_progress(&self, upload_id: &str) -> ProgressResult<UploadProgress> {
        let progress = self.storage.get_progress(upload_id).await?;
        
        info!(
            upload_id,
            percentage = progress.percentage,
            status = ?progress.status,
            "Retrieved upload progress"
        );
        
        Ok(progress)
    }

    /// Actualizar el progreso de una sesión
    pub async fn update_progress(&self, command: UpdateProgressCommand) -> ProgressResult<UploadProgress> {
        info!(
            upload_id = %command.upload_id,
            bytes_transferred = command.bytes_transferred,
            total_bytes = command.total_bytes,
            status = ?command.status,
            "Updating upload progress"
        );

        // Actualizar en storage
        let updated_progress = self.storage.update_progress(command.clone()).await?;

        // Publicar evento
        if let Err(e) = self.event_publisher.publish_progress_update(&updated_progress).await {
            warn!(upload_id = %command.upload_id, error = %e, "Failed to publish progress update event");
        }

        // Notificar en tiempo real
        if let Err(e) = self.realtime_notifier.notify_progress_update(&updated_progress).await {
            warn!(upload_id = %command.upload_id, error = %e, "Failed to send realtime progress notification");
        }

        info!(
            upload_id = %command.upload_id,
            percentage = updated_progress.percentage,
            "Upload progress updated successfully"
        );

        Ok(updated_progress)
    }

    /// Marcar una sesión como completada
    pub async fn mark_completed(&self, upload_id: &str) -> ProgressResult<UploadProgress> {
        info!(upload_id, "Marking upload as completed");

        let command = UpdateProgressCommand {
            upload_id: upload_id.to_string(),
            bytes_transferred: 0, // Se actualizará en el storage
            total_bytes: 0,       // Se actualizará en el storage
            status: UploadStatus::Completed,
        };

        let completed_progress = self.storage.update_progress(command).await?;

        // Publicar evento de completado
        if let Err(e) = self.event_publisher.publish_upload_completed(upload_id).await {
            warn!(upload_id, error = %e, "Failed to publish upload completed event");
        }

        info!(upload_id, "Upload marked as completed successfully");
        Ok(completed_progress)
    }

    /// Marcar una sesión como fallida
    pub async fn mark_failed(&self, upload_id: &str, error_message: &str) -> ProgressResult<UploadProgress> {
        warn!(upload_id, error = error_message, "Marking upload as failed");

        let command = UpdateProgressCommand {
            upload_id: upload_id.to_string(),
            bytes_transferred: 0, // Se actualizará en el storage
            total_bytes: 0,       // Se actualizará en el storage
            status: UploadStatus::Failed,
        };

        let failed_progress = self.storage.update_progress(command).await?;

        // Publicar evento de fallo
        if let Err(e) = self.event_publisher.publish_upload_failed(upload_id, error_message).await {
            warn!(upload_id, error = %e, "Failed to publish upload failed event");
        }

        warn!(upload_id, "Upload marked as failed");
        Ok(failed_progress)
    }

    /// Eliminar una sesión (limpieza)
    pub async fn delete_session(&self, upload_id: &str) -> ProgressResult<()> {
        info!(upload_id, "Deleting upload progress session");
        
        self.storage.delete_session(upload_id).await?;
        
        info!(upload_id, "Upload progress session deleted successfully");
        Ok(())
    }

    /// Listar todas las sesiones activas (para admin/monitoring)
    pub async fn list_sessions(&self) -> ProgressResult<Vec<UploadProgress>> {
        info!("Listing all active upload sessions");
        
        let sessions = self.storage.list_sessions().await?;
        
        info!(session_count = sessions.len(), "Retrieved active upload sessions");
        Ok(sessions)
    }

    /// Suscribir cliente a updates de una subida específica
    pub async fn subscribe_client(&self, upload_id: &str, client_id: &str) -> ProgressResult<()> {
        info!(upload_id, client_id, "Subscribing client to upload progress");
        
        self.realtime_notifier.subscribe(upload_id, client_id).await?;
        
        info!(upload_id, client_id, "Client subscribed successfully");
        Ok(())
    }

    /// Desuscribir cliente
    pub async fn unsubscribe_client(&self, client_id: &str) -> ProgressResult<()> {
        info!(client_id, "Unsubscribing client from upload progress");
        
        self.realtime_notifier.unsubscribe(client_id).await?;
        
        info!(client_id, "Client unsubscribed successfully");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use crate::features::upload_artifact::upload_progress::ProgressError;
    use async_trait::async_trait;

    #[derive(Default)]
    struct MockProgressStorage {
        sessions: Mutex<std::collections::HashMap<String, UploadProgress>>,
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
    struct MockEventPublisher {
        published_events: Mutex<Vec<String>>,
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
    struct MockRealtimeNotifier {
        notifications: Mutex<Vec<String>>,
        subscriptions: Mutex<Vec<(String, String)>>,
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

    #[tokio::test]
    async fn test_create_and_get_session() {
        let storage = Arc::new(MockProgressStorage::default());
        let event_publisher = Arc::new(MockEventPublisher::default());
        let realtime_notifier = Arc::new(MockRealtimeNotifier::default());
        
        let service = UploadProgressService::new(storage.clone(), event_publisher, realtime_notifier);
        
        // Crear sesión
        service.create_session("test-upload".to_string(), 1000).await.unwrap();
        
        // Obtener progreso
        let progress = service.get_progress("test-upload").await.unwrap();
        
        assert_eq!(progress.upload_id, "test-upload");
        assert_eq!(progress.total_bytes, 1000);
        assert_eq!(progress.bytes_transferred, 0);
        assert_eq!(progress.percentage, 0);
        assert_eq!(progress.status, UploadStatus::InProgress);
    }

    #[tokio::test]
    async fn test_update_progress() {
        let storage = Arc::new(MockProgressStorage::default());
        let event_publisher = Arc::new(MockEventPublisher::default());
        let realtime_notifier = Arc::new(MockRealtimeNotifier::default());
        
        let service = UploadProgressService::new(storage.clone(), event_publisher.clone(), realtime_notifier.clone());
        
        // Crear sesión
        service.create_session("test-upload".to_string(), 1000).await.unwrap();
        
        // Actualizar progreso
        let command = UpdateProgressCommand {
            upload_id: "test-upload".to_string(),
            bytes_transferred: 500,
            total_bytes: 1000,
            status: UploadStatus::InProgress,
        };
        
        let updated = service.update_progress(command).await.unwrap();
        
        assert_eq!(updated.bytes_transferred, 500);
        assert_eq!(updated.percentage, 50);
        
        // Verificar que se publicaron eventos
        let events = event_publisher.published_events.lock().unwrap();
        assert!(events.contains(&"progress_update".to_string()));
        
        // Verificar que se enviaron notificaciones
        let notifications = realtime_notifier.notifications.lock().unwrap();
        assert!(notifications.contains(&"notify_test-upload_50".to_string()));
    }

    #[tokio::test]
    async fn test_mark_completed() {
        let storage = Arc::new(MockProgressStorage::default());
        let event_publisher = Arc::new(MockEventPublisher::default());
        let realtime_notifier = Arc::new(MockRealtimeNotifier::default());
        
        let service = UploadProgressService::new(storage.clone(), event_publisher.clone(), realtime_notifier);
        
        // Crear sesión
        service.create_session("test-upload".to_string(), 1000).await.unwrap();
        
        // Marcar como completado
        let completed = service.mark_completed("test-upload").await.unwrap();
        
        assert_eq!(completed.status, UploadStatus::Completed);
        assert_eq!(completed.percentage, 100);
        
        // Verificar evento de completado
        let events = event_publisher.published_events.lock().unwrap();
        assert!(events.contains(&"completed_test-upload".to_string()));
    }
}