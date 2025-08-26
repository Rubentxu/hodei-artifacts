use iam::application::api::IamApi;

use iam::infrastructure::mongo_policy_repository::MongoPolicyRepository;
use infra_mongo::client::MongoClientFactory;
use infra_mongo::test_util::mongo_test_container::ephemeral_store;
use std::sync::Arc;
use cedar_policy::PolicyId;
use iam::features::create_policy::CreatePolicyCommand;
use iam::features::get_policy::GetPolicyQuery;
use iam::features::list_policies::ListPoliciesQuery;
use iam::features::delete_policy::DeletePolicyCommand;
use iam::domain::policy::{Policy, PolicyStatus};
use iam::error::IamError;

async fn setup() -> (IamApi, mongodb::Client) {
    let (factory, _container) = ephemeral_store().await.unwrap();
    let client = factory.client().await.unwrap();
    let policy_collection = client.database("hodei-test-db").collection::<Policy>("policies");
    let policy_repo = Arc::new(MongoPolicyRepository::new(policy_collection));

    let api = IamApi::new(
        // UserRepository is not used in policy management tests
        Arc::new(iam::mocks::user_repository::MockUserRepository::new()),
        policy_repo,
        Arc::new(iam::infrastructure::cedar_policy_validator::CedarPolicyValidator),
    );
    (api, client.clone())
}

#[tokio::test]
async fn test_policy_management_flow() {
    let (api, client) = setup().await;

    // Clean up before test
    client.database("hodei-test-db").collection::<Policy>("policies").delete_many(mongodb::bson::doc!{}).await.unwrap();

    // 1. Create Policy
    let create_cmd = CreatePolicyCommand {
        name: "test_policy".to_string(),
        description: Some("A test policy".to_string()),
        content: "permit(principal, action, resource);".to_string(),
    };
    let policy_id = api.create_policy(create_cmd).await.unwrap();
    assert!(!policy_id.to_string().is_empty());

    // 2. Get Policy
    let get_query = GetPolicyQuery { id: policy_id.clone() };
    let fetched_policy = api.get_policy(get_query).await.unwrap();
    assert_eq!(fetched_policy.id, policy_id);
    assert_eq!(fetched_policy.name, "test_policy");
    assert_eq!(fetched_policy.content, "permit(principal, action, resource);");

    // 3. List Policies
    let list_query = ListPoliciesQuery {};
    let policies = api.list_policies(list_query).await.unwrap();
    assert_eq!(policies.len(), 1);
    assert_eq!(policies[0].id, policy_id);

    // 4. Delete Policy
    let delete_cmd = DeletePolicyCommand { id: policy_id.clone() };
    api.delete_policy(delete_cmd).await.unwrap();

    let result = api.get_policy(GetPolicyQuery { id: policy_id.clone() }).await;
    assert!(result.is_err());
    match result.err().unwrap() {
        IamError::NotFound => (),
        _ => panic!("Expected NotFound error"),
    }
}
