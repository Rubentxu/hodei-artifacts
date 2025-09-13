
// crates/repository/src/infrastructure/mongodb_adapter.rs

use async_trait::async_trait;
use mongodb::{Collection, Database, bson::{doc, Document, Bson}};
use mongodb::options::{FindOneOptions, UpdateOptions};
use tracing::{debug, error, info, warn, instrument};

use shared::hrn::{RepositoryId, OrganizationId, UserId, Hrn};
use shared::enums::Ecosystem;
use crate::domain::{RepositoryResult, RepositoryError};
use crate::domain::repository::{Repository, RepositoryType, RepositoryConfig, HostedConfig, ProxyConfig, VirtualConfig, DeploymentPolicy, CacheSettings, ProxyAuth, ResolutionOrder};

// Import all ports from all features
use crate::features::create_repository::ports::{
    OrganizationExistsPort, RepositoryExistsPort, RepositoryCreatorPort, StorageBackendExistsPort,
};
use crate::features::get_repository::ports::{RepositoryReaderPort, RepositoryStats, RepositoryStatsPort};
use crate::features::update_repository::ports::RepositoryUpdaterPort;
use crate::features::delete_repository::ports::{RepositoryDeleterPort, ArtifactDeleterPort};


/// Adaptador MongoDB unificado para todas las operaciones CRUD de repositorios
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

    /// Convierte un Repository al formato de documento MongoDB
    fn repository_to_document(&self, repository: &Repository) -> RepositoryDocument {
        RepositoryDocument {
            id: repository.hrn.to_string(),
            hrn: repository.hrn.to_string(),
            organization_hrn: repository.organization_hrn.to_string(),
            name: repository.name.clone(),
            region: repository.region.clone(),
            repo_type: format!("{:?}", repository.repo_type),
            format: format!("{:?}", repository.format),
            config: self.config_to_document(&repository.config),
            storage_backend_hrn: repository.storage_backend_hrn.to_string(),
            lifecycle: LifecycleDocument {
                created_at: repository.lifecycle.created_at,
                created_by: repository.lifecycle.created_by.to_string(),
                updated_at: repository.lifecycle.updated_at,
                updated_by: repository.lifecycle.updated_by.to_string(),
            },
        }
    }

    /// Convierte la configuración del repositorio a documento MongoDB
    fn config_to_document(&self, config: &RepositoryConfig) -> Document {
        match config {
            RepositoryConfig::Hosted(hosted) => {
                doc! {
                    "type": "Hosted",
                    "deployment_policy": format!("{:?}", hosted.deployment_policy),
                }
            },
            RepositoryConfig::Proxy(proxy) => {
                let mut doc = doc! {
                    "type": "Proxy",
                    "remote_url": proxy.remote_url.to_string(),
                    "cache_settings": {
                        "metadata_ttl_seconds": proxy.cache_settings.metadata_ttl_seconds,
                        "artifact_ttl_seconds": proxy.cache_settings.artifact_ttl_seconds,
                    },
                };
                
                if let Some(auth) = &proxy.remote_authentication {
                    doc.insert("remote_authentication", doc! {
                        "username": &auth.username,
                        "password_secret_hrn": auth.password_secret_hrn.to_string(),
                    });
                }
                
                doc
            },
            RepositoryConfig::Virtual(virtual_config) => {
                let aggregated_repos: Vec<String> = virtual_config.aggregated_repositories
                    .iter()
                    .map(|repo_id| repo_id.to_string())
                    .collect();
                
                doc! {
                    "type": "Virtual",
                    "aggregated_repositories": aggregated_repos,
                    "resolution_order": format!("{:?}", virtual_config.resolution_order),
                }
            },
        }
    }

    /// Convierte un documento MongoDB a Repository
    fn document_to_repository(&self, doc: RepositoryDocument) -> RepositoryResult<Repository> {
        let repo_type = match doc.repo_type.as_str() {
            "Hosted" => RepositoryType::Hosted,
            "Proxy" => RepositoryType::Proxy,
            "Virtual" => RepositoryType::Virtual,
            _ => return Err(RepositoryError::InvalidConfiguration(format!("Unknown repository type: {}", doc.repo_type))),
        };

        let format = doc.format.parse::<Ecosystem>().map_err(|_| RepositoryError::InvalidConfiguration(format!("Unknown ecosystem: {}", doc.format)))?;

        let config = self.document_to_config(&doc.config, repo_type)?;

        Ok(Repository {
            hrn: doc.hrn.parse()?,
            organization_hrn: doc.organization_hrn.parse()?,
            name: doc.name,
            region: doc.region,
            repo_type,
            format,
            config,
            storage_backend_hrn: doc.storage_backend_hrn.parse()?,
            lifecycle: shared::lifecycle::Lifecycle {
                created_at: doc.lifecycle.created_at,
                created_by: doc.lifecycle.created_by.parse()?,
                updated_at: doc.lifecycle.updated_at,
                updated_by: doc.lifecycle.updated_by.parse()?,
                state: shared::lifecycle::LifecycleState::Active, // Assuming default state
            },
        })
    }

    /// Convierte un documento MongoDB a configuración de repositorio
    fn document_to_config(&self, doc: &Document, repo_type: RepositoryType) -> RepositoryResult<RepositoryConfig> {
        let config_type = doc.get_str("type")?;

        match (config_type, repo_type) {
            ("Hosted", RepositoryType::Hosted) => {
                let deployment_policy_str = doc.get_str("deployment_policy")?;
                let deployment_policy = deployment_policy_str.parse::<DeploymentPolicy>().map_err(|_| RepositoryError::InvalidConfiguration(format!("Unknown deployment policy: {}", deployment_policy_str)))?;
                Ok(RepositoryConfig::Hosted(HostedConfig { deployment_policy }))
            },
            ("Proxy", RepositoryType::Proxy) => {
                let remote_url = doc.get_str("remote_url")?.parse()?;
                let cache_settings_doc = doc.get_document("cache_settings")?;
                let cache_settings = CacheSettings {
                    metadata_ttl_seconds: cache_settings_doc.get_i32("metadata_ttl_seconds")? as u32,
                    artifact_ttl_seconds: cache_settings_doc.get_i32("artifact_ttl_seconds")? as u32,
                };
                let remote_authentication = if let Ok(auth_doc) = doc.get_document("remote_authentication") {
                    Some(ProxyAuth {
                        username: auth_doc.get_str("username")?.to_string(),
                        password_secret_hrn: auth_doc.get_str("password_secret_hrn")?.parse()?,
                    })
                } else {
                    None
                };
                Ok(RepositoryConfig::Proxy(ProxyConfig { remote_url, cache_settings, remote_authentication }))
            },
            ("Virtual", RepositoryType::Virtual) => {
                let aggregated_repositories = doc.get_array("aggregated_repositories")?
                    .iter()
                    .map(|bson| bson.as_str().ok_or(RepositoryError::InvalidConfiguration("Invalid repository ID format in aggregated list".to_string()))?.parse())
                    .collect::<Result<Vec<_>, _>>()?;
                let resolution_order_str = doc.get_str("resolution_order")?;
                let resolution_order = resolution_order_str.parse::<ResolutionOrder>().map_err(|_| RepositoryError::InvalidConfiguration(format!("Unknown resolution order: {}", resolution_order_str)))?;
                Ok(RepositoryConfig::Virtual(VirtualConfig { aggregated_repositories, resolution_order }))
            },
            _ => Err(RepositoryError::InvalidConfiguration(format!("Config type '{}' incompatible with repository type '{:?}'", config_type, repo_type))),
        }
    }
}

#[async_trait]
impl OrganizationExistsPort for MongoDbRepositoryAdapter {
    async fn organization_exists(&self, organization_id: &OrganizationId) -> RepositoryResult<bool> {
        debug!("Checking if organization exists: {}", organization_id);
        let filter = doc! { "_id": organization_id.to_string() };
        let count = self.organizations_collection().count_documents(filter, None).await?;
        Ok(count > 0)
    }
}

#[async_trait]
impl RepositoryExistsPort for MongoDbRepositoryAdapter {
    async fn repository_exists(&self, organization_id: &OrganizationId, name: &str) -> RepositoryResult<bool> {
        debug!("Checking if repository '{}' exists in organization '{}'", name, organization_id);
        let filter = doc! { "organization_hrn": organization_id.to_string(), "name": name };
        let count = self.repositories_collection().count_documents(filter, None).await?;
        Ok(count > 0)
    }
}

#[async_trait]
impl RepositoryCreatorPort for MongoDbRepositoryAdapter {
    async fn create_repository(&self, repository: &Repository) -> RepositoryResult<()> {
        info!("Creating repository: {}", repository.hrn);
        let doc = self.repository_to_document(repository);
        self.repositories_collection().insert_one(doc, None).await?;
        info!("Repository created successfully: {}", repository.hrn);
        Ok(())
    }
}

#[async_trait]
impl StorageBackendExistsPort for MongoDbRepositoryAdapter {
    async fn storage_backend_exists(&self, storage_backend_hrn: &str) -> RepositoryResult<bool> {
        debug!("Checking if storage backend exists: {}", storage_backend_hrn);
        let filter = doc! { "_id": storage_backend_hrn.to_string() };
        let count = self.storage_backends_collection().count_documents(filter).await?;
        Ok(count > 0)
    }
}

#[async_trait]
impl RepositoryReaderPort for MongoDbRepositoryAdapter {
    async fn get_repository(&self, repository_id: &RepositoryId) -> RepositoryResult<Option<Repository>> {
        debug!("Getting repository: {}", repository_id);
        let filter = doc! { "_id": repository_id.to_string() };
        if let Some(doc) = self.repositories_collection().find_one(filter, None).await? {
            let repository = self.document_to_repository(doc)?;
            info!("Found repository: {}", repository.name);
            Ok(Some(repository))
        } else {
            debug!("Repository not found: {}", repository_id);
            Ok(None)
        }
    }
    
    async fn get_repository_stats(&self, repository_id: &RepositoryId) -> RepositoryResult<RepositoryStats> {
        // Placeholder implementation
        debug!("Getting stats for repository: {}", repository_id);
        Ok(RepositoryStats {
            artifact_count: 0,
            total_size_bytes: 0,
            last_artifact_uploaded_at: None,
            total_downloads: 0,
        })
    }
}

#[async_trait]
impl RepositoryUpdaterPort for MongoDbRepositoryAdapter {
    async fn update_repository(&self, repository: &Repository) -> RepositoryResult<()> {
        info!("Updating repository: {}", repository.hrn);
        let filter = doc! { "_id": repository.hrn.to_string() };
        let update_doc = self.repository_to_document(repository);
        let result = self.repositories_collection().replace_one(filter, update_doc, None).await?;
        if result.matched_count == 0 {
            Err(RepositoryError::RepositoryNotFound(repository.hrn.to_string()))
        } else {
            info!("Repository updated successfully: {}", repository.hrn);
            Ok(())
        }
    }
    
    async fn get_repository_for_update(&self, repository_id: &RepositoryId) -> RepositoryResult<Option<Repository>> {
        self.get_repository(repository_id).await
    }
}

#[async_trait]
impl RepositoryDeleterPort for MongoDbRepositoryAdapter {
    async fn delete_repository(&self, repository_id: &RepositoryId) -> RepositoryResult<()> {
        info!("Deleting repository: {}", repository_id);
        let filter = doc! { "_id": repository_id.to_string() };
        let result = self.repositories_collection().delete_one(filter, None).await?;
        if result.deleted_count == 0 {
            Err(RepositoryError::RepositoryNotFound(repository_id.to_string()))
        } else {
            info!("Repository deleted successfully: {}", repository_id);
            Ok(())
        }
    }
    
    async fn get_repository_for_deletion(&self, repository_id: &RepositoryId) -> RepositoryResult<Option<Repository>> {
        self.get_repository(repository_id).await
    }
    
    async fn is_repository_empty(&self, repository_id: &RepositoryId) -> RepositoryResult<bool> {
        // Placeholder
        debug!("Checking if repository is empty: {}", repository_id);
        Ok(true)
    }
}

#[async_trait]
impl ArtifactDeleterPort for MongoDbRepositoryAdapter {
    async fn delete_repository_artifacts(&self, repository_id: &RepositoryId) -> RepositoryResult<u64> {
        // Placeholder
        warn!("Artifact deletion not implemented yet for repo {}", repository_id);
        Ok(0)
    }
    
    async fn count_repository_artifacts(&self, repository_id: &RepositoryId) -> RepositoryResult<u64> {
        // Placeholder
        debug!("Counting artifacts for repo {}: 0", repository_id);
        Ok(0)
    }
}


/// Documento MongoDB para repositorios
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
    pub config: Document,
    pub storage_backend_hrn: String,
    pub lifecycle: LifecycleDocument,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct LifecycleDocument {
    pub created_at: time::OffsetDateTime,
    pub created_by: String,
    pub updated_at: time::OffsetDateTime,
    pub updated_by: String,
}
