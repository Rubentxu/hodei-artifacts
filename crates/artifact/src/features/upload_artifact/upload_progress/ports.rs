use async_trait::async_trait;
use super::dto::{UploadProgress, UpdateProgressCommand};

/// Tipo de resultado para los ports de progress tracking
pub type ProgressResult<T> = Result<T, ProgressError>;

/// Errores específicos del progress tracking
#[derive(Debug, thiserror::Error)]
pub enum ProgressError {
    #[error("Upload session not found: {0}")]
    SessionNotFound(String),
    
    #[error("Invalid upload ID: {0}")]
    InvalidUploadId(String),
    
    #[error("Access denied to upload session: {0}")]
    AccessDenied(String),
    
    #[error("Session expired: {0}")]
    SessionExpired(String),
    
    #[error("Storage error: {0}")]
    StorageError(String),
}

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