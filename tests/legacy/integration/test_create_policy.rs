//! Integration tests for create_policy handler
//!
//! This module tests the create_policy endpoint with real database operations,
//! covering success cases, validation errors, and edge cases.

use crate::common::{
    TestDb, create_policy_request_invalid_json, create_policy_request_json, setup_test_db,
    test_policy_hrn, valid_policy_content,
};
use hodei_iam::features::create_policy::use_case::CreatePolicyUseCase;
use hodei_iam::infrastructure::surreal::policy_adapter::SurrealPolicyAdapter;
use hodei_policies::features::build_schema::BuildSchemaUseCase;
use hodei_policies::features::validate_policy::use_case::ValidatePolicyUseCase;
use kernel::HodeiPolicy;
use std::sync::{Arc, Mutex};
use tracing_test::traced_test;

/// Test: Successfully create a policy with valid content
#[tokio::test]
#[traced_test]
async fn test_create_policy_success() {
    // Setup
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));

    // Create engine builder and schema storage for validator
    let engine_builder = Arc::new(Mutex::new(hodei_policies::EngineBuilder::new()));
    let schema_storage = Arc::new(crate::common::MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));

    let use_case = CreatePolicyUseCase::new(adapter.clone(), validator);

    // Create policy command
    let command = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
        policy_id: "test-policy-001".to_string(),
        policy_content: valid_policy_content(),
        description: Some("Test policy for integration test".to_string()),
    };

    // Execute
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok(), "Policy creation should succeed");
    let policy_view = result.unwrap();
    assert_eq!(policy_view.content, valid_policy_content());
    assert_eq!(
        policy_view.description,
        Some("Test policy for integration test".to_string())
    );

    // Verify policy exists in database
    let policy: Option<HodeiPolicy> = db
        .client
        .select(("policy", "test-policy-001"))
        .await
        .expect("Failed to query database");
    assert!(
        policy.is_some(),
        "Policy should exist in database after creation"
    );
}

/// Test: Create policy with empty ID should fail
#[tokio::test]
#[traced_test]
async fn test_create_policy_empty_id() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(crate::common::MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));
    let use_case = CreatePolicyUseCase::new(adapter, validator);

    let command = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
        policy_id: "".to_string(),
        policy_content: valid_policy_content(),
        description: None,
    };

    let result = use_case.execute(command).await;
    assert!(result.is_err(), "Should fail with empty policy ID");

    match result.unwrap_err() {
        hodei_iam::features::create_policy::error::CreatePolicyError::InvalidPolicyId(_) => {}
        e => panic!("Expected InvalidPolicyId error, got: {:?}", e),
    }
}

/// Test: Create policy with empty content should fail
#[tokio::test]
#[traced_test]
async fn test_create_policy_empty_content() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(crate::common::MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));
    let use_case = CreatePolicyUseCase::new(adapter, validator);

    let command = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
        policy_id: "test-policy".to_string(),
        policy_content: "".to_string(),
        description: None,
    };

    let result = use_case.execute(command).await;
    assert!(result.is_err(), "Should fail with empty content");

    match result.unwrap_err() {
        hodei_iam::features::create_policy::error::CreatePolicyError::EmptyPolicyContent => {}
        e => panic!("Expected EmptyPolicyContent error, got: {:?}", e),
    }
}

/// Test: Create policy with invalid Cedar syntax should fail
#[tokio::test]
#[traced_test]
async fn test_create_policy_invalid_syntax() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(crate::common::MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));
    let use_case = CreatePolicyUseCase::new(adapter, validator);

    let command = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
        policy_id: "test-policy".to_string(),
        policy_content: "permit(principal action resource".to_string(), // Invalid syntax
        description: None,
    };

    let result = use_case.execute(command).await;
    assert!(result.is_err(), "Should fail with invalid syntax");

    match result.unwrap_err() {
        hodei_iam::features::create_policy::error::CreatePolicyError::InvalidPolicyContent(_) => {}
        e => panic!("Expected InvalidPolicyContent error, got: {:?}", e),
    }
}

/// Test: Create duplicate policy should fail
#[tokio::test]
#[traced_test]
async fn test_create_policy_duplicate() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(crate::common::MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));
    let use_case = CreatePolicyUseCase::new(adapter.clone(), validator.clone());

    // Create first policy
    let command1 = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
        policy_id: "duplicate-policy".to_string(),
        policy_content: valid_policy_content(),
        description: None,
    };
    let result1 = use_case.execute(command1).await;
    assert!(result1.is_ok(), "First creation should succeed");

    // Try to create duplicate
    let command2 = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
        policy_id: "duplicate-policy".to_string(),
        policy_content: valid_policy_content(),
        description: None,
    };
    let result2 = use_case.execute(command2).await;
    assert!(result2.is_err(), "Duplicate creation should fail");

    match result2.unwrap_err() {
        hodei_iam::features::create_policy::error::CreatePolicyError::PolicyAlreadyExists(_) => {}
        e => panic!("Expected PolicyAlreadyExists error, got: {:?}", e),
    }
}

/// Test: Create multiple policies sequentially
#[tokio::test]
#[traced_test]
async fn test_create_multiple_policies_sequential() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(crate::common::MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));
    let use_case = CreatePolicyUseCase::new(adapter, validator);

    // Create 5 policies
    for i in 0..5 {
        let command = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
            policy_id: format!("test-policy-{}", i),
            policy_content: valid_policy_content(),
            description: Some(format!("Policy number {}", i)),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_ok(), "Policy {} creation should succeed", i);
    }

    // Verify all policies exist
    let policies: Vec<HodeiPolicy> = db
        .client
        .select("policy")
        .await
        .expect("Failed to query policies");
    assert_eq!(policies.len(), 5, "Should have 5 policies in database");
}

/// Test: Create policy with special characters in ID
#[tokio::test]
#[traced_test]
async fn test_create_policy_special_characters_in_id() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(crate::common::MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));
    let use_case = CreatePolicyUseCase::new(adapter, validator);

    let command = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
        policy_id: "test-policy_with-special.chars".to_string(),
        policy_content: valid_policy_content(),
        description: None,
    };

    let result = use_case.execute(command).await;
    assert!(
        result.is_ok(),
        "Policy with special characters should succeed"
    );
}

/// Test: Create policy with very long content
#[tokio::test]
#[traced_test]
async fn test_create_policy_long_content() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(crate::common::MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));
    let use_case = CreatePolicyUseCase::new(adapter, validator);

    // Generate long but valid policy content
    let mut long_content = String::from("permit(\n    principal,\n    action in [\n");
    for i in 0..100 {
        long_content.push_str(&format!("        Action::\"Action{}\",\n", i));
    }
    long_content.push_str("    ],\n    resource\n);");

    let command = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
        policy_id: "long-policy".to_string(),
        policy_content: long_content.clone(),
        description: None,
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok(), "Long policy content should be accepted");

    let policy_view = result.unwrap();
    assert_eq!(policy_view.content, long_content);
}

/// Test: Create policy with Unicode characters in description
#[tokio::test]
#[traced_test]
async fn test_create_policy_unicode_description() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(crate::common::MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));
    let use_case = CreatePolicyUseCase::new(adapter, validator);

    let command = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
        policy_id: "unicode-policy".to_string(),
        policy_content: valid_policy_content(),
        description: Some("Política de prueba 测试策略 テストポリシー 🚀".to_string()),
    };

    let result = use_case.execute(command).await;
    assert!(
        result.is_ok(),
        "Policy with Unicode description should succeed"
    );
}

/// Test: Verify created_at and updated_at timestamps are set
#[tokio::test]
#[traced_test]
async fn test_create_policy_timestamps() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let schema_storage = Arc::new(crate::common::MockSchemaStorage::new());
    let validator = Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage));
    let use_case = CreatePolicyUseCase::new(adapter, validator);

    let before = chrono::Utc::now();

    let command = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
        policy_id: "timestamp-policy".to_string(),
        policy_content: valid_policy_content(),
        description: None,
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());

    let after = chrono::Utc::now();
    let policy_view = result.unwrap();

    // Timestamps should be between before and after
    assert!(
        policy_view.created_at >= before && policy_view.created_at <= after,
        "created_at should be set to current time"
    );
    assert!(
        policy_view.updated_at >= before && policy_view.updated_at <= after,
        "updated_at should be set to current time"
    );
    assert_eq!(
        policy_view.created_at, policy_view.updated_at,
        "created_at and updated_at should be equal on creation"
    );
}
