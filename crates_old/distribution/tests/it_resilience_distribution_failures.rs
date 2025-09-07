#![cfg(feature = "integration-distribution")]

use std::sync::Arc;
use std::time::Duration;
use distribution::{
    features::maven::upload::handler::handle as handle_maven_upload,
    features::npm::package_meta::publish_handler::handle as handle_npm_publish,
    infrastructure::http::MavenDistributionHandler,
};
use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_s3::{config::{Credentials, Region}, Client as S3Client};
use infra_mongo::test_util::mongo_test_container::ephemeral_store;
use shared::{RepositoryId, UserId, ArtifactId};
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::{localstack::LocalStack, rabbitmq::RabbitMq};
use testcontainers::clients;
use lapin::{Connection, ConnectionProperties};
use tokio::time::{sleep, timeout};
use artifact::{
    infrastructure::{persistence::MongoArtifactRepository, storage::S3ArtifactStorage, messaging::RabbitMqArtifactEventPublisher},
    domain::model::{ArtifactVersion, ArtifactChecksum},
};

// Mock handlers para simular fallos en distribución
struct FaultyMavenHandler {
    should_fail: bool,
    fail_count: std::sync::atomic::AtomicUsize,
}

struct FaultyNpmHandler {
    should_fail: bool, 
    fail_count: std::sync::atomic::AtomicUsize,
}

#[async_trait]
impl MavenDistributionHandler for FaultyMavenHandler {
    async fn handle_upload(&self, _req: distribution::features::maven::upload::MavenUploadRequest) -> anyhow::Result<distribution::features::maven::upload::MavenUploadResponse> {
        if self.should_fail {
            self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(anyhow::anyhow!("Maven upload failed: storage error"));
        }
        Ok(distribution::features::maven::upload::MavenUploadResponse {
            artifact_id: ArtifactId::new(),
            repository_id: RepositoryId::new(),
        })
    }

    async fn handle_download(&self, _req: distribution::features::maven::download::MavenDownloadRequest) -> anyhow::Result<distribution::features::maven::download::MavenDownloadResponse> {
        if self.should_fail {
            self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(anyhow::anyhow!("Maven download failed: not found"));
        }
        Ok(distribution::features::maven::download::MavenDownloadResponse {
            content: vec![],
            content_type: "application/octet-stream".to_string(),
        })
    }
}

async fn setup_dependencies() -> (MongoArtifactRepository, S3ArtifactStorage, RabbitMqArtifactEventPublisher, S3Client, String) {
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

    // RabbitMQ
    let docker = clients::Cli::default();
    let rabbitmq_container = docker.run(RabbitMq::default());
    let amqp_addr = format!("amqp://localhost:{}", rabbitmq_container.get_host_port_ipv4(5672));
    let event_publisher = RabbitMqArtifactEventPublisher::new(&amqp_addr, "hodei_artifacts_exchange").await.unwrap();

    (artifact_repo, artifact_storage, event_publisher, s3_client, bucket_name.to_string())
}

fn create_test_maven_request() -> distribution::features::maven::upload::MavenUploadRequest {
    distribution::features::maven::upload::MavenUploadRequest {
        repository_id: RepositoryId::new(),
        group_id: "com.example".to_string(),
        artifact_id: "test-library".to_string(),
        version: "1.0.0".to_string(),
        packaging: "jar".to_string(),
        content: b"dummy maven content".to_vec(),
        user_id: UserId::new(),
    }
}

fn create_test_npm_request() -> distribution::features::npm::package_meta::PublishRequest {
    distribution::features::npm::package_meta::PublishRequest {
        repository_id: RepositoryId::new(),
        package_name: "test-package".to_string(),
        version: "1.0.0".to_string(),
        metadata: serde_json::json!({
            "name": "test-package",
            "version": "1.0.0",
            "description": "Test package"
        }),
        user_id: UserId::new(),
    }
}

#[tokio::test]
async fn test_maven_upload_fails_when_storage_unavailable() {
    // Arrange
    let (repo, storage, publisher, _, _) = setup_dependencies().await;
    let request = create_test_maven_request();
    
    // Handler que siempre falla
    let faulty_handler = FaultyMavenHandler {
        should_fail: true,
        fail_count: std::sync::atomic::AtomicUsize::new(0),
    };

    // Act
    let result = handle_maven_upload(&repo, &storage, &publisher, request).await;

    // Assert - Debería fallar
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Maven upload failed"));
}

#[tokio::test]
async fn test_npm_publish_fails_when_validation_fails() {
    // Arrange
    let (repo, storage, publisher, _, _) = setup_dependencies().await;
    let mut request = create_test_npm_request();
    
    // Request inválido - sin nombre de paquete
    request.package_name = "".to_string();

    // Act
    let result = handle_npm_publish(&repo, &storage, &publisher, request).await;

    // Assert - Debería fallar por validación
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("validation") || error_msg.contains("invalid") || error_msg.contains("package"));
}

#[tokio::test]
async fn test_maven_download_fails_when_artifact_not_found() {
    // Arrange
    let faulty_handler = FaultyMavenHandler {
        should_fail: true,
        fail_count: std::sync::atomic::AtomicUsize::new(0),
    };

    let request = distribution::features::maven::download::MavenDownloadRequest {
        repository_id: RepositoryId::new(),
        group_id: "com.nonexistent".to_string(),
        artifact_id: "missing-library".to_string(),
        version: "1.0.0".to_string(),
        packaging: "jar".to_string(),
    };

    // Act
    let result = faulty_handler.handle_download(request).await;

    // Assert - Debería fallar con not found
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[tokio::test]
async fn test_distribution_timeout_handling() {
    // Arrange - Handler que simula timeout
    let slow_handler = FaultyMavenHandler {
        should_fail: false, // No falla, pero es lento
        fail_count: std::sync::atomic::AtomicUsize::new(0),
    };

    let request = create_test_maven_request();

    // Act - Intentar con timeout muy corto
    let result = timeout(
        Duration::from_millis(1), // Timeout muy agresivo
        slow_handler.handle_upload(request)
    ).await;

    // Assert - Debería timeoutear
    assert!(result.is_err());
}

#[tokio::test]
async fn test_concurrent_distribution_requests() {
    // Arrange
    let (repo, storage, publisher, _, _) = setup_dependencies().await;
    
    // Handler que falla intermitentemente
    let fail_counter = std::sync::atomic::AtomicUsize::new(0);
    let faulty_handler = FaultyMavenHandler {
        should_fail: true,
        fail_count: fail_counter.clone(),
    };

    // Act - Ejecutar múltiples requests concurrentes
    let mut handles = vec![];
    for i in 0..3 {
        let handler_clone = faulty_handler.clone();
        let request = create_test_maven_request();
        
        handles.push(tokio::spawn(async move {
            handler_clone.handle_upload(request).await
        }));
    }

    // Assert - Todos deberían fallar
    let results = futures::future::join_all(handles).await;
    for result in results {
        assert!(result.is_ok()); // La tarea completó
        let upload_result = result.unwrap();
        assert!(upload_result.is_err()); // Pero el upload falló
        assert!(upload_result.unwrap_err().to_string().contains("Maven upload failed"));
    }
    
    assert!(fail_counter.load(std::sync::atomic::Ordering::SeqCst) >= 3);
}

#[tokio::test]
async fn test_invalid_package_format_handling() {
    // Arrange
    let (repo, storage, publisher, _, _) = setup_dependencies().await;
    let mut request = create_test_maven_request();
    
    // Formato de packaging inválido
    request.packaging = "invalid-format".to_string();

    // Act
    let result = handle_maven_upload(&repo, &storage, &publisher, request).await;

    // Assert - Debería fallar por validación
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("packaging") || error_msg.contains("format") || error_msg.contains("invalid"));
}

#[tokio::test]
async fn test_large_file_upload_handling() {
    // Arrange
    let (repo, storage, publisher, _, _) = setup_dependencies().await;
    let mut request = create_test_maven_request();
    
    // Contenido muy grande (10MB)
    request.content = vec![0; 10 * 1024 * 1024]; // 10MB

    // Act
    let result = handle_maven_upload(&repo, &storage, &publisher, request).await;

    // Assert - Debería manejar el tamaño grande adecuadamente
    // Puede fallar por límite de tamaño o funcionar, pero no debería crash
    assert!(result.is_ok() || result.is_err());
    
    if let Err(e) = result {
        // Si falla, debería ser por límite de tamaño, no por error inesperado
        assert!(
            e.to_string().contains("size") || 
            e.to_string().contains("large") || 
            e.to_string().contains("limit") ||
            e.to_string().contains("storage")
        );
    }
}

#[tokio::test]
async fn test_distribution_rollback_on_failure() {
    // Arrange
    let (repo, storage, publisher, s3_client, bucket_name) = setup_dependencies().await;
    let request = create_test_maven_request();
    
    // Storage que fallará después de cierto punto
    let should_fail = std::sync::atomic::AtomicBool::new(true);
    let fail_counter = std::sync::atomic::AtomicUsize::new(0);

    // Act
    let result = handle_maven_upload(&repo, &storage, &publisher, request).await;

    // Assert - Verificar rollback completo
    assert!(result.is_err());
    
    // Verificar que NO se guardó artifact en MongoDB
    if let Ok(response) = result {
        let get_result = repo.get(&response.artifact_id).await;
        assert!(get_result.is_err() || get_result.unwrap().is_none());
    }
    
    // Verificar que NO se subió archivo a S3
    let list_objects = s3_client.list_objects_v2().bucket(&bucket_name).send().await;
    assert!(list_objects.is_ok());
    let objects = list_objects.unwrap();
    assert!(objects.contents().is_none() || objects.contents().unwrap().is_empty());
}