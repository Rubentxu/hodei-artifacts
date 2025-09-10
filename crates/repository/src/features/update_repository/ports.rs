// crates/repository/src/features/update_repository/ports.rs

use async_trait::async_trait;
use shared::hrn::{RepositoryId, UserId};
use crate::domain::{RepositoryResult, repository::Repository};

/// Puerto para actualizar repositorios
#[async_trait]
pub trait RepositoryUpdaterPort: Send + Sync {
    /// Actualiza un repositorio existente
    async fn update_repository(&self, repository: &Repository) -> RepositoryResult<()>;
    
    /// Obtiene un repositorio para actualización (con bloqueo si es necesario)
    async fn get_repository_for_update(&self, repository_id: &RepositoryId) -> RepositoryResult<Option<Repository>>;
}

/// Puerto para verificar autorización de actualización
#[async_trait]
pub trait RepositoryUpdateAuthorizationPort: Send + Sync {
    /// Verifica si un usuario tiene permiso para actualizar un repositorio
    async fn can_update_repository(&self, user_id: &UserId, repository_id: &RepositoryId) -> RepositoryResult<bool>;
}

/// Puerto para validar cambios de configuración
#[async_trait]
pub trait RepositoryConfigValidatorPort: Send + Sync {
    /// Valida que los cambios de configuración sean válidos
    async fn validate_config_update(&self, current_config: &crate::domain::repository::RepositoryConfig, 
                                   new_config: &crate::domain::repository::RepositoryConfig) -> RepositoryResult<()>;
    
    /// Valida que el tipo de repositorio no cambie
    async fn validate_type_consistency(&self, current_type: crate::domain::repository::RepositoryType,
                                      new_type: crate::domain::repository::RepositoryType) -> RepositoryResult<()>;
}

/// Puerto para publicar eventos de actualización
#[async_trait]
pub trait RepositoryUpdateEventPublisherPort: Send + Sync {
    /// Publica un evento indicando que un repositorio fue actualizado
    async fn publish_repository_updated(&self, repository_id: &RepositoryId, updated_by: &UserId, 
                                       changes: Vec<String>) -> RepositoryResult<()>;
}