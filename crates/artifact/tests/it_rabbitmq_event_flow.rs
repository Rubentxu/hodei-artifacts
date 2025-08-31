#![cfg(feature = "integration-rabbitmq")]

use std::sync::Arc;
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
use lapin::options::{BasicConsumeOptions, QueueDeclareOptions};
use lapin::types::FieldTable;
use tokio::time::{sleep, Duration};

async fn setup_rabbitmq_consumer(amqp_addr: &str, queue_name: &str) -> lapin::Consumer {
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

async fn setup_dependencies_with_rabbitmq() -> (
    MongoArtifactRepository,
    S3ArtifactStorage,
    RabbitMqArtifactEventPublisher,
    S3Client,
    String,
    String,
    lapin::Consumer,
) {
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
    
    // Setup consumer for verification
    let consumer = setup_rabbitmq_consumer(&amqp_addr, "test_upload_queue").await;

    (artifact_repo, artifact_storage, event_publisher, s3_client, bucket_name.to_string(), amqp_addr, consumer)
}

fn create_dummy_command(repo_id: RepositoryId, user_id: UserId) -> UploadArtifactCommand {
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
async fn test_upload_artifact_publishes_to_rabbitmq() {
    // Arrange
    let (repo, storage, publisher, s3_client, bucket_name, amqp_addr, consumer) = setup_dependencies_with_rabbitmq().await;
    let user_id = UserId::new();
    let repo_id = RepositoryId::new();
    let cmd = create_dummy_command(repo_id, user_id);

    // Act
    let result = handle(&repo, &storage, &publisher, cmd).await;

    // Assert upload success
    assert!(result.is_ok());
    let artifact_id = result.unwrap().artifact_id;

    // Verify artifact was saved
    let saved_artifact = repo.get(&artifact_id).await.unwrap().unwrap();
    assert_eq!(saved_artifact.id, artifact_id);

    // Verify event was published to RabbitMQ
    sleep(Duration::from_secs(2)).await; // Give time for message to be published

    let delivery = consumer.next().await.unwrap().unwrap();
    let payload = String::from_utf8(delivery.data.to_vec()).unwrap();
    let event_artifact: Artifact = serde_json::from_str(&payload).unwrap();

    assert_eq!(event_artifact.id, artifact_id);
    assert_eq!(delivery.routing_key, "artifact.uploaded");
}

#[tokio::test]
async fn test_rabbitmq_event_contains_correct_metadata() {
    // Arrange
    let (repo, storage, publisher, _, _, amqp_addr, consumer) = setup_dependencies_with_rabbitmq().await;
    let user_id = UserId::new();
    let repo_id = RepositoryId::new();
    let cmd = create_dummy_command(repo_id, user_id);

    // Act
    let result = handle(&repo, &storage, &publisher, cmd).await.unwrap();

    // Wait for event
    sleep(Duration::from_secs(2)).await;

    // Assert event content
    let delivery = consumer.next().await.unwrap().unwrap();
    let payload = String::from_utf8(delivery.data.to_vec()).unwrap();
    let event_artifact: Artifact = serde_json::from_str(&payload).unwrap();

    assert_eq!(event_artifact.repository_id, repo_id);
    assert_eq!(event_artifact.created_by, user_id);
    assert_eq!(event_artifact.file_name, "test.txt");
    assert_eq!(event_artifact.version.0, "1.0.0");
}