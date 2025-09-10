// crates/repository/src/features/get_repository/ports.rs

use async_trait::async_trait;
use shared::hrn::RepositoryId;
use crate::domain::{RepositoryResult, repository::Repository};

/// Puerto para obtener repositorios por HRN
#[async_trait]
pub trait RepositoryReaderPort: Send + Sync {
    /// Obtiene un repositorio por su HRN
    async fn get_repository(&self, repository_id: &RepositoryId) -> RepositoryResult<Option<Repository>>;
    
    /// Obtiene estadísticas de un repositorio
    async fn get_repository_stats(&self, repository_id: &RepositoryId) -> RepositoryResult<RepositoryStats>;
}

/// Puerto para verificar autorización de acceso
#[async_trait]
pub trait RepositoryAuthorizationPort: Send + Sync {
    /// Verifica si un usuario tiene permiso para leer un repositorio
    async fn can_read_repository(&self, user_id: &shared::hrn::UserId, repository_id: &RepositoryId) -> RepositoryResult<bool>;
}

/// Puerto para obtener estadísticas de repositorio
#[async_trait]
pub trait RepositoryStatsPort: Send + Sync {
    /// Obtiene el número de artefactos en el repositorio
    async fn get_artifact_count(&self, repository_id: &RepositoryId) -> RepositoryResult<u64>;
    
    /// Obtiene el tamaño total del repositorio en bytes
    async fn get_total_size(&self, repository_id: &RepositoryId) -> RepositoryResult<u64>;
    
    /// Obtiene la fecha del último artefacto subido
    async fn get_last_upload_date(&self, repository_id: &RepositoryId) -> RepositoryResult<Option<time::OffsetDateTime>>;
    
    /// Obtiene el número total de descargas
    async fn get_total_downloads(&self, repository_id: &RepositoryId) -> RepositoryResult<u64>;
}

/// Estadísticas de repositorio
#[derive(Debug, Clone)]
pub struct RepositoryStats {
    pub artifact_count: u64,
    pub total_size_bytes: u64,
    pub last_artifact_uploaded_at: Option<time::OffsetDateTime>,
    pub total_downloads: u64,
}