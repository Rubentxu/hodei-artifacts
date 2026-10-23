//! Integration tests for delete_policy handler

use crate::common::{TestDb, insert_test_policy, setup_test_db, test_policy_hrn, valid_policy_content, assert_policy_not_exists};
use hodei_iam::features::delete_policy::use_case::DeletePolicyUseCase;
use hodei_iam::infrastructure::surreal::policy_adapter::SurrealPolicyAdapter;
use kernel::HodeiPolicy;
use std::sync::Arc;
use tracing_test::traced_test;

#[tokio::test]
#[traced_test]
async fn test_delete_policy_success() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let use_case = DeletePolicyUseCase::new(adapter);

    // Insert test policy
    let policy = HodeiPolicy::new("delete-test-policy".to_string(), valid_policy_content());
    insert_test_policy(&db.client, policy).await.unwrap();

    // Delete
    let hrn = test_policy_hrn("delete-test-policy");
    let command = hodei_iam::features::delete_policy::dto::DeletePolicyCommand {
        policy_hrn: hrn,
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());

    // Verify deleted
    assert_policy_not_exists(&db.client, "delete-test-policy").await;
}

#[tokio::test]
#[traced_test]
async fn test_delete_policy_not_found() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let use_case = DeletePolicyUseCase::new(adapter);

    let hrn = test_policy_hrn("non-existent");
    let command = hodei_iam::features::delete_policy::dto::DeletePolicyCommand {
        policy_hrn: hrn,
    };

    let result = use_case.execute(command).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        hodei_iam::features::delete_policy::error::DeletePolicyError::PolicyNotFound(_) => {}
        e => panic!("Expected PolicyNotFound, got: {:?}", e),
    }
}

#[tokio::test]
#[traced_test]
async fn test_delete_policy_and_recreate() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let use_case = DeletePolicyUseCase::new(adapter);

    // Create, delete, recreate with same ID
    let policy = HodeiPolicy::new("recreate-test".to_string(), valid_policy_content());
    insert_test_policy(&db.client, policy).await.unwrap();

    let hrn = test_policy_hrn("recreate-test");
    let command = hodei_iam::features::delete_policy::dto::DeletePolicyCommand {
        policy_hrn: hrn.clone(),
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());

    // Recreate
    let policy2 = HodeiPolicy::new("recreate-test".to_string(), valid_policy_content());
    insert_test_policy(&db.client, policy2).await.unwrap();
}
