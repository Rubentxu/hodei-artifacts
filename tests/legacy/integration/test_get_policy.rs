//! Integration tests for get_policy handler
//!
//! This module tests the get_policy endpoint with real database operations,
//! covering success cases, not found scenarios, and edge cases.

use crate::common::{
    TestDb, assert_policy_exists, insert_test_policy, setup_test_db, test_policy_hrn,
    valid_policy_content,
};
use hodei_iam::features::get_policy::use_case::GetPolicyUseCase;
use hodei_iam::infrastructure::surreal::policy_adapter::SurrealPolicyAdapter;
use kernel::HodeiPolicy;
use std::sync::Arc;
use tracing_test::traced_test;

/// Test: Successfully get an existing policy
#[tokio::test]
#[traced_test]
async fn test_get_policy_success() {
    // Setup
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let use_case = GetPolicyUseCase::new(adapter);

    // Insert test policy
    let policy = HodeiPolicy::new("test-policy-get".to_string(), valid_policy_content());
    insert_test_policy(&db.client, policy.clone())
        .await
        .expect("Failed to insert test policy");

    // Create query
    let hrn = test_policy_hrn("test-policy-get");
    let query = hodei_iam::features::get_policy::dto::GetPolicyQuery {
        policy_hrn: hrn.clone(),
    };

    // Execute
    let result = use_case.execute(query).await;

    // Assert
    assert!(result.is_ok(), "Get policy should succeed");
    let policy_view = result.unwrap();
    assert_eq!(policy_view.hrn, hrn);
    assert_eq!(policy_view.name, "test-policy-get");
    assert_eq!(policy_view.content, valid_policy_content());
}

/// Test: Get non-existent policy should return not found error
#[tokio::test]
#[traced_test]
async fn test_get_policy_not_found() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let use_case = GetPolicyUseCase::new(adapter);

    let hrn = test_policy_hrn("non-existent-policy");
    let query = hodei_iam::features::get_policy::dto::GetPolicyQuery { policy_hrn: hrn };

    let result = use_case.execute(query).await;
    assert!(result.is_err(), "Should fail with not found error");

    match result.unwrap_err() {
        hodei_iam::features::get_policy::error::GetPolicyError::PolicyNotFound(_) => {}
        e => panic!("Expected PolicyNotFound error, got: {:?}", e),
    }
}

/// Test: Get policy with invalid HRN type should fail
#[tokio::test]
#[traced_test]
async fn test_get_policy_invalid_hrn_type() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let use_case = GetPolicyUseCase::new(adapter);

    // Create HRN with wrong resource type (User instead of Policy)
    let hrn = kernel::Hrn::new(
        "hodei".to_string(),
        "iam".to_string(),
        "test".to_string(),
        "User".to_string(),
        "some-user".to_string(),
    );

    let query = hodei_iam::features::get_policy::dto::GetPolicyQuery { policy_hrn: hrn };

    let result = use_case.execute(query).await;
    assert!(result.is_err(), "Should fail with invalid HRN error");

    match result.unwrap_err() {
        hodei_iam::features::get_policy::error::GetPolicyError::InvalidHrn(_) => {}
        e => panic!("Expected InvalidHrn error, got: {:?}", e),
    }
}

/// Test: Get multiple policies sequentially
#[tokio::test]
#[traced_test]
async fn test_get_multiple_policies_sequential() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let use_case = GetPolicyUseCase::new(adapter);

    // Insert multiple policies
    for i in 0..5 {
        let policy = HodeiPolicy::new(format!("policy-{}", i), valid_policy_content());
        insert_test_policy(&db.client, policy)
            .await
            .expect("Failed to insert test policy");
    }

    // Get each policy
    for i in 0..5 {
        let hrn = test_policy_hrn(&format!("policy-{}", i));
        let query = hodei_iam::features::get_policy::dto::GetPolicyQuery {
            policy_hrn: hrn.clone(),
        };

        let result = use_case.execute(query).await;
        assert!(result.is_ok(), "Get policy {} should succeed", i);

        let policy_view = result.unwrap();
        assert_eq!(policy_view.name, format!("policy-{}", i));
    }
}

/// Test: Get policy with special characters in ID
#[tokio::test]
#[traced_test]
async fn test_get_policy_special_characters() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let use_case = GetPolicyUseCase::new(adapter);

    let policy_id = "policy-with_special.chars-123";
    let policy = HodeiPolicy::new(policy_id.to_string(), valid_policy_content());
    insert_test_policy(&db.client, policy)
        .await
        .expect("Failed to insert test policy");

    let hrn = test_policy_hrn(policy_id);
    let query = hodei_iam::features::get_policy::dto::GetPolicyQuery {
        policy_hrn: hrn.clone(),
    };

    let result = use_case.execute(query).await;
    assert!(result.is_ok(), "Should get policy with special characters");

    let policy_view = result.unwrap();
    assert_eq!(policy_view.name, policy_id);
}

/// Test: Get policy immediately after creation
#[tokio::test]
#[traced_test]
async fn test_get_policy_after_creation() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let use_case = GetPolicyUseCase::new(adapter);

    let policy_id = "just-created-policy";
    let policy = HodeiPolicy::new(policy_id.to_string(), valid_policy_content());

    // Insert and immediately get
    insert_test_policy(&db.client, policy)
        .await
        .expect("Failed to insert test policy");

    let hrn = test_policy_hrn(policy_id);
    let query = hodei_iam::features::get_policy::dto::GetPolicyQuery { policy_hrn: hrn };

    let result = use_case.execute(query).await;
    assert!(
        result.is_ok(),
        "Should get policy immediately after creation"
    );
}

/// Test: Get policy with very long content
#[tokio::test]
#[traced_test]
async fn test_get_policy_long_content() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let use_case = GetPolicyUseCase::new(adapter);

    // Create policy with long content
    let mut long_content = String::from("permit(\n    principal,\n    action in [\n");
    for i in 0..100 {
        long_content.push_str(&format!("        Action::\"Action{}\",\n", i));
    }
    long_content.push_str("    ],\n    resource\n);");

    let policy = HodeiPolicy::new("long-content-policy".to_string(), long_content.clone());
    insert_test_policy(&db.client, policy)
        .await
        .expect("Failed to insert test policy");

    let hrn = test_policy_hrn("long-content-policy");
    let query = hodei_iam::features::get_policy::dto::GetPolicyQuery { policy_hrn: hrn };

    let result = use_case.execute(query).await;
    assert!(result.is_ok(), "Should get policy with long content");

    let policy_view = result.unwrap();
    assert_eq!(policy_view.content, long_content);
    assert!(policy_view.content.len() > 1000);
}

/// Test: Concurrent get requests for same policy
#[tokio::test]
#[traced_test]
async fn test_get_policy_concurrent_reads() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));

    // Insert test policy
    let policy = HodeiPolicy::new("concurrent-read-policy".to_string(), valid_policy_content());
    insert_test_policy(&db.client, policy)
        .await
        .expect("Failed to insert test policy");

    // Spawn 10 concurrent read tasks
    let mut handles = vec![];
    for _ in 0..10 {
        let adapter_clone = adapter.clone();
        let handle = tokio::spawn(async move {
            let use_case = GetPolicyUseCase::new(adapter_clone);
            let hrn = test_policy_hrn("concurrent-read-policy");
            let query = hodei_iam::features::get_policy::dto::GetPolicyQuery { policy_hrn: hrn };
            use_case.execute(query).await
        });
        handles.push(handle);
    }

    // Wait for all tasks
    for handle in handles {
        let result = handle.await.expect("Task panicked");
        assert!(result.is_ok(), "Concurrent read should succeed");
    }
}

/// Test: Get policy performance benchmark
#[tokio::test]
#[traced_test]
async fn test_get_policy_performance() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let use_case = GetPolicyUseCase::new(adapter);

    // Insert test policy
    let policy = HodeiPolicy::new("perf-test-policy".to_string(), valid_policy_content());
    insert_test_policy(&db.client, policy)
        .await
        .expect("Failed to insert test policy");

    let hrn = test_policy_hrn("perf-test-policy");
    let query = hodei_iam::features::get_policy::dto::GetPolicyQuery { policy_hrn: hrn };

    // Measure execution time
    let start = std::time::Instant::now();
    let result = use_case.execute(query).await;
    let duration = start.elapsed();

    assert!(result.is_ok(), "Get policy should succeed");
    assert!(
        duration.as_millis() < 1000,
        "Get policy should complete in less than 1 second, took: {}ms",
        duration.as_millis()
    );
}
