#![cfg(feature = "integration-rabbitmq")]

use std::sync::Arc;
use distribution::{
    features::{
        maven::upload::handler::handle_maven_upload,
        npm::package_meta::publish_handler::{handle_npm_publish, create_npm_publish_request},
    },
    error::DistributionError,
};
use shared::{RepositoryId, UserId};
use shared_test::setup_test_environment;
use testcontainers::clients;
use testcontainers_modules::rabbitmq::RabbitMq;
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

#[tokio::test]
async fn it_maven_upload_publishes_to_rabbitmq() {
    // Setup test environment with RabbitMQ
    let docker = clients::Cli::default();
    let rabbitmq_container = docker.run(RabbitMq::default());
    let amqp_addr = format!("amqp://localhost:{}", rabbitmq_container.get_host_port_ipv4(5672));
    
    // Set RabbitMQ as the event broker
    std::env::set_var("EVENT_BROKER_TYPE", "rabbitmq");
    std::env::set_var("AMQP_ADDR", &amqp_addr);
    
    let env = setup_test_environment(None).await;
    let consumer = setup_rabbitmq_consumer(&amqp_addr, "maven_test_queue").await;
    
    let repository_id = RepositoryId::new();
    let user_id = UserId::new();
    
    // Upload Maven artifact
    let upload_result = handle_maven_upload(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        env.artifact_event_publisher.clone(),
        env.authorization.clone(),
        repository_id.clone(),
        "com.example".to_string(),
        "test-artifact".to_string(),
        "1.0.0".to_string(),
        "test-artifact-1.0.0.jar".to_string(),
        vec![1, 2, 3, 4, 5],
    ).await;
    
    assert!(upload_result.is_ok());
    
    // Verify event was published to RabbitMQ
    sleep(Duration::from_secs(2)).await;
    
    let delivery = consumer.next().await.unwrap().unwrap();
    let payload = String::from_utf8(delivery.data.to_vec()).unwrap();
    
    // Verify the event contains the expected routing key and payload structure
    assert_eq!(delivery.routing_key, "artifact.uploaded");
    assert!(payload.contains("test-artifact"));
    assert!(payload.contains("1.0.0"));
}

#[tokio::test]
async fn it_npm_publish_publishes_to_rabbitmq() {
    // Setup test environment with RabbitMQ
    let docker = clients::Cli::default();
    let rabbitmq_container = docker.run(RabbitMq::default());
    let amqp_addr = format!("amqp://localhost:{}", rabbitmq_container.get_host_port_ipv4(5672));
    
    // Set RabbitMQ as the event broker
    std::env::set_var("EVENT_BROKER_TYPE", "rabbitmq");
    std::env::set_var("AMQP_ADDR", &amqp_addr);
    
    let env = setup_test_environment(None).await;
    let consumer = setup_rabbitmq_consumer(&amqp_addr, "npm_test_queue").await;
    
    let repository_id = RepositoryId::new();
    
    // Create and publish npm package
    let package_name = "test-npm-package".to_string();
    let version = "1.0.0".to_string();
    let tarball_data = vec![1, 2, 3, 4, 5];
    let request = create_npm_publish_request(&package_name, &version, &tarball_data);
    let bytes = serde_json::to_vec(&request).unwrap();
    
    let publish_result = handle_npm_publish(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        env.artifact_event_publisher.clone(),
        env.authorization.clone(),
        repository_id,
        package_name.clone(),
        bytes,
    ).await;
    
    assert!(publish_result.is_ok());
    
    // Verify event was published to RabbitMQ
    sleep(Duration::from_secs(2)).await;
    
    let delivery = consumer.next().await.unwrap().unwrap();
    let payload = String::from_utf8(delivery.data.to_vec()).unwrap();
    
    // Verify the event contains the expected routing key and payload structure
    assert_eq!(delivery.routing_key, "artifact.uploaded");
    assert!(payload.contains("test-npm-package"));
    assert!(payload.contains("1.0.0"));
}

#[tokio::test]
async fn it_distribution_events_contain_correct_metadata() {
    // Setup test environment with RabbitMQ
    let docker = clients::Cli::default();
    let rabbitmq_container = docker.run(RabbitMq::default());
    let amqp_addr = format!("amqp://localhost:{}", rabbitmq_container.get_host_port_ipv4(5672));
    
    std::env::set_var("EVENT_BROKER_TYPE", "rabbitmq");
    std::env::set_var("AMQP_ADDR", &amqp_addr);
    
    let env = setup_test_environment(None).await;
    let consumer = setup_rabbitmq_consumer(&amqp_addr, "metadata_test_queue").await;
    
    let repository_id = RepositoryId::new();
    
    // Test Maven upload with specific metadata
    let upload_result = handle_maven_upload(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        env.artifact_event_publisher.clone(),
        env.authorization.clone(),
        repository_id.clone(),
        "com.example.metadata".to_string(),
        "metadata-artifact".to_string(),
        "2.1.0".to_string(),
        "metadata-artifact-2.1.0.jar".to_string(),
        vec![6, 7, 8, 9, 10],
    ).await;
    
    assert!(upload_result.is_ok());
    
    sleep(Duration::from_secs(2)).await;
    
    let delivery = consumer.next().await.unwrap().unwrap();
    let payload = String::from_utf8(delivery.data.to_vec()).unwrap();
    
    // Verify specific metadata in the event
    assert!(payload.contains("com.example.metadata"));
    assert!(payload.contains("metadata-artifact"));
    assert!(payload.contains("2.1.0"));
    assert!(payload.contains("jar")); // Should contain file extension info
}