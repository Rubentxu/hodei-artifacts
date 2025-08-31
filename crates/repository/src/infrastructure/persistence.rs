//! Implementación Mongo del `RepositoryStore` (INFRA-T3).
//!
//! Responsabilidades:
//! - Persistir entidades `Repository` en colección `repositories`.
//! - Garantizar unicidad de `name` mediante índice único (`uq_repo_name`).
//! - Mapear errores de clave duplicada a `RepositoryError::DuplicateName` (REPO-T4 soporte 409).
//!
//! NOTA: Se usa el mismo `MongoClientFactory` compartido que otros bounded contexts para
//! reducir conexiones. El bootstrap de índices se realiza en `main` (BOOT-T1 extensión).
//!
//! Futuras extensiones:
//! - Búsqueda por nombre case-insensitive (crear índice collation).
//! - Atributos adicionales (visibility, tags, owners) versionando el documento.
//!
use std::sync::Arc;
use async_trait::async_trait;
use infra_mongo::{MongoClientFactory, MongoInfraError};
use mongodb::{
    bson::{doc, DateTime as BsonDateTime},
    error::{Error as MongoError, ErrorKind, WriteFailure},
    options::IndexOptions,
    Collection, IndexModel,
};
use crate::application::ports::RepositoryStore;
use crate::domain::model::{Repository, RepositoryDescription, RepositoryName};
use crate::error::RepositoryError;
use shared::{RepositoryId, UserId, IsoTimestamp};

const COLLECTION: &str = "repositories";

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct RepositoryDocument {
    _id: String,
    name: String,
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
        let id = RepositoryId(uuid::Uuid::parse_str(&d._id).map_err(|e| RepositoryError::Persistence(format!("UUID inválido id: {e}")))?);
        let created_by = UserId(uuid::Uuid::parse_str(&d.created_by).map_err(|e| RepositoryError::Persistence(format!("UUID inválido created_by: {e}")))?);
        Ok(Repository {
            id,
            name: RepositoryName(d.name),
            description: d.description.map(RepositoryDescription),
            created_at: IsoTimestamp(bson_date_to_chrono(d.created_at)),
            created_by,
        })
    }
}

fn bson_date_to_chrono(dt: BsonDateTime) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::<chrono::Utc>::from(dt.to_system_time())
}

/// Store Mongo para repositorios.
pub struct MongoRepositoryStore {
    factory: Arc<MongoClientFactory>,
}

impl MongoRepositoryStore {
    pub fn new(factory: Arc<MongoClientFactory>) -> Self {
        Self { factory }
    }

    async fn collection(&self) -> Result<Collection<RepositoryDocument>, RepositoryError> {
        let db = self.factory.database().await.map_err(map_infra_err)?;
        Ok(db.inner().collection::<RepositoryDocument>(COLLECTION))
    }

    /// Crea índices requeridos (único por `name`).
    pub async fn ensure_indexes(&self) -> Result<(), RepositoryError> {
        let coll = self.collection().await?;
        let idx_options = IndexOptions::builder()
            .unique(true)
            .name(Some("uq_repo_name".into()))
            .build();
        let model = IndexModel::builder()
            .keys(doc! { "name": 1 })
            .options(idx_options)
            .build();
        if let Err(e) = coll.create_index(model).await {
            return Err(RepositoryError::Persistence(format!("create_index name: {e}")));
        }
        Ok(())
    }
}

fn is_duplicate_key(err: &MongoError) -> bool {
    matches!(err.kind.as_ref(), ErrorKind::Write(WriteFailure::WriteError(we)) if we.code == 11000)
}

fn map_infra_err(e: MongoInfraError) -> RepositoryError {
    RepositoryError::Persistence(format!("infra mongo: {e}"))
}

#[async_trait]
impl RepositoryStore for MongoRepositoryStore {
    async fn save(&self, repo: &Repository) -> Result<(), RepositoryError> {
        let coll = self.collection().await?;
        let doc: RepositoryDocument = repo.into();
        if let Err(e) = coll.insert_one(doc).await {
            if is_duplicate_key(&e) {
                return Err(RepositoryError::DuplicateName);
            }
            return Err(RepositoryError::Persistence(format!("insert: {e}")));
        }
        Ok(())
    }

    async fn get(&self, id: &RepositoryId) -> Result<Option<Repository>, RepositoryError> {
        let coll = self.collection().await?;
        let filter = doc! { "_id": id.0.to_string() };
        let found = coll
            .find_one(filter)
            .await
            .map_err(|e| RepositoryError::Persistence(format!("find_one: {e}")))?;
        match found {
            Some(doc) => Ok(Some(doc.try_into()?)),
            None => Ok(None),
        }
    }

    async fn find_by_name(&self, name: &RepositoryName) -> Result<Option<Repository>, RepositoryError> {
        let coll = self.collection().await?;
        let filter = doc! { "name": name.0.clone() };
        let found = coll
            .find_one(filter)
            .await
            .map_err(|e| RepositoryError::Persistence(format!("find_by_name: {e}")))?;
        match found {
            Some(doc) => Ok(Some(doc.try_into()?)),
            None => Ok(None),
        }
    }

    async fn delete(&self, id: &RepositoryId) -> Result<(), RepositoryError> {
        let coll = self.collection().await?;
        let filter = doc! { "_id": id.0.to_string() };
        let result = coll.delete_one(filter).await
            .map_err(|e| RepositoryError::Persistence(format!("delete: {}", e)))?;

        if result.deleted_count == 0 {
            return Err(RepositoryError::NotFound);
        }
        Ok(())
    }
}
