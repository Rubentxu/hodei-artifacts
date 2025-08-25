use thiserror::Error;
use shared::domain::model::{ArtifactId, RepositoryId};

#[derive(Debug, Error)]
pub enum ArtifactError {
    // Validation errors
    #[error("Comando de upload inválido: {reason}")]
    InvalidUploadCommand { reason: String },
    
    #[error("Checksum inválido: esperado formato 'sha256:hash', recibido '{checksum}'")]
    InvalidChecksum { checksum: String },
    
    #[error("Tamaño de archivo inválido: {size_bytes} bytes excede el límite de {limit_bytes} bytes")]
    FileSizeExceeded { size_bytes: u64, limit_bytes: u64 },
    
    #[error("Nombre de archivo vacío o inválido")]
    InvalidFileName,
    
    #[error("Versión del artifact inválida: '{version}'")]
    InvalidVersion { version: String },
    
    // Repository errors
    #[error("Error al acceder al repositorio: {0}")]
    RepositoryAccess(String),
    
    #[error("Error al guardar en repositorio: {operation}")]
    RepositorySave { operation: String, #[source] source: Box<dyn std::error::Error + Send + Sync> },
    
    #[error("Error al buscar en repositorio por repo_id={repo_id}, checksum={checksum}")]
    RepositoryQuery { 
        repo_id: String, 
        checksum: String, 
        #[source] source: Box<dyn std::error::Error + Send + Sync> 
    },
    
    // Storage errors
    #[error("Error al subir archivo al storage: {0}")]
    StorageUpload(String),
    
    #[error("Error al descargar archivo del storage: {0}")]
    StorageDownload(String),
    
    #[error("Archivo no encontrado en storage: {path}")]
    StorageFileNotFound { path: String },
    
    // Event publishing errors
    #[error("Error al publicar evento {event_type}")]
    EventPublishing { event_type: String, #[source] source: Box<dyn std::error::Error + Send + Sync> },
    
    #[error("Error al construir evento: {reason}")]
    EventBuilding { reason: String },
    
    // Business logic errors
    #[error("Conflicto de checksum: artifact con repo_id={repo_id} y mismo nombre existe con checksum diferente. Existente: {existing_checksum}, nuevo: {new_checksum}")]
    ChecksumConflict { 
        repo_id: String,
        existing_checksum: String, 
        new_checksum: String 
    },
    
    #[error("Artifact ya existe: id={artifact_id}")]
    ArtifactAlreadyExists { artifact_id: ArtifactId },
    
    #[error("Artifact no encontrado: id={artifact_id}")]
    ArtifactNotFound { artifact_id: ArtifactId },
    
    #[error("Repositorio no encontrado: id={repository_id}")]
    RepositoryNotFound { repository_id: RepositoryId },
    
    // Infrastructure errors
    #[error("Error de infraestructura: {component}")]
    Infrastructure { component: String, #[source] source: Box<dyn std::error::Error + Send + Sync> },
    
    #[error("Timeout en operación: {operation} después de {timeout_ms}ms")]
    Timeout { operation: String, timeout_ms: u64 },
    
    // Serialization errors
    #[error("Error de serialización: {0}")]
    Serialization(#[source] Box<dyn std::error::Error + Send + Sync>),
    
    // Legacy compatibility variants (to be migrated)
    #[error("Error de repositorio: {0}")]
    Repository(String),
    
    #[error("Error de storage: {0}")]
    Storage(String),
    
    #[error("Error de evento: {0}")]
    Event(String),
    
    #[error("Artifact duplicado")]
    Duplicate,
    
    #[error("Artifact no encontrado")]
    NotFound,
    
    // Generic fallback (to be avoided in new code)
    #[error("Error no clasificado: {message}")]
    #[deprecated = "Use specific error variants instead"]
    Generic { message: String },
}

impl ArtifactError {
    /// Creates a validation error for upload commands
    pub fn invalid_upload_command(reason: impl Into<String>) -> Self {
        Self::InvalidUploadCommand { reason: reason.into() }
    }
    
    /// Creates a checksum validation error
    pub fn invalid_checksum(checksum: impl Into<String>) -> Self {
        Self::InvalidChecksum { checksum: checksum.into() }
    }
    
    /// Creates a file size validation error
    pub fn file_size_exceeded(size_bytes: u64, limit_bytes: u64) -> Self {
        Self::FileSizeExceeded { size_bytes, limit_bytes }
    }
    
    /// Creates a repository access error
    pub fn repository_access(message: impl Into<String>) -> Self {
        Self::RepositoryAccess(message.into())
    }
    
    /// Creates a repository save error
    pub fn repository_save(operation: impl Into<String>, source: Box<dyn std::error::Error + Send + Sync>) -> Self {
        Self::RepositorySave { 
            operation: operation.into(), 
            source 
        }
    }
    
    /// Creates a repository query error
    pub fn repository_query(
        repo_id: impl Into<String>, 
        checksum: impl Into<String>, 
        source: Box<dyn std::error::Error + Send + Sync>
    ) -> Self {
        Self::RepositoryQuery { 
            repo_id: repo_id.into(),
            checksum: checksum.into(),
            source 
        }
    }
    
    /// Creates a storage upload error
    pub fn storage_upload(message: impl Into<String>) -> Self {
        Self::StorageUpload(message.into())
    }
    
    /// Creates a storage download error
    pub fn storage_download(message: impl Into<String>) -> Self {
        Self::StorageDownload(message.into())
    }
    
    /// Creates a checksum conflict error
    pub fn checksum_conflict(
        repo_id: impl Into<String>,
        existing_checksum: impl Into<String>, 
        new_checksum: impl Into<String>
    ) -> Self {
        Self::ChecksumConflict { 
            repo_id: repo_id.into(),
            existing_checksum: existing_checksum.into(),
            new_checksum: new_checksum.into()
        }
    }
    
    /// Creates an event publishing error
    pub fn event_publishing(event_type: impl Into<String>, source: Box<dyn std::error::Error + Send + Sync>) -> Self {
        Self::EventPublishing { 
            event_type: event_type.into(), 
            source 
        }
    }
    
    /// Checks if this error represents a retryable condition
    pub fn is_retryable(&self) -> bool {
        matches!(self, 
            ArtifactError::RepositoryAccess(_) |
            ArtifactError::StorageUpload(_) |
            ArtifactError::StorageDownload(_) |
            ArtifactError::EventPublishing { .. } |
            ArtifactError::Infrastructure { .. } |
            ArtifactError::Timeout { .. }
        )
    }
    
    /// Checks if this error represents a client error (4xx equivalent)
    pub fn is_client_error(&self) -> bool {
        matches!(self, 
            ArtifactError::InvalidUploadCommand { .. } |
            ArtifactError::InvalidChecksum { .. } |
            ArtifactError::FileSizeExceeded { .. } |
            ArtifactError::InvalidFileName |
            ArtifactError::InvalidVersion { .. } |
            ArtifactError::ArtifactNotFound { .. } |
            ArtifactError::RepositoryNotFound { .. }
        )
    }
    
    /// Checks if this error represents a server error (5xx equivalent)
    pub fn is_server_error(&self) -> bool {
        !self.is_client_error()
    }
}

pub type ArtifactResult<T> = Result<T, ArtifactError>;
