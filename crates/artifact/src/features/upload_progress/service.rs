// crates/artifact/src/features/upload_artifact/upload_progress/service.rs

use std::sync::Arc;
use async_trait::async_trait;
use tracing::{info, warn, error};

use super::ports::{ProgressStorage, ProgressEventPublisher, RealtimeNotifier, ChunkedUploadStorage, ProgressResult, ProgressError};
use super::dto::{UploadProgress, UploadStatus, UpdateProgressCommand, ReceivedChunksResponse, ReceivedChunkInfo};

/// Servicio principal para gestión de progreso de subidas
#[derive(Clone)]
pub struct UploadProgressService {
    storage: Arc<dyn ProgressStorage>,
    event_publisher: Arc<dyn ProgressEventPublisher>,
    realtime_notifier: Arc<dyn RealtimeNotifier>,
    chunked_storage: Option<Arc<dyn ChunkedUploadStorage + Send + Sync>>,
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
            chunked_storage: None,
        }
    }
    
    /// Nuevo constructor que incluye el chunked storage
    pub fn new_with_chunked_storage(
        storage: Arc<dyn ProgressStorage>,
        event_publisher: Arc<dyn ProgressEventPublisher>,
        realtime_notifier: Arc<dyn RealtimeNotifier>,
        chunked_storage: Arc<dyn ChunkedUploadStorage + Send + Sync>,
    ) -> Self {
        Self {
            storage,
            event_publisher,
            realtime_notifier,
            chunked_storage: Some(chunked_storage),
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
        info!(upload_id = %upload_id, client_id = %client_id, "Subscribing client to upload progress");
        
        self.realtime_notifier.subscribe(upload_id, client_id).await?;
        
        info!(upload_id = %upload_id, client_id = %client_id, "Client subscribed successfully");
        Ok(())
    }

    /// Desuscribir cliente
    pub async fn unsubscribe_client(&self, client_id: &str) -> ProgressResult<()> {
        info!(client_id = %client_id, "Unsubscribing client from upload progress");
        
        self.realtime_notifier.unsubscribe(client_id).await?;
        
        info!(client_id = %client_id, "Client unsubscribed successfully");
        Ok(())
    }
    
    /// Obtener la lista de chunks recibidos para una subida
    pub async fn get_received_chunks(&self, upload_id: &str) -> ProgressResult<ReceivedChunksResponse> {
        info!(upload_id = %upload_id, "Getting received chunks for upload");
        
        // Verificar que tenemos acceso al chunked storage
        let chunked_storage = self.chunked_storage.as_ref()
            .ok_or_else(|| ProgressError::StorageError("Chunked storage not available".to_string()))?;
            
        // Obtener el progreso para obtener el total de chunks
        let progress = self.get_progress(upload_id).await?;
        
        // Obtener la lista de números de chunks recibidos
        let chunk_numbers = chunked_storage.get_received_chunk_numbers(upload_id)
            .await
            .map_err(|e| match e {
                ProgressError::StorageError(msg) => ProgressError::StorageError(msg),
                _ => ProgressError::StorageError("Unknown error getting chunk numbers".to_string()),
            })?;
        
        // Crear la lista de información de chunks
        let mut received_chunks = Vec::new();
        for &chunk_number in &chunk_numbers {
            // Para obtener el tamaño real del chunk, necesitaríamos acceder al storage
            // Por ahora usamos un tamaño ficticio
            received_chunks.push(ReceivedChunkInfo {
                chunk_number,
                size: 0, // Tamaño por determinar
            });
        }
        
        // Calcular total_chunks basado en el progreso
        // Asumimos un tamaño de chunk de 1MB (1024*1024 bytes)
        let chunk_size = 1024 * 1024;
        let total_chunks = ((progress.total_bytes as f64) / (chunk_size as f64)).ceil() as usize;
        
        let response = ReceivedChunksResponse {
            upload_id: upload_id.to_string(),
            total_chunks,
            received_chunks,
            received_chunk_numbers: chunk_numbers,
        };
        
        info!(upload_id = %upload_id, received_count = response.received_chunk_numbers.len(), "Retrieved received chunks");
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
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
    
    #[derive(Default)]
    struct MockChunkedUploadStorage {
        chunks: Mutex<std::collections::HashMap<String, Vec<usize>>>,
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

        async fn assemble_chunks(&self, _upload_id: &str, _total_chunks: usize, _file_name: &str) -> Result<(PathBuf, String), ProgressError> {
            Ok((PathBuf::from("/tmp/test"), "test-hash".to_string()))
        }

        async fn cleanup(&self, _upload_id: &str) -> Result<(), ProgressError> {
            Ok(())
        }
        
        async fn get_received_chunk_numbers(&self, upload_id: &str) -> Result<Vec<usize>, ProgressError> {
            let chunks = self.chunks.lock().unwrap();
            Ok(chunks.get(upload_id).cloned().unwrap_or_else(Vec::new))
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
        
        let service = UploadProgressService::new(storage.clone(), event_publisher, realtime_notifier);
        
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
    
    #[tokio::test]
    async fn test_get_received_chunks() {
        let storage = Arc::new(MockProgressStorage::default());
        let event_publisher = Arc::new(MockEventPublisher::default());
        let realtime_notifier = Arc::new(MockRealtimeNotifier::default());
        
        // Crear un mock de ChunkedUploadStorage
        let chunked_storage = Arc::new(MockChunkedUploadStorage::default());
        
        let service = UploadProgressService::new_with_chunked_storage(
            storage.clone(), 
            event_publisher, 
            realtime_notifier,
            chunked_storage.clone(),
        );
        
        // Crear sesión de prueba
        service.create_session("test-user-chunks-123".to_string(), 3 * 1024 * 1024).await.unwrap(); // 3MB
        
        // Simular que se han recibido algunos chunks
        {
            let chunks = &chunked_storage.chunks.lock().unwrap();
            chunks.insert("test-user-chunks-123".to_string(), vec![1, 2]);
        }
        
        // Obtener chunks recibidos
        let chunks_response = service.get_received_chunks("test-user-chunks-123").await.unwrap();
        
        assert_eq!(chunks_response.upload_id, "test-user-chunks-123");
        assert_eq!(chunks_response.total_chunks, 3); // 3MB / 1MB por chunk
        assert_eq!(chunks_response.received_chunk_numbers, vec![1, 2]);
        assert_eq!(chunks_response.received_chunks.len(), 2);
    }

    #[tokio::test]
    async fn test_get_received_chunks_no_chunks_received() {
        let storage = Arc::new(MockProgressStorage::default());
        let event_publisher = Arc::new(MockEventPublisher::default());
        let realtime_notifier = Arc::new(MockRealtimeNotifier::default());
        let chunked_storage = Arc::new(MockChunkedUploadStorage::default());
        
        let service = UploadProgressService::new_with_chunked_storage(
            storage.clone(), 
            event_publisher, 
            realtime_notifier,
            chunked_storage
        );
        
        // Crear sesión de prueba
        service.create_session("test-upload-no-chunks".to_string(), 5 * 1024 * 1024).await.unwrap(); // 5MB
        
        // Obtener chunks recibidos (no hay ninguno)
        let chunks_response = service.get_received_chunks("test-upload-no-chunks").await.unwrap();
        
        assert_eq!(chunks_response.upload_id, "test-upload-no-chunks");
        assert_eq!(chunks_response.total_chunks, 5); // 5MB / 1MB por chunk
        assert_eq!(chunks_response.received_chunk_numbers.len(), 0);
        assert_eq!(chunks_response.received_chunks.len(), 0);
    }

    #[tokio::test]
    async fn test_get_received_chunks_upload_not_found() {
        let storage = Arc::new(MockProgressStorage::default());
        let event_publisher = Arc::new(MockEventPublisher::default());
        let realtime_notifier = Arc::new(MockRealtimeNotifier::default());
        let chunked_storage = Arc::new(MockChunkedUploadStorage::default());
        
        let service = UploadProgressService::new_with_chunked_storage(
            storage.clone(), 
            event_publisher, 
            realtime_notifier,
            chunked_storage
        );
        
        // Intentar obtener chunks de una sesión inexistente
        let result = service.get_received_chunks("non-existent-upload").await;
        
        assert!(result.is_err());
        match result.unwrap_err() {
            ProgressError::SessionNotFound(_) => {}, // Correcto
            _ => panic!("Se esperaba SessionNotFound"),
        }
    }

    #[tokio::test]
    async fn test_get_received_chunks_chunked_storage_not_available() {
        let storage = Arc::new(MockProgressStorage::default());
        let event_publisher = Arc::new(MockEventPublisher::default());
        let realtime_notifier = Arc::new(MockRealtimeNotifier::default());
        
        // Crear servicio sin chunked storage
        let service = UploadProgressService::new(storage, event_publisher, realtime_notifier);
        
        // Intentar obtener chunks sin chunked storage
        let result = service.get_received_chunks("any-upload").await;
        
        assert!(result.is_err());
        match result.unwrap_err() {
            ProgressError::StorageError(msg) => {
                assert!(msg.contains("Chunked storage not available"));
            },
            _ => panic!("Se esperaba StorageError"),
        }
    }
}