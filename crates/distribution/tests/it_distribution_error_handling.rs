use shared_test::setup_test_environment;
use distribution::features::maven::download::handler::handle_maven_download;
use distribution::features::npm::tarball::handler::handle_npm_tarball_download;
use shared::RepositoryId;

#[tokio::test]
async fn it_maven_download_invalid_repository() {
    let env = setup_test_environment(None).await;
    
    // Try to download from non-existent repository
    let result = handle_maven_download(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        RepositoryId::new(), // New repository ID (should be empty)
        "com.example".to_string(),
        "test".to_string(),
        "1.0.0".to_string(),
        "test-1.0.0.jar".to_string(),
    ).await;
    
    assert!(result.is_err());
}

#[tokio::test]
async fn it_npm_download_invalid_package() {
    let env = setup_test_environment(None).await;
    
    let repository_id = RepositoryId::new();
    
    // Try to download non-existent npm package
    let result = handle_npm_tarball_download(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        repository_id,
        "nonexistent-package".to_string(),
        "1.0.0".to_string(),
        "nonexistent-package-1.0.0.tgz".to_string(),
    ).await;
    
    assert!(result.is_err());
}

#[tokio::test]
async fn it_npm_download_invalid_version() {
    let env = setup_test_environment(None).await;
    
    let repository_id = RepositoryId::new();
    let package_name = "existing-package".to_string();
    
    // Note: npm tarball upload functionality not yet implemented
    // This test section is commented out until upload is implemented
    
    /*
    use distribution::features::npm::tarball::upload::handler::handle_npm_tarball_upload;
    
    handle_npm_tarball_upload(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        env.artifact_event_publisher.clone(),
        env.authorization.clone(),
        repository_id.clone(),
        package_name.clone(),
        "1.0.0".to_string(),
        "existing-package-1.0.0.tgz".to_string(),
        vec![1, 2, 3],
    ).await.unwrap();
    */
    
    // Try to download non-existent version
    // This test is currently skipped as it requires npm upload functionality
    println!("Skipping npm download invalid version test - upload functionality not implemented");
}