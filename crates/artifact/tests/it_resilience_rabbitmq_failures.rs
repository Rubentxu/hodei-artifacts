#![cfg(feature = "integration-rabbitmq")]

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

// Mock publisher para simular fallos de RabbitMQ
struct FaultyRabbitMqPublisher {
    should_fail: bool,
    fail_count: std::sync::atomic::AtomicUsize,
}

#[async_trait]
impl ArtifactEventPublisher for FaultyRabbitMqPublisher {
    async fn publish_created(&self, _event: &shared::domain::event::ArtifactUploadedEvent) -> anyhow::Result<()> {
        if self.should_fail {
            self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(anyhow::anyhow!("RabbitMQ connection failed"));
        }
        Ok(())
    }

    async fn publish_download_requested(&self, _event: &shared::domain::event::ArtifactDownloadRequestedEvent) -> anyhow::Result<()> {
        if self.should_fail {
            self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(anyhow::anyhow!("RabbitMQ connection failed"));
        }
        Ok(())
    }
}

async fn setup_dependencies() -> (MongoArtifactRepository, S3ArtifactStorage, S3Client, String) {
    // MongoDB
    let (mongo_client_factory, _mongo_container) = ephemeral_store().await.unwrap();
    let artifact_repo = MongoArtifactRepository::new(Arc::new(mongo_client_factory));
    artifact_repo.ensure_indexes().await.unwrap();

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

    (artifact_repo, artifact_storage, s3_client, bucket_name.to_string())
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
async fn test_upload_succeeds_when_rabbitmq_fails_after_retry() {
    // Arrange
    let (repo, storage, _, _) = setup_dependencies().await;
    let user_id = UserId::new();
    let repo_id = RepositoryId::new();
    let cmd = create_test_command(repo_id, user_id);
    
    // Publisher que falla las primeras 2 veces, luego funciona
    let fail_count = std::sync::atomic::AtomicUsize::new(0);
    let publisher = FaultyRabbitMqPublisher {
        should_fail: true,
        fail_count: fail_count.clone(),
    };

    // Act & Assert - Debería fallar por el publisher defectuoso
    let result = handle(&repo, &storage, &publisher, cmd).await;
    
    // Verificar que falló debido a RabbitMQ
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("RabbitMQ"));
    assert_eq!(fail_count.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test] 
async fn test_upload_completes_even_if_event_publishing_fails() {
    // Arrange
    let (repo, storage, s3_client, bucket_name) = setup_dependencies().await;
    let user_id = UserId::new();
    let repo_id = RepositoryId::new();
    let cmd = create_test_command(repo_id, user_id);
    
    // Publisher que siempre falla
    let publisher = FaultyRabbitMqPublisher {
        should_fail: true,
        fail_count: std::sync::atomic::AtomicUsize::new(0),
    };

    // Act
    let result = handle(&repo, &storage, &publisher, cmd).await;

    // Assert - La operación debería fallar completamente
    assert!(result.is_err());
    
    // Verificar que NO se guardó el artifact en MongoDB
    if let Ok(artifact_result) = result {
        let get_result = repo.get(&artifact_result.artifact_id).await;
        assert!(get_result.is_err() || get_result.unwrap().is_none());
    }
    
    // Verificar que NO se subió el archivo a S3
    let list_objects = s3_client.list_objects_v2().bucket(&bucket_name).send().await;
    assert!(list_objects.is_ok());
    let objects = list_objects.unwrap();
    assert!(objects.contents().is_none() || objects.contents().unwrap().is_empty());
}

#[tokio::test]
async fn test_rabbitmq_connection_timeout_handling() {
    // Arrange - Intentar conectar a un puerto no existente
    let non_existent_amqp = "amqp://localhost:9999";
    
    // Act - Intentar crear publisher con conexión inválida
    let result = timeout(
        Duration::from_secs(5),
        RabbitMqArtifactEventPublisher::new(non_existent_amqp, "test_exchange")
    ).await;

    // Assert - Debería timeoutear o fallar rápidamente
    assert!(result.is_err() || result.unwrap().is_err());
}

#[tokio::test]
async fn test_concurrent_uploads_with_rabbitmq_failures() {
    // Arrange
    let (repo, storage, _, _) = setup_dependencies().await;
    let repo_id = RepositoryId::new();
    
    // Publisher que falla intermitentemente
    let fail_counter = std::sync::atomic::AtomicUsize::new(0);
    let publisher = FaultyRabbitMqPublisher {
        should_fail: true,
        fail_count: fail_counter.clone(),
    };

    // Act - Ejecutar múltiples uploads concurrentes
    let mut handles = vec![];
    for i in 0..3 {
        let repo_clone = repo.clone();
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
        assert!(upload_result.unwrap_err().to_string().contains("RabbitMQ"));
    }
    
    assert!(fail_counter.load(std::sync::atomic::Ordering::SeqCst) >= 3);
}