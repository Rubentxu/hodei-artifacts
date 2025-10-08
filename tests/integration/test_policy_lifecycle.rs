//! Integration tests for policy lifecycle operations
//!
//! This module tests complete policy lifecycle scenarios including:
//! - Full CRUD lifecycle (Create → Read → Update → Delete)
//! - Bulk operations (Create many → List → Update many → Delete all)
//! - State transitions and consistency
//! - Error recovery and rollback scenarios

use crate::common::{
    MockSchemaStorage, TestDb, assert_policy_count, assert_policy_exists, assert_policy_not_exists,
    setup_test_db, test_policy_hrn, valid_policy_content, valid_policy_with_conditions,
};
use hodei_iam::features::create_policy::use_case::CreatePolicyUseCase;
use hodei_iam::features::delete_policy::use_case::DeletePolicyUseCase;
use hodei_iam::features::get_policy::use_case::GetPolicyUseCase;
use hodei_iam::features::list_policies::use_case::ListPoliciesUseCase;
use hodei_iam::features::update_policy::use_case::UpdatePolicyUseCase;
use hodei_iam::infrastructure::surreal::policy_adapter::SurrealPolicyAdapter;
use hodei_policies::features::validate_policy::use_case::ValidatePolicyUseCase;
use kernel::HodeiPolicy;
use std::sync::Arc;
use tracing_test::traced_test;

/// Test: Complete CRUD lifecycle for a single policy
/// Create → Get → Update → Get → Delete → Verify
#[tokio::test]
#[traced_test]
async fn test_complete_crud_lifecycle() {
    // Setup
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));

    let create_uc = CreatePolicyUseCase::new(adapter.clone(), validator.clone());
    let get_uc = GetPolicyUseCase::new(adapter.clone());
    let update_uc = UpdatePolicyUseCase::new(adapter.clone(), validator.clone());
    let delete_uc = DeletePolicyUseCase::new(adapter.clone());

    let policy_id = "lifecycle-test-policy";

    // 1. CREATE
    let create_cmd = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
        policy_id: policy_id.to_string(),
        policy_content: valid_policy_content(),
        description: Some("Initial description".to_string()),
    };
    let create_result = create_uc.execute(create_cmd).await;
    assert!(create_result.is_ok(), "Create should succeed");
    let created = create_result.unwrap();
    assert_eq!(created.content, valid_policy_content());

    // 2. GET (verify creation)
    let hrn = test_policy_hrn(policy_id);
    let get_query = hodei_iam::features::get_policy::dto::GetPolicyQuery {
        policy_hrn: hrn.clone(),
    };
    let get_result = get_uc.execute(get_query.clone()).await;
    assert!(get_result.is_ok(), "Get should succeed after create");
    let retrieved = get_result.unwrap();
    assert_eq!(retrieved.content, valid_policy_content());

    // 3. UPDATE
    let update_cmd = hodei_iam::features::update_policy::dto::UpdatePolicyCommand {
        policy_hrn: hrn.clone(),
        policy_content: Some(valid_policy_with_conditions()),
        description: Some("Updated description".to_string()),
    };
    let update_result = update_uc.execute(update_cmd).await;
    assert!(update_result.is_ok(), "Update should succeed");
    let updated = update_result.unwrap();
    assert_eq!(updated.content, valid_policy_with_conditions());

    // 4. GET (verify update)
    let get_after_update = get_uc.execute(get_query).await;
    assert!(get_after_update.is_ok(), "Get should succeed after update");
    let retrieved_updated = get_after_update.unwrap();
    assert_eq!(retrieved_updated.content, valid_policy_with_conditions());

    // 5. DELETE
    let delete_cmd = hodei_iam::features::delete_policy::dto::DeletePolicyCommand {
        policy_hrn: hrn.clone(),
    };
    let delete_result = delete_uc.execute(delete_cmd).await;
    assert!(delete_result.is_ok(), "Delete should succeed");

    // 6. VERIFY deletion
    assert_policy_not_exists(&db.client, policy_id).await;

    // 7. GET (should fail after deletion)
    let get_after_delete = get_uc
        .execute(hodei_iam::features::get_policy::dto::GetPolicyQuery { policy_hrn: hrn })
        .await;
    assert!(get_after_delete.is_err(), "Get should fail after deletion");
}

/// Test: Bulk create and list workflow
/// Create 50 policies → List with pagination → Verify count
#[tokio::test]
#[traced_test]
async fn test_bulk_create_and_list() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));

    let create_uc = CreatePolicyUseCase::new(adapter.clone(), validator);
    let list_uc = ListPoliciesUseCase::new(adapter.clone());

    // Create 50 policies
    for i in 0..50 {
        let cmd = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
            policy_id: format!("bulk-policy-{:03}", i),
            policy_content: valid_policy_content(),
            description: Some(format!("Bulk policy {}", i)),
        };
        let result = create_uc.execute(cmd).await;
        assert!(result.is_ok(), "Bulk create {} should succeed", i);
    }

    // List all policies
    let query = hodei_iam::features::list_policies::dto::ListPoliciesQuery {
        limit: 100,
        offset: 0,
    };
    let list_result = list_uc.execute(query).await;
    assert!(list_result.is_ok(), "List should succeed");
    let response = list_result.unwrap();
    assert_eq!(response.total_count, 50, "Should have 50 policies");
    assert_eq!(response.policies.len(), 50, "Should return 50 policies");

    // Verify database count
    assert_policy_count(&db.client, 50).await;
}

/// Test: Bulk update workflow
/// Create 10 policies → Update all → Verify all updated
#[tokio::test]
#[traced_test]
async fn test_bulk_update_workflow() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));

    let create_uc = CreatePolicyUseCase::new(adapter.clone(), validator.clone());
    let update_uc = UpdatePolicyUseCase::new(adapter.clone(), validator);
    let get_uc = GetPolicyUseCase::new(adapter.clone());

    // Create 10 policies
    for i in 0..10 {
        let cmd = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
            policy_id: format!("update-bulk-{}", i),
            policy_content: valid_policy_content(),
            description: None,
        };
        create_uc.execute(cmd).await.unwrap();
    }

    // Update all policies
    for i in 0..10 {
        let hrn = test_policy_hrn(&format!("update-bulk-{}", i));
        let cmd = hodei_iam::features::update_policy::dto::UpdatePolicyCommand {
            policy_hrn: hrn,
            policy_content: Some(valid_policy_with_conditions()),
            description: Some("Bulk updated".to_string()),
        };
        let result = update_uc.execute(cmd).await;
        assert!(result.is_ok(), "Bulk update {} should succeed", i);
    }

    // Verify all policies were updated
    for i in 0..10 {
        let hrn = test_policy_hrn(&format!("update-bulk-{}", i));
        let query = hodei_iam::features::get_policy::dto::GetPolicyQuery { policy_hrn: hrn };
        let result = get_uc.execute(query).await;
        assert!(result.is_ok());
        let policy = result.unwrap();
        assert_eq!(policy.content, valid_policy_with_conditions());
    }
}

/// Test: Bulk delete workflow
/// Create 10 policies → Delete all → Verify empty
#[tokio::test]
#[traced_test]
async fn test_bulk_delete_workflow() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));

    let create_uc = CreatePolicyUseCase::new(adapter.clone(), validator);
    let delete_uc = DeletePolicyUseCase::new(adapter.clone());

    // Create 10 policies
    for i in 0..10 {
        let cmd = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
            policy_id: format!("delete-bulk-{}", i),
            policy_content: valid_policy_content(),
            description: None,
        };
        create_uc.execute(cmd).await.unwrap();
    }

    assert_policy_count(&db.client, 10).await;

    // Delete all policies
    for i in 0..10 {
        let hrn = test_policy_hrn(&format!("delete-bulk-{}", i));
        let cmd = hodei_iam::features::delete_policy::dto::DeletePolicyCommand { policy_hrn: hrn };
        let result = delete_uc.execute(cmd).await;
        assert!(result.is_ok(), "Bulk delete {} should succeed", i);
    }

    // Verify all deleted
    assert_policy_count(&db.client, 0).await;
}

/// Test: Create-Update-Delete-Recreate cycle
#[tokio::test]
#[traced_test]
async fn test_create_update_delete_recreate_cycle() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));

    let create_uc = CreatePolicyUseCase::new(adapter.clone(), validator.clone());
    let update_uc = UpdatePolicyUseCase::new(adapter.clone(), validator.clone());
    let delete_uc = DeletePolicyUseCase::new(adapter.clone());

    let policy_id = "cycle-test";

    // Cycle 1: Create → Update → Delete
    let create_cmd = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
        policy_id: policy_id.to_string(),
        policy_content: valid_policy_content(),
        description: Some("First cycle".to_string()),
    };
    create_uc.execute(create_cmd).await.unwrap();

    let hrn = test_policy_hrn(policy_id);
    let update_cmd = hodei_iam::features::update_policy::dto::UpdatePolicyCommand {
        policy_hrn: hrn.clone(),
        policy_content: Some(valid_policy_with_conditions()),
        description: Some("Updated in first cycle".to_string()),
    };
    update_uc.execute(update_cmd).await.unwrap();

    let delete_cmd = hodei_iam::features::delete_policy::dto::DeletePolicyCommand {
        policy_hrn: hrn.clone(),
    };
    delete_uc.execute(delete_cmd).await.unwrap();

    assert_policy_not_exists(&db.client, policy_id).await;

    // Cycle 2: Recreate with same ID
    let recreate_cmd = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
        policy_id: policy_id.to_string(),
        policy_content: valid_policy_content(),
        description: Some("Second cycle".to_string()),
    };
    let recreate_result = create_uc.execute(recreate_cmd).await;
    assert!(
        recreate_result.is_ok(),
        "Should be able to recreate after delete"
    );

    assert_policy_exists(&db.client, policy_id).await;
}

/// Test: Multiple sequential operations on same policy
#[tokio::test]
#[traced_test]
async fn test_multiple_sequential_operations() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));

    let create_uc = CreatePolicyUseCase::new(adapter.clone(), validator.clone());
    let get_uc = GetPolicyUseCase::new(adapter.clone());
    let update_uc = UpdatePolicyUseCase::new(adapter.clone(), validator);

    let policy_id = "sequential-ops";

    // Create
    let create_cmd = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
        policy_id: policy_id.to_string(),
        policy_content: valid_policy_content(),
        description: Some("v1".to_string()),
    };
    create_uc.execute(create_cmd).await.unwrap();

    let hrn = test_policy_hrn(policy_id);

    // Multiple updates
    for i in 2..=5 {
        let update_cmd = hodei_iam::features::update_policy::dto::UpdatePolicyCommand {
            policy_hrn: hrn.clone(),
            policy_content: Some(valid_policy_content()),
            description: Some(format!("v{}", i)),
        };
        update_uc.execute(update_cmd).await.unwrap();

        // Get after each update
        let get_query = hodei_iam::features::get_policy::dto::GetPolicyQuery {
            policy_hrn: hrn.clone(),
        };
        let result = get_uc.execute(get_query).await;
        assert!(result.is_ok(), "Get after update {} should succeed", i);
    }

    // Final get to verify last state
    let final_query = hodei_iam::features::get_policy::dto::GetPolicyQuery { policy_hrn: hrn };
    let final_result = get_uc.execute(final_query).await;
    assert!(final_result.is_ok());
}

/// Test: Policy lifecycle with error recovery
/// Attempt operations that should fail, verify state consistency
#[tokio::test]
#[traced_test]
async fn test_lifecycle_with_error_recovery() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));

    let create_uc = CreatePolicyUseCase::new(adapter.clone(), validator.clone());
    let update_uc = UpdatePolicyUseCase::new(adapter.clone(), validator);
    let delete_uc = DeletePolicyUseCase::new(adapter.clone());

    let policy_id = "error-recovery";
    let hrn = test_policy_hrn(policy_id);

    // 1. Try to update non-existent policy (should fail)
    let update_cmd = hodei_iam::features::update_policy::dto::UpdatePolicyCommand {
        policy_hrn: hrn.clone(),
        policy_content: Some(valid_policy_content()),
        description: None,
    };
    let update_result = update_uc.execute(update_cmd).await;
    assert!(
        update_result.is_err(),
        "Update should fail for non-existent"
    );

    // 2. Create the policy
    let create_cmd = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
        policy_id: policy_id.to_string(),
        policy_content: valid_policy_content(),
        description: None,
    };
    create_uc.execute(create_cmd).await.unwrap();

    // 3. Try to create duplicate (should fail)
    let duplicate_cmd = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
        policy_id: policy_id.to_string(),
        policy_content: valid_policy_content(),
        description: None,
    };
    let duplicate_result = create_uc.execute(duplicate_cmd).await;
    assert!(duplicate_result.is_err(), "Duplicate create should fail");

    // 4. Verify original policy still exists and is unchanged
    assert_policy_exists(&db.client, policy_id).await;

    // 5. Delete the policy
    let delete_cmd = hodei_iam::features::delete_policy::dto::DeletePolicyCommand {
        policy_hrn: hrn.clone(),
    };
    delete_uc.execute(delete_cmd).await.unwrap();

    // 6. Try to delete again (should fail)
    let delete_again_cmd =
        hodei_iam::features::delete_policy::dto::DeletePolicyCommand { policy_hrn: hrn };
    let delete_again_result = delete_uc.execute(delete_again_cmd).await;
    assert!(
        delete_again_result.is_err(),
        "Delete should fail for deleted"
    );

    // 7. Verify policy doesn't exist
    assert_policy_not_exists(&db.client, policy_id).await;
}

/// Test: Complete workflow timing
/// Measure performance of complete CRUD cycle
#[tokio::test]
#[traced_test]
async fn test_complete_workflow_performance() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));

    let create_uc = CreatePolicyUseCase::new(adapter.clone(), validator.clone());
    let get_uc = GetPolicyUseCase::new(adapter.clone());
    let update_uc = UpdatePolicyUseCase::new(adapter.clone(), validator);
    let delete_uc = DeletePolicyUseCase::new(adapter.clone());

    let policy_id = "perf-test";
    let hrn = test_policy_hrn(policy_id);

    let start = std::time::Instant::now();

    // Create
    let create_cmd = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
        policy_id: policy_id.to_string(),
        policy_content: valid_policy_content(),
        description: None,
    };
    create_uc.execute(create_cmd).await.unwrap();

    // Get
    let get_query = hodei_iam::features::get_policy::dto::GetPolicyQuery {
        policy_hrn: hrn.clone(),
    };
    get_uc.execute(get_query).await.unwrap();

    // Update
    let update_cmd = hodei_iam::features::update_policy::dto::UpdatePolicyCommand {
        policy_hrn: hrn.clone(),
        policy_content: Some(valid_policy_with_conditions()),
        description: None,
    };
    update_uc.execute(update_cmd).await.unwrap();

    // Delete
    let delete_cmd =
        hodei_iam::features::delete_policy::dto::DeletePolicyCommand { policy_hrn: hrn };
    delete_uc.execute(delete_cmd).await.unwrap();

    let duration = start.elapsed();

    assert!(
        duration.as_millis() < 5000,
        "Complete CRUD cycle should complete in less than 5 seconds, took: {}ms",
        duration.as_millis()
    );
}
