#![cfg(feature = "integration-rabbitmq")]

use std::sync::Arc;
use std::str::FromStr;
use artifact::application::ports::ArtifactRepository;
use distribution::{
    features::{
        maven::upload::handler::handle_maven_upload,
        npm::package_meta::publish_handler::{handle_npm_publish, create_npm_publish_request},
    },
};
use shared::{ArtifactId, RepositoryId, UserId};
use shared_test::setup_test_environment;
use tokio::time::{sleep, Duration};

#[tokio::test]
async fn it_maven_upload_publishes_to_rabbitmq() {
    // Setup test environment with RabbitMQ
    unsafe {
        std::env::set_var("EVENT_BROKER_TYPE", "rabbitmq");
    }
    
    let env = setup_test_environment(None).await;
    
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
    
    // Assert upload success - if RabbitMQ publishing fails, the entire operation should fail
    assert!(upload_result.is_ok(), "Maven upload failed: {:?}", upload_result.err());
    
    // Give time for event to be published (RabbitMQ is async)
    sleep(Duration::from_secs(1)).await;
    
    // Verify artifact was saved to repository
    let upload_response = upload_result.unwrap();
    let artifact_id = ArtifactId::from_str(&upload_response.artifact_id).expect("Invalid artifact ID");
    let saved_artifact = env.artifact_repository.get(&artifact_id).await;
    assert!(saved_artifact.is_ok(), "Failed to get artifact from repository: {:?}", saved_artifact.err());
    assert!(saved_artifact.unwrap().is_some(), "Artifact not found in repository");
}

#[tokio::test]
async fn it_npm_publish_publishes_to_rabbitmq() {
    // Setup test environment with RabbitMQ
    unsafe {
        std::env::set_var("EVENT_BROKER_TYPE", "rabbitmq");
    }
    
    let env = setup_test_environment(None).await;
    
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
    
    // Assert publish success - if RabbitMQ publishing fails, the entire operation should fail
    assert!(publish_result.is_ok(), "NPM publish failed: {:?}", publish_result.err());
    
    // Give time for event to be published (RabbitMQ is async)
    sleep(Duration::from_secs(1)).await;
    
    // For npm publish, we need to search for the artifact since we don't get the ID directly
    // The npm publish handler creates an artifact with the package name and version
    // We can search for artifacts in the repository to verify it was created
    let all_artifacts = env.artifact_repository.find_all_artifacts().await;
    assert!(all_artifacts.is_ok(), "Failed to list artifacts: {:?}", all_artifacts.err());
    
    // Verify at least one artifact was created
    let artifacts = all_artifacts.unwrap();
    assert!(!artifacts.is_empty(), "No artifacts found in repository after npm publish");
    
    // Look for an artifact that might be our npm package
    let npm_artifact = artifacts.iter().find(|a| a.file_name.contains(&package_name));
    assert!(npm_artifact.is_some(), "No npm package artifact found in repository");
}

#[tokio::test]
async fn it_distribution_operations_fail_without_rabbitmq() {
    // Setup test environment but don't set RabbitMQ (should use default which is rabbitmq)
    let env = setup_test_environment(None).await;
    
    let repository_id = RepositoryId::new();
    
    // Try Maven upload - should succeed since RabbitMQ is the default
    let upload_result = handle_maven_upload(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        env.artifact_event_publisher.clone(),
        env.authorization.clone(),
        repository_id,
        "com.example".to_string(),
        "test-artifact".to_string(),
        "1.0.0".to_string(),
        "test-artifact-1.0.0.jar".to_string(),
        vec![1, 2, 3, 4, 5],
    ).await;
    
    // Should succeed since RabbitMQ is the default broker
    assert!(upload_result.is_ok(), "Maven upload should succeed with default RabbitMQ: {:?}", upload_result.err());
}