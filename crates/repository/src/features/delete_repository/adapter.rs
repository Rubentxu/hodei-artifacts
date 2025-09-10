// crates/repository/src/features/delete_repository/adapter.rs

use async_trait::async_trait;
use mongodb::{Collection, Database};
use mongodb::bson::{doc, Document};
use tracing::{debug, error, info, warn};

use shared::hrn::RepositoryId;
use crate::domain::{RepositoryResult, RepositoryError};
use crate::domain::repository::Repository;
use super::ports::{
    RepositoryDeleterPort, RepositoryDeleteAuthorizationPort, ArtifactDeleterPort,
    RepositoryDeleteEventPublisherPort
};

/// Adaptador MongoDB para operaciones de eliminación de repositorios
pub struct MongoDbRepositoryDeleterAdapter {
    db: Database,
}

impl MongoDbRepositoryDeleterAdapter {
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
impl RepositoryDeleterPort for MongoDbRepositoryDeleterAdapter {
    async fn delete_repository(&self, repository_id: &RepositoryId) -> RepositoryResult<()> {
        debug!("Deleting repository with HRN: {}", repository_id.as_str());
        
        let filter = doc! { "_id": repository_id.as_str() };
        
        let result = self.repositories_collection()
            .delete_one(filter)
            .await
            .map_err(|e| {
                error!("Database error deleting repository: {}", e);
                RepositoryError::DatabaseError(e.to_string())
            })?;

        if result.deleted_count == 0 {
            return Err(RepositoryError::RepositoryNotFound(
                repository_id.as_str().to_string()
            ));
        }

        info!("Repository deleted successfully: {}", repository_id.as_str());
        Ok(())
    }

    async fn get_repository_for_deletion(&self, repository_id: &RepositoryId) -> RepositoryResult<Option<Repository>> {
        debug!("Getting repository for deletion: {}", repository_id.as_str());
        
        let filter = doc! { "_id": repository_id.as_str() };
        let doc = self.repositories_collection()
            .find_one(filter)
            .await
            .map_err(|e| {
                error!("Database error getting repository for deletion: {}", e);
                RepositoryError::DatabaseError(e.to_string())
            })?;

        match doc {
            Some(doc) => {
                let repository: Repository = doc.try_into()?;
                info!("Found repository for deletion: {}", repository.name);
                Ok(Some(repository))
            },
            None => {
                debug!("Repository not found for deletion: {}", repository_id.as_str());
                Ok(None)
            },
        }
    }

    async fn is_repository_empty(&self, repository_id: &RepositoryId) -> RepositoryResult<bool> {
        debug!("Checking if repository is empty: {}", repository_id.as_str());
        
        // TODO: Implementar verificación real de artefactos
        // Por ahora, asumimos que está vacío (placeholder para integración con artifact crate)
        debug!("Repository empty check passed (placeholder)");
        Ok(true)
    }
}

/// Adaptador de autorización para eliminación
pub struct RepositoryDeleteAuthorizationAdapter;

impl RepositoryDeleteAuthorizationAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RepositoryDeleteAuthorizationPort for RepositoryDeleteAuthorizationAdapter {
    async fn can_delete_repository(&self, _user_id: &shared::hrn::UserId, _repository_id: &RepositoryId) -> RepositoryResult<bool> {
        // TODO: Implementar verificación real de autorización con Cedar
        // Por ahora, permitir eliminación a todos los usuarios autenticados
        debug!("Authorization check passed for repository deletion");
        Ok(true)
    }
}

/// Adaptador de eliminación de artefactos
pub struct ArtifactDeleterAdapter;

impl ArtifactDeleterAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ArtifactDeleterPort for ArtifactDeleterAdapter {
    async fn delete_repository_artifacts(&self, repository_id: &RepositoryId) -> RepositoryResult<u64> {
        info!("Deleting artifacts from repository: {}", repository_id.as_str());
        
        // TODO: Implementar eliminación real de artefactos
        // Por ahora, simular eliminación exitosa
        debug!("Artifacts deleted successfully (placeholder)");
        Ok(0)
    }

    async fn count_repository_artifacts(&self, repository_id: &RepositoryId) -> RepositoryResult<u64> {
        debug!("Counting artifacts in repository: {}", repository_id.as_str());
        
        // TODO: Implementar conteo real de artefactos
        // Por ahora, asumimos que está vacío (placeholder para integración con artifact crate)
        debug!("Artifact count: 0 (placeholder)");
        Ok(0)
    }
}

/// Adaptador de publicación de eventos
pub struct RepositoryDeleteEventPublisherAdapter;

impl RepositoryDeleteEventPublisherAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RepositoryDeleteEventPublisherPort for RepositoryDeleteEventPublisherAdapter {
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