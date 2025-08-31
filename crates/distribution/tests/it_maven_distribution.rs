use shared_test::setup_test_environment;
use distribution::features::maven::upload::handler::handle_maven_upload;
use distribution::features::maven::download::handler::handle_maven_download;
use shared::{RepositoryId, UserId};

#[tokio::test]
async fn it_maven_upload_and_download_happy_path() {
    let env = setup_test_environment(None).await;
    
    let repository_id = RepositoryId::new();
    let user_id = UserId::new();
    
    // Upload Maven artifact
    let group_id = "com.example".to_string();
    let artifact_id = "test-artifact".to_string();
    let version = "1.0.0".to_string();
    let file_name = "test-artifact-1.0.0.jar".to_string();
    let content = vec![1, 2, 3, 4, 5]; // Simple test content
    
    let upload_result = handle_maven_upload(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        env.artifact_event_publisher.clone(),
        env.authorization.clone(),
        repository_id.clone(),
        group_id.clone(),
        artifact_id.clone(),
        version.clone(),
        file_name.clone(),
        content.clone(),
    ).await.unwrap();
    
    // Download the artifact
    let download_result = handle_maven_download(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        repository_id.clone(),
        group_id.clone(),
        artifact_id.clone(),
        version.clone(),
        file_name.clone(),
    ).await.unwrap();
    
    assert_eq!(download_result, content);
}

#[tokio::test]
async fn it_maven_download_nonexistent_artifact() {
    let env = setup_test_environment(None).await;
    
    let repository_id = RepositoryId::new();
    
    // Try to download non-existent artifact
    let result = handle_maven_download(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        repository_id,
        "com.nonexistent".to_string(),
        "missing".to_string(),
        "1.0.0".to_string(),
        "missing-1.0.0.jar".to_string(),
    ).await;
    
    assert!(result.is_err());
}

#[tokio::test]
async fn it_maven_multiple_versions_same_artifact() {
    let env = setup_test_environment(None).await;
    
    let repository_id = RepositoryId::new();
    let user_id = UserId::new();
    
    let group_id = "com.example".to_string();
    let artifact_id = "multi-version".to_string();
    
    // Upload version 1.0.0
    let upload_result_1 = handle_maven_upload(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        env.artifact_event_publisher.clone(),
        env.authorization.clone(),
        repository_id.clone(),
        group_id.clone(),
        artifact_id.clone(),
        "1.0.0".to_string(),
        "multi-version-1.0.0.jar".to_string(),
        vec![1, 2, 3],
    ).await.unwrap();
    
    // Upload version 2.0.0
    let upload_result_2 = handle_maven_upload(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        env.artifact_event_publisher.clone(),
        env.authorization.clone(),
        repository_id.clone(),
        group_id.clone(),
        artifact_id.clone(),
        "2.0.0".to_string(),
        "multi-version-2.0.0.jar".to_string(),
        vec![4, 5, 6],
    ).await.unwrap();
    
    // Download both versions
    let download_1 = handle_maven_download(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        repository_id.clone(),
        group_id.clone(),
        artifact_id.clone(),
        "1.0.0".to_string(),
        "multi-version-1.0.0.jar".to_string(),
    ).await.unwrap();
    
    let download_2 = handle_maven_download(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        repository_id.clone(),
        group_id.clone(),
        artifact_id.clone(),
        "2.0.0".to_string(),
        "multi-version-2.0.0.jar".to_string(),
    ).await.unwrap();
    
    assert_eq!(download_1, vec![1, 2, 3]);
    assert_eq!(download_2, vec![4, 5, 6]);
}