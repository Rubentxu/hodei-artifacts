// crates/repository/src/features/delete_repository/ports.rs

use async_trait::async_trait;
use shared::hrn::{RepositoryId, UserId};
use crate::domain::{RepositoryResult, repository::Repository};

/// Puerto para eliminar repositorios
#[async_trait]
pub trait RepositoryDeleterPort: Send + Sync {
    /// Elimina un repositorio existente
    async fn delete_repository(&self, repository_id: &RepositoryId) -> RepositoryResult<()>;
    
    /// Obtiene un repositorio antes de eliminarlo
    async fn get_repository_for_deletion(&self, repository_id: &RepositoryId) -> RepositoryResult<Option<Repository>>;
    
    /// Verifica si un repositorio está vacío (sin artefactos)
    async fn is_repository_empty(&self, repository_id: &RepositoryId) -> RepositoryResult<bool>;
}

/// Puerto para verificar autorización de eliminación
#[async_trait]
pub trait RepositoryDeleteAuthorizationPort: Send + Sync {
    /// Verifica si un usuario tiene permiso para eliminar un repositorio
    async fn can_delete_repository(&self, user_id: &UserId, repository_id: &RepositoryId) -> RepositoryResult<bool>;
}

/// Puerto para eliminar artefactos asociados
#[async_trait]
pub trait ArtifactDeleterPort: Send + Sync {
    /// Elimina todos los artefactos de un repositorio
    async fn delete_repository_artifacts(&self, repository_id: &RepositoryId) -> RepositoryResult<u64>;
    
    /// Obtiene el número de artefactos en un repositorio
    async fn count_repository_artifacts(&self, repository_id: &RepositoryId) -> RepositoryResult<u64>;
}

/// Puerto para publicar eventos de eliminación
#[async_trait]
pub trait RepositoryDeleteEventPublisherPort: Send + Sync {
    /// Publica un evento indicando que un repositorio fue eliminado
    async fn publish_repository_deleted(&self, repository_id: &RepositoryId, deleted_by: &UserId, 
                                       artifact_count: u64, total_size_bytes: u64) -> RepositoryResult<()>;
}