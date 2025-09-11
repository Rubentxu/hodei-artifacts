use async_trait::async_trait;
use bytes::Bytes;
use aws_config::SdkConfig;
use aws_sdk_s3::{
    Client as S3Client,
    error::SdkError,
    operation::put_object::PutObjectError,
    primitives::ByteStream,
};
use sha2::Digest;
use lapin::{
    options::{BasicPublishOptions, ExchangeDeclareOptions},
    types::FieldTable,
    BasicProperties, Channel, Connection, ConnectionProperties, ExchangeKind,
};
use mongodb::{
    bson::doc,
    options::ClientOptions,
    Client as MongoClient,
    Database,
};
use serde_json::to_string;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

use crate::domain::{
    events::ArtifactEvent,
    package_version::PackageVersion,
    physical_artifact::PhysicalArtifact,
};
use super::error::UploadArtifactError;
use super::ports::{ArtifactStorage, EventPublisher, PortResult, ArtifactRepository, ChunkedUploadStorage};
use super::ports::ArtifactValidator;

// --- ArtifactStorage: S3 ---
pub struct S3ArtifactStorage {
    client: S3Client,
    bucket_name: String,
}

impl S3ArtifactStorage {
    pub fn new(sdk_config: &SdkConfig, bucket_name: String) -> Self {
        Self {
            client: S3Client::new(sdk_config),
            bucket_name,
        }
    }
}

#[async_trait]
impl ArtifactStorage for S3ArtifactStorage {
    async fn upload(&self, content: Bytes, content_hash: &str) -> PortResult<String> {
        let stream = ByteStream::from(content);
        self.client
            .put_object()
            .bucket(&self.bucket_name)
            .key(content_hash)
            .body(stream)
            .send()
            .await
            .map_err(|e: SdkError<PutObjectError>| UploadArtifactError::StorageError(e.to_string()))?;

        Ok(format!("s3://{}/{}", self.bucket_name, content_hash))
    }

    async fn upload_from_path(&self, path: &Path, content_hash: &str) -> PortResult<String> {
        let stream = ByteStream::from_path(path).await.map_err(|e| UploadArtifactError::StorageError(e.to_string()))?;
        self.client
            .put_object()
            .bucket(&self.bucket_name)
            .key(content_hash)
            .body(stream)
            .send()
            .await
            .map_err(|e: SdkError<PutObjectError>| UploadArtifactError::StorageError(e.to_string()))?;

        Ok(format!("s3://{}/{}", self.bucket_name, content_hash))
    }
}

// --- ArtifactRepository: MongoDB ---
pub struct MongoDbRepository {
    db: Database,
}

impl MongoDbRepository {
    pub async fn new(connection_string: &str, db_name: &str) -> mongodb::error::Result<Self> {
        let client_options = ClientOptions::parse(connection_string).await?;
        let client = MongoClient::with_options(client_options)?;
        Ok(Self {
            db: client.database(db_name),
        })
    }
}

#[async_trait]
impl ArtifactRepository for MongoDbRepository {
    async fn save_package_version(&self, package_version: &PackageVersion) -> PortResult<()> {
        let collection = self.db.collection("package_versions");
        let doc = mongodb::bson::to_document(package_version).map_err(|e| UploadArtifactError::RepositoryError(e.to_string()))?;
        collection.insert_one(doc).await.map_err(|e| UploadArtifactError::RepositoryError(e.to_string()))?;
        Ok(())
    }

    async fn save_physical_artifact(&self, physical_artifact: &PhysicalArtifact) -> PortResult<()> {
        let collection = self.db.collection("physical_artifacts");
        let doc = mongodb::bson::to_document(physical_artifact).map_err(|e| UploadArtifactError::RepositoryError(e.to_string()))?;
        collection.insert_one(doc).await.map_err(|e| UploadArtifactError::RepositoryError(e.to_string()))?;
        Ok(())
    }

    async fn find_physical_artifact_by_hash(&self, hash: &str) -> PortResult<Option<PhysicalArtifact>> {
        let collection = self.db.collection("physical_artifacts");
        let filter = doc! { "content_hash.value": hash };
        let result = collection.find_one(filter).await.map_err(|e| UploadArtifactError::RepositoryError(e.to_string()))?;
        match result {
            Some(doc) => {
                let artifact: PhysicalArtifact = mongodb::bson::from_document(doc).map_err(|e| UploadArtifactError::RepositoryError(e.to_string()))?;
                Ok(Some(artifact))
            }
            None => Ok(None),
        }
    }
}

// --- EventPublisher: RabbitMQ ---
pub struct RabbitMqEventPublisher {
    #[allow(dead_code)]
    connection: Connection,
    channel: Channel,
    exchange: String,
}

impl RabbitMqEventPublisher {
    pub async fn new(amqp_url: &str, exchange: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let connection = Connection::connect(amqp_url, ConnectionProperties::default()).await?;
        let channel = connection.create_channel().await?;

        // Declare the exchange
        channel.exchange_declare(
            exchange,
            ExchangeKind::Topic,
            ExchangeDeclareOptions::default(),
            FieldTable::default(),
        ).await?;

        Ok(Self {
            connection,
            channel,
            exchange: exchange.to_string(),
        })
    }
}

#[async_trait]
impl EventPublisher for RabbitMqEventPublisher {
    async fn publish(&self, event: &ArtifactEvent) -> PortResult<()> {
        let payload = to_string(event).map_err(|e| UploadArtifactError::EventError(e.to_string()))?;

        self.channel.basic_publish(
            &self.exchange,
            "artifact.uploaded",
            BasicPublishOptions::default(),
            payload.as_bytes(),
            BasicProperties::default(),
        ).await
        .map_err(|e| UploadArtifactError::EventError(e.to_string()))?;

        Ok(())
    }
}

// Validador por defecto (no-op)
pub struct NoopArtifactValidator;

#[async_trait]
impl ArtifactValidator for NoopArtifactValidator {
    async fn validate(&self, _command: &crate::features::upload_artifact::dto::UploadArtifactCommand, _content: &Bytes) -> Result<(), Vec<String>> {
        Ok(())
    }
}

// --- ArtifactStorage: Local filesystem ---
pub struct LocalFsArtifactStorage {
    base_dir: PathBuf,
}

impl LocalFsArtifactStorage {
    pub fn new<P: Into<PathBuf>>(base_dir: P) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    fn target_path(&self, content_hash: &str) -> PathBuf {
        self.base_dir.join(content_hash)
    }
}

#[async_trait]
impl ArtifactStorage for LocalFsArtifactStorage {
    async fn upload(&self, content: Bytes, content_hash: &str) -> PortResult<String> {
        let path = self.target_path(content_hash);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| UploadArtifactError::StorageError(e.to_string()))?;
        }
        tokio::fs::write(&path, content).await.map_err(|e| UploadArtifactError::StorageError(e.to_string()))?;
        Ok(format!("file://{}", path.display()))
    }

    async fn upload_from_path(&self, path: &Path, content_hash: &str) -> PortResult<String> {
        let dst = self.target_path(content_hash);
        if let Some(parent) = dst.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| UploadArtifactError::StorageError(e.to_string()))?;
        }
        match tokio::fs::rename(path, &dst).await {
            Ok(_) => {}
            Err(_) => {
                tokio::fs::copy(path, &dst).await.map_err(|e| UploadArtifactError::StorageError(e.to_string()))?;
            }
        }
        Ok(format!("file://{}", dst.display()))
    }
}

// --- ChunkedUploadStorage: Local filesystem ---
pub struct LocalFsChunkedUploadStorage {
    temp_dir: PathBuf,
}

impl LocalFsChunkedUploadStorage {
    pub fn new(temp_dir: PathBuf) -> Self {
        Self { temp_dir }
    }

    fn get_upload_dir(&self, upload_id: &str) -> PathBuf {
        self.temp_dir.join(upload_id)
    }
}

#[async_trait]
impl ChunkedUploadStorage for LocalFsChunkedUploadStorage {
    async fn save_chunk(&self, upload_id: &str, chunk_number: usize, data: bytes::Bytes) -> Result<(), UploadArtifactError> {
        let upload_dir = self.get_upload_dir(upload_id);
        tokio::fs::create_dir_all(&upload_dir).await.map_err(|e| UploadArtifactError::StorageError(format!("Failed to create chunk directory: {}", e)))?;

        let chunk_path = upload_dir.join(format!("{}", chunk_number));
        let mut file = tokio::fs::File::create(&chunk_path).await.map_err(|e| UploadArtifactError::StorageError(format!("Failed to create chunk file: {}", e)))?;
        file.write_all(&data).await.map_err(|e| UploadArtifactError::StorageError(format!("Failed to write to chunk file: {}", e)))?;

        Ok(())
    }

    async fn get_received_chunks_count(&self, upload_id: &str) -> Result<usize, UploadArtifactError> {
        let upload_dir = self.get_upload_dir(upload_id);
        if !upload_dir.exists() {
            return Ok(0);
        }
        let mut read_dir = tokio::fs::read_dir(upload_dir).await.map_err(|e| UploadArtifactError::StorageError(format!("Failed to read chunk directory: {}", e)))?;
        let mut count = 0;
        while let Some(_) = read_dir.next_entry().await.map_err(|e| UploadArtifactError::StorageError(format!("Failed to iterate chunk directory: {}", e)))? {
            count += 1;
        }
        Ok(count)
    }

    async fn get_received_chunk_numbers(&self, upload_id: &str) -> Result<Vec<usize>, UploadArtifactError> {
        let upload_dir = self.get_upload_dir(upload_id);
        if !upload_dir.exists() {
            return Ok(vec![]);
        }
        let mut read_dir = tokio::fs::read_dir(upload_dir).await.map_err(|e| UploadArtifactError::StorageError(format!("Failed to read chunk directory: {}", e)))?;
        let mut chunk_numbers = Vec::new();
        while let Some(entry) = read_dir.next_entry().await.map_err(|e| UploadArtifactError::StorageError(format!("Failed to iterate chunk directory: {}", e)))? {
            if let Some(file_name) = entry.file_name().to_str() {
                if let Ok(chunk_number) = file_name.parse::<usize>() {
                    chunk_numbers.push(chunk_number);
                }
            }
        }
        chunk_numbers.sort();
        Ok(chunk_numbers)
    }

    async fn assemble_chunks(&self, upload_id: &str, total_chunks: usize, file_name: &str) -> Result<(PathBuf, String), UploadArtifactError> {
        let upload_dir = self.get_upload_dir(upload_id);
        let final_path = self.temp_dir.join(file_name);
        let mut final_file = tokio::fs::File::create(&final_path).await.map_err(|e| UploadArtifactError::StorageError(format!("Failed to create assembled file: {}", e)))?;
        let mut hasher = sha2::Sha256::new();

        for i in 1..=total_chunks {
            let chunk_path = upload_dir.join(format!("{}", i));
            let chunk_data = tokio::fs::read(&chunk_path).await.map_err(|e| UploadArtifactError::StorageError(format!("Failed to read chunk file {}: {}", i, e)))?;

            // Update hash
            hasher.update(&chunk_data);

            // Write to final file
            final_file.write_all(&chunk_data).await.map_err(|e| UploadArtifactError::StorageError(format!("Failed to write chunk {}: {}", i, e)))?;
        }

        let hash = hex::encode(hasher.finalize());
        Ok((final_path, hash))
    }

    async fn cleanup(&self, upload_id: &str) -> Result<(), UploadArtifactError> {
        let upload_dir = self.get_upload_dir(upload_id);
        if upload_dir.exists() {
            tokio::fs::remove_dir_all(upload_dir).await.map_err(|e| UploadArtifactError::StorageError(format!("Failed to clean up chunk directory: {}", e)))?;
        }
        Ok(())
    }
}
use async_trait::async_trait;
use semver::Version;

use crate::features::upload_artifact::ports::{VersionValidator, ParsedVersion};

/// Implementación por defecto del validador de versiones usando la librería semver
#[derive(Debug, Clone)]
pub struct DefaultVersionValidator;

impl DefaultVersionValidator {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl VersionValidator for DefaultVersionValidator {
    async fn validate_version(&self, version_str: &str) -> Result<(), String> {
        // Manejar versiones SNAPSHOT (especialmente para Maven)
        let is_snapshot = version_str.to_lowercase().ends_with("-snapshot");
        let version_to_parse = if is_snapshot {
            &version_str[..version_str.len() - 9] // Remover "-snapshot"
        } else {
            version_str
        };
        
        // Parsear la versión usando la librería semver
        Version::parse(version_to_parse)
            .map(|_| ())
            .map_err(|e| format!("Invalid semantic version '{}': {}", version_str, e))
    }

    async fn parse_version(&self, version_str: &str) -> Result<ParsedVersion, String> {
        // Manejar versiones SNAPSHOT (especialmente para Maven)
        let is_snapshot = version_str.to_lowercase().ends_with("-snapshot");
        let version_to_parse = if is_snapshot {
            &version_str[..version_str.len() - 9] // Remover "-snapshot"
        } else {
            version_str
        };
        
        // Parsear la versión usando la librería semver
        let version = Version::parse(version_to_parse)
            .map_err(|e| format!("Invalid semantic version '{}': {}", version_str, e))?;
        
        Ok(ParsedVersion {
            original: version_str.to_string(),
            major: version.major,
            minor: version.minor,
            patch: version.patch,
            prerelease: if version.pre.is_empty() {
                None
            } else {
                Some(version.pre.as_str().to_string())
            },
            build_metadata: if version.build.is_empty() {
                None
            } else {
                Some(version.build.as_str().to_string())
            },
            is_snapshot,
        })
    }
}