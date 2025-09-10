// crates/repository/src/features/create_repository/adapter.rs

use async_trait::async_trait;
use mongodb::{Collection, Database};
use mongodb::bson::{doc, Document};
use serde::{Serialize, Deserialize};
use tracing::{debug, error, info};

use shared::hrn::{OrganizationId, RepositoryId};
use crate::domain::{RepositoryResult, RepositoryError};
use crate::domain::repository::Repository;
use super::ports::{
    OrganizationExistsPort, RepositoryExistsPort, RepositoryCreatorPort,
    StorageBackendExistsPort, EventPublisherPort, RepositoryNameValidatorPort,
    RepositoryConfigValidatorPort
};

/// Adaptador MongoDB para operaciones de creación de repositorios
pub struct MongoDbRepositoryAdapter {
    db: Database,
}

impl MongoDbRepositoryAdapter {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    fn repositories_collection(&self) -> Collection<RepositoryDocument> {
        self.db.collection("repositories")
    }

    fn organizations_collection(&self) -> Collection<Document> {
        self.db.collection("organizations")
    }

    fn storage_backends_collection(&self) -> Collection<Document> {
        self.db.collection("storage_backends")
    }
}

/// Documento MongoDB para repositorios
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RepositoryDocument {
    #[serde(rename = "_id")]
    pub id: String,
    pub hrn: String,
    pub organization_hrn: String,
    pub name: String,
    pub region: String,
    pub repo_type: String,
    pub format: String,
    pub config: Document,
    pub storage_backend_hrn: String,
    pub lifecycle: LifecycleDocument,
    pub is_public: bool,
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LifecycleDocument {
    pub created_at: time::OffsetDateTime,
    pub created_by: String,
    pub updated_at: time::OffsetDateTime,
    pub updated_by: String,
}

impl From<&Repository> for RepositoryDocument {
    fn from(repo: &Repository) -> Self {
        RepositoryDocument {
            id: repo.hrn.as_str().to_string(),
            hrn: repo.hrn.as_str().to_string(),
            organization_hrn: repo.organization_hrn.as_str().to_string(),
            name: repo.name.clone(),
            region: repo.region.clone(),
            repo_type: format!("{:?}", repo.repo_type),
            format: format!("{:?}", repo.format),
            config: mongodb::bson::to_document(&repo.config).unwrap_or_default(),
            storage_backend_hrn: repo.storage_backend_hrn.clone(),
            lifecycle: LifecycleDocument {
                created_at: repo.lifecycle.created_at,
                created_by: repo.lifecycle.created_by.clone(),
                updated_at: repo.lifecycle.updated_at,
                updated_by: repo.lifecycle.updated_by.clone(),
            },
            is_public: false, // Default to private
            metadata: None,
        }
    }
}

#[async_trait]
impl OrganizationExistsPort for MongoDbRepositoryAdapter {
    async fn organization_exists(&self, organization_id: &OrganizationId) -> RepositoryResult<bool> {
        debug!("Checking if organization exists: {}", organization_id.as_str());
        
        let filter = doc! { "_id": organization_id.as_str() };
        let count = self.organizations_collection()
            .count_documents(filter)
            .await
            .map_err(|e| {
                error!("Database error checking organization existence: {}", e);
                RepositoryError::DatabaseError(e.to_string())
            })?;
        
        Ok(count > 0)
    }
}

#[async_trait]
impl RepositoryExistsPort for MongoDbRepositoryAdapter {
    async fn repository_exists(&self, organization_id: &OrganizationId, name: &str) -> RepositoryResult<bool> {
        debug!("Checking if repository '{}' exists in organization '{}'", name, organization_id.as_str());
        
        let filter = doc! { 
            "organization_hrn": organization_id.as_str(),
            "name": name 
        };
        let count = self.repositories_collection()
            .count_documents(filter)
            .await
            .map_err(|e| {
                error!("Database error checking repository existence: {}", e);
                RepositoryError::DatabaseError(e.to_string())
            })?;
        
        Ok(count > 0)
    }
}

#[async_trait]
impl RepositoryCreatorPort for MongoDbRepositoryAdapter {
    async fn create_repository(&self, repository: &Repository) -> RepositoryResult<()> {
        info!("Creating repository '{}' with HRN: {}", repository.name, repository.hrn.as_str());
        
        let doc = RepositoryDocument::from(repository);
        
        self.repositories_collection()
            .insert_one(doc)
            .await
            .map_err(|e| {
                error!("Database error creating repository: {}", e);
                RepositoryError::DatabaseError(e.to_string())
            })?;
        
        info!("Repository '{}' created successfully", repository.name);
        Ok(())
    }
}

#[async_trait]
impl StorageBackendExistsPort for MongoDbRepositoryAdapter {
    async fn storage_backend_exists(&self, storage_backend_hrn: &str) -> RepositoryResult<bool> {
        debug!("Checking if storage backend exists: {}", storage_backend_hrn);
        
        let filter = doc! { "_id": storage_backend_hrn };
        let count = self.storage_backends_collection()
            .count_documents(filter)
            .await
            .map_err(|e| {
                error!("Database error checking storage backend existence: {}", e);
                RepositoryError::DatabaseError(e.to_string())
            })?;
        
        Ok(count > 0)
    }
}

/// Adaptador de eventos (placeholder para implementación real)
pub struct EventPublisherAdapter;

impl EventPublisherAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl EventPublisherPort for EventPublisherAdapter {
    async fn publish_repository_created(&self, repository: &Repository) -> RepositoryResult<()> {
        info!("Publishing RepositoryCreated event for repository: {}", repository.hrn.as_str());
        
        // TODO: Implementar publicación real de eventos a RabbitMQ/Kafka
        // Por ahora, solo logueamos el evento
        
        debug!("RepositoryCreated event published for: {}", repository.hrn.as_str());
        Ok(())
    }
}

/// Validador de nombres de repositorio
pub struct RepositoryNameValidatorAdapter;

impl RepositoryNameValidatorAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl RepositoryNameValidatorPort for RepositoryNameValidatorAdapter {
    fn validate_repository_name(&self, name: &str) -> RepositoryResult<()> {
        debug!("Validating repository name: {}", name);
        
        // Validar longitud
        if name.is_empty() {
            return Err(RepositoryError::InvalidRepositoryName("Repository name cannot be empty".to_string()));
        }
        
        if name.len() > 100 {
            return Err(RepositoryError::InvalidRepositoryName("Repository name cannot exceed 100 characters".to_string()));
        }
        
        // Validar caracteres permitidos (alfanuméricos, guiones y guiones bajos)
        if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
            return Err(RepositoryError::InvalidRepositoryName("Repository name can only contain alphanumeric characters, hyphens, and underscores".to_string()));
        }
        
        // No puede comenzar ni terminar con guión o guión bajo
        if name.starts_with('-') || name.starts_with('_') || name.ends_with('-') || name.ends_with('_') {
            return Err(RepositoryError::InvalidRepositoryName("Repository name cannot start or end with hyphens or underscores".to_string()));
        }
        
        debug!("Repository name '{}' is valid", name);
        Ok(())
    }
}

/// Validador de configuraciones de repositorio
pub struct RepositoryConfigValidatorAdapter;

impl RepositoryConfigValidatorAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl RepositoryConfigValidatorPort for RepositoryConfigValidatorAdapter {
    fn validate_repository_config(
        &self, 
        repo_type: &crate::domain::repository::RepositoryType, 
        config: &crate::domain::repository::RepositoryConfig
    ) -> RepositoryResult<()> {
        debug!("Validating repository config for type: {:?}", repo_type);
        
        match (repo_type, config) {
            (crate::domain::repository::RepositoryType::Hosted, crate::domain::repository::RepositoryConfig::Hosted(_)) => {
                // Configuración Hosted es válida
                Ok(())
            },
            (crate::domain::repository::RepositoryType::Proxy, crate::domain::repository::RepositoryConfig::Proxy(proxy_config)) => {
                // Validar URL del repositorio remoto
                if proxy_config.remote_url.scheme() != "http" && proxy_config.remote_url.scheme() != "https" {
                    return Err(RepositoryError::InvalidConfiguration(
                        "Proxy repository remote URL must use HTTP or HTTPS protocol".to_string()
                    ));
                }
                Ok(())
            },
            (crate::domain::repository::RepositoryType::Virtual, crate::domain::repository::RepositoryConfig::Virtual(virtual_config)) => {
                // Validar que hay al menos 2 repositorios agregados
                if virtual_config.aggregated_repositories.len() < 2 {
                    return Err(RepositoryError::InvalidConfiguration(
                        "Virtual repository must aggregate at least 2 repositories".to_string()
                    ));
                }
                Ok(())
            },
            _ => {
                Err(RepositoryError::RepositoryTypeMismatch {
                    expected: format!("{:?}", repo_type),
                    actual: format!("{:?}", config),
                })
            },
        }
    }
}