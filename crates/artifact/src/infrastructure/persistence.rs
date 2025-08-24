//! Implementación Mongo del `ArtifactRepository` (INFRA-T2).

use std::sync::Arc;
use async_trait::async_trait;
use infra_mongo::{MongoClientFactory, MongoInfraError};
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

pub struct MongoArtifactRepository {
    factory: Arc<MongoClientFactory>,
}

impl MongoArtifactRepository {
    pub fn new(factory: Arc<MongoClientFactory>) -> Self {
        Self { factory }
    }

    async fn collection(&self) -> Result<Collection<ArtifactDocument>, ArtifactError> {
        let db = self.factory.database().await.map_err(map_infra_err)?;
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
        coll.create_index(model).await.map_err(|e| ArtifactError::Repository(format!("create_index repo_checksum: {e}")))?;
        Ok(())
    }
}

fn is_duplicate_key(err: &MongoError) -> bool {
    matches!(err.kind.as_ref(), ErrorKind::Write(WriteFailure::WriteError(we)) if we.code == 11000)
}

fn map_infra_err(e: MongoInfraError) -> ArtifactError {
    ArtifactError::Repository(format!("infra mongo: {e}"))
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
}
