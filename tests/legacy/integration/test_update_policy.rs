//! Integration tests for update_policy handler

use crate::common::{TestDb, insert_test_policy, setup_test_db, test_policy_hrn, valid_policy_content, valid_policy_with_conditions, MockSchemaStorage};
use hodei_iam::features::update_policy::use_case::UpdatePolicyUseCase;
use hodei_iam::infrastructure::surreal::policy_adapter::SurrealPolicyAdapter;
use hodei_policies::features::validate_policy::use_case::ValidatePolicyUseCase;
use kernel::HodeiPolicy;
use std::sync::Arc;
use tracing_test::traced_test;

#[tokio::test]
#[traced_test]
async fn test_update_policy_success() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));
    let use_case = UpdatePolicyUseCase::new(adapter, validator);

    // Insert test policy
    let policy = HodeiPolicy::new("update-test-policy".to_string(), valid_policy_content());
    insert_test_policy(&db.client, policy).await.unwrap();

    // Update
    let hrn = test_policy_hrn("update-test-policy");
    let command = hodei_iam::features::update_policy::dto::UpdatePolicyCommand {
        policy_hrn: hrn.clone(),
        policy_content: Some(valid_policy_with_conditions()),
        description: Some("Updated description".to_string()),
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());
    let policy_view = result.unwrap();
    assert_eq!(policy_view.id, hrn);
    assert_eq!(policy_view.content, valid_policy_with_conditions());
}

#[tokio::test]
#[traced_test]
async fn test_update_policy_not_found() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));
    let use_case = UpdatePolicyUseCase::new(adapter, validator);

    let hrn = test_policy_hrn("non-existent");
    let command = hodei_iam::features::update_policy::dto::UpdatePolicyCommand {
        policy_hrn: hrn,
        policy_content: Some(valid_policy_content()),
        description: None,
    };

    let result = use_case.execute(command).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        hodei_iam::features::update_policy::error::UpdatePolicyError::PolicyNotFound(_) => {}
        e => panic!("Expected PolicyNotFound, got: {:?}", e),
    }
}

#[tokio::test]
#[traced_test]
async fn test_update_policy_empty_content() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));
    let use_case = UpdatePolicyUseCase::new(adapter, validator);

    let policy = HodeiPolicy::new("update-test".to_string(), valid_policy_content());
    insert_test_policy(&db.client, policy).await.unwrap();

    let hrn = test_policy_hrn("update-test");
    let command = hodei_iam::features::update_policy::dto::UpdatePolicyCommand {
        policy_hrn: hrn,
        policy_content: Some("".to_string()),
        description: None,
    };

    let result = use_case.execute(command).await;
    assert!(result.is_err());
}
