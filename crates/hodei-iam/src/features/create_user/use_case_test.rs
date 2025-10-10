//! Unit tests for CreateUserUseCase
//!
//! These tests verify the business logic for creating users in the IAM system.
//! They use mocked dependencies to isolate the use case logic.

use crate::features::create_user::{
    dto::CreateUserCommand,
    error::CreateUserError,
    mocks::{MockCreateUserPort, MockHrnGenerator},   
    use_case::CreateUserUseCase,
};
use kernel::domain::Hrn;
use std::sync::Arc;

/// Test that a user can be created successfully with valid input
#[tokio::test]
async fn test_create_user_success() {
    // Setup
    let mock_port = Arc::new(MockCreateUserPort::new());
    let mock_hrn_generator = Arc::new(MockHrnGenerator::new(Hrn::new(
        "hodei".to_string(),
        "iam".to_string(),
        "default".to_string(),
        "User".to_string(),
        "test-user-123".to_string(),
    )));

    let use_case = CreateUserUseCase::new(mock_port.clone(), mock_hrn_generator);

    // Execute
    let cmd = CreateUserCommand {
        name: "John Doe".to_string(),
        email: "john.doe@example.com".to_string(),
        tags: vec!["admin".to_string()],
    };

    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.name, "John Doe");
    assert_eq!(view.email, "john.doe@example.com");
    assert_eq!(view.tags, vec!["admin".to_string()]);
    assert_eq!(view.hrn, "hrn:hodei:iam::default:User/test-user-123");
}

/// Test that user creation fails when the repository fails
#[tokio::test]
async fn test_create_user_repository_error() {
    // Setup
    let mock_port = Arc::new(MockCreateUserPort::failing());
    let mock_hrn_generator = Arc::new(MockHrnGenerator::new(Hrn::new(
        "hodei".to_string(),
        "iam".to_string(),
        "default".to_string(),
        "User".to_string(),
        "test-user-123".to_string(),
    )));

    let use_case = CreateUserUseCase::new(mock_port, mock_hrn_generator);

    // Execute
    let cmd = CreateUserCommand {
        name: "John Doe".to_string(),
        email: "john.doe@example.com".to_string(),
        tags: vec!["admin".to_string()],
    };

    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        CreateUserError::PersistenceError(_) => {} // Expected
        _ => panic!("Expected PersistenceError"),
    }
}

/// Test that user creation works with empty name (no validation in current implementation)
#[tokio::test]
async fn test_create_user_empty_name() {
    // Setup
    let mock_port = Arc::new(MockCreateUserPort::new());
    let mock_hrn_generator = Arc::new(MockHrnGenerator::new(Hrn::new(
        "hodei".to_string(),
        "iam".to_string(),
        "default".to_string(),
        "User".to_string(),
        "test-user-123".to_string(),
    )));

    let use_case = CreateUserUseCase::new(mock_port, mock_hrn_generator);

    // Execute
    let cmd = CreateUserCommand {
        name: "".to_string(),
        email: "john.doe@example.com".to_string(),
        tags: vec!["admin".to_string()],
    };

    let result = use_case.execute(cmd).await;

    // Assert - current implementation doesn't validate empty names
    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.name, "");
    assert_eq!(view.email, "john.doe@example.com");
    assert_eq!(view.tags, vec!["admin".to_string()]);
}

/// Test that user creation works with invalid email (no validation in current implementation)
#[tokio::test]
async fn test_create_user_invalid_email() {
    // Setup
    let mock_port = Arc::new(MockCreateUserPort::new());
    let mock_hrn_generator = Arc::new(MockHrnGenerator::new(Hrn::new(
        "hodei".to_string(),
        "iam".to_string(),
        "default".to_string(),
        "User".to_string(),
        "test-user-123".to_string(),
    )));

    let use_case = CreateUserUseCase::new(mock_port, mock_hrn_generator);

    // Execute
    let cmd = CreateUserCommand {
        name: "John Doe".to_string(),
        email: "invalid-email".to_string(),
        tags: vec!["admin".to_string()],
    };

    let result = use_case.execute(cmd).await;

    // Assert - current implementation doesn't validate email format
    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.name, "John Doe");
    assert_eq!(view.email, "invalid-email");
    assert_eq!(view.tags, vec!["admin".to_string()]);
}

/// Test that user creation works with minimal required fields
#[tokio::test]
async fn test_create_user_minimal_fields() {
    // Setup
    let mock_port = Arc::new(MockCreateUserPort::new());
    let mock_hrn_generator = Arc::new(MockHrnGenerator::new(Hrn::new(
        "hodei".to_string(),
        "iam".to_string(),
        "default".to_string(),
        "User".to_string(),
        "test-user-123".to_string(),
    )));

    let use_case = CreateUserUseCase::new(mock_port.clone(), mock_hrn_generator);

    // Execute
    let cmd = CreateUserCommand {
        name: "Jane Smith".to_string(),
        email: "jane.smith@example.com".to_string(),
        tags: vec![],
    };

    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.name, "Jane Smith");
    assert_eq!(view.email, "jane.smith@example.com");
    assert_eq!(view.tags, Vec::<String>::new());
    assert_eq!(view.hrn, "hrn:hodei:iam::default:User/test-user-123");
}

/// Test that HRN generation is used correctly
#[tokio::test]
async fn test_hrn_generation_used() {
    // Setup
    let mock_port = Arc::new(MockCreateUserPort::new());
    let expected_hrn = Hrn::new(
        "hodei".to_string(),
        "iam".to_string(),
        "default".to_string(),
        "User".to_string(),
        "specific-user-id".to_string(),
    );
    let mock_hrn_generator = Arc::new(MockHrnGenerator::new(expected_hrn.clone()));

    let use_case = CreateUserUseCase::new(mock_port.clone(), mock_hrn_generator);

    // Execute
    let cmd = CreateUserCommand {
        name: "Test User".to_string(),
        email: "test@example.com".to_string(),
        tags: vec!["test".to_string()],
    };

    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.hrn, expected_hrn.to_string());
}
