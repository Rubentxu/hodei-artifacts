//! Implementación Mongo del `ArtifactRepository` (INFRA-T2).

use std::sync::Arc;
use async_trait::async_trait;
use infra_mongo::MongoClientFactory;
use mongodb::{
    bson::{doc, DateTime as BsonDateTime},
    error::{Error as MongoError, ErrorKind, WriteFailure},
    options::IndexOptions,
    Collection, IndexModel,
};
use crate::application::ports::ArtifactRepository;
use crate::domain::model::{Artifact, ArtifactChecksum, ArtifactVersion, PackageCoordinates};
use crate::error::ArtifactError;
use shared::{ArtifactId, RepositoryId, UserId, IsoTimestamp};
use futures_util::stream::StreamExt;
use repository::application::ports::RepositoryStore;
use repository::error::RepositoryError;
use repository::domain::model::{Repository, RepositoryName, RepositoryDescription};

#[async_trait]
impl RepositoryStore for MongoArtifactRepository {
    async fn save(&self, repo: &Repository) -> Result<(), RepositoryError> {
        let coll = self.factory.database().await.map_err(|e| RepositoryError::Persistence(format!("Mongo infra error: {}", e)))?.inner().collection::<RepositoryDocument>("repositories");
        let doc: RepositoryDocument = repo.into();
        if let Err(e) = coll.insert_one(doc).await {
            if is_duplicate_key(&e) {
                return Err(RepositoryError::DuplicateName);
            }
            return Err(RepositoryError::Persistence(format!("insert: {}", e)));
        }
        Ok(())
    }

    async fn get(&self, id: &RepositoryId) -> Result<Option<Repository>, RepositoryError> {
        let coll = self.factory.database().await.map_err(|e| RepositoryError::Persistence(format!("Mongo infra error: {}", e)))?.inner().collection::<RepositoryDocument>("repositories");
        let filter = doc! { "_id": id.0.to_string() };
        let found = coll
            .find_one(filter)
            .await
            .map_err(|e| RepositoryError::Persistence(format!("find_one: {}", e)))?;
        match found {
            Some(doc) => Ok(Some(doc.try_into()?)),
            None => Ok(None),
        }
    }

    async fn find_by_name(&self, name: &RepositoryName) -> Result<Option<Repository>, RepositoryError> {
        let coll = self.factory.database().await.map_err(|e| RepositoryError::Persistence(format!("Mongo infra error: {}", e)))?.inner().collection::<RepositoryDocument>("repositories");
        let filter = doc! { "name": name.0.to_string() };
        let found = coll
            .find_one(filter)
            .await
            .map_err(|e| RepositoryError::Persistence(format!("find_by_name: {}", e)))?;
        match found {
            Some(doc) => Ok(Some(doc.try_into()?)),
            None => Ok(None),
        }
    }

    async fn delete(&self, id: &RepositoryId) -> Result<(), RepositoryError> {
        let coll = self.factory.database().await.map_err(|e| RepositoryError::Persistence(format!("Mongo infra error: {}", e)))?.inner().collection::<RepositoryDocument>("repositories");
        let filter = doc! { "_id": id.0.to_string() };
        let result = coll.delete_one(filter).await
            .map_err(|e| RepositoryError::Persistence(format!("delete: {}", e)))?;

        if result.deleted_count == 0 {
            return Err(RepositoryError::NotFound);
        }
        Ok(())
    }
}

// Helper struct for MongoDB document mapping
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct RepositoryDocument {
    _id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    created_at: BsonDateTime,
    created_by: String,
}

impl From<&Repository> for RepositoryDocument {
    fn from(r: &Repository) -> Self {
        Self {
            _id: r.id.0.to_string(),
            name: r.name.0.clone(),
            description: r.description.as_ref().map(|d| d.0.clone()),
            created_at: BsonDateTime::from_millis(r.created_at.0.timestamp_millis()),
            created_by: r.created_by.0.to_string(),
        }
    }
}

impl TryFrom<RepositoryDocument> for Repository {
    type Error = RepositoryError;
    fn try_from(d: RepositoryDocument) -> Result<Self, Self::Error> {
        let id = RepositoryId(uuid::Uuid::parse_str(&d._id).map_err(|e| RepositoryError::Persistence(format!("Invalid UUID for id: {}", e)))?);
        let created_by = UserId(uuid::Uuid::parse_str(&d.created_by).map_err(|e| RepositoryError::Persistence(format!("Invalid UUID for created_by: {}", e)))?);
        Ok(Repository {
            id,
            name: RepositoryName(d.name),
            description: d.description.map(RepositoryDescription),
            created_at: IsoTimestamp(bson_date_to_chrono(d.created_at)),
            created_by,
        })
    }
}

const COLLECTION: &str = "artifacts";

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ArtifactDocument {
    _id: String,
    repository_id: String,
    version: String,
    file_name: String,
    size_bytes: i64,
    checksum: String,
    created_at: BsonDateTime,
    created_by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    coordinates: Option<PackageCoordinates>,
}

impl From<&Artifact> for ArtifactDocument {
    fn from(a: &Artifact) -> Self {
        Self {
            _id: a.id.0.to_string(),
            repository_id: a.repository_id.0.to_string(),
            version: a.version.0.clone(),
            file_name: a.file_name.clone(),
            size_bytes: a.size_bytes as i64,
            checksum: a.checksum.0.clone(),
            created_at: BsonDateTime::from_millis(a.created_at.0.timestamp_millis()),
            created_by: a.created_by.0.to_string(),
            coordinates: a.coordinates.clone(),
        }
    }
}

impl TryFrom<ArtifactDocument> for Artifact {
    type Error = ArtifactError;
    fn try_from(d: ArtifactDocument) -> Result<Self, Self::Error> {
        let id = ArtifactId(uuid::Uuid::parse_str(&d._id).map_err(|e| ArtifactError::Repository(format!("UUID inválido id: {e}")))?);
        let repository_id = RepositoryId(uuid::Uuid::parse_str(&d.repository_id).map_err(|e| ArtifactError::Repository(format!("UUID inválido repository_id: {e}")))?);
        let created_by = UserId(uuid::Uuid::parse_str(&d.created_by).map_err(|e| ArtifactError::Repository(format!("UUID inválido created_by: {e}")))?);
        Ok(Artifact {
            id,
            repository_id,
            version: ArtifactVersion(d.version),
            file_name: d.file_name,
            size_bytes: d.size_bytes as u64,
            checksum: ArtifactChecksum(d.checksum),
            created_at: IsoTimestamp(bson_date_to_chrono(d.created_at)),
            created_by,
            coordinates: d.coordinates,
        })
    }
}

fn bson_date_to_chrono(dt: BsonDateTime) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::<chrono::Utc>::from(dt.to_system_time())
}

#[derive(Clone)]
pub struct MongoArtifactRepository {
    factory: Arc<MongoClientFactory>,
}

impl MongoArtifactRepository {
    pub fn new(factory: Arc<MongoClientFactory>) -> Self {
        Self { factory }
    }

    async fn collection(&self) -> Result<Collection<ArtifactDocument>, ArtifactError> {
        let db = self.factory.database().await.map_err(|e| ArtifactError::Repository(format!("Mongo infra error: {}", e)))?;
        Ok(db.inner().collection::<ArtifactDocument>(COLLECTION))
    }

    pub async fn ensure_indexes(&self) -> Result<(), ArtifactError> {
        let coll = self.collection().await?;
        let idx_options = IndexOptions::builder()
            .unique(true)
            .name(Some("uq_repo_checksum".into()))
            .build();
        let model = IndexModel::builder()
            .keys(doc! { "repository_id": 1, "checksum": 1 })
            .options(idx_options)
            .build();
        coll.create_index(model).await.map_err(|e| ArtifactError::Repository(format!("create_index repo_checksum: {}", e)))?;
        Ok(())
    }
}

fn is_duplicate_key(err: &MongoError) -> bool {
    matches!(err.kind.as_ref(), ErrorKind::Write(WriteFailure::WriteError(we)) if we.code == 11000)
}



#[async_trait]
impl ArtifactRepository for MongoArtifactRepository {
    async fn save(&self, artifact: &Artifact) -> Result<(), ArtifactError> {
        let coll = self.collection().await?;
        let doc: ArtifactDocument = artifact.into();
        if let Err(e) = coll.insert_one(doc).await {
            if is_duplicate_key(&e) {
                return Err(ArtifactError::Duplicate);
            }
            return Err(ArtifactError::Repository(format!("insert: {e}")));
        }
        Ok(())
    }

    async fn get(&self, id: &ArtifactId) -> Result<Option<Artifact>, ArtifactError> {
        let coll = self.collection().await?;
        let filter = doc! { "_id": id.0.to_string() };
        let found = coll
            .find_one(filter)
            .await
            .map_err(|e| ArtifactError::Repository(format!("find_one: {e}")))?;
        match found {
            Some(doc) => Ok(Some(doc.try_into()?)),
            None => Ok(None),
        }
    }

    async fn find_by_repo_and_checksum(&self, repository_id: &RepositoryId, checksum: &ArtifactChecksum) -> Result<Option<Artifact>, ArtifactError> {
        let coll = self.collection().await?;
        let filter = doc! { "repository_id": repository_id.0.to_string(), "checksum": checksum.0.clone() };
        let found = coll
            .find_one(filter)
            .await
            .map_err(|e| ArtifactError::Repository(format!("find_by_repo_and_checksum: {e}")))?;
        match found {
            Some(doc) => Ok(Some(doc.try_into()?)),
            None => Ok(None),
        }
    }

    async fn find_by_maven_coordinates(&self, group_id: &str, artifact_id: &str, version: &str, file_name: &str) -> Result<Option<Artifact>, ArtifactError> {
        let coll = self.collection().await?;
        let canonical_name = format!("maven:{}:{}:{}", group_id, artifact_id, version);
        let filter = doc! {
            "coordinates.canonical": canonical_name,
            "file_name": file_name,
        };
        let found = coll
            .find_one(filter)
            .await
            .map_err(|e| ArtifactError::Repository(format!("find_by_maven_coordinates: {e}")))?;
        match found {
            Some(doc) => Ok(Some(doc.try_into()?)),
            None => Ok(None),
        }
    }

    async fn find_by_npm_package_name(&self, package_name: &str) -> Result<Vec<Artifact>, ArtifactError> {
        let coll = self.collection().await?;
        let filter = doc! {
            "coordinates.ecosystem": "npm",
            "coordinates.name": package_name,
        };
        let mut cursor = coll
            .find(filter)
            .await
            .map_err(|e| ArtifactError::Repository(format!("find_by_npm_package_name: {e}")))?;

        let mut results = Vec::new();
        while let Some(doc) = cursor.next().await {
            results.push(doc.map_err(|e| ArtifactError::Repository(format!("find_by_npm_package_name: {e}")))?);
        }
        Ok(results.into_iter().filter_map(|doc| doc.try_into().ok()).collect())
    }

    async fn find_all_artifacts(&self) -> Result<Vec<Artifact>, ArtifactError> {
        let coll = self.collection().await?;
        let filter = doc! {}; // Empty filter to find all documents
        let mut cursor = coll
            .find(filter)
            .await
            .map_err(|e| ArtifactError::Repository(format!("find_all_artifacts: {}", e)))?;

        let mut results = Vec::new();
        while let Some(doc) = cursor.next().await {
            results.push(doc.map_err(|e| ArtifactError::Repository(format!("find_all_artifacts: {}", e)))?);
        }
        Ok(results.into_iter().filter_map(|doc| doc.try_into().ok()).collect())
    }
}
