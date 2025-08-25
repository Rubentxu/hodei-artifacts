#![cfg(feature = "integration-mongo")]

use artifact::application::ports::ArtifactEventPublisher;
use artifact::domain::model::{Artifact, ArtifactChecksum, ArtifactVersion};
use artifact::infrastructure::messaging::KafkaArtifactEventPublisher;
use shared::{RepositoryId, UserId, ArtifactId};
use testcontainers::clients;
use testcontainers_modules::kafka::Kafka;
use tokio::time::{sleep, Duration};
use rdkafka::consumer::{BaseConsumer, Consumer};
use testcontainers::core::Container;

async fn setup_kafka() -> (KafkaArtifactEventPublisher, String, Container<'static, Kafka>) {
    let docker = clients::Cli::default();
    let kafka_node = docker.run(Kafka::default());
    let bootstrap_servers = format!("localhost:{}", kafka_node.get_host_port_ipv4(9093));
    let publisher = KafkaArtifactEventPublisher::new(&bootstrap_servers).unwrap();
    (publisher, bootstrap_servers, kafka_node)
}

#[tokio::test]
async fn it_should_publish_artifact_created_event() {
    // Arrange
    let (publisher, bootstrap_servers, _kafka_node) = setup_kafka().await;
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

    // Verify the message was actually sent
    let consumer: BaseConsumer = rdkafka::ClientConfig::new()
        .set("bootstrap.servers", &bootstrap_servers)
        .set("group.id", "test-group")
        .set("auto.offset.reset", "earliest")
        .create()
        .unwrap();

    consumer.subscribe(&["artifact_created"]).unwrap();

    sleep(Duration::from_secs(1)).await; // Give it a moment to propagate

    let message = consumer.poll(Duration::from_secs(5)).unwrap().unwrap();
    let payload = String::from_utf8(message.payload().unwrap().to_vec()).unwrap();
    let received_artifact: Artifact = serde_json::from_str(&payload).unwrap();

    assert_eq!(artifact.id, received_artifact.id);
}
