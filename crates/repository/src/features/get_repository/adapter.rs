// crates/repository/src/features/get_repository/adapter.rs

use async_trait::async_trait;
use mongodb::{Collection, Database};
use mongodb::bson::{doc, Document};
use tracing::{debug, error, info};

use shared::hrn::RepositoryId;
use crate::domain::{RepositoryResult, RepositoryError};
use crate::domain::repository::Repository;
use super::ports::{RepositoryReaderPort, RepositoryAuthorizationPort, RepositoryStatsPort, RepositoryStats};

/// Adaptador MongoDB para operaciones de lectura de repositorios
pub struct MongoDbRepositoryReaderAdapter {
    db: Database,
}

impl MongoDbRepositoryReaderAdapter {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    fn repositories_collection(&self) -> Collection<RepositoryDocument> {
        self.db.collection("repositories")
    }

    fn artifacts_collection(&self) -> Collection<Document> {
        self.db.collection("artifacts")
    }
}

/// Documento MongoDB para repositorios (reutilizado del adaptador de creación)
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
                created_by: doc.lifecycle.created_by,
                updated_at: doc.lifecycle.updated_at,
                updated_by: doc.lifecycle.updated_by,
            },
        })
    }
}

#[async_trait]
impl RepositoryReaderPort for MongoDbRepositoryReaderAdapter {
    async fn get_repository(&self, repository_id: &RepositoryId) -> RepositoryResult<Option<Repository>> {
        debug!("Getting repository with HRN: {}", repository_id.as_str());
        
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
                let repository: Repository = doc.try_into()?;
                info!("Found repository: {}", repository.name);
                Ok(Some(repository))
            },
            None => {
                debug!("Repository not found: {}", repository_id.as_str());
                Ok(None)
            },
        }
    }

    async fn get_repository_stats(&self, repository_id: &RepositoryId) -> RepositoryResult<RepositoryStats> {
        debug!("Getting stats for repository: {}", repository_id.as_str());
        
        // Contar artefactos en el repositorio
        let artifact_filter = doc! { "repository_hrn": repository_id.as_str() };
        let artifact_count = self.artifacts_collection()
            .count_documents(artifact_filter.clone())
            .await
            .map_err(|e| {
                error!("Database error counting artifacts: {}", e);
                RepositoryError::DatabaseError(e.to_string())
            })?;

        // Obtener tamaño total (placeholder - necesitaría modelo de artefacto completo)
        let total_size = 0u64; // TODO: Implementar cuando tengamos el modelo de artefacto

        // Obtener fecha del último artefacto
        let last_artifact_doc = self.artifacts_collection()
            .find_one(artifact_filter)
            .sort(doc! { "uploaded_at": -1 })
            .await
            .map_err(|e| {
                error!("Database error getting last artifact: {}", e);
                RepositoryError::DatabaseError(e.to_string())
            })?;

        let last_upload_date = last_artifact_doc
            .and_then(|doc| doc.get_datetime("uploaded_at").ok())
            .map(|dt| time::OffsetDateTime::from_unix_timestamp(dt.timestamp_millis() / 1000).unwrap_or_else(|_| time::OffsetDateTime::now_utc()));

        // Obtener total de descargas (placeholder)
        let total_downloads = 0u64; // TODO: Implementar cuando tengamos tracking de descargas

        let stats = RepositoryStats {
            artifact_count,
            total_size_bytes: total_size,
            last_artifact_uploaded_at: last_upload_date,
            total_downloads,
        };

        info!("Repository stats - artifacts: {}, size: {} bytes, downloads: {}", 
               stats.artifact_count, stats.total_size_bytes, stats.total_downloads);

        Ok(stats)
    }
}

/// Adaptador de autorización (placeholder para implementación real)
pub struct RepositoryAuthorizationAdapter;

impl RepositoryAuthorizationAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RepositoryAuthorizationPort for RepositoryAuthorizationAdapter {
    async fn can_read_repository(&self, _user_id: &shared::hrn::UserId, _repository_id: &RepositoryId) -> RepositoryResult<bool> {
        // TODO: Implementar verificación real de autorización con Cedar
        // Por ahora, permitir acceso a todos los usuarios autenticados
        debug!("Authorization check passed for repository access");
        Ok(true)
    }
}

/// Adaptador de estadísticas (reutiliza el adaptador de lectura)
pub struct RepositoryStatsAdapter {
    reader_adapter: Arc<dyn RepositoryReaderPort>,
}

impl RepositoryStatsAdapter {
    pub fn new(reader_adapter: Arc<dyn RepositoryReaderPort>) -> Self {
        Self { reader_adapter }
    }
}

#[async_trait]
impl RepositoryStatsPort for RepositoryStatsAdapter {
    async fn get_artifact_count(&self, repository_id: &RepositoryId) -> RepositoryResult<u64> {
        let stats = self.reader_adapter.get_repository_stats(repository_id).await?;
        Ok(stats.artifact_count)
    }

    async fn get_total_size(&self, repository_id: &RepositoryId) -> RepositoryResult<u64> {
        let stats = self.reader_adapter.get_repository_stats(repository_id).await?;
        Ok(stats.total_size_bytes)
    }

    async fn get_last_upload_date(&self, repository_id: &RepositoryId) -> RepositoryResult<Option<time::OffsetDateTime>> {
        let stats = self.reader_adapter.get_repository_stats(repository_id).await?;
        Ok(stats.last_artifact_uploaded_at)
    }

    async fn get_total_downloads(&self, repository_id: &RepositoryId) -> RepositoryResult<u64> {
        let stats = self.reader_adapter.get_repository_stats(repository_id).await?;
        Ok(stats.total_downloads)
    }
}