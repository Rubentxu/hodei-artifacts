//! Clean unit tests for update_policy use case
//!
//! This module contains clean unit tests for the UpdatePolicyUseCase using custom mock implementations.

use chrono::Utc;
use std::str::FromStr;
use std::sync::Arc;

use policies::domain::ids::PolicyId;
use policies::domain::policy::{Policy, PolicyVersion};
use policies::features::update_policy::dto::{UpdatePolicyCommand, UpdatePolicyResponse};
use policies::features::update_policy::error::UpdatePolicyError;
use policies::features::update_policy::mocks::{
    MockPolicyRetriever, MockPolicyUpdateAuditor, MockPolicyUpdateStorage, MockPolicyUpdateValidator
};
use policies::features::update_policy::ports::{
    PolicyRetriever, PolicyUpdateAuditor, PolicyUpdateStorage, PolicyUpdateValidator
};
use policies::features::update_policy::use_case::UpdatePolicyUseCase;
use shared::hrn::{Hrn, UserId};

/// Create a test policy for use in tests
fn create_test_policy() -> Policy {
    let now = Utc::now();
    Policy {
        id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap(),
        name: "Test Policy".to_string(),
        description: Some("A test policy".to_string()),
        status: "active".to_string(),
        version: 1,
        created_at: now,
        updated_at: now,
        current_version: PolicyVersion {
            id: Hrn::new("hrn:hodei:iam::system:organization/test-org/policy/test-policy/versions/1").unwrap(),
            policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap(),
            version: 1,
            content: r#"permit(principal, action == "read", resource);"#.to_string(),
            created_at: now,
            created_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
        },
    }
}

/// Create a test command for use in tests
fn create_test_command() -> UpdatePolicyCommand {
    UpdatePolicyCommand {
        name: Some("Updated Test Policy".to_string()),
        description: Some("An updated test policy".to_string()),
        content: Some(r#"permit(principal, action == "write", resource);"#.to_string()),
        expected_version: Some(1),
        updated_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
    }
}

#[tokio::test]
async fn test_update_policy_success() {
    // Setup mocks with success results
    let validator = MockPolicyUpdateValidator::new()
        .with_validate_update_allowed_result(Ok(()))
        .with_validate_policy_content_result(Ok(()))
        .with_validate_policy_syntax_result(Ok(()))
        .with_validate_policy_semantics_result(Ok(()));

    let retriever = MockPolicyRetriever::new()
        .with_get_policy_result(Ok(Some(create_test_policy())));

    let storage = MockPolicyUpdateStorage::new()
        .with_update_result(Ok(()))
        .with_create_version_result(Ok(()));

    let auditor = MockPolicyUpdateAuditor::new()
        .with_log_policy_update_result(Ok(()));

    // Create use case with mocks
    let use_case = UpdatePolicyUseCase::new(
        Arc::new(validator),
        Arc::new(retriever),
        Arc::new(storage),
        Arc::new(auditor),
    );

    // Execute test
    let policy_id = PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap();
    let command = create_test_command();
    let result = use_case.execute(&policy_id, command).await;

    // Verify results
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.name, "Updated Test Policy");
    assert_eq!(response.version, 2); // Version should be incremented
}

#[tokio::test]
async fn test_update_policy_not_found() {
    // Setup mocks where policy is not found
    let validator = MockPolicyUpdateValidator::new();
    let retriever = MockPolicyRetriever::new()
        .with_get_policy_result(Ok(None)); // Policy not found

    let storage = MockPolicyUpdateStorage::new();
    let auditor = MockPolicyUpdateAuditor::new();

    // Create use case with mocks
    let use_case = UpdatePolicyUseCase::new(
        Arc::new(validator),
        Arc::new(retriever),
        Arc::new(storage),
        Arc::new(auditor),
    );

    // Execute test
    let policy_id = PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/non-existent").unwrap();
    let command = create_test_command();
    let result = use_case.execute(&policy_id, command).await;

    // Verify results
    assert!(result.is_err());
    let error = result.unwrap_err();
    match error {
        UpdatePolicyError::PolicyNotFound(_) => {},
        _ => panic!("Expected PolicyNotFound error"),
    }
}

#[tokio::test]
async fn test_update_policy_version_conflict() {
    // Setup mocks with a policy that has a different version
    let mut test_policy = create_test_policy();
    test_policy.version = 1; // Current version is 1
    
    let validator = MockPolicyUpdateValidator::new();
    let retriever = MockPolicyRetriever::new()
        .with_get_policy_result(Ok(Some(test_policy)));

    let storage = MockPolicyUpdateStorage::new();
    let auditor = MockPolicyUpdateAuditor::new();

    // Create use case with mocks
    let use_case = UpdatePolicyUseCase::new(
        Arc::new(validator),
        Arc::new(retriever),
        Arc::new(storage),
        Arc::new(auditor),
    );

    // Execute test with wrong expected version
    let policy_id = PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap();
    let mut command = create_test_command();
    command.expected_version = Some(2); // Wrong expected version
    let result = use_case.execute(&policy_id, command).await;

    // Verify results
    assert!(result.is_err());
    let error = result.unwrap_err();
    match error {
        UpdatePolicyError::PolicyVersionConflict { .. } => {},
        _ => panic!("Expected VersionMismatch error"),
    }
}

#[tokio::test]
async fn test_update_policy_validation_failure() {
    // Setup mocks where validation fails
    let validator = MockPolicyUpdateValidator::new()
        .with_validate_update_allowed_result(Ok(()))
        .with_validate_policy_content_result(Err(UpdatePolicyError::validation_failed("Invalid content")));

    let retriever = MockPolicyRetriever::new()
        .with_get_policy_result(Ok(Some(create_test_policy())));

    let storage = MockPolicyUpdateStorage::new();
    let auditor = MockPolicyUpdateAuditor::new();

    // Create use case with mocks
    let use_case = UpdatePolicyUseCase::new(
        Arc::new(validator),
        Arc::new(retriever),
        Arc::new(storage),
        Arc::new(auditor),
    );

    // Execute test
    let policy_id = PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap();
    let command = create_test_command();
    let result = use_case.execute(&policy_id, command).await;

    // Verify results
    assert!(result.is_err());
    let error = result.unwrap_err();
    match error {
        UpdatePolicyError::PolicyValidationFailed(_) => {},
        _ => panic!("Expected validation error"),
    }
}

#[tokio::test]
async fn test_update_policy_storage_failure() {
    // Setup mocks where storage fails
    let validator = MockPolicyUpdateValidator::new()
        .with_validate_update_allowed_result(Ok(()))
        .with_validate_policy_content_result(Ok(()))
        .with_validate_policy_syntax_result(Ok(()))
        .with_validate_policy_semantics_result(Ok(()));

    let retriever = MockPolicyRetriever::new()
        .with_get_policy_result(Ok(Some(create_test_policy())));

    let storage = MockPolicyUpdateStorage::new()
        .with_update_result(Err(UpdatePolicyError::storage_error("Update failed")));

    let auditor = MockPolicyUpdateAuditor::new();

    // Create use case with mocks
    let use_case = UpdatePolicyUseCase::new(
        Arc::new(validator),
        Arc::new(retriever),
        Arc::new(storage),
        Arc::new(auditor),
    );

    // Execute test
    let policy_id = PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap();
    let command = create_test_command();
    let result = use_case.execute(&policy_id, command).await;

    // Verify results
    assert!(result.is_err());
    let error = result.unwrap_err();
    match error {
        UpdatePolicyError::StorageError(_) => {},
        _ => panic!("Expected storage error"),
    }
}