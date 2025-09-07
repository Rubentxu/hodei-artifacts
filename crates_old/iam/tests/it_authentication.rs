use iam::application::api::IamApi;

use iam::infrastructure::mongo_user_repository::MongoUserRepository;
use infra_mongo::client::MongoClientFactory;
use std::sync::Arc;
use shared::UserId;
use iam::features::create_user::CreateUserCommand;
use iam::features::login::{LoginCommand, LoginResponse};
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
async fn test_authentication_flow() {
    let (api, client) = setup().await;

    // Clean up before test
    client.database("hodei-test-db").collection::<User>("users").delete_many(mongodb::bson::doc!{}).await.unwrap();

    // 1. Create User
    let create_cmd = CreateUserCommand {
        username: "authuser".to_string(),
        email: "auth@example.com".to_string(),
        password: "authpassword".to_string(),
        attributes: serde_json::json!({}),
    };
    let user_id = api.create_user(create_cmd).await.unwrap();
    assert!(!user_id.to_string().is_empty());

    // 2. Login with correct credentials
    let login_cmd = LoginCommand {
        username: "authuser".to_string(),
        password: "authpassword".to_string(),
    };
    let login_response = api.login(login_cmd).await.unwrap();
    assert!(!login_response.token.is_empty());

    // 3. Login with incorrect password
    let login_cmd_wrong_password = LoginCommand {
        username: "authuser".to_string(),
        password: "wrongpassword".to_string(),
    };
    let result_wrong_password = api.login(login_cmd_wrong_password).await;
    assert!(result_wrong_password.is_err());
    match result_wrong_password.err().unwrap() {
        IamError::Unauthorized => (),
        _ => panic!("Expected Unauthorized error"),
    }

    // 4. Login with non-existent user
    let login_cmd_non_existent_user = LoginCommand {
        username: "nonexistent".to_string(),
        password: "anypassword".to_string(),
    };
    let result_non_existent_user = api.login(login_cmd_non_existent_user).await;
    assert!(result_non_existent_user.is_err());
    match result_non_existent_user.err().unwrap() {
        IamError::NotFound => (),
        _ => panic!("Expected NotFound error"),
    }
}
