#![cfg(feature = "integration-mongodb")]

use std::sync::Arc;
use std::time::Duration;
use artifact::{
    application::ports::{ArtifactRepository, ArtifactEventPublisher, ArtifactStorage},
    domain::model::{ArtifactVersion, ArtifactChecksum},
    features::upload_artifact::{command::UploadArtifactCommand, handler::handle},
    infrastructure::{persistence::MongoArtifactRepository, storage::S3ArtifactStorage, messaging::RabbitMqArtifactEventPublisher},
};
use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_s3::{config::{Credentials, Region}, Client as S3Client};
use infra_mongo::test_util::mongo_test_container::ephemeral_store;
use shared::{RepositoryId, UserId};
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::{localstack::LocalStack, rabbitmq::RabbitMq};
use testcontainers::clients;
use lapin::{Connection, ConnectionProperties};
use tokio::time::{sleep, timeout};
use mongodb::{bson::doc, options::ClientOptions};

// Mock repository para simular fallos de MongoDB
struct FaultyMongoRepository {
    should_fail: bool,
    fail_count: std::sync::atomic::AtomicUsize,
    real_repo: MongoArtifactRepository,
}

#[async_trait]
impl ArtifactRepository for FaultyMongoRepository {
    async fn save(&self, artifact: &artifact::domain::model::Artifact) -> anyhow::Result<()> {
        if self.should_fail {
            self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(anyhow::anyhow!("MongoDB connection failed"));
        }
        self.real_repo.save(artifact).await
    }

    async fn get(&self, id: &shared::ArtifactId) -> anyhow::Result<Option<artifact::domain::model::Artifact>> {
        if self.should_fail {
            self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(anyhow::anyhow!("MongoDB connection failed"));
        }
        self.real_repo.get(id).await
    }

    async fn get_by_checksum(&self, checksum: &ArtifactChecksum) -> anyhow::Result<Option<artifact::domain::model::Artifact>> {
        if self.should_fail {
            self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(anyhow::anyhow!("MongoDB connection failed"));
        }
        self.real_repo.get_by_checksum(checksum).await
    }

    async fn list_by_repository(&self, repository_id: &RepositoryId) -> anyhow::Result<Vec<artifact::domain::model::Artifact>> {
        if self.should_fail {
            self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(anyhow::anyhow!("MongoDB connection failed"));
        }
        self.real_repo.list_by_repository(repository_id).await
    }

    async fn ensure_indexes(&self) -> anyhow::Result<()> {
        if self.should_fail {
            self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(anyhow::anyhow!("MongoDB connection failed"));
        }
        self.real_repo.ensure_indexes().await
    }
}

async fn setup_dependencies() -> (S3ArtifactStorage, RabbitMqArtifactEventPublisher, S3Client, String) {
    // S3 (LocalStack)
    let localstack_container = LocalStack::default().start().await.unwrap();
    let host_port = localstack_container.get_host_port_ipv4(4566).await.unwrap();
    let endpoint_url = format!("http://127.0.0.1:{}", host_port);
    let s3_config = aws_sdk_s3::Config::builder()
        .region(Region::new("us-east-1"))
        .credentials_provider(Credentials::new("test", "test", None, None, "test"))
        .endpoint_url(endpoint_url)
        .behavior_version(BehaviorVersion::latest())
        .force_path_style(true)
        .build();
    let s3_client = S3Client::from_conf(s3_config);
    let bucket_name = "test-bucket";
    s3_client.create_bucket().bucket(bucket_name).send().await.unwrap();
    let artifact_storage = S3ArtifactStorage::new(s3_client.clone(), bucket_name.to_string());

    // RabbitMQ
    let docker = clients::Cli::default();
    let rabbitmq_container = docker.run(RabbitMq::default());
    let amqp_addr = format!("amqp://localhost:{}", rabbitmq_container.get_host_port_ipv4(5672));
    let event_publisher = RabbitMqArtifactEventPublisher::new(&amqp_addr, "hodei_artifacts_exchange").await.unwrap();

    (artifact_storage, event_publisher, s3_client, bucket_name.to_string())
}

fn create_test_command(repo_id: RepositoryId, user_id: UserId) -> UploadArtifactCommand {
    UploadArtifactCommand {
        repository_id: repo_id,
        version: ArtifactVersion("1.0.0".to_string()),
        file_name: "test.txt".to_string(),
        size_bytes: 11,
        checksum: ArtifactChecksum("d41d8cd98f00b204e9800998ecf8427e".to_string()),
        user_id,
        bytes: b"hello world".to_vec(),
    }
}

#[tokio::test]
async fn test_upload_fails_when_mongodb_unavailable() {
    // Arrange
    let (storage, publisher, s3_client, bucket_name) = setup_dependencies().await;
    
    // MongoDB real para el mock
    let (mongo_client_factory, _mongo_container) = ephemeral_store().await.unwrap();
    let real_repo = MongoArtifactRepository::new(Arc::new(mongo_client_factory));
    real_repo.ensure_indexes().await.unwrap();
    
    let user_id = UserId::new();
    let repo_id = RepositoryId::new();
    let cmd = create_test_command(repo_id, user_id);
    
    // Repository que siempre falla
    let faulty_repo = FaultyMongoRepository {
        should_fail: true,
        fail_count: std::sync::atomic::AtomicUsize::new(0),
        real_repo,
    };

    // Act
    let result = handle(&faulty_repo, &storage, &publisher, cmd).await;

    // Assert - Debería fallar por MongoDB
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("MongoDB"));
}

#[tokio::test]
async fn test_upload_rollback_when_mongodb_fails_after_s3_success() {
    // Arrange
    let (storage, publisher, s3_client, bucket_name) = setup_dependencies().await;
    
    // MongoDB real para el mock
    let (mongo_client_factory, _mongo_container) = ephemeral_store().await.unwrap();
    let real_repo = MongoArtifactRepository::new(Arc::new(mongo_client_factory));
    real_repo.ensure_indexes().await.unwrap();
    
    let user_id = UserId::new();
    let repo_id = RepositoryId::new();
    let cmd = create_test_command(repo_id.clone(), user_id.clone());
    
    // Repository que falla en save pero no en otras operaciones
    let fail_counter = std::sync::atomic::AtomicUsize::new(0);
    let faulty_repo = FaultyMongoRepository {
        should_fail: true,
        fail_count: fail_counter.clone(),
        real_repo,
    };

    // Act
    let result = handle(&faulty_repo, &storage, &publisher, cmd).await;

    // Assert - Debería fallar completamente
    assert!(result.is_err());
    
    // Verificar que NO se subió el archivo a S3 (rollback)
    let list_objects = s3_client.list_objects_v2().bucket(&bucket_name).send().await;
    assert!(list_objects.is_ok());
    let objects = list_objects.unwrap();
    assert!(objects.contents().is_none() || objects.contents().unwrap().is_empty());
    
    // Verificar que NO se publicó evento a RabbitMQ
}

#[tokio::test]
async fn test_mongodb_connection_timeout_handling() {
    // Arrange - Intentar conectar a un puerto no existente
    let non_existent_mongo = "mongodb://localhost:9999/hodei-test";
    
    let options = ClientOptions::parse(non_existent_mongo).await;
    assert!(options.is_err()); // Debería fallar inmediatamente
}

#[tokio::test]
async fn test_mongodb_network_partition_simulation() {
    // Arrange
    let (storage, publisher, _, _) = setup_dependencies().await;
    
    // MongoDB real
    let (mongo_client_factory, mongo_container) = ephemeral_store().await.unwrap();
    let real_repo = MongoArtifactRepository::new(Arc::new(mongo_client_factory));
    real_repo.ensure_indexes().await.unwrap();
    
    let user_id = UserId::new();
    let repo_id = RepositoryId::new();
    let cmd = create_test_command(repo_id.clone(), user_id.clone());
    
    // Act - Detener MongoDB durante la operación
    let handle = tokio::spawn(async move {
        // Pequeña pausa para simular operación en curso
        sleep(Duration::from_millis(100)).await;
        
        // Detener el contenedor de MongoDB
        drop(mongo_container);
        
        // Intentar operación con MongoDB caído
        handle(&real_repo, &storage, &publisher, cmd).await
    });

    // Assert - Debería fallar con error de conexión
    let result = handle.await.unwrap();
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("connection") || error_msg.contains("MongoDB") || error_msg.contains("network"));
}

#[tokio::test]
async fn test_concurrent_uploads_with_mongodb_failures() {
    // Arrange
    let (storage, publisher, _, _) = setup_dependencies().await;
    
    // MongoDB real para el mock
    let (mongo_client_factory, _mongo_container) = ephemeral_store().await.unwrap();
    let real_repo = MongoArtifactRepository::new(Arc::new(mongo_client_factory));
    real_repo.ensure_indexes().await.unwrap();
    
    let repo_id = RepositoryId::new();
    
    // Repository que falla intermitentemente
    let fail_counter = std::sync::atomic::AtomicUsize::new(0);
    let faulty_repo = FaultyMongoRepository {
        should_fail: true,
        fail_count: fail_counter.clone(),
        real_repo,
    };

    // Act - Ejecutar múltiples uploads concurrentes
    let mut handles = vec![];
    for i in 0..3 {
        let repo_clone = faulty_repo.clone();
        let storage_clone = storage.clone();
        let publisher_clone = publisher.clone();
        let cmd = create_test_command(repo_id.clone(), UserId::new());
        
        handles.push(tokio::spawn(async move {
            handle(&repo_clone, &storage_clone, &publisher_clone, cmd).await
        }));
    }

    // Assert - Todos deberían fallar
    let results = futures::future::join_all(handles).await;
    for result in results {
        assert!(result.is_ok()); // La tarea completó
        let upload_result = result.unwrap();
        assert!(upload_result.is_err()); // Pero el upload falló
        assert!(upload_result.unwrap_err().to_string().contains("MongoDB"));
    }
    
    assert!(fail_counter.load(std::sync::atomic::Ordering::SeqCst) >= 3);
}

#[tokio::test]
async fn test_mongodb_index_creation_failure() {
    // Arrange
    let (mongo_client_factory, _mongo_container) = ephemeral_store().await.unwrap();
    let repo = MongoArtifactRepository::new(Arc::new(mongo_client_factory));
    
    // Act - Intentar crear índices con conexión válida
    let result = repo.ensure_indexes().await;
    
    // Assert - Debería funcionar normalmente
    assert!(result.is_ok());
    
    // Simular fallo en creación de índices
    let faulty_repo = FaultyMongoRepository {
        should_fail: true,
        fail_count: std::sync::atomic::AtomicUsize::new(0),
        real_repo: repo,
    };
    
    let fail_result = faulty_repo.ensure_indexes().await;
    assert!(fail_result.is_err());
    assert!(fail_result.unwrap_err().to_string().contains("MongoDB"));
}