use shared_test::setup_test_environment;
use distribution::features::maven::upload::handler::handle_maven_upload;
use search::application::api::{SearchApi, SearchArtifactsQuery};
use shared::RepositoryId;

#[tokio::test]
async fn it_search_artifacts_full_flow() {
    let env = setup_test_environment(None).await;

    let search_api = SearchApi::new(
        env.artifact_repository.clone(),
        env.authorization.clone(),
    );

    let repository_id = RepositoryId::new();

    // Upload a few artifacts
    let artifacts_to_upload = vec![
        ("com.example", "app-core", "1.0.0", "app-core-1.0.0.jar", vec![1, 2, 3]),
        ("com.example", "app-core", "1.0.1", "app-core-1.0.1.jar", vec![4, 5, 6]),
        ("org.another", "lib-util", "2.0.0", "lib-util-2.0.0.jar", vec![7, 8, 9]),
    ];

    for (group_id, artifact_id, version, file_name, bytes) in artifacts_to_upload.iter() {
        handle_maven_upload(
            env.artifact_storage.clone(),
            env.artifact_repository.clone(),
            env.artifact_event_publisher.clone(),
            env.authorization.clone(),
            repository_id.clone(),
            group_id.to_string(),
            artifact_id.to_string(),
            version.to_string(),
            file_name.to_string(),
            bytes.clone(),
        ).await.unwrap();
    }

    // Search for artifacts
    let query = SearchArtifactsQuery {
        query: Some("app-core".to_string()),
        ecosystem: None,
        group_id: None,
        artifact_id: None,
        version: None,
    };

    let search_results = search_api.search_artifacts(query).await.unwrap();
    assert_eq!(search_results.artifacts.len(), 2);

    let query_all = SearchArtifactsQuery {
        query: None,
        ecosystem: None,
        group_id: None,
        artifact_id: None,
        version: None,
    };
    let search_results_all = search_api.search_artifacts(query_all).await.unwrap();
    assert_eq!(search_results_all.artifacts.len(), 3);
}
