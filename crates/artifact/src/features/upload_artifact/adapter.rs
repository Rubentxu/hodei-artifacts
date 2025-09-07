use async_trait::async_trait;
use bytes::Bytes;
use aws_sdk_s3::{
    Client as S3Client,
    error::SdkError,
    operation::put_object::PutObjectError,
    primitives::ByteStream,
};
use aws_config::SdkConfig;
use mongodb::{
    Client as MongoClient,
    Database,
    options::ClientOptions,
    bson::doc,
};
use lapin::{
    options::{BasicPublishOptions, ExchangeDeclareOptions},
    types::FieldTable,
    BasicProperties, Channel, Connection, ConnectionProperties, ExchangeKind,
};
use serde_json::to_string;

use crate::domain::{
    events::ArtifactEvent,
    package_version::PackageVersion,
    physical_artifact::PhysicalArtifact,
};
use super::ports::{ArtifactStorage, EventPublisher, UploadArtifactRepository, PortResult};
use super::error::UploadArtifactError;

// --- Production Adapters ---

/// Concrete implementation of the ArtifactStorage port using AWS S3.
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
        let stream = ByteStream::from(content.clone());
        self.client
            .put_object()
            .bucket(&self.bucket_name)
            .key(content_hash)
            .body(stream)
            .content_length(content.len() as i64)
            .send()
            .await
            .map_err(|e: SdkError<PutObjectError>| UploadArtifactError::StorageError(e.to_string()))?;

        Ok(format!("s3://{}/{}", self.bucket_name, content_hash))
    }
}


/// Concrete implementation of the UploadArtifactRepository port using MongoDB.
pub struct MongoDbRepository {
    db: Database,
}

impl MongoDbRepository {
    pub async fn new(connection_string: &str, db_name: &str) -> mongodb::error::Result<Self> {
        let client_options = ClientOptions::parse(connection_string).await?;
        let client = MongoClient::with_options(client_options)?;
        let db = client.database(db_name);
        Ok(Self { db })
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

/// Concrete implementation of the EventPublisher port using RabbitMQ.
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


// --- Test Adapters ---

#[cfg(test)]
pub mod test {
    use async_trait::async_trait;
    use bytes::Bytes;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use crate::domain::{
        package_version::PackageVersion,
        physical_artifact::PhysicalArtifact,
        events::ArtifactEvent,
    };
    use super::super::ports::{UploadArtifactRepository, ArtifactStorage, EventPublisher, PortResult};

    pub struct MockArtifactRepository {
        physical_artifacts: Mutex<HashMap<String, PhysicalArtifact>>,
        package_versions: Mutex<HashMap<String, PackageVersion>>,
    }

    impl MockArtifactRepository {
        pub fn new() -> Self {
            Self {
                physical_artifacts: Mutex::new(HashMap::new()),
                package_versions: Mutex::new(HashMap::new()),
            }
        }

        pub async fn count_physical_artifacts(&self) -> usize {
            self.physical_artifacts.lock().unwrap().len()
        }

        pub async fn count_package_versions(&self) -> usize {
            self.package_versions.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl UploadArtifactRepository for MockArtifactRepository {
        async fn save_package_version(&self, package_version: &PackageVersion) -> PortResult<()> {
            self.package_versions.lock().unwrap().insert(package_version.hrn.to_string(), package_version.clone());
            Ok(())
        }

        async fn save_physical_artifact(&self, physical_artifact: &PhysicalArtifact) -> PortResult<()> {
            self.physical_artifacts.lock().unwrap().insert(physical_artifact.content_hash.value.clone(), physical_artifact.clone());
            Ok(())
        }

        async fn find_physical_artifact_by_hash(&self, hash: &str) -> PortResult<Option<PhysicalArtifact>> {
            let artifact = self.physical_artifacts.lock().unwrap().get(hash).cloned();
            Ok(artifact)
        }
    }

    pub struct MockArtifactStorage;

    #[async_trait]
    impl ArtifactStorage for MockArtifactStorage {
        async fn upload(&self, _content: Bytes, content_hash: &str) -> PortResult<String> {
            Ok(format!("s3://mock-bucket/{}", content_hash))
        }
    }

    pub struct MockEventPublisher {
        pub events: Arc<Mutex<Vec<ArtifactEvent>>>
    }
    
    impl MockEventPublisher {
        pub fn new() -> Self {
            Self { events: Arc::new(Mutex::new(Vec::new())) }
        }
    }

    #[async_trait]
    impl EventPublisher for MockEventPublisher {
        async fn publish(&self, event: &ArtifactEvent) -> PortResult<()> {
            self.events.lock().unwrap().push(event.clone());
            Ok(())
        }
    }
}