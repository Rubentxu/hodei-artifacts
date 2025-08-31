use shared_test::setup_test_environment;
use distribution::features::maven::download::handler::handle_maven_download;
use distribution::features::maven::upload::handler::handle_maven_upload;
use shared::RepositoryId;

#[tokio::test]
async fn it_maven_full_flow() {
    let env = setup_test_environment(None).await;

    // Test Maven Upload
    let group_id = "com.example".to_string();
    let artifact_id = "my-maven-lib".to_string();
    let version = "1.0.0".to_string();
    let file_name = "my-maven-lib-1.0.0.jar".to_string();
    let bytes = vec![1, 2, 3, 4, 5, 6, 7, 8];

    let repository_id = RepositoryId::new(); // Create a RepositoryId for the test

    handle_maven_upload(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        env.artifact_event_publisher.clone(),
        env.authorization.clone(),
        repository_id.clone(), // Pass the RepositoryId
        group_id.clone(),
        artifact_id.clone(),
        version.clone(),
        file_name.clone(),
        bytes.clone(),
    ).await.unwrap();

    // Test Maven Download
    let download_result = handle_maven_download(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        env.authorization.clone(),
        group_id.clone(),
        artifact_id.clone(),
        version.clone(),
        file_name.clone(),
    ).await;

    if let Err(e) = &download_result {
        eprintln!("Download failed with error: {:?}", e);
    }
    assert!(download_result.is_ok());
    assert_eq!(download_result.unwrap(), bytes);
}
