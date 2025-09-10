// crates/repository/src/features/create_repository/ports.rs

use async_trait::async_trait;
use shared::hrn::{OrganizationId, RepositoryId};
use crate::domain::{RepositoryResult, repository::Repository};

/// Puerto para verificar la existencia de organizaciones
#[async_trait]
pub trait OrganizationExistsPort: Send + Sync {
    /// Verifica si una organización existe
    async fn organization_exists(&self, organization_id: &OrganizationId) -> RepositoryResult<bool>;
}

/// Puerto para verificar la existencia de repositorios
#[async_trait]
pub trait RepositoryExistsPort: Send + Sync {
    /// Verifica si un repositorio existe con el nombre dado en la organización
    async fn repository_exists(&self, organization_id: &OrganizationId, name: &str) -> RepositoryResult<bool>;
}

/// Puerto para guardar repositorios
#[async_trait]
pub trait RepositoryCreatorPort: Send + Sync {
    /// Guarda un nuevo repositorio
    async fn create_repository(&self, repository: &Repository) -> RepositoryResult<()>;
}

/// Puerto para verificar backends de almacenamiento
#[async_trait]
pub trait StorageBackendExistsPort: Send + Sync {
    /// Verifica si un backend de almacenamiento existe
    async fn storage_backend_exists(&self, storage_backend_hrn: &str) -> RepositoryResult<bool>;
}

/// Puerto para publicar eventos
#[async_trait]
pub trait EventPublisherPort: Send + Sync {
    /// Publica un evento de repositorio creado
    async fn publish_repository_created(&self, repository: &Repository) -> RepositoryResult<()>;
}

/// Puerto para validar nombres de repositorio
pub trait RepositoryNameValidatorPort: Send + Sync {
    /// Valida que el nombre del repositorio sea válido
    fn validate_repository_name(&self, name: &str) -> RepositoryResult<()>;
}

/// Puerto para validar configuraciones de repositorio
pub trait RepositoryConfigValidatorPort: Send + Sync {
    /// Valida la configuración del repositorio según su tipo
    fn validate_repository_config(&self, repo_type: &crate::domain::repository::RepositoryType, config: &crate::domain::repository::RepositoryConfig) -> RepositoryResult<()>;
}