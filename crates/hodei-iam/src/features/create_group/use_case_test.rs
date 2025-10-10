//! Unit tests for CreateGroupUseCase
//!
//! These tests verify the business logic for creating groups in the IAM system.
//! They use mocked dependencies to isolate the use case logic.

use crate::features::create_group::{
    dto::CreateGroupCommand,
    error::CreateGroupError,
    mocks::{MockCreateGroupPort, MockHrnGenerator},   
    use_case::CreateGroupUseCase,
};
use kernel::domain::Hrn;
use std::sync::Arc;

/// Test that a group can be created successfully with valid input
#[tokio::test]
async fn test_create_group_success() {
    // Setup
    let mock_port = Arc::new(MockCreateGroupPort::new());
    let mock_hrn_generator = Arc::new(MockHrnGenerator::new(Hrn::new(
        "hodei".to_string(),
        "iam".to_string(),
        "default".to_string(),
        "Group".to_string(),
        "test-group-123".to_string(),
    )));

    let use_case = CreateGroupUseCase::new(mock_port.clone(), mock_hrn_generator);

    // Execute
    let cmd = CreateGroupCommand {
        group_name: "Admins".to_string(),
        tags: vec!["admin".to_string()],
    };

    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.name, "Admins");
    assert_eq!(view.hrn, "hrn:hodei:iam::default:Group/test-group-123");
    assert_eq!(view.tags, Vec::<String>::new());
}

/// Test that group creation fails when the repository fails
#[tokio::test]
async fn test_create_group_repository_error() {
    // Setup
    let mock_port = Arc::new(MockCreateGroupPort::failing());
    let mock_hrn_generator = Arc::new(MockHrnGenerator::new(Hrn::new(
        "hodei".to_string(),
        "iam".to_string(),
        "default".to_string(),
        "Group".to_string(),
        "test-group-123".to_string(),
    )));

    let use_case = CreateGroupUseCase::new(mock_port, mock_hrn_generator);

    // Execute
    let cmd = CreateGroupCommand {
        group_name: "Admins".to_string(),
        tags: vec!["admin".to_string()],
    };

    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        CreateGroupError::PersistenceError(_) => {} // Expected
        _ => panic!("Expected PersistenceError"),
    }
}

/// Test that group creation works with empty name (no validation in current implementation)
#[tokio::test]
async fn test_create_group_empty_name() {
    // Setup
    let mock_port = Arc::new(MockCreateGroupPort::new());
    let mock_hrn_generator = Arc::new(MockHrnGenerator::new(Hrn::new(
        "hodei".to_string(),
        "iam".to_string(),
        "default".to_string(),
        "Group".to_string(),
        "test-group-123".to_string(),
    )));

    let use_case = CreateGroupUseCase::new(mock_port, mock_hrn_generator);

    // Execute
    let cmd = CreateGroupCommand {
        group_name: "".to_string(),
        tags: vec![],
    };

    let result = use_case.execute(cmd).await;

    // Assert - current implementation doesn't validate empty names
    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.name, "");
    assert_eq!(view.hrn, "hrn:hodei:iam::default:Group/test-group-123");
}

/// Test that HRN generation is used correctly
#[tokio::test]
async fn test_hrn_generation_used() {
    // Setup
    let mock_port = Arc::new(MockCreateGroupPort::new());
    let expected_hrn = Hrn::new(
        "hodei".to_string(),
        "iam".to_string(),
        "default".to_string(),
        "Group".to_string(),
        "specific-group-id".to_string(),
    );
    let mock_hrn_generator = Arc::new(MockHrnGenerator::new(expected_hrn.clone()));

    let use_case = CreateGroupUseCase::new(mock_port.clone(), mock_hrn_generator);

    // Execute
    let cmd = CreateGroupCommand {
        group_name: "Test Group".to_string(),
        tags: vec!["test".to_string()],
    };

    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.hrn, expected_hrn.to_string());
    assert_eq!(view.name, "Test Group");
}
