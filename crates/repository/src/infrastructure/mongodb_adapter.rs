// crates/repository/src/infrastructure/mongodb_adapter.rs

use async_trait::async_trait;
use mongodb::{Collection, Database, bson::{doc, Document, Bson}};
use mongodb::options::{FindOneOptions, UpdateOptions};
use tracing::{debug, error, info, warn, instrument};

use shared::hrn::{RepositoryId, OrganizationId, UserId};
use crate::domain::{RepositoryResult, RepositoryError};
use crate::domain::repository::{Repository, RepositoryType, Ecosystem, RepositoryConfig, HostedConfig, ProxyConfig, VirtualConfig, DeploymentPolicy, CacheSettings, ProxyAuth, ResolutionOrder};

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

    /// Convierte un Repository al formato de documento MongoDB
    fn repository_to_document(&self, repository: &Repository) -> RepositoryDocument {
        RepositoryDocument {
            id: repository.hrn.as_str().to_string(),
            hrn: repository.hrn.as_str().to_string(),
            organization_hrn: repository.organization_hrn.as_str().to_string(),
            name: repository.name.clone(),
            region: repository.region.clone(),
            repo_type: format!("{:?}", repository.repo_type),
            format: format!("{:?}", repository.format),
            config: self.config_to_document(&repository.config),
            storage_backend_hrn: repository.storage_backend_hrn.clone(),
            lifecycle: LifecycleDocument {
                created_at: repository.lifecycle.created_at,
                created_by: repository.lifecycle.created_by.as_str().to_string(),
                updated_at: repository.lifecycle.updated_at,
                updated_by: repository.lifecycle.updated_by.as_str().to_string(),
            },
            is_public: false, // TODO: Implementar cuando se añada al modelo
            metadata: None,   // TODO: Implementar cuando se añada al modelo
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
                    "remote_url": proxy.remote_url.as_str(),
                    "cache_settings": {
                        "metadata_ttl_seconds": proxy.cache_settings.metadata_ttl_seconds,
                        "artifact_ttl_seconds": proxy.cache_settings.artifact_ttl_seconds,
                    },
                };
                
                if let Some(auth) = &proxy.remote_authentication {
                    doc.insert("remote_authentication", doc! {
                        "username": &auth.username,
                        "password_secret_hrn": auth.password_secret_hrn.as_str(),
                    });
                }
                
                doc
            },
            RepositoryConfig::Virtual(virtual_config) => {
                let aggregated_repos: Vec<String> = virtual_config.aggregated_repositories
                    .iter()
                    .map(|repo_id| repo_id.as_str().to_string())
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
        let config = self.document_to_config(&doc.config, repo_type)?;

        Ok(Repository {
            hrn: RepositoryId(shared::hrn::Hrn::new(&doc.hrn).map_err(|e| {
                RepositoryError::InvalidRepositoryName(format!("Invalid HRN: {}", e))
            })?),
            organization_hrn: OrganizationId(shared::hrn::Hrn::new(&doc.organization_hrn).map_err(|e| {
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
                created_by: UserId(shared::hrn::Hrn::new(&doc.lifecycle.created_by).map_err(|e| {
                    RepositoryError::InvalidRepositoryName(format!("Invalid created_by HRN: {}", e))
                })?),
                updated_at: doc.lifecycle.updated_at,
                updated_by: UserId(shared::hrn::Hrn::new(&doc.lifecycle.updated_by).map_err(|e| {
                    RepositoryError::InvalidRepositoryName(format!("Invalid updated_by HRN: {}", e))
                })?),
            },
        })
    }

    /// Convierte un documento MongoDB a configuración de repositorio
    fn document_to_config(&self, doc: &Document, repo_type: RepositoryType) -> RepositoryResult<RepositoryConfig> {
        let config_type = doc.get_str("type").map_err(|e| {
            RepositoryError::InvalidConfiguration(format!("Missing config type: {}", e))
        })?;

        match (config_type, repo_type) {
            ("Hosted", RepositoryType::Hosted) => {
                let deployment_policy_str = doc.get_str("deployment_policy").map_err(|e| {
                    RepositoryError::InvalidConfiguration(format!("Missing deployment_policy: {}", e))
                })?;

                let deployment_policy = match deployment_policy_str {
                    "AllowSnapshots" => DeploymentPolicy::AllowSnapshots,
                    "BlockSnapshots" => DeploymentPolicy::BlockSnapshots,
                    "AllowRedeploy" => DeploymentPolicy::AllowRedeploy,
                    "BlockRedeploy" => DeploymentPolicy::BlockRedeploy,
                    _ => return Err(RepositoryError::InvalidConfiguration(
                        format!("Unknown deployment policy: {}", deployment_policy_str)
                    )),
                };

                Ok(RepositoryConfig::Hosted(HostedConfig {
                    deployment_policy,
                }))
            },
            ("Proxy", RepositoryType::Proxy) => {
                let remote_url_str = doc.get_str("remote_url").map_err(|e| {
                    RepositoryError::InvalidConfiguration(format!("Missing remote_url: {}", e))
                })?;
                
                let remote_url = url::Url::parse(remote_url_str).map_err(|e| {
                    RepositoryError::InvalidConfiguration(format!("Invalid remote URL: {}", e))
                })?;

                let cache_settings_doc = doc.get_document("cache_settings").map_err(|e| {
                    RepositoryError::InvalidConfiguration(format!("Missing cache_settings: {}", e))
                })?;

                let cache_settings = CacheSettings {
                    metadata_ttl_seconds: cache_settings_doc.get_i32("metadata_ttl_seconds").map_err(|e| {
                        RepositoryError::InvalidConfiguration(format!("Missing metadata_ttl_seconds: {}", e))
                    })? as u32,
                    artifact_ttl_seconds: cache_settings_doc.get_i32("artifact_ttl_seconds").map_err(|e| {
                        RepositoryError::InvalidConfiguration(format!("Missing artifact_ttl_seconds: {}", e))
                    })? as u32,
                };

                let mut remote_authentication = None;
                if let Ok(auth_doc) = doc.get_document("remote_authentication") {
                    let username = auth_doc.get_str("username").map_err(|e| {
                        RepositoryError::InvalidConfiguration(format!("Missing username in auth: {}", e))
                    })?.to_string();
                    
                    let password_secret_hrn_str = auth_doc.get_str("password_secret_hrn").map_err(|e| {
                        RepositoryError::InvalidConfiguration(format!("Missing password_secret_hrn in auth: {}", e))
                    })?;
                    
                    let password_secret_hrn = shared::hrn::Hrn::new(password_secret_hrn_str).map_err(|e| {
                        RepositoryError::InvalidConfiguration(format!("Invalid password_secret_hrn: {}", e))
                    })?;

                    remote_authentication = Some(ProxyAuth {
                        username,
                        password_secret_hrn,
                    });
                }

                Ok(RepositoryConfig::Proxy(ProxyConfig {
                    remote_url,
                    cache_settings,
                    remote_authentication,
                }))
            },
            ("Virtual", RepositoryType::Virtual) => {
                let aggregated_repos_array = doc.get_array("aggregated_repositories").map_err(|e| {
                    RepositoryError::InvalidConfiguration(format!("Missing aggregated_repositories: {}", e))
                })?;

                let mut aggregated_repositories = Vec::new();
                for repo_id_bson in aggregated_repos_array {
                    if let Bson::String(repo_id_str) = repo_id_bson {
                        let repo_id = RepositoryId(shared::hrn::Hrn::new(repo_id_str).map_err(|e| {
                            RepositoryError::InvalidConfiguration(format!("Invalid repository ID in aggregated list: {}", e))
                        })?);
                        aggregated_repositories.push(repo_id);
                    } else {
                        return Err(RepositoryError::InvalidConfiguration(
                            "Invalid repository ID format in aggregated list".to_string()
                        ));
                    }
                }

                let resolution_order_str = doc.get_str("resolution_order").map_err(|e| {
                    RepositoryError::InvalidConfiguration(format!("Missing resolution_order: {}", e))
                })?;

                let resolution_order = match resolution_order_str {
                    "FirstFound" => ResolutionOrder::FirstFound,
                    _ => return Err(RepositoryError::InvalidConfiguration(
                        format!("Unknown resolution order: {}", resolution_order_str)
                    )),
                };

                Ok(RepositoryConfig::Virtual(VirtualConfig {
                    aggregated_repositories,
                    resolution_order,
                }))
            },
            _ => Err(RepositoryError::InvalidConfiguration(
                format!("Config type '{}' incompatible with repository type '{:?}'", config_type, repo_type)
            )),
        }
    }
}

/// Operaciones CRUD implementadas
impl MongoDbRepositoryAdapter {
    /// Crea un nuevo repositorio
    #[instrument(skip(self, repository))]
    pub async fn create_repository(&self, repository: &Repository) -> RepositoryResult<()> {
        info!("Creating repository: {}", repository.hrn.as_str());
        
        let doc = self.repository_to_document(repository);
        
        // Verificar que no existe un repositorio con el mismo HRN
        let filter = doc! { "_id": repository.hrn.as_str() };
        let existing = self.repositories_collection()
            .find_one(filter)
            .await
            .map_err(|e| {
                error!("Database error checking existing repository: {}", e);
                RepositoryError::DatabaseError(e.to_string())
            })?;

        if existing.is_some() {
            return Err(RepositoryError::RepositoryAlreadyExists(
                repository.name.clone()
            ));
        }

        // Insertar el nuevo repositorio
        self.repositories_collection()
            .insert_one(&doc)
            .await
            .map_err(|e| {
                error!("Database error creating repository: {}", e);
                RepositoryError::DatabaseError(e.to_string())
            })?;

        info!("Repository created successfully: {}", repository.hrn.as_str());
        Ok(())
    }

    /// Obtiene un repositorio por su HRN
    #[instrument(skip(self, repository_id))]
    pub async fn get_repository(&self, repository_id: &RepositoryId) -> RepositoryResult<Option<Repository>> {
        debug!("Getting repository: {}", repository_id.as_str());
        
        let filter = doc! { "_id": repository_id.as_str() };
        let doc = self.repositories_collection()
            .find_one(filter)
            .await
            .map_err(|e| {
                error!("Database error getting repository: {}", e);
                RepositoryError::DatabaseError(e.to_string())
            })?;

        match doc {
            Some(doc) => {
                let repository = self.document_to_repository(doc)?;
                info!("Found repository: {}", repository.name);
                Ok(Some(repository))
            },
            None => {
                debug!("Repository not found: {}", repository_id.as_str());
                Ok(None)
            },
        }
    }

    /// Actualiza un repositorio existente
    #[instrument(skip(self, repository))]
    pub async fn update_repository(&self, repository: &Repository) -> RepositoryResult<()> {
        info!("Updating repository: {}", repository.hrn.as_str());
        
        let filter = doc! { "_id": repository.hrn.as_str() };
        let update_doc = self.repository_to_document(repository);
        
        let result = self.repositories_collection()
            .replace_one(filter, &update_doc)
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

    /// Elimina un repositorio
    #[instrument(skip(self, repository_id))]
    pub async fn delete_repository(&self, repository_id: &RepositoryId) -> RepositoryResult<()> {
        info!("Deleting repository: {}", repository_id.as_str());
        
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

    /// Lista todos los repositorios de una organización
    #[instrument(skip(self, organization_id))]
    pub async fn list_repositories(&self, organization_id: &OrganizationId) -> RepositoryResult<Vec<Repository>> {
        debug!("Listing repositories for organization: {}", organization_id.as_str());
        
        let filter = doc! { "organization_hrn": organization_id.as_str() };
        let mut cursor = self.repositories_collection()
            .find(filter)
            .await
            .map_err(|e| {
                error!("Database error listing repositories: {}", e);
                RepositoryError::DatabaseError(e.to_string())
            })?;

        let mut repositories = Vec::new();
        while let Some(doc) = cursor.try_next().await.map_err(|e| {
            error!("Database error iterating repositories: {}", e);
            RepositoryError::DatabaseError(e.to_string())
        })? {
            let repository = self.document_to_repository(doc)?;
            repositories.push(repository);
        }

        info!("Found {} repositories for organization", repositories.len());
        Ok(repositories)
    }

    /// Verifica si un repositorio está vacío (sin artefactos)
    #[instrument(skip(self, repository_id))]
    pub async fn is_repository_empty(&self, repository_id: &RepositoryId) -> RepositoryResult<bool> {
        debug!("Checking if repository is empty: {}", repository_id.as_str());
        
        // TODO: Implementar verificación real de artefactos cuando esté disponible el crate de artifact
        // Por ahora, asumimos que está vacío (placeholder para integración)
        debug!("Repository empty check passed (placeholder)");
        Ok(true)
    }

    /// Cuenta los artefactos en un repositorio
    #[instrument(skip(self, repository_id))]
    pub async fn count_repository_artifacts(&self, repository_id: &RepositoryId) -> RepositoryResult<u64> {
        debug!("Counting artifacts in repository: {}", repository_id.as_str());
        
        // TODO: Implementar conteo real de artefactos cuando esté disponible el crate de artifact
        // Por ahora, asumimos que está vacío (placeholder para integración)
        debug!("Artifact count: 0 (placeholder)");
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

#[cfg(test)]
mod tests {
    use super::*;
    use shared::hrn::{OrganizationId, RepositoryId, UserId};
    use shared::enums::Ecosystem;
    use crate::domain::repository::{RepositoryType, DeploymentPolicy, RepositoryConfig, HostedConfig};

    #[tokio::test]
    async fn test_repository_document_conversion() {
        // Crear un repositorio de prueba
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

        // Convertir a documento y de vuelta a repositorio
        let adapter = MongoDbRepositoryAdapter::new(mongodb::Database::default()); // Mock database
        let doc = adapter.repository_to_document(&repository);
        let converted_repository = adapter.document_to_repository(doc).unwrap();

        // Verificar que la conversión es correcta
        assert_eq!(converted_repository.hrn.as_str(), repository.hrn.as_str());
        assert_eq!(converted_repository.name, repository.name);
        assert_eq!(converted_repository.repo_type, repository.repo_type);
        assert_eq!(converted_repository.format, repository.format);
    }
}