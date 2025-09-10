// crates/repository/src/features/update_repository/adapter.rs

use async_trait::async_trait;
use mongodb::{Collection, Database};
use mongodb::bson::{doc, Document};
use tracing::{debug, error, info};

use shared::hrn::RepositoryId;
use crate::domain::{RepositoryResult, RepositoryError};
use crate::domain::repository::Repository;
use super::ports::{RepositoryUpdaterPort, RepositoryUpdateAuthorizationPort, RepositoryConfigValidatorPort, RepositoryUpdateEventPublisherPort};

/// Adaptador MongoDB para operaciones de actualización de repositorios
pub struct MongoDbRepositoryUpdaterAdapter {
    db: Database,
}

impl MongoDbRepositoryUpdaterAdapter {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    fn repositories_collection(&self) -> Collection<RepositoryDocument> {
        self.db.collection("repositories")
    }
}

/// Documento MongoDB para repositorios (reutilizado de otras features)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RepositoryDocument {
    #[serde(rename = "_id")]
    pub id: String,
    pub hrn: String,
    pub organization_hrn: String,
    pub name: String,
    pub region: String,
    pub repo_type: String,
    pub format: String,
    pub config: mongodb::bson::Document,
    pub storage_backend_hrn: String,
    pub lifecycle: LifecycleDocument,
    pub is_public: bool,
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct LifecycleDocument {
    pub created_at: time::OffsetDateTime,
    pub created_by: String,
    pub updated_at: time::OffsetDateTime,
    pub updated_by: String,
}

impl From<&Repository> for RepositoryDocument {
    fn from(repository: &Repository) -> Self {
        RepositoryDocument {
            id: repository.hrn.as_str().to_string(),
            hrn: repository.hrn.as_str().to_string(),
            organization_hrn: repository.organization_hrn.as_str().to_string(),
            name: repository.name.clone(),
            region: repository.region.clone(),
            repo_type: format!("{:?}", repository.repo_type),
            format: format!("{:?}", repository.format),
            config: mongodb::bson::to_document(&repository.config).unwrap_or_else(|_| mongodb::bson::Document::new()),
            storage_backend_hrn: repository.storage_backend_hrn.clone(),
            lifecycle: LifecycleDocument {
                created_at: repository.lifecycle.created_at,
                created_by: repository.lifecycle.created_by.as_str().to_string(),
                updated_at: repository.lifecycle.updated_at,
                updated_by: repository.lifecycle.updated_by.as_str().to_string(),
            },
            is_public: false, // TODO: Agregar campo al modelo de dominio
            metadata: None,   // TODO: Agregar campo al modelo de dominio
        }
    }
}

impl TryFrom<RepositoryDocument> for Repository {
    type Error = RepositoryError;

    fn try_from(doc: RepositoryDocument) -> Result<Self, Self::Error> {
        use crate::domain::repository::{RepositoryType, Ecosystem, RepositoryConfig};
        
        // Parsear el tipo de repositorio
        let repo_type = match doc.repo_type.as_str() {
            "Hosted" => RepositoryType::Hosted,
            "Proxy" => RepositoryType::Proxy,
            "Virtual" => RepositoryType::Virtual,
            _ => return Err(RepositoryError::InvalidConfiguration(
                format!("Unknown repository type: {}", doc.repo_type)
            )),
        };

        // Parsear el formato/ecosistema
        let format = match doc.format.as_str() {
            "Maven" => Ecosystem::Maven,
            "Npm" => Ecosystem::Npm,
            "Docker" => Ecosystem::Docker,
            "Oci" => Ecosystem::Oci,
            "Pypi" => Ecosystem::Pypi,
            "Nuget" => Ecosystem::Nuget,
            "Go" => Ecosystem::Go,
            "RubyGems" => Ecosystem::RubyGems,
            "Helm" => Ecosystem::Helm,
            "Generic" => Ecosystem::Generic,
            _ => return Err(RepositoryError::InvalidConfiguration(
                format!("Unknown ecosystem: {}", doc.format)
            )),
        };

        // Parsear la configuración
        let config = mongodb::bson::from_document(doc.config)
            .map_err(|e| RepositoryError::InvalidConfiguration(
                format!("Failed to parse repository config: {}", e)
            ))?;

        Ok(Repository {
            hrn: shared::hrn::RepositoryId(shared::hrn::Hrn::new(&doc.hrn).map_err(|e| {
                RepositoryError::InvalidRepositoryName(format!("Invalid HRN: {}", e))
            })?),
            organization_hrn: shared::hrn::OrganizationId(shared::hrn::Hrn::new(&doc.organization_hrn).map_err(|e| {
                RepositoryError::InvalidRepositoryName(format!("Invalid organization HRN: {}", e))
            })?),
            name: doc.name,
            region: doc.region,
            repo_type,
            format,
            config,
            storage_backend_hrn: doc.storage_backend_hrn,
            lifecycle: shared::lifecycle::Lifecycle {
                created_at: doc.lifecycle.created_at,
                created_by: shared::hrn::Hrn::new(&doc.lifecycle.created_by).map_err(|e| {
                    RepositoryError::InvalidRepositoryName(format!("Invalid created_by HRN: {}", e))
                })?,
                updated_at: doc.lifecycle.updated_at,
                updated_by: shared::hrn::Hrn::new(&doc.lifecycle.updated_by).map_err(|e| {
                    RepositoryError::InvalidRepositoryName(format!("Invalid updated_by HRN: {}", e))
                })?,
            },
        })
    }
}

#[async_trait]
impl RepositoryUpdaterPort for MongoDbRepositoryUpdaterAdapter {
    async fn update_repository(&self, repository: &Repository) -> RepositoryResult<()> {
        debug!("Updating repository with HRN: {}", repository.hrn.as_str());
        
        let doc = RepositoryDocument::from(repository);
        let filter = doc! { "_id": repository.hrn.as_str() };
        
        let update_doc = doc! {
            "$set": {
                "name": &doc.name,
                "region": &doc.region,
                "config": &doc.config,
                "storage_backend_hrn": &doc.storage_backend_hrn,
                "lifecycle": &doc.lifecycle,
                "is_public": &doc.is_public,
                "metadata": &doc.metadata,
            }
        };
        
        let result = self.repositories_collection()
            .update_one(filter, update_doc)
            .await
            .map_err(|e| {
                error!("Database error updating repository: {}", e);
                RepositoryError::DatabaseError(e.to_string())
            })?;

        if result.matched_count == 0 {
            return Err(RepositoryError::RepositoryNotFound(
                repository.hrn.as_str().to_string()
            ));
        }

        info!("Repository updated successfully: {}", repository.hrn.as_str());
        Ok(())
    }

    async fn get_repository_for_update(&self, repository_id: &RepositoryId) -> RepositoryResult<Option<Repository>> {
        debug!("Getting repository for update: {}", repository_id.as_str());
        
        let filter = doc! { "_id": repository_id.as_str() };
        let doc = self.repositories_collection()
            .find_one(filter)
            .await
            .map_err(|e| {
                error!("Database error getting repository for update: {}", e);
                RepositoryError::DatabaseError(e.to_string())
            })?;

        match doc {
            Some(doc) => {
                let repository: Repository = doc.try_into()?;
                info!("Found repository for update: {}", repository.name);
                Ok(Some(repository))
            },
            None => {
                debug!("Repository not found for update: {}", repository_id.as_str());
                Ok(None)
            },
        }
    }
}

/// Adaptador de autorización para actualización
pub struct RepositoryUpdateAuthorizationAdapter;

impl RepositoryUpdateAuthorizationAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RepositoryUpdateAuthorizationPort for RepositoryUpdateAuthorizationAdapter {
    async fn can_update_repository(&self, _user_id: &shared::hrn::UserId, _repository_id: &RepositoryId) -> RepositoryResult<bool> {
        // TODO: Implementar verificación real de autorización con Cedar
        // Por ahora, permitir actualización a todos los usuarios autenticados
        debug!("Authorization check passed for repository update");
        Ok(true)
    }
}

/// Adaptador de validación de configuración
pub struct RepositoryConfigValidatorAdapter;

impl RepositoryConfigValidatorAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RepositoryConfigValidatorPort for RepositoryConfigValidatorAdapter {
    async fn validate_config_update(&self, _current_config: &crate::domain::repository::RepositoryConfig, 
                                   _new_config: &crate::domain::repository::RepositoryConfig) -> RepositoryResult<()> {
        // TODO: Implementar validaciones específicas de configuración
        // Por ahora, permitir cualquier cambio de configuración
        debug!("Configuration update validation passed");
        Ok(())
    }

    async fn validate_type_consistency(&self, current_type: crate::domain::repository::RepositoryType,
                                      new_type: crate::domain::repository::RepositoryType) -> RepositoryResult<()> {
        if current_type != new_type {
            return Err(RepositoryError::RepositoryTypeMismatch {
                expected: format!("{:?}", current_type),
                actual: format!("{:?}", new_type),
            });
        }
        debug!("Repository type consistency validation passed");
        Ok(())
    }
}

/// Adaptador de publicación de eventos
pub struct RepositoryUpdateEventPublisherAdapter;

impl RepositoryUpdateEventPublisherAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RepositoryUpdateEventPublisherPort for RepositoryUpdateEventPublisherAdapter {
    async fn publish_repository_updated(&self, repository_id: &RepositoryId, updated_by: &UserId, 
                                       changes: Vec<String>) -> RepositoryResult<()> {
        info!("Publishing repository updated event for {} by {} with changes: {:?}", 
              repository_id.as_str(), updated_by.as_str(), changes);
        
        // TODO: Implementar publicación real de eventos (Kafka/RabbitMQ)
        // Por ahora, solo loguear el evento
        debug!("Repository updated event published (mock)");
        Ok(())
    }
}