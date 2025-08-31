use shared_test::setup_test_environment;
use distribution::features::npm::tarball::upload::handler::{handle_npm_tarball_upload, NpmTarballUploadRequest};
use shared::{RepositoryId, UserId};
use artifact::application::ports::ArtifactRepository;

#[tokio::test]
async fn it_npm_tarball_upload_basic() {
    let env = setup_test_environment(None).await;
    
    let repository_id = RepositoryId::new();
    let user_id = UserId::new();
    let package_name = "test-package".to_string();
    let version = "1.0.0".to_string();
    
    // Create a simple test tarball (in real scenario this would be a real .tgz file)
    let tarball_data = create_test_tarball(&package_name, &version);
    
    let request = NpmTarballUploadRequest {
        repository_id: repository_id.clone(),
        user_id: user_id.clone(),
        package_name: package_name.clone(),
        version: version.clone(),
        tarball_data,
        user_agent: Some("test-agent/1.0".to_string()),
        client_ip: Some("127.0.0.1".to_string()),
    };
    
    // Upload the tarball
    let result = handle_npm_tarball_upload(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        env.artifact_event_publisher.clone(),
        env.authorization.clone(),
        request,
    ).await;
    
    assert!(result.is_ok(), "Tarball upload failed: {:?}", result.err());
    
    let artifact = result.unwrap();
    
    // Verify artifact metadata
    assert_eq!(artifact.file_name, format!("{}-{}.tgz", package_name, version));
    assert_eq!(artifact.repository_id, repository_id);
    assert_eq!(artifact.created_by, user_id);
    assert!(artifact.size_bytes > 0);
    assert!(!artifact.checksum.0.is_empty());
    
    // Verify artifact can be retrieved from repository
    let found_artifact = env.artifact_repository.get(&artifact.id)
        .await
        .expect("Failed to retrieve artifact")
        .expect("Artifact not found in repository");
    
    assert_eq!(found_artifact.id, artifact.id);
    assert_eq!(found_artifact.file_name, artifact.file_name);
}

#[tokio::test]
async fn it_npm_tarball_upload_duplicate_prevention() {
    let env = setup_test_environment(None).await;
    
    let repository_id = RepositoryId::new();
    let user_id = UserId::new();
    let package_name = "test-package".to_string();
    let version = "1.0.0".to_string();
    
    let tarball_data = create_test_tarball(&package_name, &version);
    
    let request = NpmTarballUploadRequest {
        repository_id: repository_id.clone(),
        user_id: user_id.clone(),
        package_name: package_name.clone(),
        version: version.clone(),
        tarball_data: tarball_data.clone(),
        user_agent: None,
        client_ip: None,
    };
    
    // First upload should succeed
    let result1 = handle_npm_tarball_upload(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        env.artifact_event_publisher.clone(),
        env.authorization.clone(),
        request,
    ).await;
    
    assert!(result1.is_ok(), "First upload failed: {:?}", result1.err());
    
    // Second upload with same data should be handled by idempotency mechanism
    // (either succeed with same artifact ID or fail with appropriate error)
    let request2 = NpmTarballUploadRequest {
        repository_id: repository_id.clone(),
        user_id: user_id.clone(),
        package_name: package_name.clone(),
        version: version.clone(),
        tarball_data: tarball_data.clone(),
        user_agent: None,
        client_ip: None,
    };
    
    let result2 = handle_npm_tarball_upload(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        env.artifact_event_publisher.clone(),
        env.authorization.clone(),
        request2,
    ).await;
    
    // Depending on idempotency implementation, this could either:
    // 1. Succeed and return the same artifact (idempotent)
    // 2. Fail with a duplicate error
    // Both are acceptable behaviors
    assert!(result2.is_ok() || matches!(result2.err(), Some(distribution::error::DistributionError::Artifact(artifact::error::ArtifactError::Duplicate))));
}

#[tokio::test]
async fn it_npm_tarball_upload_version_mismatch() {
    let env = setup_test_environment(None).await;
    
    let repository_id = RepositoryId::new();
    let user_id = UserId::new();
    let package_name = "test-package".to_string();
    
    // Create tarball with version 1.0.0 but request version 2.0.0
    let tarball_data = create_test_tarball(&package_name, "1.0.0");
    
    let request = NpmTarballUploadRequest {
        repository_id: repository_id.clone(),
        user_id: user_id.clone(),
        package_name: package_name.clone(),
        version: "2.0.0".to_string(), // Different from tarball contents
        tarball_data,
        user_agent: None,
        client_ip: None,
    };
    
    let result = handle_npm_tarball_upload(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        env.artifact_event_publisher.clone(),
        env.authorization.clone(),
        request,
    ).await;
    
    assert!(result.is_err(), "Should fail with version mismatch");
    
    let error = result.unwrap_err();
    assert!(matches!(error, distribution::error::DistributionError::InvalidNpmPackage(msg) if msg.contains("version")));
}

#[tokio::test] 
async fn it_npm_tarball_upload_invalid_tarball() {
    let env = setup_test_environment(None).await;
    
    let repository_id = RepositoryId::new();
    let user_id = UserId::new();
    let package_name = "test-package".to_string();
    let version = "1.0.0".to_string();
    
    // Invalid tarball data
    let invalid_tarball_data = b"not a valid gzip file".to_vec();
    
    let request = NpmTarballUploadRequest {
        repository_id: repository_id.clone(),
        user_id: user_id.clone(),
        package_name: package_name.clone(),
        version: version.clone(),
        tarball_data: invalid_tarball_data,
        user_agent: None,
        client_ip: None,
    };
    
    let result = handle_npm_tarball_upload(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        env.artifact_event_publisher.clone(),
        env.authorization.clone(),
        request,
    ).await;
    
    assert!(result.is_err(), "Should fail with invalid tarball");
    assert!(matches!(result.unwrap_err(), distribution::error::DistributionError::InvalidNpmPackage(_)));
}

// Helper function to create a simple test tarball
fn create_test_tarball(package_name: &str, version: &str) -> Vec<u8> {
    // In a real test, we would create an actual .tgz file
    // For now, return some test data that represents a tarball
    // This would be replaced with actual tar + gzip creation in a real implementation
    
    let package_json = format!(r#"{{
        "name": "{}",
        "version": "{}",
        "description": "Test package for integration tests"
    }"#, package_name, version);
    
    // Simulate tarball data (in real scenario, this would be proper gzipped tar)
    package_json.into_bytes()
}