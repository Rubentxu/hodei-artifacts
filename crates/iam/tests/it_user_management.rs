use iam::application::api::IamApi;

use iam::infrastructure::mongo_user_repository::MongoUserRepository;
use infra_mongo::client::MongoClientFactory;
use std::sync::Arc;
use shared::UserId;
use iam::features::create_user::CreateUserCommand;
use iam::features::get_user::GetUserQuery;
use iam::features::list_users::ListUsersQuery;
use iam::features::update_user_attributes::UpdateUserAttributesCommand;
use iam::features::delete_user::DeleteUserCommand;
use iam::domain::user::{User, UserStatus};
use iam::error::IamError;

use infra_mongo::test_util::mongo_test_container::ephemeral_store;
use mongodb::Client as MongoDbClient;

async fn setup() -> (IamApi, mongodb::Client) {
    let (factory, _container) = ephemeral_store().await.unwrap();
    let client = factory.client().await.unwrap();
    let user_collection = client.database("hodei-test-db").collection::<User>("users");
    let user_repo = Arc::new(MongoUserRepository::new(user_collection));

    let api = IamApi::new(
        user_repo,
        Arc::new(iam::mocks::policy_repository::MockPolicyRepository::new()),
        Arc::new(iam::infrastructure::cedar_policy_validator::CedarPolicyValidator),
    );
    (api, client.clone())
}

#[tokio::test]
async fn test_user_management_flow() {
    let (api, client) = setup().await;

    // Clean up before test
    client.database("hodei-test-db").collection::<User>("users").delete_many(mongodb::bson::doc!{}).await.unwrap();

    // 1. Create User
    let create_cmd = CreateUserCommand {
        username: "testuser".to_string(),
        email: "test@example.com".to_string(),
        password: "password123".to_string(),
        attributes: serde_json::json!({ "department": "IT" }),
    };
    let user_id = api.create_user(create_cmd).await.unwrap();
    assert!(!user_id.to_string().is_empty());

    // 2. Get User
    let get_query = GetUserQuery { id: user_id.clone() };
    let fetched_user = api.get_user(get_query).await.unwrap();
    assert_eq!(fetched_user.id, user_id);
    assert_eq!(fetched_user.username, "testuser");
    assert_eq!(fetched_user.email, "test@example.com");
    assert_eq!(fetched_user.attributes, serde_json::json!({ "department": "IT" }));
    assert_eq!(fetched_user.status, UserStatus::Active);
    assert!(fetched_user.policies.is_empty());

    // 3. List Users
    let list_query = ListUsersQuery {};
    let users = api.list_users(list_query).await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].id, user_id);

    // 4. Update User Attributes
    let update_cmd = UpdateUserAttributesCommand {
        user_id: user_id.clone(),
        attributes: serde_json::json!({ "department": "HR", "location": "NY" }),
    };
    api.update_user_attributes(update_cmd).await.unwrap();

    let updated_user = api.get_user(GetUserQuery { id: user_id.clone() }).await.unwrap();
    assert_eq!(updated_user.email, "new_email@example.com");
    assert_eq!(updated_user.attributes, serde_json::json!({ "department": "HR", "location": "NY" }));
    assert_eq!(updated_user.status, UserStatus::Inactive);

    // 5. Delete User
    let delete_cmd = DeleteUserCommand { id: user_id.clone() };
    api.delete_user(delete_cmd).await.unwrap();

    let result = api.get_user(GetUserQuery { id: user_id.clone() }).await;
    assert!(result.is_err());
    match result.err().unwrap() {
        IamError::NotFound => (),
        _ => panic!("Expected NotFound error"),
    }
}
