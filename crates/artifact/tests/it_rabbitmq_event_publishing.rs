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
use shared::{RepositoryId, UserId, domain::event::ArtifactUploadedEvent};
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::{localstack::LocalStack, rabbitmq::RabbitMq};
use testcontainers::clients;
use lapin::{Connection, ConnectionProperties, options::*, types::FieldTable};
use tokio::time::{sleep, timeout};
use serde_json::json;

async fn setup_dependencies() -> (MongoArtifactRepository, S3ArtifactStorage, RabbitMqArtifactEventPublisher, S3Client, String, String) {
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
    let exchange_name = "hodei_artifacts_exchange";
    let event_publisher = RabbitMqArtifactEventPublisher::new(&amqp_addr, exchange_name).await.unwrap();

    (artifact_repo, artifact_storage, event_publisher, s3_client, bucket_name.to_string(), exchange_name.to_string())
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

async fn setup_rabbitmq_consumer(amqp_addr: &str, exchange_name: &str, queue_name: &str) -> anyhow::Result<lapin::Channel> {
    let connection = Connection::connect(amqp_addr, ConnectionProperties::default()).await?;
    let channel = connection.create_channel().await?;

    // Declare exchange
    channel.exchange_declare(
        exchange_name,
        lapin::ExchangeKind::Topic,
        ExchangeDeclareOptions::default(),
        FieldTable::default(),
    ).await?;

    // Declare queue
    channel.queue_declare(
        queue_name,
        QueueDeclareOptions::default(),
        FieldTable::default(),
    ).await?;

    // Bind queue to exchange
    channel.queue_bind(
        queue_name,
        exchange_name,
        "artifact.uploaded",
        QueueBindOptions::default(),
        FieldTable::default(),
    ).await?;

    Ok(channel)
}

#[tokio::test]
async fn test_rabbitmq_event_publishing_on_upload() {
    // Arrange
    let (repo, storage, publisher, s3_client, bucket_name, exchange_name) = setup_dependencies().await;
    let user_id = UserId::new();
    let repo_id = RepositoryId::new();
    let cmd = create_test_command(repo_id.clone(), user_id.clone());

    // Setup RabbitMQ consumer
    let docker = clients::Cli::default();
    let rabbitmq_container = docker.run(RabbitMq::default());
    let amqp_addr = format!("amqp://localhost:{}", rabbitmq_container.get_host_port_ipv4(5672));
    let queue_name = "test_artifact_events";
    let channel = setup_rabbitmq_consumer(&amqp_addr, &exchange_name, queue_name).await.unwrap();

    // Act
    let result = handle(&repo, &storage, &publisher, cmd).await;

    // Assert - Upload should succeed
    assert!(result.is_ok());
    let artifact_result = result.unwrap();

    // Verify event was published to RabbitMQ
    let consumer = channel.basic_consume(
        queue_name,
        "test_consumer",
        BasicConsumeOptions::default(),
        FieldTable::default(),
    ).await.unwrap();

    // Wait for message with timeout
    let message_result = timeout(Duration::from_secs(5), consumer.into_stream().next()).await;
    assert!(message_result.is_ok());
    
    if let Some(Ok((_channel, delivery))) = message_result.unwrap() {
        let message_content = String::from_utf8_lossy(&delivery.data);
        
        // Verify message contains expected event data
        assert!(message_content.contains(&artifact_result.artifact_id.to_string()));
        assert!(message_content.contains(&repo_id.to_string()));
        assert!(message_content.contains(&user_id.to_string()));
        
        // Acknowledge message
        delivery.ack(BasicAckOptions::default()).await.unwrap();
    } else {
        panic!("No message received from RabbitMQ");
    }
}

#[tokio::test]
async fn test_rabbitmq_event_content_structure() {
    // Arrange
    let (repo, storage, publisher, _, _, exchange_name) = setup_dependencies().await;
    let user_id = UserId::new();
    let repo_id = RepositoryId::new();
    let cmd = create_test_command(repo_id.clone(), user_id.clone());

    // Setup RabbitMQ consumer
    let docker = clients::Cli::default();
    let rabbitmq_container = docker.run(RabbitMq::default());
    let amqp_addr = format!("amqp://localhost:{}", rabbitmq_container.get_host_port_ipv4(5672));
    let queue_name = "test_event_structure";
    let channel = setup_rabbitmq_consumer(&amqp_addr, &exchange_name, queue_name).await.unwrap();

    // Act
    let result = handle(&repo, &storage, &publisher, cmd).await;
    assert!(result.is_ok());
    let artifact_result = result.unwrap();

    // Verify event structure
    let consumer = channel.basic_consume(
        queue_name,
        "structure_consumer",
        BasicConsumeOptions::default(),
        FieldTable::default(),
    ).await.unwrap();

    let message_result = timeout(Duration::from_secs(5), consumer.into_stream().next()).await.unwrap();
    assert!(message_result.is_some());
    
    let (_, delivery) = message_result.unwrap().unwrap();
    let message_content = String::from_utf8_lossy(&delivery.data);
    
    // Parse JSON and verify structure
    let event_data: serde_json::Value = serde_json::from_str(&message_content).unwrap();
    
    assert_eq!(event_data["event_type"], "ArtifactUploaded");
    assert_eq!(event_data["artifact_id"], artifact_result.artifact_id.to_string());
    assert_eq!(event_data["repository_id"], repo_id.to_string());
    assert_eq!(event_data["user_id"], user_id.to_string());
    assert!(event_data["timestamp"].is_string());
    assert!(event_data["version"].is_string());
    assert!(event_data["file_name"].is_string());
    assert!(event_data["size_bytes"].is_number());
    
    delivery.ack(BasicAckOptions::default()).await.unwrap();
}

#[tokio::test]
async fn test_rabbitmq_event_routing_key() {
    // Arrange
    let (repo, storage, publisher, _, _, exchange_name) = setup_dependencies().await;
    let user_id = UserId::new();
    let repo_id = RepositoryId::new();
    let cmd = create_test_command(repo_id, user_id);

    // Setup RabbitMQ consumer with specific routing key
    let docker = clients::Cli::default();
    let rabbitmq_container = docker.run(RabbitMq::default());
    let amqp_addr = format!("amqp://localhost:{}", rabbitmq_container.get_host_port_ipv4(5672));
    let queue_name = "test_routing_key";
    let channel = setup_rabbitmq_consumer(&amqp_addr, &exchange_name, queue_name).await.unwrap();

    // Act
    let result = handle(&repo, &storage, &publisher, cmd).await;
    assert!(result.is_ok());

    // Verify routing key
    let consumer = channel.basic_consume(
        queue_name,
        "routing_consumer",
        BasicConsumeOptions::default(),
        FieldTable::default(),
    ).await.unwrap();

    let message_result = timeout(Duration::from_secs(5), consumer.into_stream().next()).await.unwrap();
    assert!(message_result.is_some());
    
    let (_, delivery) = message_result.unwrap().unwrap();
    
    // Verify routing key is "artifact.uploaded"
    assert_eq!(delivery.routing_key.as_str(), "artifact.uploaded");
    
    delivery.ack(BasicAckOptions::default()).await.unwrap();
}

#[tokio::test]
async fn test_concurrent_uploads_publish_events_correctly() {
    // Arrange
    let (repo, storage, publisher, _, _, exchange_name) = setup_dependencies().await;
    let repo_id = RepositoryId::new();

    // Setup RabbitMQ consumer
    let docker = clients::Cli::default();
    let rabbitmq_container = docker.run(RabbitMq::default());
    let amqp_addr = format!("amqp://localhost:{}", rabbitmq_container.get_host_port_ipv4(5672));
    let queue_name = "test_concurrent_events";
    let channel = setup_rabbitmq_consumer(&amqp_addr, &exchange_name, queue_name).await.unwrap();

    // Act - Execute multiple concurrent uploads
    let mut handles = vec![];
    for i in 0..3 {
        let repo_clone = repo.clone();
        let storage_clone = storage.clone();
        let publisher_clone = publisher.clone();
        let cmd = UploadArtifactCommand {
            repository_id: repo_id.clone(),
            version: ArtifactVersion(format!("1.0.{}", i)),
            file_name: format!("test{}.txt", i),
            size_bytes: 10 + i as u64,
            checksum: ArtifactChecksum(format!("checksum{}", i)),
            user_id: UserId::new(),
            bytes: format!("content{}", i).into_bytes(),
        };

        handles.push(tokio::spawn(async move {
            handle(&repo_clone, &storage_clone, &publisher_clone, cmd).await
        }));
    }

    // Wait for all uploads to complete
    let results = futures::future::join_all(handles).await;
    for result in results {
        assert!(result.is_ok());
        assert!(result.unwrap().is_ok());
    }

    // Assert - Verify all events were published
    let consumer = channel.basic_consume(
        queue_name,
        "concurrent_consumer",
        BasicConsumeOptions::default(),
        FieldTable::default(),
    ).await.unwrap();

    let mut received_messages = 0;
    let mut message_stream = consumer.into_stream();
    
    // Collect messages with timeout
    while let Ok(Some(Ok((_channel, delivery)))) = timeout(Duration::from_secs(3), message_stream.next()).await {
        received_messages += 1;
        delivery.ack(BasicAckOptions::default()).await.unwrap();
        
        if received_messages >= 3 {
            break;
        }
    }

    assert_eq!(received_messages, 3, "Expected 3 events, got {}", received_messages);
}

#[tokio::test]
async fn test_rabbitmq_event_publishing_failure_handling() {
    // Arrange
    let (repo, storage, _, s3_client, bucket_name, _) = setup_dependencies().await;
    let user_id = UserId::new();
    let repo_id = RepositoryId::new();
    let cmd = create_test_command(repo_id, user_id);

    // Create publisher with invalid RabbitMQ connection
    let invalid_amqp_addr = "amqp://localhost:9999"; // Non-existent port
    let publisher = RabbitMqArtifactEventPublisher::new(invalid_amqp_addr, "test_exchange").await;

    // Act - Should fail due to RabbitMQ connection failure
    let result = handle(&repo, &storage, &publisher.unwrap(), cmd).await;

    // Assert - Operation should fail completely (transactional rollback)
    assert!(result.is_err());
    
    // Verify rollback: no artifact in MongoDB
    if let Ok(artifact_result) = result {
        let get_result = repo.get(&artifact_result.artifact_id).await;
        assert!(get_result.is_err() || get_result.unwrap().is_none());
    }
    
    // Verify rollback: no file in S3
    let list_objects = s3_client.list_objects_v2().bucket(&bucket_name).send().await;
    assert!(list_objects.is_ok());
    let objects = list_objects.unwrap();
    assert!(objects.contents().is_none() || objects.contents().unwrap().is_empty());
}