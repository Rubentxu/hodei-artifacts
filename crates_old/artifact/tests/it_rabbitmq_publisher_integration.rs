#![cfg(feature = "integration-rabbitmq")]

use artifact::application::ports::ArtifactEventPublisher;
use artifact::domain::model::{Artifact, ArtifactChecksum, ArtifactVersion};
use artifact::infrastructure::messaging::RabbitMqArtifactEventPublisher;
use shared::{RepositoryId, UserId, ArtifactId};
use testcontainers::clients;
use testcontainers_modules::rabbitmq::RabbitMq;
use tokio::time::{sleep, Duration};
use lapin::{Connection, ConnectionProperties};
use lapin::options::{BasicConsumeOptions, QueueDeclareOptions};
use lapin::types::FieldTable;

async fn setup_rabbitmq() -> (RabbitMqArtifactEventPublisher, String, Container<'static, RabbitMq>) {
    let docker = clients::Cli::default();
    let rabbitmq_node = docker.run(RabbitMq::default());
    let amqp_addr = format!("amqp://localhost:{}", rabbitmq_node.get_host_port_ipv4(5672));
    let publisher = RabbitMqArtifactEventPublisher::new(&amqp_addr, "hodei_artifacts_exchange").await.unwrap();
    (publisher, amqp_addr, rabbitmq_node)
}

async fn setup_consumer(amqp_addr: &str, queue_name: &str) -> lapin::Consumer {
    let connection = Connection::connect(amqp_addr, ConnectionProperties::default())
        .await
        .unwrap();
    let channel = connection.create_channel().await.unwrap();
    
    // Declare exchange
    channel.exchange_declare(
        "hodei_artifacts_exchange",
        lapin::ExchangeKind::Direct,
        lapin::options::ExchangeDeclareOptions::default(),
        FieldTable::default(),
    ).await.unwrap();
    
    // Declare queue and bind to exchange
    let queue = channel.queue_declare(
        queue_name,
        QueueDeclareOptions::default(),
        FieldTable::default(),
    ).await.unwrap();
    
    channel.queue_bind(
        queue_name,
        "hodei_artifacts_exchange",
        "artifact.uploaded",
        FieldTable::default(),
    ).await.unwrap();
    
    channel.basic_consume(
        queue_name,
        "test-consumer",
        BasicConsumeOptions::default(),
        FieldTable::default(),
    ).await.unwrap()
}

#[tokio::test]
async fn it_should_publish_artifact_uploaded_event_to_rabbitmq() {
    // Arrange
    let (publisher, amqp_addr, _rabbitmq_node) = setup_rabbitmq().await;
    let consumer = setup_consumer(&amqp_addr, "test_queue").await;
    
    let artifact = Artifact::new(
        RepositoryId::new(),
        ArtifactVersion::new("1.0.0".to_string()),
        "test.txt".to_string(),
        123,
        ArtifactChecksum::new("sha256:123".to_string()),
        UserId::new(),
    );

    // Act
    let result = publisher.publish_created(&artifact).await;

    // Assert
    assert!(result.is_ok());

    // Verify the message was actually sent and received
    sleep(Duration::from_secs(1)).await; // Give it a moment to propagate

    let delivery = consumer.next().await.unwrap().unwrap();
    let payload = String::from_utf8(delivery.data.to_vec()).unwrap();
    let received_artifact: Artifact = serde_json::from_str(&payload).unwrap();

    assert_eq!(artifact.id, received_artifact.id);
    assert_eq!(delivery.routing_key, "artifact.uploaded");
}

#[tokio::test] 
async fn it_should_handle_rabbitmq_connection_failures_gracefully() {
    // Arrange - Use invalid connection string
    let invalid_addr = "amqp://localhost:9999";
    
    // Act & Assert - Should return error rather than panic
    let result = RabbitMqArtifactEventPublisher::new(invalid_addr, "test_exchange").await;
    assert!(result.is_err());
}