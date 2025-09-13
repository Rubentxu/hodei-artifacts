// crates/repository/src/infrastructure/unified_adapter.rs

use std::sync::Arc;
use async_trait::async_trait;
use mongodb::Database;
use tracing::{info, debug, error, instrument};

use shared::hrn::{RepositoryId, OrganizationId, UserId};
use crate::domain::{RepositoryResult, RepositoryError};
use crate::domain::repository::Repository;

// Importar todos los puertos de las features
use crate::features::create_repository::ports::{RepositoryCreatorPort, RepositoryExistsPort};
use crate::features::get_repository::ports::RepositoryReaderPort;
use crate::features::update_repository::ports::RepositoryUpdaterPort;
use crate::features::delete_repository::ports::{
    RepositoryDeleterPort, RepositoryDeleteAuthorizationPort, ArtifactDeleterPort,
    RepositoryDeleteEventPublisherPort
};

use super::mongodb_adapter::MongoDbRepositoryAdapter;

/// Adaptador unificado que implementa todos los puertos de CRUD para repositorios
pub struct UnifiedRepositoryAdapter {
    mongo_adapter: Arc<MongoDbRepositoryAdapter>,
}

impl UnifiedRepositoryAdapter {
    pub fn new(db: Database) -> Self {
        Self {
            mongo_adapter: Arc::new(MongoDbRepositoryAdapter::new(db)),
        }
    }

    pub fn get_mongo_adapter(&self) -> &MongoDbRepositoryAdapter {
        &self.mongo_adapter
    }
}

// Implementación para Create Repository
#[async_trait]
impl RepositoryCreatorPort for UnifiedRepositoryAdapter {
    #[instrument(skip(self, repository))]
    async fn create_repository(&self, repository: &Repository) -> RepositoryResult<()> {
        info!("Creating repository via unified adapter: {}", repository.hrn.as_str());
        self.mongo_adapter.create_repository(repository).await
    }
}

// Implementación para Repository Exists
#[async_trait]
impl RepositoryExistsPort for UnifiedRepositoryAdapter {
    #[instrument(skip(self, organization_id, name))]
    async fn repository_exists(&self, organization_id: &OrganizationId, name: &str) -> RepositoryResult<bool> {
        debug!("Checking if repository exists: {} in organization {}", name, organization_id.as_str());
        
        // Try to find repository by organization and name
        let repositories = self.mongo_adapter.list_repositories(organization_id).await?;
        
        let exists = repositories.iter().any(|repo| repo.name == name);
        Ok(exists)
    }
}

// Implementación para Get Repository
#[async_trait]
impl RepositoryReaderPort for UnifiedRepositoryAdapter {
    #[instrument(skip(self, repository_id))]
    async fn get_repository(&self, repository_id: &RepositoryId) -> RepositoryResult<Option<Repository>> {
        debug!("Getting repository via unified adapter: {}", repository_id.as_str());
        self.mongo_adapter.get_repository(repository_id).await
    }
}

// Implementación para Update Repository
#[async_trait]
impl RepositoryUpdaterPort for UnifiedRepositoryAdapter {
    #[instrument(skip(self, repository))]
    async fn update_repository(&self, repository: &Repository) -> RepositoryResult<()> {
        info!("Updating repository via unified adapter: {}", repository.hrn.as_str());
        self.mongo_adapter.update_repository(repository).await
    }
}

// Implementación para Delete Repository
#[async_trait]
impl RepositoryDeleterPort for UnifiedRepositoryAdapter {
    #[instrument(skip(self, repository_id))]
    async fn delete_repository(&self, repository_id: &RepositoryId) -> RepositoryResult<()> {
        info!("Deleting repository via unified adapter: {}", repository_id.as_str());
        self.mongo_adapter.delete_repository(repository_id).await
    }

    #[instrument(skip(self, repository_id))]
    async fn get_repository_for_deletion(&self, repository_id: &RepositoryId) -> RepositoryResult<Option<Repository>> {
        debug!("Getting repository for deletion via unified adapter: {}", repository_id.as_str());
        self.mongo_adapter.get_repository(repository_id).await
    }

    #[instrument(skip(self, repository_id))]
    async fn is_repository_empty(&self, repository_id: &RepositoryId) -> RepositoryResult<bool> {
        debug!("Checking if repository is empty via unified adapter: {}", repository_id.as_str());
        self.mongo_adapter.is_repository_empty(repository_id).await
    }
}

// Implementación para Authorization (placeholder para Cedar)
#[async_trait]
impl RepositoryDeleteAuthorizationPort for UnifiedRepositoryAdapter {
    #[instrument(skip(self, user_id, repository_id))]
    async fn can_delete_repository(&self, user_id: &UserId, repository_id: &RepositoryId) -> RepositoryResult<bool> {
        debug!("Checking authorization for repository deletion: user={}, repo={}", 
               user_id.as_str(), repository_id.as_str());
        
        // TODO: Implementar verificación real de autorización con Cedar
        // Por ahora, permitir eliminación a todos los usuarios autenticados
        info!("Authorization check passed for repository deletion (mock)");
        Ok(true)
    }
}

// Implementación para Artifact Management (placeholder para integración)
#[async_trait]
impl ArtifactDeleterPort for UnifiedRepositoryAdapter {
    #[instrument(skip(self, repository_id))]
    async fn delete_repository_artifacts(&self, repository_id: &RepositoryId) -> RepositoryResult<u64> {
        info!("Deleting artifacts from repository via unified adapter: {}", repository_id.as_str());
        
        // TODO: Implementar eliminación real de artefactos cuando esté disponible el crate de artifact
        // Por ahora, simular eliminación exitosa
        let count = self.mongo_adapter.count_repository_artifacts(repository_id).await?;
        debug!("Artifacts deleted successfully (placeholder) - count: {}", count);
        Ok(count)
    }

    #[instrument(skip(self, repository_id))]
    async fn count_repository_artifacts(&self, repository_id: &RepositoryId) -> RepositoryResult<u64> {
        debug!("Counting artifacts in repository via unified adapter: {}", repository_id.as_str());
        self.mongo_adapter.count_repository_artifacts(repository_id).await
    }
}

/// Adaptador de eventos (placeholder para Kafka/RabbitMQ)
pub struct EventPublisherAdapter;

impl EventPublisherAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RepositoryDeleteEventPublisherPort for EventPublisherAdapter {
    #[instrument(skip(self, repository_id, deleted_by, artifact_count, total_size_bytes))]
    async fn publish_repository_deleted(&self, repository_id: &RepositoryId, deleted_by: &UserId, 
                                       artifact_count: u64, total_size_bytes: u64) -> RepositoryResult<()> {
        info!("Publishing repository deleted event for {} by {} with {} artifacts and {} bytes", 
              repository_id.as_str(), deleted_by.as_str(), artifact_count, total_size_bytes);
        
        // TODO: Implementar publicación real de eventos (Kafka/RabbitMQ)
        // Por ahora, solo loguear el evento
        debug!("Repository deleted event published (mock)");
        Ok(())
    }
}

/// Builder para crear el adaptador unificado con todas las dependencias
pub struct UnifiedRepositoryAdapterBuilder {
    db: Option<Database>,
}

impl UnifiedRepositoryAdapterBuilder {
    pub fn new() -> Self {
        Self { db: None }
    }

    pub fn with_database(mut self, db: Database) -> Self {
        self.db = Some(db);
        self
    }

    pub fn build(self) -> Result<UnifiedRepositoryAdapter, String> {
        let db = self.db.ok_or("Database is required")?;
        Ok(UnifiedRepositoryAdapter::new(db))
    }
}

/// Helper para crear el contenedor DI con el adaptador unificado
pub fn create_unified_di_container(db: Database) -> UnifiedRepositoryAdapter {
    UnifiedRepositoryAdapter::new(db)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::hrn::{OrganizationId, RepositoryId, UserId};
    use shared::enums::Ecosystem;
    use crate::domain::repository::{RepositoryType, DeploymentPolicy, RepositoryConfig, HostedConfig};

    #[tokio::test]
    async fn test_unified_adapter_operations() {
        // Crear adaptador con base de datos mock
        let adapter = UnifiedRepositoryAdapter::new(mongodb::Database::default());
        
        let org_id = OrganizationId::new("test-org").unwrap();
        let repo_id = RepositoryId::new(&org_id, "test-repo").unwrap();
        let user_id = UserId::new_system_user();
        
        let repository = Repository {
            hrn: repo_id.clone(),
            organization_hrn: org_id,
            name: "test-repo".to_string(),
            region: "us-east-1".to_string(),
            repo_type: RepositoryType::Hosted,
            format: Ecosystem::Maven,
            config: RepositoryConfig::Hosted(HostedConfig {
                deployment_policy: DeploymentPolicy::AllowSnapshots,
            }),
            storage_backend_hrn: "hrn:hodei:repository:us-east-1:test-storage".to_string(),
            lifecycle: shared::lifecycle::Lifecycle::new(user_id.0),
        };

        // Test create
        let result = adapter.create_repository(&repository).await;
        // Como es una base de datos mock, esperamos un error de conexión
        assert!(result.is_err());
        
        // Test exists
        let exists = adapter.repository_exists(&repo_id).await.unwrap();
        assert!(!exists); // No existe en base de datos mock
        
        // Test get
        let repo = adapter.get_repository(&repo_id).await.unwrap();
        assert!(repo.is_none()); // No existe en base de datos mock
        
        // Test authorization
        let can_delete = adapter.can_delete_repository(&user_id, &repo_id).await.unwrap();
        assert!(can_delete); // Siempre autoriza en modo mock
        
        // Test artifact operations
        let count = adapter.count_repository_artifacts(&repo_id).await.unwrap();
        assert_eq!(count, 0); // Siempre 0 en modo mock
        
        let deleted = adapter.delete_repository_artifacts(&repo_id).await.unwrap();
        assert_eq!(deleted, 0); // Siempre 0 en modo mock
    }
}