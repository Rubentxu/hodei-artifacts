use async_trait::async_trait;
use bytes::Bytes;
use aws_config::SdkConfig;
use aws_sdk_s3::{
    Client as S3Client,
    error::SdkError,
    operation::put_object::PutObjectError,
    primitives::ByteStream,
};
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
use std::io::ErrorKind;
use tokio::io::AsyncWriteExt;

use crate::domain::{
    events::ArtifactEvent,
    package_version::PackageVersion,
    physical_artifact::PhysicalArtifact,
};
use super::error::UploadArtifactError;
use super::ports::{ArtifactStorage, EventPublisher, PortResult, UploadArtifactRepository};

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

// --- UploadArtifactRepository: MongoDB ---
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
impl UploadArtifactRepository for MongoDbRepository {
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

