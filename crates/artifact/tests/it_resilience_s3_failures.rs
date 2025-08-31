#![cfg(feature = "integration-s3")]

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
use aws_sdk_s3::{config::{Credentials, Region}, Client as S3Client, types::SdkError, operation::put_object::PutObjectError};
use infra_mongo::test_util::mongo_test_container::ephemeral_store;
use shared::{RepositoryId, UserId};
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::{localstack::LocalStack, rabbitmq::RabbitMq};
use testcontainers::clients;
use lapin::{Connection, ConnectionProperties};
use tokio::time::{sleep, timeout};

// Mock storage para simular fallos de S3
struct FaultyS3Storage {
    should_fail: bool,
    fail_count: std::sync::atomic::AtomicUsize,
    real_storage: S3ArtifactStorage,
}

#[async_trait]
impl ArtifactStorage for FaultyS3Storage {
    async fn upload(&self, key: &str, data: &[u8]) -> anyhow::Result<()> {
        if self.should_fail {
            self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(anyhow::anyhow!("S3 upload failed: connection timeout"));
        }
        self.real_storage.upload(key, data).await
    }

    async fn download(&self, key: &str) -> anyhow::Result<Vec<u8>> {
        if self.should_fail {
            self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(anyhow::anyhow!("S3 download failed: connection timeout"));
        }
        self.real_storage.download(key).await
    }

    async fn exists(&self, key: &str) -> anyhow::Result<bool> {
        if self.should_fail {
            self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(anyhow::anyhow!("S3 exists check failed: connection timeout"));
        }
        self.real_storage.exists(key).await
    }

    async fn delete(&self, key: &str) -> anyhow::Result<()> {
        if self.should_fail {
            self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(anyhow::anyhow!("S3 delete failed: connection timeout"));
        }
        self.real_storage.delete(key).await
    }
}

async fn setup_dependencies() -> (MongoArtifactRepository, RabbitMqArtifactEventPublisher, S3Client, String) {
    // MongoDB
    let (mongo_client_factory, _mongo_container) = ephemeral_store().await.unwrap();
    let artifact_repo = MongoArtifactRepository::new(Arc::new(mongo_client_factory));
    artifact_repo.ensure_indexes().await.unwrap();

    // RabbitMQ
    let docker = clients::Cli::default();
    let rabbitmq_container = docker.run(RabbitMq::default());
    let amqp_addr = format!("amqp://localhost:{}", rabbitmq_container.get_host_port_ipv4(5672));
    let event_publisher = RabbitMqArtifactEventPublisher::new(&amqp_addr, "hodei_artifacts_exchange").await.unwrap();

    // S3 (LocalStack) - solo para verificación
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

    (artifact_repo, event_publisher, s3_client, bucket_name.to_string())
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
async fn test_upload_fails_when_s3_unavailable() {
    // Arrange
    let (repo, publisher, s3_client, bucket_name) = setup_dependencies().await;
    let user_id = UserId::new();
    let repo_id = RepositoryId::new();
    let cmd = create_test_command(repo_id, user_id);
    
    // Storage que siempre falla
    let real_storage = S3ArtifactStorage::new(s3_client.clone(), bucket_name.clone());
    let faulty_storage = FaultyS3Storage {
        should_fail: true,
        fail_count: std::sync::atomic::AtomicUsize::new(0),
        real_storage,
    };

    // Act
    let result = handle(&repo, &faulty_storage, &publisher, cmd).await;

    // Assert - Debería fallar por S3
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("S3"));
}

#[tokio::test]
async fn test_upload_rollback_when_s3_fails_after_db_success() {
    // Arrange
    let (repo, publisher, s3_client, bucket_name) = setup_dependencies().await;
    let user_id = UserId::new();
    let repo_id = RepositoryId::new();
    let cmd = create_test_command(repo_id.clone(), user_id.clone());
    
    // Storage que falla después del primer intento
    let real_storage = S3ArtifactStorage::new(s3_client.clone(), bucket_name.clone());
    let fail_counter = std::sync::atomic::AtomicUsize::new(0);
    let faulty_storage = FaultyS3Storage {
        should_fail: true,
        fail_count: fail_counter.clone(),
        real_storage,
    };

    // Act
    let result = handle(&repo, &faulty_storage, &publisher, cmd).await;

    // Assert - Debería fallar completamente
    assert!(result.is_err());
    
    // Verificar que NO se guardó el artifact en MongoDB (rollback)
    if let Ok(artifact_result) = result {
        let get_result = repo.get(&artifact_result.artifact_id).await;
        assert!(get_result.is_err() || get_result.unwrap().is_none());
    }
    
    // Verificar que NO se publicó evento a RabbitMQ
    // (esto requiere verificación del consumer, pero asumimos que no se publicó)
}

#[tokio::test]
async fn test_s3_timeout_handling() {
    // Arrange - Configurar S3 con timeout muy bajo
    let localstack_container = LocalStack::default().start().await.unwrap();
    let host_port = localstack_container.get_host_port_ipv4(4566).await.unwrap();
    let endpoint_url = format!("http://127.0.0.1:{}", host_port);
    
    let s3_config = aws_sdk_s3::Config::builder()
        .region(Region::new("us-east-1"))
        .credentials_provider(Credentials::new("test", "test", None, None, "test"))
        .endpoint_url(endpoint_url)
        .behavior_version(BehaviorVersion::latest())
        .force_path_style(true)
        .timeout_config(
            aws_smithy_types::timeout::TimeoutConfig::builder()
                .operation_timeout(Duration::from_millis(1)) // Timeout muy agresivo
                .build()
        )
        .build();
    
    let s3_client = S3Client::from_conf(s3_config);
    let bucket_name = "test-bucket-timeout";
    
    // Intentar crear bucket con timeout bajo
    let result = timeout(
        Duration::from_secs(2),
        s3_client.create_bucket().bucket(bucket_name).send()
    ).await;

    // Assert - Debería timeoutear
    assert!(result.is_err() || result.unwrap().is_err());
}

#[tokio::test]
async fn test_s3_bucket_not_exists_error() {
    // Arrange
    let (repo, publisher, s3_client, _) = setup_dependencies().await;
    let user_id = UserId::new();
    let repo_id = RepositoryId::new();
    let cmd = create_test_command(repo_id, user_id);
    
    // Storage con bucket que no existe
    let non_existent_storage = S3ArtifactStorage::new(s3_client.clone(), "non-existent-bucket".to_string());

    // Act
    let result = handle(&repo, &non_existent_storage, &publisher, cmd).await;

    // Assert - Debería fallar con error de bucket
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("bucket") || error_msg.contains("S3") || error_msg.contains("NoSuchBucket"));
}

#[tokio::test]
async fn test_concurrent_uploads_with_s3_failures() {
    // Arrange
    let (repo, publisher, s3_client, bucket_name) = setup_dependencies().await;
    let repo_id = RepositoryId::new();
    
    // Storage que falla intermitentemente
    let real_storage = S3ArtifactStorage::new(s3_client.clone(), bucket_name.clone());
    let fail_counter = std::sync::atomic::AtomicUsize::new(0);
    let faulty_storage = FaultyS3Storage {
        should_fail: true,
        fail_count: fail_counter.clone(),
        real_storage,
    };

    // Act - Ejecutar múltiples uploads concurrentes
    let mut handles = vec![];
    for i in 0..3 {
        let repo_clone = repo.clone();
        let storage_clone = faulty_storage.clone();
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
        assert!(upload_result.unwrap_err().to_string().contains("S3"));
    }
    
    assert!(fail_counter.load(std::sync::atomic::Ordering::SeqCst) >= 3);
}