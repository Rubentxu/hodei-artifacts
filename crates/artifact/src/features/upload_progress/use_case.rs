// crates/artifact/src/features/upload_artifact/upload_progress/use_case.rs

use std::sync::Arc;
use tracing::{info, warn};

use super::ports::{ProgressStorage, ProgressEventPublisher, RealtimeNotifier, ChunkedUploadStorage, ProgressResult};
use super::dto::{UploadProgress, UploadStatus, UpdateProgressCommand, ReceivedChunksResponse, ReceivedChunkInfo};
use crate::features::upload_progress::ProgressError;

/// Caso de uso principal para gestión de progreso de subidas
#[derive(Clone)]
pub struct UploadProgressUseCase {
    storage: Arc<dyn ProgressStorage>,
    event_publisher: Arc<dyn ProgressEventPublisher>,
    realtime_notifier: Arc<dyn RealtimeNotifier>,
    chunked_storage: Option<Arc<dyn ChunkedUploadStorage + Send + Sync>>,
}

impl UploadProgressUseCase {
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

