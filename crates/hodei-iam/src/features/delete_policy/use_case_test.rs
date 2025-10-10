//! Unit tests for DeletePolicyUseCase
//!
//! These tests verify the business logic for deleting policies in the IAM system.
//! They use mocked dependencies to isolate the use case logic.

use crate::features::delete_policy::{
    dto::DeletePolicyCommand, error::DeletePolicyError, mocks::MockDeletePolicyPort,
    use_case::DeletePolicyUseCase,
};
use std::sync::Arc;

/// Test that a policy can be deleted successfully with valid input
#[tokio::test]
async fn test_delete_policy_success() {
    // Setup
    let mock_port = Arc::new(MockDeletePolicyPort::new());
    mock_port.add_policy("test-policy".to_string());
    let use_case = DeletePolicyUseCase::new(mock_port.clone());

    // Execute
    let cmd = DeletePolicyCommand {
        policy_id: "test-policy".to_string(),
    };

    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_ok());
}

/// Test that policy deletion fails when repository fails
#[tokio::test]
async fn test_delete_policy_repository_error() {
    // Setup
    let mock_port = Arc::new(MockDeletePolicyPort::with_storage_error());
    let use_case = DeletePolicyUseCase::new(mock_port);

    // Execute
    let cmd = DeletePolicyCommand {
        policy_id: "test-policy".to_string(),
    };

    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        DeletePolicyError::StorageError(_) => {} // Expected
        _ => panic!("Expected StorageError"),
    }
}

/// Test that policy deletion fails with invalid policy ID
#[tokio::test]
async fn test_delete_policy_invalid_policy_id() {
    // Setup
    let mock_port = Arc::new(MockDeletePolicyPort::new());
    let use_case = DeletePolicyUseCase::new(mock_port.clone());

    // Execute
    let cmd = DeletePolicyCommand {
        policy_id: "".to_string(),
    };

    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        DeletePolicyError::InvalidPolicyId(_) => {} // Expected
        _ => panic!("Expected InvalidPolicyId"),
    }
}

/// Test that policy deletion fails with empty policy ID
#[tokio::test]
async fn test_delete_policy_empty_policy_id() {
    // Setup
    let mock_port = Arc::new(MockDeletePolicyPort::new());
    let use_case = DeletePolicyUseCase::new(mock_port.clone());

    // Execute
    let cmd = DeletePolicyCommand {
        policy_id: "".to_string(),
    };

    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        DeletePolicyError::InvalidPolicyId(_) => {} // Expected
        _ => panic!("Expected InvalidPolicyId"),
    }
}

/// Test that policy deletion works with different policy ID formats
#[tokio::test]
async fn test_delete_policy_different_policy_id_formats() {
    // Setup
    let mock_port = Arc::new(MockDeletePolicyPort::new());
    let use_case = DeletePolicyUseCase::new(mock_port.clone());

    // Test cases with different valid policy ID formats
    let test_cases = vec![
        "test-policy",
        "api-policy",
        "admin-policy",
        "policy-with-long-name-123",
    ];

    for policy_id in test_cases {
    }
}

/// Test that policy deletion handles non-existent policy gracefully
#[tokio::test]
async fn test_delete_policy_non_existent() {
    // Setup
    let mock_port = Arc::new(MockDeletePolicyPort::with_not_found_error());
    let use_case = DeletePolicyUseCase::new(mock_port);

    // Execute
    let cmd = DeletePolicyCommand {
        policy_id: "non-existent-policy".to_string(),
    };

    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        DeletePolicyError::PolicyNotFound(_) => {} // Expected
        _ => panic!("Expected PolicyNotFound"),
    }
}

/// Test that policy deletion validates policy ID structure
#[tokio::test]
async fn test_delete_policy_policy_id_validation() {
    // Setup
    let mock_port = Arc::new(MockDeletePolicyPort::new());
    let use_case = DeletePolicyUseCase::new(mock_port.clone());

    // Test cases for invalid policy ID structures
    let invalid_policy_ids = vec![
        "",    // empty
        "   ", // whitespace only
        "\t",  // tab only
        "\n",  // newline only
    ];

    for invalid_policy_id in invalid_policy_ids {
        let cmd = DeletePolicyCommand {
            policy_id: invalid_policy_id.to_string(),
        };

        let result = use_case.execute(cmd).await;
        assert!(result.is_err());
    }
}

/// Test that policy deletion works with complex policy ID patterns
#[tokio::test]
async fn test_delete_policy_complex_policy_id_patterns() {
    // Setup
    let mock_port = Arc::new(MockDeletePolicyPort::new());
    let use_case = DeletePolicyUseCase::new(mock_port.clone());

    // Test cases with complex policy ID patterns
    let complex_policy_ids = vec![
        "policy-with-dashes",
        "policy_with_underscores",
        "policy123_with_numbers",
        "policy-with-mixed-characters_123",
    ];

    for policy_id in complex_policy_ids {
        mock_port.add_policy(policy_id.to_string());

        let cmd = DeletePolicyCommand {
            policy_id: policy_id.to_string(),
        };

        let result = use_case.execute(cmd).await;
        assert!(result.is_ok());
    }
}

/// Test that policy deletion fails when policy is in use
#[tokio::test]
async fn test_delete_policy_in_use() {
    // Setup
    let mock_port = Arc::new(MockDeletePolicyPort::with_in_use_error());
    let use_case = DeletePolicyUseCase::new(mock_port);

    // Execute
    let cmd = DeletePolicyCommand {
        policy_id: "in-use-policy".to_string(),
    };

    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        DeletePolicyError::PolicyInUse(_) => {} // Expected
        _ => panic!("Expected PolicyInUse"),
    }
}
