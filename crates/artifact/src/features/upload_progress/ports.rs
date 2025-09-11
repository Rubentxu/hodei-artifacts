// crates/artifact/src/features/upload_artifact/upload_progress/ports.rs

use async_trait::async_trait;
use std::path::PathBuf;

use super::dto::{UploadProgress, UpdateProgressCommand, ReceivedChunkInfo};
use crate::features::upload_progress::ProgressError;

/// Tipo de resultado para los ports de progress tracking
pub type ProgressResult<T> = Result<T, ProgressError>;

/// Almacenamiento para sesiones de progreso de subida
#[async_trait]
pub trait ProgressStorage: Send + Sync {
    /// Crear una nueva sesión de progreso
    async fn create_session(&self, progress: UploadProgress) -> ProgressResult<()>;
    
    /// Obtener el progreso de una sesión
    async fn get_progress(&self, upload_id: &str) -> ProgressResult<UploadProgress>;
    
    /// Actualizar el progreso de una sesión
    async fn update_progress(&self, command: UpdateProgressCommand) -> ProgressResult<UploadProgress>;
    
    /// Eliminar una sesión (limpieza)
    async fn delete_session(&self, upload_id: &str) -> ProgressResult<()>;
    
    /// Listar todas las sesiones activas (para admin/monitoring)
    async fn list_sessions(&self) -> ProgressResult<Vec<UploadProgress>>;
}

/// Publicador de eventos de progreso
#[async_trait]
pub trait ProgressEventPublisher: Send + Sync {
    /// Publicar evento de actualización de progreso
    async fn publish_progress_update(&self, progress: &UploadProgress) -> ProgressResult<()>;
    
    /// Publicar evento de finalización de subida
    async fn publish_upload_completed(&self, upload_id: &str) -> ProgressResult<()>;
    
    /// Publicar evento de fallo de subida
    async fn publish_upload_failed(&self, upload_id: &str, error: &str) -> ProgressResult<()>;
}

/// Servicio de notificaciones en tiempo real
#[async_trait]
pub trait RealtimeNotifier: Send + Sync {
    /// Notificar a clientes WebSocket sobre actualización de progreso
    async fn notify_progress_update(&self, progress: &UploadProgress) -> ProgressResult<()>;
    
    /// Suscribir cliente a updates de una subida específica
    async fn subscribe(&self, upload_id: &str, client_id: &str) -> ProgressResult<()>;
    
    /// Desuscribir cliente
    async fn unsubscribe(&self, client_id: &str) -> ProgressResult<()>;
}

/// Almacenamiento para chunks de subida
#[async_trait]
pub trait ChunkedUploadStorage: Send + Sync {
    /// Guardar un chunk de una subida
    async fn save_chunk(&self, upload_id: &str, chunk_number: usize, data: bytes::Bytes) -> Result<(), ProgressError>;
    
    /// Obtener el conteo de chunks recibidos para una subida
    async fn get_received_chunks_count(&self, upload_id: &str) -> Result<usize, ProgressError>;
    
    /// Obtener la lista de números de chunks recibidos para una subida
    async fn get_received_chunk_numbers(&self, upload_id: &str) -> Result<Vec<usize>, ProgressError>;
    
    /// Obtener la información detallada de los chunks recibidos para una subida
    async fn get_received_chunks_info(&self, upload_id: &str) -> Result<Vec<ReceivedChunkInfo>, ProgressError>;
    
    /// Ensamblar los chunks en un archivo completo
    async fn assemble_chunks(&self, upload_id: &str, total_chunks: usize, file_name: &str) -> Result<(PathBuf, String), ProgressError>;
    
    /// Limpiar los chunks temporales de una subida
    async fn cleanup(&self, upload_id: &str) -> Result<(), ProgressError>;
}

/// Error específico para el almacenamiento de chunks
#[derive(Debug, thiserror::Error)]
pub enum ChunkStorageError {
    #[error("Chunk storage error: {0}")]
    StorageError(String),
    
    #[error("Chunk not found: {0}")]
    ChunkNotFound(String),
    
    #[error("Invalid chunk number: {0}")]
    InvalidChunkNumber(usize),
    
    #[error("Assembly error: {0}")]
    AssemblyError(String),
}

