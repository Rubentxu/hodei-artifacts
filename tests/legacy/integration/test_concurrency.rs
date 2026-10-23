//! Integration tests for concurrent policy operations
//!
//! This module tests concurrent access patterns and race conditions:
//! - Concurrent creates (different IDs - should all succeed)
//! - Concurrent creates (same ID - conflict detection)
//! - Concurrent updates (same policy - version conflict)
//! - Concurrent reads while writing
//! - Race condition handling
//! - Deadlock prevention

use crate::common::{
    MockSchemaStorage, TestDb, assert_policy_count, insert_test_policy, setup_test_db,
    test_policy_hrn, valid_policy_content, valid_policy_with_conditions,
};
use hodei_iam::features::create_policy::use_case::CreatePolicyUseCase;
use hodei_iam::features::delete_policy::use_case::DeletePolicyUseCase;
use hodei_iam::features::get_policy::use_case::GetPolicyUseCase;
use hodei_iam::features::update_policy::use_case::UpdatePolicyUseCase;
use hodei_iam::infrastructure::surreal::policy_adapter::SurrealPolicyAdapter;
use hodei_policies::features::validate_policy::use_case::ValidatePolicyUseCase;
use kernel::HodeiPolicy;
use std::sync::Arc;
use tracing_test::traced_test;

/// Test: Concurrent creates with different IDs - all should succeed
#[tokio::test]
#[traced_test]
async fn test_concurrent_creates_different_ids() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));

    let mut handles = vec![];

    // Spawn 20 concurrent create tasks with different IDs
    for i in 0..20 {
        let adapter_clone = adapter.clone();
        let validator_clone = validator.clone();

        let handle = tokio::spawn(async move {
            let use_case = CreatePolicyUseCase::new(adapter_clone, validator_clone);
            let cmd = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
                policy_id: format!("concurrent-policy-{}", i),
                policy_content: valid_policy_content(),
                description: Some(format!("Concurrent policy {}", i)),
            };
            use_case.execute(cmd).await
        });

        handles.push(handle);
    }

    // Wait for all tasks and verify all succeeded
    let mut success_count = 0;
    for handle in handles {
        let result = handle.await.expect("Task panicked");
        if result.is_ok() {
            success_count += 1;
        }
    }

    assert_eq!(success_count, 20, "All concurrent creates should succeed");
    assert_policy_count(&db.client, 20).await;
}

/// Test: Concurrent creates with same ID - only one should succeed
#[tokio::test]
#[traced_test]
async fn test_concurrent_creates_same_id_conflict() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));

    let mut handles = vec![];
    let policy_id = "duplicate-concurrent-policy";

    // Spawn 10 concurrent create tasks with the SAME ID
    for _ in 0..10 {
        let adapter_clone = adapter.clone();
        let validator_clone = validator.clone();
        let id = policy_id.to_string();

        let handle = tokio::spawn(async move {
            let use_case = CreatePolicyUseCase::new(adapter_clone, validator_clone);
            let cmd = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
                policy_id: id,
                policy_content: valid_policy_content(),
                description: None,
            };
            use_case.execute(cmd).await
        });

        handles.push(handle);
    }

    // Wait for all tasks
    let mut success_count = 0;
    let mut conflict_count = 0;

    for handle in handles {
        let result = handle.await.expect("Task panicked");
        match result {
            Ok(_) => success_count += 1,
            Err(
                hodei_iam::features::create_policy::error::CreatePolicyError::PolicyAlreadyExists(
                    _,
                ),
            ) => conflict_count += 1,
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    assert_eq!(
        success_count, 1,
        "Only one concurrent create should succeed"
    );
    assert_eq!(conflict_count, 9, "Nine creates should get conflict error");
    assert_policy_count(&db.client, 1).await;
}

/// Test: Concurrent updates on same policy
#[tokio::test]
#[traced_test]
async fn test_concurrent_updates_same_policy() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));

    let policy_id = "concurrent-update-policy";

    // Create initial policy
    let policy = HodeiPolicy::new(policy_id.to_string(), valid_policy_content());
    insert_test_policy(&db.client, policy).await.unwrap();

    let mut handles = vec![];
    let hrn = test_policy_hrn(policy_id);

    // Spawn 10 concurrent update tasks on the SAME policy
    for i in 0..10 {
        let adapter_clone = adapter.clone();
        let validator_clone = validator.clone();
        let hrn_clone = hrn.clone();

        let handle = tokio::spawn(async move {
            let use_case = UpdatePolicyUseCase::new(adapter_clone, validator_clone);
            let cmd = hodei_iam::features::update_policy::dto::UpdatePolicyCommand {
                policy_hrn: hrn_clone,
                policy_content: Some(valid_policy_with_conditions()),
                description: Some(format!("Update {}", i)),
            };
            use_case.execute(cmd).await
        });

        handles.push(handle);
    }

    // Wait for all tasks
    let mut success_count = 0;
    for handle in handles {
        let result = handle.await.expect("Task panicked");
        if result.is_ok() {
            success_count += 1;
        }
    }

    // At least some updates should succeed (exact number depends on timing)
    assert!(
        success_count >= 1,
        "At least one concurrent update should succeed"
    );

    // Verify policy still exists and was updated
    let final_policy: Option<HodeiPolicy> = db
        .client
        .select(("policy", policy_id))
        .await
        .expect("Failed to query");
    assert!(final_policy.is_some());
    assert_eq!(
        final_policy.unwrap().content(),
        valid_policy_with_conditions()
    );
}

/// Test: Concurrent reads while updating
#[tokio::test]
#[traced_test]
async fn test_concurrent_reads_during_update() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));

    let policy_id = "read-during-update";

    // Create initial policy
    let policy = HodeiPolicy::new(policy_id.to_string(), valid_policy_content());
    insert_test_policy(&db.client, policy).await.unwrap();

    let hrn = test_policy_hrn(policy_id);
    let mut handles = vec![];

    // Spawn 1 update task
    let adapter_update = adapter.clone();
    let validator_update = validator.clone();
    let hrn_update = hrn.clone();
    let update_handle = tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        let use_case = UpdatePolicyUseCase::new(adapter_update, validator_update);
        let cmd = hodei_iam::features::update_policy::dto::UpdatePolicyCommand {
            policy_hrn: hrn_update,
            policy_content: Some(valid_policy_with_conditions()),
            description: None,
        };
        use_case.execute(cmd).await
    });
    handles.push(update_handle);

    // Spawn 20 concurrent read tasks
    for _ in 0..20 {
        let adapter_clone = adapter.clone();
        let hrn_clone = hrn.clone();

        let handle = tokio::spawn(async move {
            let use_case = GetPolicyUseCase::new(adapter_clone);
            let query = hodei_iam::features::get_policy::dto::GetPolicyQuery {
                policy_hrn: hrn_clone,
            };
            use_case.execute(query).await
        });

        handles.push(handle);
    }

    // Wait for all tasks
    let mut success_count = 0;
    for handle in handles {
        let result = handle.await.expect("Task panicked");
        if result.is_ok() {
            success_count += 1;
        }
    }

    assert_eq!(success_count, 21, "All reads and update should succeed");
}

/// Test: Concurrent deletes on different policies
#[tokio::test]
#[traced_test]
async fn test_concurrent_deletes_different_policies() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));

    // Create 10 policies
    for i in 0..10 {
        let policy = HodeiPolicy::new(format!("delete-concurrent-{}", i), valid_policy_content());
        insert_test_policy(&db.client, policy).await.unwrap();
    }

    assert_policy_count(&db.client, 10).await;

    let mut handles = vec![];

    // Spawn 10 concurrent delete tasks
    for i in 0..10 {
        let adapter_clone = adapter.clone();
        let hrn = test_policy_hrn(&format!("delete-concurrent-{}", i));

        let handle = tokio::spawn(async move {
            let use_case = DeletePolicyUseCase::new(adapter_clone);
            let cmd =
                hodei_iam::features::delete_policy::dto::DeletePolicyCommand { policy_hrn: hrn };
            use_case.execute(cmd).await
        });

        handles.push(handle);
    }

    // Wait for all tasks
    let mut success_count = 0;
    for handle in handles {
        let result = handle.await.expect("Task panicked");
        if result.is_ok() {
            success_count += 1;
        }
    }

    assert_eq!(success_count, 10, "All concurrent deletes should succeed");
    assert_policy_count(&db.client, 0).await;
}

/// Test: Concurrent delete attempts on same policy
#[tokio::test]
#[traced_test]
async fn test_concurrent_deletes_same_policy() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));

    let policy_id = "concurrent-delete-policy";

    // Create policy
    let policy = HodeiPolicy::new(policy_id.to_string(), valid_policy_content());
    insert_test_policy(&db.client, policy).await.unwrap();

    let mut handles = vec![];
    let hrn = test_policy_hrn(policy_id);

    // Spawn 5 concurrent delete tasks on SAME policy
    for _ in 0..5 {
        let adapter_clone = adapter.clone();
        let hrn_clone = hrn.clone();

        let handle = tokio::spawn(async move {
            let use_case = DeletePolicyUseCase::new(adapter_clone);
            let cmd = hodei_iam::features::delete_policy::dto::DeletePolicyCommand {
                policy_hrn: hrn_clone,
            };
            use_case.execute(cmd).await
        });

        handles.push(handle);
    }

    // Wait for all tasks
    let mut success_count = 0;
    let mut not_found_count = 0;

    for handle in handles {
        let result = handle.await.expect("Task panicked");
        match result {
            Ok(_) => success_count += 1,
            Err(hodei_iam::features::delete_policy::error::DeletePolicyError::PolicyNotFound(
                _,
            )) => not_found_count += 1,
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    assert_eq!(success_count, 1, "Only one delete should succeed");
    assert_eq!(not_found_count, 4, "Four deletes should get not found");
    assert_policy_count(&db.client, 0).await;
}

/// Test: Mixed concurrent operations (create, read, update, delete)
#[tokio::test]
#[traced_test]
async fn test_mixed_concurrent_operations() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));

    // Pre-create some policies
    for i in 0..5 {
        let policy = HodeiPolicy::new(format!("mixed-op-{}", i), valid_policy_content());
        insert_test_policy(&db.client, policy).await.unwrap();
    }

    let mut handles = vec![];

    // Spawn mixed operations
    // 5 creates
    for i in 5..10 {
        let adapter_clone = adapter.clone();
        let validator_clone = validator.clone();
        let handle = tokio::spawn(async move {
            let use_case = CreatePolicyUseCase::new(adapter_clone, validator_clone);
            let cmd = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
                policy_id: format!("mixed-op-{}", i),
                policy_content: valid_policy_content(),
                description: None,
            };
            use_case.execute(cmd).await.map(|_| "create")
        });
        handles.push(handle);
    }

    // 10 reads
    for i in 0..10 {
        let adapter_clone = adapter.clone();
        let handle = tokio::spawn(async move {
            let use_case = GetPolicyUseCase::new(adapter_clone);
            let hrn = test_policy_hrn(&format!("mixed-op-{}", i % 5));
            let query = hodei_iam::features::get_policy::dto::GetPolicyQuery { policy_hrn: hrn };
            use_case.execute(query).await.map(|_| "read")
        });
        handles.push(handle);
    }

    // 5 updates
    for i in 0..5 {
        let adapter_clone = adapter.clone();
        let validator_clone = validator.clone();
        let handle = tokio::spawn(async move {
            let use_case = UpdatePolicyUseCase::new(adapter_clone, validator_clone);
            let hrn = test_policy_hrn(&format!("mixed-op-{}", i));
            let cmd = hodei_iam::features::update_policy::dto::UpdatePolicyCommand {
                policy_hrn: hrn,
                policy_content: Some(valid_policy_with_conditions()),
                description: None,
            };
            use_case.execute(cmd).await.map(|_| "update")
        });
        handles.push(handle);
    }

    // Wait for all
    let mut operation_counts = std::collections::HashMap::new();
    for handle in handles {
        if let Ok(Ok(op_type)) = handle.await {
            *operation_counts.entry(op_type).or_insert(0) += 1;
        }
    }

    // Verify operations executed
    assert!(
        *operation_counts.get("create").unwrap_or(&0) >= 1,
        "Some creates should succeed"
    );
    assert!(
        *operation_counts.get("read").unwrap_or(&0) >= 1,
        "Some reads should succeed"
    );
    assert!(
        *operation_counts.get("update").unwrap_or(&0) >= 1,
        "Some updates should succeed"
    );
}

/// Test: High concurrency stress test
#[tokio::test]
#[traced_test]
async fn test_high_concurrency_stress() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));

    let mut handles = vec![];

    // Spawn 100 concurrent operations
    for i in 0..100 {
        let adapter_clone = adapter.clone();
        let validator_clone = validator.clone();

        let handle = tokio::spawn(async move {
            let use_case = CreatePolicyUseCase::new(adapter_clone, validator_clone);
            let cmd = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
                policy_id: format!("stress-test-{}", i),
                policy_content: valid_policy_content(),
                description: None,
            };
            use_case.execute(cmd).await
        });

        handles.push(handle);
    }

    let start = std::time::Instant::now();

    // Wait for all
    let mut success_count = 0;
    for handle in handles {
        if let Ok(Ok(_)) = handle.await {
            success_count += 1;
        }
    }

    let duration = start.elapsed();

    assert!(
        success_count >= 95,
        "At least 95% of operations should succeed, got {}",
        success_count
    );
    assert!(
        duration.as_secs() < 30,
        "High concurrency test should complete in 30s, took {}s",
        duration.as_secs()
    );
}
