#![cfg(feature = "integration-rabbitmq")]

use std::time::Duration;
use artifact::{
    domain::model::{ArtifactVersion, ArtifactChecksum},
    features::upload_artifact::{command::UploadArtifactCommand, handler::handle},
};
use shared_test::setup_test_environment;
use shared::{RepositoryId, UserId};
use futures_util::{StreamExt, TryStreamExt};
use lapin::{Connection, ConnectionProperties, options::*, types::FieldTable};
use tokio::time::timeout;

async fn setup_rabbitmq_consumer(amqp_addr: &str, exchange_name: &str, queue_name: &str) -> anyhow::Result<lapin::Channel> {
    let connection = Connection::connect(amqp_addr, ConnectionProperties::default()).await?;
    let channel = connection.create_channel().await?;

    // Declare exchange - must match publisher configuration (durable: true)
    channel.exchange_declare(
        exchange_name,
        lapin::ExchangeKind::Topic,
        ExchangeDeclareOptions {
            durable: true,
            ..Default::default()
        },
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

fn create_test_command(repo_id: RepositoryId, user_id: UserId) -> UploadArtifactCommand {
    UploadArtifactCommand {
        repository_id: repo_id,
        version: ArtifactVersion("1.0.0".to_string()),
        file_name: "test.txt".to_string(),
        size_bytes: 11,
        checksum: ArtifactChecksum("d41d8cd98f00b204e9800998ecf8427e".to_string()),
        user_id,
        mime_type: "text/plain".to_string(),
        bytes: b"hello world".to_vec(),
    }
}

#[tokio::test]
async fn test_rabbitmq_event_publishing_on_upload() {
    // Arrange - Use shared test environment
    let test_env = setup_test_environment(None).await;
    let user_id = UserId::new();
    let repo_id = RepositoryId::new();
    let cmd = create_test_command(repo_id.clone(), user_id.clone());

    // Setup RabbitMQ consumer for verification - use dynamic port from test environment
    let rabbitmq_port = test_env.dynamic_ports.as_ref().map(|p| p.rabbitmq_port).unwrap_or(5672);
    let amqp_addr = format!("amqp://localhost:{}", rabbitmq_port);
    let exchange_name = "hodei_artifacts_exchange";
    let queue_name = "test_artifact_events";
    let channel = setup_rabbitmq_consumer(&amqp_addr, exchange_name, queue_name).await.unwrap();

    // Act
    let result = handle(
        &*test_env.artifact_repository, 
        &*test_env.artifact_storage, 
        &*test_env.artifact_event_publisher, 
        cmd
    ).await;

    // Assert - Upload should succeed
    if let Err(e) = &result {
        eprintln!("Upload failed with error: {:?}", e);
    }
    assert!(result.is_ok(), "Upload failed: {:?}", result.err());
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
    
    if let Some(Ok(delivery)) = message_result.unwrap() {
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
    let test_env = setup_test_environment(None).await;
    let user_id = UserId::new();
    let repo_id = RepositoryId::new();
    let cmd = create_test_command(repo_id.clone(), user_id.clone());

    // Setup RabbitMQ consumer - use dynamic port from test environment
    let rabbitmq_port = test_env.dynamic_ports.as_ref().map(|p| p.rabbitmq_port).unwrap_or(5672);
    let amqp_addr = format!("amqp://localhost:{}", rabbitmq_port);
    let exchange_name = "hodei_artifacts_exchange";
    let queue_name = "test_event_structure";
    let channel = setup_rabbitmq_consumer(&amqp_addr, exchange_name, queue_name).await.unwrap();

    // Act
    let result = handle(
        &*test_env.artifact_repository, 
        &*test_env.artifact_storage, 
        &*test_env.artifact_event_publisher, 
        cmd
    ).await;
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
    
    let delivery = message_result.unwrap().unwrap();
    let message_content = String::from_utf8_lossy(&delivery.data);
    
    // Parse JSON and verify structure
    let event_data: serde_json::Value = serde_json::from_str(&message_content).unwrap();
    
    
    assert_eq!(event_data["event_type"], "ArtifactUploaded.v1");
    assert_eq!(event_data["data"]["artifact_id"], artifact_result.artifact_id.to_string());
    assert_eq!(event_data["data"]["repository_id"], repo_id.to_string());
    assert_eq!(event_data["data"]["uploader"], user_id.to_string());
    assert!(event_data["timestamp"].is_string());
    assert!(event_data["version"].is_string());
    assert!(event_data["data"]["size_bytes"].is_number());
    
    delivery.ack(BasicAckOptions::default()).await.unwrap();
}