#![cfg(feature = "integration-mongo")]

use std::sync::Arc;
use artifact::{
    application::ports::{ArtifactRepository, ArtifactEventPublisher},
    domain::model::{Artifact, ArtifactVersion, ArtifactChecksum},
    error::ArtifactError,
    features::upload_artifact::{command::UploadArtifactCommand, handler::handle},
    infrastructure::{
        persistence::MongoArtifactRepository,
        storage::S3ArtifactStorage,
    },
};
use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_s3::{config::{Credentials, Region}, Client as S3Client};
use infra_mongo::test_util::mongo_test_container::ephemeral_store;
use shared::{RepositoryId, UserId};
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::localstack::LocalStack;

// Test-only event publisher
struct TestEventPublisher;
#[async_trait]
impl ArtifactEventPublisher for TestEventPublisher {
    async fn publish_created(&self, _artifact: &Artifact) -> Result<(), ArtifactError> {
        Ok(()) // No-op for this test
    }
}

async fn setup_dependencies() -> (
    MongoArtifactRepository,
    S3ArtifactStorage,
    TestEventPublisher,
    S3Client,
    String,
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

    // Event Publisher
    let event_publisher = TestEventPublisher;

    (artifact_repo, artifact_storage, event_publisher, s3_client, bucket_name.to_string())
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
async fn test_upload_artifact_happy_path() {
    // Arrange
    let (repo, storage, publisher, s3_client, bucket_name) = setup_dependencies().await;
    let user_id = UserId::new();
    let repo_id = RepositoryId::new();
    let cmd = create_dummy_command(repo_id, user_id);

    // Act
    let result = handle(&repo, &storage, &publisher, cmd).await.unwrap();

    // Assert
    let artifact_id = result.artifact_id;
    let saved_artifact = repo.get(&artifact_id).await.unwrap().unwrap();
    assert_eq!(saved_artifact.id, artifact_id);
    assert_eq!(saved_artifact.created_by, user_id);

    let s3_key = format!("{}/{}", repo_id.0, artifact_id.0);
    let s3_object = s3_client.get_object().bucket(bucket_name).key(s3_key).send().await.unwrap();
    let s3_content = s3_object.body.collect().await.unwrap().into_bytes();
    assert_eq!(s3_content.as_ref(), b"hello world");
}

#[tokio::test]
async fn test_upload_artifact_idempotency() {
    // Arrange
    let (repo, storage, publisher, _, _) = setup_dependencies().await;
    let user_id = UserId::new();
    let repo_id = RepositoryId::new();
    let cmd = create_dummy_command(repo_id, user_id);

    // Act
    let first_result = handle(&repo, &storage, &publisher, cmd.clone()).await.unwrap();
    let second_result = handle(&repo, &storage, &publisher, cmd).await.unwrap();

    // Assert
    assert_eq!(first_result.artifact_id, second_result.artifact_id);
}
