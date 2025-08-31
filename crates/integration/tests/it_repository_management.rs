use shared_test::setup_test_environment;
use repository::application::api::RepositoryApi;
use repository::features::create_repository::CreateRepositoryCommand;
use repository::features::get_repository::GetRepositoryCommand;
use repository::features::delete_repository::DeleteRepositoryCommand;
use shared::{UserId, IsoTimestamp};

#[tokio::test]
async fn it_repository_management_full_flow() {
    let env = setup_test_environment(None).await;

    let repository_api = RepositoryApi::new(
        env.artifact_repository.clone(),
        env.artifact_event_publisher.clone(),
        env.authorization.clone(),
    );

    // Test Create Repository
    let create_command = CreateRepositoryCommand {
        name: "test-repo".to_string(),
        description: Some("A test repository".to_string()),
        created_by: UserId::new(), // Placeholder
    };

    let created_repo_id = repository_api.create_repository(create_command.clone()).await.unwrap();
    assert_eq!(created_repo_id.name, create_command.name);

    // Test Get Repository
    let get_command = GetRepositoryCommand {
        repository_id: created_repo_id.id.clone(),
    };
    let retrieved_repo = repository_api.get_repository(get_command).await.unwrap();
    assert_eq!(retrieved_repo.name.0, create_command.name);

    // Test Delete Repository
    let delete_command = DeleteRepositoryCommand {
        repository_id: created_repo_id.id.clone(),
        deleted_by: UserId::new(), // Placeholder
        occurred_at: IsoTimestamp::now(),
    };
    repository_api.delete_repository(delete_command).await.unwrap();

    // Verify deletion by trying to get the repository again
    let get_command_after_delete = GetRepositoryCommand {
        repository_id: created_repo_id.id.clone(),
    };
    let result = repository_api.get_repository(get_command_after_delete).await;
    assert!(result.is_err());
    // TODO: Assert specific error type (e.g., NotFound)
}
