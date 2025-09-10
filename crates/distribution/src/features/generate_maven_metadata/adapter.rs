// crates/distribution/src/features/generate_maven_metadata/adapter.rs

//! Adaptadores de infraestructura para generar Maven metadata

use std::sync::Arc;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{info, warn, error, instrument};
use crate::domain::maven::{MavenCoordinates, MavenMetadata, MavenVersion};
use super::ports::{
    MavenMetadataGenerator, MavenArtifactLister, MavenMetadataCache,
    MavenMetadataGeneratorError, MavenArtifactListerError, MavenMetadataCacheError,
};
use super::dto::MavenMetadataDto;

/// Adaptador S3 para generar Maven metadata
pub struct S3MavenMetadataGenerator {
    s3_client: Arc<dyn S3Client>,
    bucket_name: String,
}

impl S3MavenMetadataGenerator {
    pub fn new(s3_client: Arc<dyn S3Client>, bucket_name: String) -> Self {
        Self { s3_client, bucket_name }
    }
}

#[async_trait]
impl MavenMetadataGenerator for S3MavenMetadataGenerator {
    #[instrument(
        name = "s3_generate_maven_metadata",
        skip(self, coordinates),
        fields(
            bucket = %self.bucket_name,
            group_id = %coordinates.group_id(),
            artifact_id = %coordinates.artifact_id()
        )
    )]
    async fn generate_metadata(
        &self,
        coordinates: &MavenCoordinates,
        versions: Vec<MavenVersion>,
    ) -> Result<MavenMetadataDto, MavenMetadataGeneratorError> {
        info!(
            bucket = %self.bucket_name,
            group_id = %coordinates.group_id(),
            artifact_id = %coordinates.artifact_id(),
            versions_count = versions.len(),
            "Generating Maven metadata from S3"
        );
        
        // Obtener la última versión
        let latest_version = versions.iter()
            .max()
            .ok_or_else(|| MavenMetadataGeneratorError::NoVersionsAvailable {
                coordinates: coordinates.to_string(),
            })?;
        
        // Obtener la versión de release (última versión estable)
        let release_version = versions.iter()
            .filter(|v| !v.to_string().contains("-SNAPSHOT"))
            .max()
            .unwrap_or(latest_version);
        
        let metadata = MavenMetadataDto {
            group_id: coordinates.group_id().to_string(),
            artifact_id: coordinates.artifact_id().to_string(),
            versions: versions.iter().map(|v| v.to_string()).collect(),
            latest_version: latest_version.to_string(),
            release_version: release_version.to_string(),
            last_updated: Utc::now(),
            from_cache: false,
        };
        
        info!(
            bucket = %self.bucket_name,
            group_id = %coordinates.group_id(),
            artifact_id = %coordinates.artifact_id(),
            latest_version = %metadata.latest_version,
            release_version = %metadata.release_version,
            versions_count = metadata.versions.len(),
            "Successfully generated Maven metadata from S3"
        );
        
        Ok(metadata)
    }
    
    #[instrument(
        name = "s3_generate_maven_metadata_xml",
        skip(self, metadata),
        fields(
            bucket = %self.bucket_name,
            group_id = %metadata.group_id,
            artifact_id = %metadata.artifact_id
        )
    )]
    async fn generate_metadata_xml(&self, metadata: &MavenMetadataDto) -> Result<String, MavenMetadataGeneratorError> {
        info!(
            bucket = %self.bucket_name,
            group_id = %metadata.group_id,
            artifact_id = %metadata.artifact_id,
            "Generating Maven metadata XML from S3"
        );
        
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<metadata>
  <groupId>{}</groupId>
  <artifactId>{}</artifactId>
  <versioning>
    <latest>{}</latest>
    <release>{}</release>
    <versions>
{}
    </versions>
    <lastUpdated>{}</lastUpdated>
  </versioning>
</metadata>"#,
            metadata.group_id,
            metadata.artifact_id,
            metadata.latest_version,
            metadata.release_version,
            metadata.versions.iter()
                .map(|v| format!("      <version>{}</version>", v))
                .collect::<Vec<_>>()
                .join("\n"),
            metadata.last_updated.format("%Y%m%d%H%M%S")
        );
        
        info!(
            bucket = %self.bucket_name,
            group_id = %metadata.group_id,
            artifact_id = %metadata.artifact_id,
            xml_length = xml.len(),
            "Successfully generated Maven metadata XML from S3"
        );
        
        Ok(xml)
    }
}

/// Adaptador MongoDB para listar artefactos Maven
pub struct MongoMavenArtifactLister {
    mongo_client: Arc<dyn MongoClient>,
    database_name: String,
}

impl MongoMavenArtifactLister {
    pub fn new(mongo_client: Arc<dyn MongoClient>, database_name: String) -> Self {
        Self { mongo_client, database_name }
    }
}

#[async_trait]
impl MavenArtifactLister for MongoMavenArtifactLister {
    #[instrument(
        name = "mongo_list_maven_artifacts",
        skip(self, coordinates),
        fields(
            database = %self.database_name,
            group_id = %coordinates.group_id(),
            artifact_id = %coordinates.artifact_id()
        )
    )]
    async fn list_versions(
        &self,
        coordinates: &MavenCoordinates,
        repository_id: &str,
    ) -> Result<Vec<MavenVersion>, MavenArtifactListerError> {
        info!(
            database = %self.database_name,
            group_id = %coordinates.group_id(),
            artifact_id = %coordinates.artifact_id(),
            repository_id = %repository_id,
            "Listing Maven artifact versions from MongoDB"
        );
        
        // Construir el filtro para buscar artefactos
        let filter = bson::doc! {
            "repository_id": repository_id,
            "group_id": coordinates.group_id(),
            "artifact_id": coordinates.artifact_id(),
            "format": "maven"
        };
        
        // Buscar en la colección de artefactos
        let collection = self.mongo_client
            .database(&self.database_name)
            .collection::<ArtifactDocument>("artifacts");
        
        let mut cursor = collection
            .find(filter)
            .await
            .map_err(|e| MavenArtifactListerError::DatabaseError {
                message: format!("Failed to query artifacts: {}", e),
            })?;
        
        let mut versions = Vec::new();
        while let Some(doc) = cursor.next().await {
            match doc {
                Ok(artifact) => {
                    if let Ok(version) = MavenVersion::new(&artifact.version) {
                        versions.push(version);
                    }
                }
                Err(e) => {
                    warn!(
                        database = %self.database_name,
                        group_id = %coordinates.group_id(),
                        artifact_id = %coordinates.artifact_id(),
                        error = %e,
                        "Error reading artifact document"
                    );
                }
            }
        }
        
        if versions.is_empty() {
            warn!(
                database = %self.database_name,
                group_id = %coordinates.group_id(),
                artifact_id = %coordinates.artifact_id(),
                repository_id = %repository_id,
                "No versions found for Maven artifact"
            );
            return Err(MavenArtifactListerError::ArtifactNotFound {
                coordinates: format!("{}:{}", coordinates.group_id(), coordinates.artifact_id()),
                repository_id: repository_id.to_string(),
            });
        }
        
        info!(
            database = %self.database_name,
            group_id = %coordinates.group_id(),
            artifact_id = %coordinates.artifact_id(),
            repository_id = %repository_id,
            versions_count = versions.len(),
            "Successfully listed Maven artifact versions from MongoDB"
        );
        
        Ok(versions)
    }
}

/// Adaptador Redis para caché de metadata Maven
pub struct RedisMavenMetadataCache {
    redis_client: Arc<dyn RedisClient>,
    ttl_seconds: u64,
}

impl RedisMavenMetadataCache {
    pub fn new(redis_client: Arc<dyn RedisClient>, ttl_seconds: u64) -> Self {
        Self { redis_client, ttl_seconds }
    }
    
    fn cache_key(coordinates: &MavenCoordinates, repository_id: &str) -> String {
        format!("maven:metadata:{}:{}:{}", repository_id, coordinates.group_id(), coordinates.artifact_id())
    }
}

#[async_trait]
impl MavenMetadataCache for RedisMavenMetadataCache {
    #[instrument(
        name = "redis_get_cached_maven_metadata",
        skip(self, coordinates),
        fields(
            repository_id = %repository_id,
            group_id = %coordinates.group_id(),
            artifact_id = %coordinates.artifact_id()
        )
    )]
    async fn get_cached_metadata(
        &self,
        coordinates: &MavenCoordinates,
        repository_id: &str,
    ) -> Result<Option<MavenMetadataDto>, MavenMetadataCacheError> {
        let key = Self::cache_key(coordinates, repository_id);
        
        info!(
            key = %key,
            repository_id = %repository_id,
            group_id = %coordinates.group_id(),
            artifact_id = %coordinates.artifact_id(),
            "Getting cached Maven metadata from Redis"
        );
        
        match self.redis_client.get(&key).await {
            Ok(Some(data)) => {
                match serde_json::from_str::<CachedMetadata>(&data) {
                    Ok(cached) => {
                        info!(
                            key = %key,
                            repository_id = %repository_id,
                            group_id = %coordinates.group_id(),
                            artifact_id = %coordinates.artifact_id(),
                            cached_at = %cached.cached_at,
                            "Successfully retrieved cached Maven metadata from Redis"
                        );
                        
                        Ok(Some(cached.metadata))
                    }
                    Err(e) => {
                        warn!(
                            key = %key,
                            repository_id = %repository_id,
                            group_id = %coordinates.group_id(),
                            artifact_id = %coordinates.artifact_id(),
                            error = %e,
                            "Failed to deserialize cached Maven metadata"
                        );
                        Ok(None)
                    }
                }
            }
            Ok(None) => {
                info!(
                    key = %key,
                    repository_id = %repository_id,
                    group_id = %coordinates.group_id(),
                    artifact_id = %coordinates.artifact_id(),
                    "No cached Maven metadata found in Redis"
                );
                Ok(None)
            }
            Err(e) => {
                error!(
                    key = %key,
                    repository_id = %repository_id,
                    group_id = %coordinates.group_id(),
                    artifact_id = %coordinates.artifact_id(),
                    error = %e,
                    "Error getting cached Maven metadata from Redis"
                );
                Err(MavenMetadataCacheError::CacheError {
                    message: format!("Redis error: {}", e),
                })
            }
        }
    }
    
    #[instrument(
        name = "redis_cache_maven_metadata",
        skip(self, coordinates, metadata),
        fields(
            repository_id = %repository_id,
            group_id = %coordinates.group_id(),
            artifact_id = %coordinates.artifact_id()
        )
    )]
    async fn cache_metadata(
        &self,
        coordinates: &MavenCoordinates,
        repository_id: &str,
        metadata: &MavenMetadataDto,
    ) -> Result<(), MavenMetadataCacheError> {
        let key = Self::cache_key(coordinates, repository_id);
        
        info!(
            key = %key,
            repository_id = %repository_id,
            group_id = %coordinates.group_id(),
            artifact_id = %coordinates.artifact_id(),
            ttl_seconds = self.ttl_seconds,
            "Caching Maven metadata in Redis"
        );
        
        let cached = CachedMetadata {
            metadata: metadata.clone(),
            cached_at: Utc::now(),
        };
        
        let data = serde_json::to_string(&cached)
            .map_err(|e| MavenMetadataCacheError::CacheError {
                message: format!("Serialization error: {}", e),
            })?;
        
        self.redis_client
            .setex(&key, self.ttl_seconds, &data)
            .await
            .map_err(|e| MavenMetadataCacheError::CacheError {
                message: format!("Redis error: {}", e),
            })?;
        
        info!(
            key = %key,
            repository_id = %repository_id,
            group_id = %coordinates.group_id(),
            artifact_id = %coordinates.artifact_id(),
            ttl_seconds = self.ttl_seconds,
            "Successfully cached Maven metadata in Redis"
        );
        
        Ok(())
    }
    
    #[instrument(
        name = "redis_invalidate_maven_metadata_cache",
        skip(self, coordinates),
        fields(
            repository_id = %repository_id,
            group_id = %coordinates.group_id(),
            artifact_id = %coordinates.artifact_id()
        )
    )]
    async fn invalidate_cache(
        &self,
        coordinates: &MavenCoordinates,
        repository_id: &str,
    ) -> Result<(), MavenMetadataCacheError> {
        let key = Self::cache_key(coordinates, repository_id);
        
        info!(
            key = %key,
            repository_id = %repository_id,
            group_id = %coordinates.group_id(),
            artifact_id = %coordinates.artifact_id(),
            "Invalidating cached Maven metadata in Redis"
        );
        
        self.redis_client
            .del(&key)
            .await
            .map_err(|e| MavenMetadataCacheError::CacheError {
                message: format!("Redis error: {}", e),
            })?;
        
        info!(
            key = %key,
            repository_id = %repository_id,
            group_id = %coordinates.group_id(),
            artifact_id = %coordinates.artifact_id(),
            "Successfully invalidated cached Maven metadata in Redis"
        );
        
        Ok(())
    }
    
    #[instrument(
        name = "redis_invalidate_maven_repository_cache",
        skip(self),
        fields(repository_id = %repository_id)
    )]
    async fn invalidate_repository_cache(&self, repository_id: &str) -> Result<(), MavenMetadataCacheError> {
        let pattern = format!("maven:metadata:{}:*", repository_id);
        
        info!(
            pattern = %pattern,
            repository_id = %repository_id,
            "Invalidating cached Maven metadata for repository in Redis"
        );
        
        let keys = self.redis_client
            .keys(&pattern)
            .await
            .map_err(|e| MavenMetadataCacheError::CacheError {
                message: format!("Redis error: {}", e),
            })?;
        
        if !keys.is_empty() {
            self.redis_client
                .del_multiple(&keys)
                .await
                .map_err(|e| MavenMetadataCacheError::CacheError {
                    message: format!("Redis error: {}", e),
                })?;
        }
        
        info!(
            pattern = %pattern,
            repository_id = %repository_id,
            keys_deleted = keys.len(),
            "Successfully invalidated cached Maven metadata for repository in Redis"
        );
        
        Ok(())
    }
}

/// Documento de artefacto en MongoDB
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArtifactDocument {
    #[serde(rename = "_id")]
    pub id: String,
    pub repository_id: String,
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
    pub format: String,
    pub path: String,
    pub size: u64,
    pub checksum: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Metadata en caché
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedMetadata {
    pub metadata: MavenMetadataDto,
    pub cached_at: DateTime<Utc>,
}

/// Cliente S3 trait para testing
#[async_trait]
pub trait S3Client: Send + Sync {
    async fn get_object(&self, bucket: &str, key: &str) -> Result<Option<Vec<u8>>, String>;
    async fn put_object(&self, bucket: &str, key: &str, data: &[u8]) -> Result<(), String>;
    async fn delete_object(&self, bucket: &str, key: &str) -> Result<(), String>;
    async fn list_objects(&self, bucket: &str, prefix: &str) -> Result<Vec<String>, String>;
}

/// Cliente MongoDB trait para testing
#[async_trait]
pub trait MongoClient: Send + Sync {
    fn database(&self, name: &str) -> Arc<dyn MongoDatabase>;
}

/// Base de datos MongoDB trait para testing
#[async_trait]
pub trait MongoDatabase: Send + Sync {
    fn collection<T>(&self, name: &str) -> Arc<dyn MongoCollection<T>>;
}

/// Colección MongoDB trait para testing
#[async_trait]
pub trait MongoCollection<T>: Send + Sync {
    async fn find(&self, filter: bson::Document) -> Result<MockCursor<T>, String>;
}

/// Cursor MongoDB mock para testing
pub struct MockCursor<T> {
    items: Vec<T>,
    index: usize,
}

impl<T> MockCursor<T> {
    pub fn new(items: Vec<T>) -> Self {
        Self { items, index: 0 }
    }
    
    pub async fn next(&mut self) -> Option<Result<T, String>> {
        if self.index < self.items.len() {
            let item = self.items[self.index].clone();
            self.index += 1;
            Some(Ok(item))
        } else {
            None
        }
    }
}

/// Cliente Redis trait para testing
#[async_trait]
pub trait RedisClient: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<String>, String>;
    async fn setex(&self, key: &str, seconds: u64, value: &str) -> Result<(), String>;
    async fn del(&self, key: &str) -> Result<(), String>;
    async fn keys(&self, pattern: &str) -> Result<Vec<String>, String>;
    async fn del_multiple(&self, keys: &[String]) -> Result<(), String>;
}

#[cfg(test)]
pub mod test {
    use super::*;
    use std::sync::Mutex;
    use std::collections::HashMap;
    
    /// Mock S3Client para testing
    pub struct MockS3Client {
        objects: Mutex<HashMap<String, Vec<u8>>>,
    }
    
    impl MockS3Client {
        pub fn new() -> Self {
            Self {
                objects: Mutex::new(HashMap::new()),
            }
        }
        
        pub fn add_object(&self, bucket: &str, key: &str, data: Vec<u8>) {
            let full_key = format!("{}:{}", bucket, key);
            self.objects.lock().unwrap().insert(full_key, data);
        }
    }
    
    #[async_trait]
    impl S3Client for MockS3Client {
        async fn get_object(&self, bucket: &str, key: &str) -> Result<Option<Vec<u8>>, String> {
            let full_key = format!("{}:{}", bucket, key);
            Ok(self.objects.lock().unwrap().get(&full_key).cloned())
        }
        
        async fn put_object(&self, bucket: &str, key: &str, data: &[u8]) -> Result<(), String> {
            let full_key = format!("{}:{}", bucket, key);
            self.objects.lock().unwrap().insert(full_key, data.to_vec());
            Ok(())
        }
        
        async fn delete_object(&self, bucket: &str, key: &str) -> Result<(), String> {
            let full_key = format!("{}:{}", bucket, key);
            self.objects.lock().unwrap().remove(&full_key);
            Ok(())
        }
        
        async fn list_objects(&self, bucket: &str, prefix: &str) -> Result<Vec<String>, String> {
            let prefix_str = format!("{}:{}", bucket, prefix);
            let objects = self.objects.lock().unwrap();
            let keys: Vec<String> = objects.keys()
                .filter(|k| k.starts_with(&prefix_str))
                .cloned()
                .collect();
            Ok(keys)
        }
    }
    
    /// Mock MongoClient para testing
    pub struct MockMongoClient {
        databases: Mutex<HashMap<String, Arc<MockMongoDatabase>>>,
    }
    
    impl MockMongoClient {
        pub fn new() -> Self {
            Self {
                databases: Mutex::new(HashMap::new()),
            }
        }
        
        pub fn add_database(&self, name: &str, database: Arc<MockMongoDatabase>) {
            self.databases.lock().unwrap().insert(name.to_string(), database);
        }
    }
    
    #[async_trait]
    impl MongoClient for MockMongoClient {
        fn database(&self, name: &str) -> Arc<dyn MongoDatabase> {
            let databases = self.databases.lock().unwrap();
            databases.get(name)
                .cloned()
                .unwrap_or_else(|| Arc::new(MockMongoDatabase::new()))
        }
    }
    
    /// Mock MongoDatabase para testing
    pub struct MockMongoDatabase {
        collections: Mutex<HashMap<String, Arc<dyn std::any::Any + Send + Sync>>>,
    }
    
    impl MockMongoDatabase {
        pub fn new() -> Self {
            Self {
                collections: Mutex::new(HashMap::new()),
            }
        }
        
        pub fn add_collection<T: 'static + Send + Sync>(
            &self,
            name: &str,
            collection: Arc<MockMongoCollection<T>>,
        ) {
            self.collections.lock().unwrap().insert(name.to_string(), collection);
        }
    }
    
    #[async_trait]
    impl MongoDatabase for MockMongoDatabase {
        fn collection<T>(&self, name: &str) -> Arc<dyn MongoCollection<T>> {
            let collections = self.collections.lock().unwrap();
            collections.get(name)
                .and_then(|c| c.clone().downcast_arc::<MockMongoCollection<T>>().ok())
                .unwrap_or_else(|| Arc::new(MockMongoCollection::<T>::new()))
        }
    }
    
    /// Mock MongoCollection para testing
    pub struct MockMongoCollection<T> {
        documents: Mutex<Vec<T>>,
    }
    
    impl<T: Clone + Send + Sync> MockMongoCollection<T> {
        pub fn new() -> Self {
            Self {
                documents: Mutex::new(Vec::new()),
            }
        }
        
        pub fn add_documents(&self, docs: Vec<T>) {
            *self.documents.lock().unwrap() = docs;
        }
    }
    
    #[async_trait]
    impl<T: Clone + Send + Sync> MongoCollection<T> for MockMongoCollection<T> {
        async fn find(&self, _filter: bson::Document) -> Result<MockCursor<T>, String> {
            let docs = self.documents.lock().unwrap().clone();
            Ok(MockCursor::new(docs))
        }
    }
    
    /// Mock RedisClient para testing
    pub struct MockRedisClient {
        data: Mutex<HashMap<String, String>>,
        expirations: Mutex<HashMap<String, DateTime<Utc>>>,
    }
    
    impl MockRedisClient {
        pub fn new() -> Self {
            Self {
                data: Mutex::new(HashMap::new()),
                expirations: Mutex::new(HashMap::new()),
            }
        }
        
        pub fn add_data(&self, key: &str, value: &str) {
            self.data.lock().unwrap().insert(key.to_string(), value.to_string());
        }
    }
    
    #[async_trait]
    impl RedisClient for MockRedisClient {
        async fn get(&self, key: &str) -> Result<Option<String>, String> {
            // Verificar expiración
            if let Some(expiration) = self.expirations.lock().unwrap().get(key) {
                if Utc::now() > *expiration {
                    self.data.lock().unwrap().remove(key);
                    self.expirations.lock().unwrap().remove(key);
                    return Ok(None);
                }
            }
            
            Ok(self.data.lock().unwrap().get(key).cloned())
        }
        
        async fn setex(&self, key: &str, seconds: u64, value: &str) -> Result<(), String> {
            self.data.lock().unwrap().insert(key.to_string(), value.to_string());
            let expiration = Utc::now() + chrono::Duration::seconds(seconds as i64);
            self.expirations.lock().unwrap().insert(key.to_string(), expiration);
            Ok(())
        }
        
        async fn del(&self, key: &str) -> Result<(), String> {
            self.data.lock().unwrap().remove(key);
            self.expirations.lock().unwrap().remove(key);
            Ok(())
        }
        
        async fn keys(&self, pattern: &str) -> Result<Vec<String>, String> {
            let data = self.data.lock().unwrap();
            let keys: Vec<String> = data.keys()
                .filter(|k| {
                    // Implementación simple de patrón con comodín
                    if pattern.ends_with("*") {
                        let prefix = &pattern[..pattern.len() - 1];
                        k.starts_with(prefix)
                    } else {
                        k == pattern
                    }
                })
                .cloned()
                .collect();
            Ok(keys)
        }
        
        async fn del_multiple(&self, keys: &[String]) -> Result<(), String> {
            let mut data = self.data.lock().unwrap();
            let mut expirations = self.expirations.lock().unwrap();
            
            for key in keys {
                data.remove(key);
                expirations.remove(key);
            }
            
            Ok(())
        }
    }
}