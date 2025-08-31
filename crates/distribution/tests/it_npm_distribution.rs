use shared_test::setup_test_environment;
use distribution::features::npm::tarball::handler::handle_npm_tarball_download;
use distribution::features::npm::package_meta::handler::handle_npm_package_meta;
use distribution::features::npm::package_meta::publish_handler::{handle_npm_publish, create_npm_publish_request};
use shared::RepositoryId;

#[tokio::test]
async fn it_npm_tarball_download() {
    let env = setup_test_environment(None).await;
    
    let repository_id = RepositoryId::new();
    let package_name = "test-package".to_string();
    let version = "1.0.0".to_string();
    
    // Create and upload a test npm package
    let tarball_data = vec![1, 2, 3, 4, 5]; // Simple test tarball
    let request = create_npm_publish_request(&package_name, &version, &tarball_data);
    let bytes = serde_json::to_vec(&request).unwrap();
    
    // Publish the package
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
    
    // Now test tarball download
    let file_name = format!("{}-{}.tgz", package_name, version);
    let download_result = handle_npm_tarball_download(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        env.authorization.clone(),
        package_name.clone(),
        file_name,
    ).await;
    
    assert!(download_result.is_ok());
    let downloaded_bytes = download_result.unwrap();
    assert_eq!(downloaded_bytes, tarball_data);
}

#[tokio::test]
async fn it_npm_package_metadata_retrieval() {
    let env = setup_test_environment(None).await;
    
    let repository_id = RepositoryId::new();
    let package_name = "test-package".to_string();
    let version = "1.0.0".to_string();
    
    // Create and upload a test npm package
    let tarball_data = vec![1, 2, 3, 4, 5];
    let request = create_npm_publish_request(&package_name, &version, &tarball_data);
    let bytes = serde_json::to_vec(&request).unwrap();
    
    // Publish the package
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
    
    // Test metadata retrieval
    let metadata_result = handle_npm_package_meta(
        env.artifact_repository.clone(),
        env.authorization.clone(),
        package_name.clone(),
    ).await;
    
    assert!(metadata_result.is_ok());
    let metadata = metadata_result.unwrap();
    assert_eq!(metadata.name, package_name);
    assert!(metadata.versions.contains_key(&version));
    
    let version_metadata = &metadata.versions[&version];
    assert_eq!(version_metadata.version, version);
    assert!(version_metadata.dist.tarball.contains(&package_name));
}

#[tokio::test]
async fn it_npm_nonexistent_package_metadata() {
    let env = setup_test_environment(None).await;
    
    let _repository_id = RepositoryId::new();
    
    // Try to get metadata for non-existent package
    let result = handle_npm_package_meta(
        env.artifact_repository.clone(),
        env.authorization.clone(),
        "nonexistent-package".to_string(),
    ).await;
    
    // Should return NotFound error for nonexistent packages
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, distribution::error::DistributionError::NotFound));
}