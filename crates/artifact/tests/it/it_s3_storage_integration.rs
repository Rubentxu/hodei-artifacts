#![cfg(feature = "integration-mongo")]

use testcontainers::runners::AsyncRunner;
use testcontainers_modules::localstack::LocalStack;
use aws_config::BehaviorVersion;
use aws_sdk_s3::{config::{Credentials, Region}, Client as S3Client};
use artifact::{
    application::ports::ArtifactStorage,
    infrastructure::storage::S3ArtifactStorage,
};
use shared::{RepositoryId, ArtifactId};

async fn setup_s3() -> (S3ArtifactStorage, S3Client, String, testcontainers::ContainerAsync<LocalStack>) {
    let container = LocalStack::default().start().await.expect("localstack container");
    let host_port = container.get_host_port_ipv4(4566).await.unwrap();
    let endpoint_url = format!("http://127.0.0.1:{}", host_port);

    let region = Region::new("us-east-1");
    let creds = Credentials::new("test", "test", None, None, "test");

    let s3_config = aws_sdk_s3::Config::builder()
        .region(region)
        .credentials_provider(creds)
        .endpoint_url(endpoint_url)
        .behavior_version(BehaviorVersion::latest())
        .force_path_style(true)
        .build();

    let s3_client = S3Client::from_conf(s3_config);
    let bucket_name = "test-bucket";

    s3_client
        .create_bucket()
        .bucket(bucket_name)
        .send()
        .await
        .expect("Failed to create bucket");

    let storage = S3ArtifactStorage::new(s3_client.clone(), bucket_name.to_string());
    (storage, s3_client, bucket_name.to_string(), container)
}

#[tokio::test]
async fn it_should_put_object_to_s3_bucket() {
    // Arrange
    let (storage, s3_client, bucket_name, _container) = setup_s3().await;
    let repository_id = RepositoryId::new();
    let artifact_id = ArtifactId::new();
    let content = b"hello world";

    // Act
    let result = storage.put_object(&repository_id, &artifact_id, content).await;

    // Assert
    assert!(result.is_ok());

    // Verify object was stored correctly
    let key = format!("{}/{}", repository_id.0, artifact_id.0);
    let response = s3_client
        .get_object()
        .bucket(bucket_name)
        .key(key)
        .send()
        .await
        .expect("Failed to get object from S3");

    let data = response.body.collect().await.unwrap().into_bytes();
    assert_eq!(data.as_ref(), content);
}
