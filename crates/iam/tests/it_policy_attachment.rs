use iam::application::api::IamApi;

use iam::infrastructure::mongo_user_repository::MongoUserRepository;
use iam::infrastructure::mongo_policy_repository::MongoPolicyRepository;
use infra_mongo::client::MongoClientFactory;
use infra_mongo::test_util::mongo_test_container::ephemeral_store;
use std::sync::Arc;
use shared::UserId;
use cedar_policy::PolicyId;
use iam::features::create_user::CreateUserCommand;
use iam::features::create_policy::CreatePolicyCommand;
use iam::features::attach_policy_to_user::AttachPolicyToUserCommand;
use iam::features::detach_policy_from_user::DetachPolicyFromUserCommand;
use iam::features::get_user::GetUserQuery;
use iam::domain::user::{User, UserStatus};
use iam::domain::policy::{Policy, PolicyStatus};
use iam::error::IamError;

async fn setup() -> (IamApi, mongodb::Client) {
    let (factory, _container) = ephemeral_store().await.unwrap();
    let client = factory.client().await.unwrap();
    let user_collection = client.database("hodei-test-db").collection::<User>("users");
    let policy_collection = client.database("hodei-test-db").collection::<Policy>("policies");
    let user_repo = Arc::new(MongoUserRepository::new(user_collection));
    let policy_repo = Arc::new(MongoPolicyRepository::new(policy_collection));

    let api = IamApi::new(
        user_repo,
        policy_repo,
        Arc::new(iam::infrastructure::cedar_policy_validator::CedarPolicyValidator),
    );
    (api, client.clone())
}

#[tokio::test]
async fn test_policy_attachment_flow() {
    let (api, client) = setup().await;

    // Clean up before test
    client.database("hodei-test-db").collection::<User>("users").delete_many(mongodb::bson::doc!{}).await.unwrap();
    client.database("hodei-test-db").collection::<Policy>("policies").delete_many(mongodb::bson::doc!{}).await.unwrap();

    // 1. Create User
    let create_user_cmd = CreateUserCommand {
        username: "policyuser".to_string(),
        email: "policy@example.com".to_string(),
        password: "policypassword".to_string(),
        attributes: serde_json::json!({}),
    };
    let user_id = api.create_user(create_user_cmd).await.unwrap();

    // 2. Create Policy
    let create_policy_cmd = CreatePolicyCommand {
        name: "test_policy_attach".to_string(),
        description: None,
        content: "permit(principal, action, resource);".to_string(),
    };
    let policy_id = api.create_policy(create_policy_cmd).await.unwrap();

    // 3. Attach Policy to User
    let attach_cmd = AttachPolicyToUserCommand {
        user_id: user_id.clone(),
        policy_id: policy_id.clone(),
    };
    api.attach_policy_to_user(attach_cmd).await.unwrap();

    let user_after_attach = api.get_user(GetUserQuery { id: user_id.clone() }).await.unwrap();
    assert!(user_after_attach.policies.contains(&policy_id));

    // 4. Detach Policy from User
    let detach_cmd = DetachPolicyFromUserCommand {
        user_id: user_id.clone(),
        policy_id: policy_id.clone(),
    };
    api.detach_policy_from_user(detach_cmd).await.unwrap();

    let user_after_detach = api.get_user(GetUserQuery { id: user_id.clone() }).await.unwrap();
    assert!(!user_after_detach.policies.contains(&policy_id));
}
