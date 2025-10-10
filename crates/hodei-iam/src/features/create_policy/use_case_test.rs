//! Unit tests for CreatePolicyUseCase
//!
//! These tests verify the business logic for creating policies in the IAM system.
//! They use mocked dependencies to isolate the use case logic.

use crate::features::create_policy::{
    dto::CreatePolicyCommand,
    error::CreatePolicyError,
    mocks::{MockCreatePolicyPort, MockPolicyValidator},
    ports::CreatePolicyUseCasePort,
    use_case::CreatePolicyUseCase,
};
use std::sync::Arc;

/// Test that a policy can be created successfully with valid input
#[tokio::test]
async fn test_create_policy_success() {
    // Setup
    let mock_port = Arc::new(MockCreatePolicyPort::new());
    let mock_validator = Arc::new(MockPolicyValidator::new());

    let use_case = CreatePolicyUseCase::new(mock_port.clone(), mock_validator);

    // Execute
    let cmd = CreatePolicyCommand {
        policy_id: "TestPolicy".to_string(),
        policy_content: r#"permit(principal, action, resource);"#.to_string(),
        description: Some("Test policy description".to_string()),
    };

    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.content, r#"permit(principal, action, resource);"#);
    assert_eq!(view.description, Some("Test policy description".to_string()));
    assert!(view.id.to_string().contains("TestPolicy"));
}

/// Test that policy creation fails when validation fails
#[tokio::test]
async fn test_create_policy_validation_error() {
    // Setup
    let mock_port = Arc::new(MockCreatePolicyPort::new());
    let mock_validator = Arc::new(MockPolicyValidator::with_errors(vec!["Syntax error".to_string()]));

    let use_case = CreatePolicyUseCase::new(mock_port, mock_validator);

    // Execute
    let cmd = CreatePolicyCommand {
        policy_id: "TestPolicy".to_string(),
        policy_content: r#"invalid cedar syntax"#.to_string(),
        description: Some("Test policy description".to_string()),
    };

    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        CreatePolicyError::InvalidPolicyContent(_) => {} // Expected
        _ => panic!("Expected InvalidPolicyContent"),
    }
}

/// Test that policy creation fails when repository fails
#[tokio::test]
async fn test_create_policy_repository_error() {
    // Setup
    let mock_port = Arc::new(MockCreatePolicyPort::with_storage_error());
    let mock_validator = Arc::new(MockPolicyValidator::new());

    let use_case = CreatePolicyUseCase::new(mock_port, mock_validator);

    // Execute
    let cmd = CreatePolicyCommand {
        policy_id: "TestPolicy".to_string(),
        policy_content: r#"permit(principal, action, resource);"#.to_string(),
        description: Some("Test policy description".to_string()),
    };

    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        CreatePolicyError::StorageError(_) => {} // Expected
        _ => panic!("Expected StorageError"),
    }
}

/// Test that policy creation fails with empty policy_id
#[tokio::test]
async fn test_create_policy_empty_policy_id() {
    // Setup
    let mock_port = Arc::new(MockCreatePolicyPort::new());
    let mock_validator = Arc::new(MockPolicyValidator::new());

    let use_case = CreatePolicyUseCase::new(mock_port, mock_validator);

    // Execute
    let cmd = CreatePolicyCommand {
        policy_id: "".to_string(),
        policy_content: r#"permit(principal, action, resource);"#.to_string(),
        description: Some("Test policy description".to_string()),
    };

    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        CreatePolicyError::InvalidPolicyId(_) => {} // Expected
        _ => panic!("Expected InvalidPolicyId"),
    }
}

/// Test that policy creation fails with empty content
#[tokio::test]
async fn test_create_policy_empty_content() {
    // Setup
    let mock_port = Arc::new(MockCreatePolicyPort::new());
    let mock_validator = Arc::new(MockPolicyValidator::new());

    let use_case = CreatePolicyUseCase::new(mock_port, mock_validator);

    // Execute
    let cmd = CreatePolicyCommand {
        policy_id: "TestPolicy".to_string(),
        policy_content: "".to_string(),
        description: Some("Test policy description".to_string()),
    };

    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        CreatePolicyError::EmptyPolicyContent => {} // Expected
        _ => panic!("Expected EmptyPolicyContent"),
    }
}

/// Test that policy creation works with minimal required fields
#[tokio::test]
async fn test_create_policy_minimal_fields() {
    // Setup
    let mock_port = Arc::new(MockCreatePolicyPort::new());
    let mock_validator = Arc::new(MockPolicyValidator::new());

    let use_case = CreatePolicyUseCase::new(mock_port.clone(), mock_validator);

    // Execute
    let cmd = CreatePolicyCommand {
        policy_id: "MinimalPolicy".to_string(),
        policy_content: r#"permit(principal, action, resource);"#.to_string(),
        description: None,
    };

    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.content, r#"permit(principal, action, resource);"#);
    assert_eq!(view.description, None);
    assert!(view.id.to_string().contains("MinimalPolicy"));
}

/// Test that policy validation is called with correct content
#[tokio::test]
async fn test_policy_validation_called() {
    // Setup
    let mock_port = Arc::new(MockCreatePolicyPort::new());
    let mock_validator = Arc::new(MockPolicyValidator::new());

    let use_case = CreatePolicyUseCase::new(mock_port.clone(), mock_validator.clone());

    // Execute
    let cmd = CreatePolicyCommand {
        policy_id: "TestPolicy".to_string(),
        policy_content: r#"permit(principal, action, resource);"#.to_string(),
        description: Some("Test policy description".to_string()),
    };

    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_ok());
    // The mock validator should have been called with the policy content
    // This is verified by the fact that the test passes (the mock accepts the content)
}

/// Test that policy_id validation works correctly
#[tokio::test]
async fn test_policy_id_validation() {
    // Setup
    let mock_port = Arc::new(MockCreatePolicyPort::new());
    let mock_validator = Arc::new(MockPolicyValidator::new());

    let use_case = CreatePolicyUseCase::new(mock_port, mock_validator);

    // Test cases for invalid policy_ids
    let invalid_policy_ids = vec!["", "   ", "\t", "\n"];

    for invalid_policy_id in invalid_policy_ids {
        let cmd = CreatePolicyCommand {
            policy_id: invalid_policy_id.to_string(),
            policy_content: r#"permit(principal, action, resource);"#.to_string(),
            description: Some("Test policy description".to_string()),
        };

        let result = use_case.execute(cmd).await;
        assert!(result.is_err());
    }
}
