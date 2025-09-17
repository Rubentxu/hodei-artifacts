//! Clean unit tests for create_policy use case
//!
//! This module contains clean unit tests for the CreatePolicyUseCase using custom mock implementations.

use chrono::Utc;
use std::str::FromStr;
use std::sync::Arc;

use policies::domain::ids::PolicyId;
use policies::domain::policy::{Policy, PolicyVersion};
use policies::features::create_policy::dto::CreatePolicyCommand;
use policies::features::create_policy::error::CreatePolicyError;
use policies::features::create_policy::mocks::{
    MockPolicyCreationAuditor, MockPolicyCreationStorage, MockPolicyCreationValidator, MockPolicyExistenceChecker
};
use policies::features::create_policy::ports::{
    PolicyCreationAuditor, PolicyCreationStorage, PolicyCreationValidator, PolicyExistenceChecker
};
use policies::features::create_policy::use_case::CreatePolicyUseCase;
use shared::hrn::{Hrn, OrganizationId, UserId};

/// Create a test command for use in tests
fn create_test_command() -> CreatePolicyCommand {
    CreatePolicyCommand {
        policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap(),
        name: "Test Policy".to_string(),
        description: Some("A test policy".to_string()),
        organization_id: OrganizationId::from_str("hrn:hodei:iam::system:organization/test-org").unwrap(),
        content: r#"permit(principal, action == "read", resource);"#.to_string(),
        created_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
    }
}

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

#[tokio::test]
async fn test_create_policy_success() {
    // Setup mocks with success results
    let validator = MockPolicyCreationValidator::new()
        .with_validate_policy_content_result(Ok(()))
        .with_validate_policy_syntax_result(Ok(()))
        .with_validate_policy_semantics_result(Ok(()));

    let existence_checker = MockPolicyExistenceChecker::new()
        .with_exists_result(Ok(false));

    let storage = MockPolicyCreationStorage::new()
        .with_save_result(Ok(()))
        .with_create_version_result(Ok(()));

    let auditor = MockPolicyCreationAuditor::new()
        .with_log_policy_creation_result(Ok(()));

    // Create use case with mocks
    let use_case = CreatePolicyUseCase::new(
        Arc::new(validator),
        Arc::new(existence_checker),
        Arc::new(storage),
        Arc::new(auditor),
    );

    // Execute test
    let command = create_test_command();
    let result = use_case.execute(command).await;

    // Verify results
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.name, "Test Policy");
    assert_eq!(response.status, "active");
    assert_eq!(response.version, 1);
}

#[tokio::test]
async fn test_create_policy_validation_failure() {
    // Setup mocks where validation fails
    let validator = MockPolicyCreationValidator::new()
        .with_validate_policy_content_result(Err(CreatePolicyError::validation_failed("Invalid content")));

    let existence_checker = MockPolicyExistenceChecker::new();
    let storage = MockPolicyCreationStorage::new();
    let auditor = MockPolicyCreationAuditor::new();

    // Create use case with mocks
    let use_case = CreatePolicyUseCase::new(
        Arc::new(validator),
        Arc::new(existence_checker),
        Arc::new(storage),
        Arc::new(auditor),
    );

    // Execute test
    let command = create_test_command();
    let result = use_case.execute(command).await;

    // Verify results
    assert!(result.is_err());
    let error = result.unwrap_err();
    match error {
        CreatePolicyError::PolicyValidationFailed(_) => {},
        _ => panic!("Expected validation error"),
    }
}

#[tokio::test]
async fn test_create_policy_already_exists() {
    // Setup mocks where policy already exists
    let validator = MockPolicyCreationValidator::new()
        .with_validate_policy_content_result(Ok(()))
        .with_validate_policy_syntax_result(Ok(()))
        .with_validate_policy_semantics_result(Ok(()));

    let existence_checker = MockPolicyExistenceChecker::new()
        .with_exists_result(Ok(true)); // Policy already exists

    let storage = MockPolicyCreationStorage::new();
    let auditor = MockPolicyCreationAuditor::new();

    // Create use case with mocks
    let use_case = CreatePolicyUseCase::new(
        Arc::new(validator),
        Arc::new(existence_checker),
        Arc::new(storage),
        Arc::new(auditor),
    );

    // Execute test
    let command = create_test_command();
    let result = use_case.execute(command).await;

    // Verify results
    assert!(result.is_err());
    let error = result.unwrap_err();
    match error {
        CreatePolicyError::PolicyAlreadyExists(_) => {},
        _ => panic!("Expected already exists error"),
    }
}

#[tokio::test]
async fn test_create_policy_storage_failure() {
    // Setup mocks where storage fails
    let validator = MockPolicyCreationValidator::new()
        .with_validate_policy_content_result(Ok(()))
        .with_validate_policy_syntax_result(Ok(()))
        .with_validate_policy_semantics_result(Ok(()));

    let existence_checker = MockPolicyExistenceChecker::new()
        .with_exists_result(Ok(false));

    let storage = MockPolicyCreationStorage::new()
        .with_save_result(Err(CreatePolicyError::storage_error("Save failed")));

    let auditor = MockPolicyCreationAuditor::new();

    // Create use case with mocks
    let use_case = CreatePolicyUseCase::new(
        Arc::new(validator),
        Arc::new(existence_checker),
        Arc::new(storage),
        Arc::new(auditor),
    );

    // Execute test
    let command = create_test_command();
    let result = use_case.execute(command).await;

    // Verify results
    assert!(result.is_err());
    let error = result.unwrap_err();
    match error {
        CreatePolicyError::StorageError(_) => {},
        _ => panic!("Expected storage error"),
    }
}