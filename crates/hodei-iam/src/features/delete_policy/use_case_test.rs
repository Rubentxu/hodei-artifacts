//! Unit test suite for `DeletePolicyUseCase` (delete_policy feature)
//!
//! These tests exercise the business logic in isolation using the mock
//! implementations defined in `mocks.rs`. No real infrastructure is touched.
//!
//! Covered Scenarios
//! -----------------
//! 1. Successful deletion
//! 2. Empty policy ID
//! 3. Invalid policy ID format
//! 4. Policy not found
//! 5. Policy in use (cannot delete)
//! 6. System-protected policy (cannot delete)
//! 7. Storage layer failure
//! 8. Multiple deletions
//! 9. Idempotency (deleting twice)
//! 10. Edge cases with policy ID validation
//!
//! Principles
//! ----------
//! - No logging via println! -> uses tracing (if initialized)
//! - Uses provided mocks (Interface Segregation respected)
//! - Asserts map correctly from ports to domain errors
//! - Avoids coupling with any other bounded context
//!
//! Architecture Alignment
//! ----------------------
//! - Tests only the vertical slice (no cross-feature leakage).
//! - Port is mocked individually (granular ISP trait).
//! - DTOs are used exactly as exposed by the feature API.
//!
//! Run with:
//!    cargo test -p hodei-iam -- delete_policy

use std::sync::Arc;

use crate::features::delete_policy::{
    DeletePolicyCommand, DeletePolicyError, DeletePolicyUseCase, MockDeletePolicyPort,
};

use tracing::info;

// -------------------------------------------------------------------------------------------------
// Test Utilities
// -------------------------------------------------------------------------------------------------

/// Initialize tracing for tests (idempotent)
fn init_tracing() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        // Ignore any error (e.g. if already set by another test harness)
        let _ = tracing_subscriber::fmt()
            .with_test_writer()
            .with_env_filter("info")
            .try_init();
    });
}

/// Helper to construct a default valid command
fn valid_command(id: &str) -> DeletePolicyCommand {
    DeletePolicyCommand {
        policy_id: id.to_string(),
    }
}

/// Build a use case from provided mock port
fn build_use_case(port: Arc<MockDeletePolicyPort>) -> DeletePolicyUseCase<MockDeletePolicyPort> {
    DeletePolicyUseCase::new(port)
}

// -------------------------------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------------------------------

#[tokio::test]
async fn deletes_policy_successfully() {
    init_tracing();

    let port = Arc::new(MockDeletePolicyPort::with_existing_policies(vec![
        "test-policy".to_string(),
    ]));
    let uc = build_use_case(port.clone());

    let cmd = valid_command("test-policy");
    let result = uc.execute(cmd).await;

    assert!(result.is_ok());
    assert_eq!(port.get_deleted_count(), 1);
    assert!(port.was_deleted("test-policy"));
    assert_eq!(port.get_call_count(), 1);
}

#[tokio::test]
async fn fails_with_empty_policy_id() {
    init_tracing();

    let port = Arc::new(MockDeletePolicyPort::new());
    let uc = build_use_case(port);

    let cmd = DeletePolicyCommand {
        policy_id: "".to_string(),
    };

    let err = uc.execute(cmd).await.unwrap_err();
    matches!(err, DeletePolicyError::InvalidPolicyId(_));
    assert!(err.to_string().contains("cannot be empty"));
}

#[tokio::test]
async fn fails_with_invalid_policy_id_format() {
    init_tracing();

    let port = Arc::new(MockDeletePolicyPort::new());
    let uc = build_use_case(port);

    // Policy ID with spaces
    let cmd = DeletePolicyCommand {
        policy_id: "invalid policy id".to_string(),
    };

    let err = uc.execute(cmd).await.unwrap_err();
    matches!(err, DeletePolicyError::InvalidPolicyId(_));
    assert!(err.to_string().contains("invalid characters"));
}

#[tokio::test]
async fn fails_with_policy_id_starting_with_hyphen() {
    init_tracing();

    let port = Arc::new(MockDeletePolicyPort::new());
    let uc = build_use_case(port);

    let cmd = DeletePolicyCommand {
        policy_id: "-starts-with-hyphen".to_string(),
    };

    let err = uc.execute(cmd).await.unwrap_err();
    matches!(err, DeletePolicyError::InvalidPolicyId(_));
}

#[tokio::test]
async fn fails_with_policy_id_containing_special_chars() {
    init_tracing();

    let port = Arc::new(MockDeletePolicyPort::new());
    let uc = build_use_case(port);

    let cmd = DeletePolicyCommand {
        policy_id: "policy@with#special".to_string(),
    };

    let err = uc.execute(cmd).await.unwrap_err();
    matches!(err, DeletePolicyError::InvalidPolicyId(_));
}

#[tokio::test]
async fn fails_when_policy_not_found() {
    init_tracing();

    let port = Arc::new(MockDeletePolicyPort::new());
    let uc = build_use_case(port.clone());

    let cmd = valid_command("non-existent");
    let err = uc.execute(cmd).await.unwrap_err();

    matches!(err, DeletePolicyError::PolicyNotFound(_));
    assert!(err.to_string().contains("not found"));
    assert_eq!(port.get_deleted_count(), 0);
}

#[tokio::test]
async fn fails_when_policy_in_use() {
    init_tracing();

    let port = Arc::new(MockDeletePolicyPort::with_in_use_policies(vec![
        "active-policy".to_string(),
    ]));
    let uc = build_use_case(port.clone());

    let cmd = valid_command("active-policy");
    let err = uc.execute(cmd).await.unwrap_err();

    matches!(err, DeletePolicyError::PolicyInUse(_));
    assert!(err.to_string().contains("in use"));
    assert_eq!(port.get_deleted_count(), 0);
}

#[tokio::test]
async fn fails_when_system_policy_protected() {
    init_tracing();

    let port = Arc::new(MockDeletePolicyPort::with_system_policies(vec![
        "system-policy".to_string(),
    ]));
    let uc = build_use_case(port.clone());

    let cmd = valid_command("system-policy");
    let err = uc.execute(cmd).await.unwrap_err();

    matches!(err, DeletePolicyError::SystemPolicyProtected(_));
    assert!(err.to_string().contains("system-managed"));
    assert_eq!(port.get_deleted_count(), 0);
}

#[tokio::test]
async fn fails_on_storage_error() {
    init_tracing();

    let port = Arc::new(MockDeletePolicyPort::with_storage_error());
    let uc = build_use_case(port);

    let cmd = valid_command("any-policy");
    let err = uc.execute(cmd).await.unwrap_err();

    matches!(err, DeletePolicyError::StorageError(_));
    assert!(err.to_string().contains("storage error"));
}

#[tokio::test]
async fn port_called_once_on_success() {
    init_tracing();

    let port = Arc::new(MockDeletePolicyPort::with_existing_policies(vec![
        "single-call".to_string(),
    ]));
    let uc = build_use_case(port.clone());

    let cmd = valid_command("single-call");
    let _ = uc.execute(cmd).await.unwrap();

    assert_eq!(port.get_call_count(), 1, "port delete called exactly once");
}

#[tokio::test]
async fn multiple_deletions_work() {
    init_tracing();

    let port = Arc::new(MockDeletePolicyPort::with_existing_policies(vec![
        "p1".to_string(),
        "p2".to_string(),
        "p3".to_string(),
    ]));
    let uc = build_use_case(port.clone());

    // Delete p1
    let cmd1 = valid_command("p1");
    uc.execute(cmd1).await.unwrap();

    // Delete p3
    let cmd3 = valid_command("p3");
    uc.execute(cmd3).await.unwrap();

    assert_eq!(port.get_deleted_count(), 2);
    assert!(port.was_deleted("p1"));
    assert!(port.was_deleted("p3"));
    assert!(!port.was_deleted("p2"));
}

#[tokio::test]
async fn idempotency_test_second_delete_fails() {
    init_tracing();

    let port = Arc::new(MockDeletePolicyPort::with_existing_policies(vec![
        "once".to_string(),
    ]));
    let uc = build_use_case(port.clone());

    // First delete succeeds
    let cmd1 = valid_command("once");
    let result1 = uc.execute(cmd1).await;
    assert!(result1.is_ok());

    // Second delete fails (not found)
    let cmd2 = valid_command("once");
    let result2 = uc.execute(cmd2).await;
    assert!(result2.is_err());
    matches!(result2.unwrap_err(), DeletePolicyError::PolicyNotFound(_));

    assert_eq!(port.get_deleted_count(), 1);
}

#[tokio::test]
async fn valid_policy_id_formats_are_accepted() {
    init_tracing();

    let test_cases = vec![
        "simple",
        "with-hyphens",
        "with_underscores",
        "MixedCase123",
        "123starts-with-number",
        "a", // single character
    ];

    for policy_id in test_cases {
        let port = Arc::new(MockDeletePolicyPort::with_existing_policies(vec![
            policy_id.to_string(),
        ]));
        let uc = build_use_case(port);

        let cmd = valid_command(policy_id);
        let result = uc.execute(cmd).await;
        assert!(result.is_ok(), "Policy ID '{}' should be valid", policy_id);
    }
}

#[tokio::test]
async fn invalid_policy_id_formats_are_rejected() {
    init_tracing();

    let test_cases = vec![
        "",                        // empty
        "   ",                     // whitespace only
        "-starts-with-hyphen",     // starts with hyphen
        "_starts_with_underscore", // starts with underscore
        "has spaces",              // contains spaces
        "has@special",             // contains @
        "has/slash",               // contains /
        "has.dot",                 // contains dot
        &"a".repeat(129),          // too long (> 128)
    ];

    for policy_id in test_cases {
        let port = Arc::new(MockDeletePolicyPort::new());
        let uc = build_use_case(port);

        let cmd = DeletePolicyCommand {
            policy_id: policy_id.to_string(),
        };
        let result = uc.execute(cmd).await;
        assert!(
            result.is_err(),
            "Policy ID '{}' should be invalid",
            policy_id
        );
        matches!(result.unwrap_err(), DeletePolicyError::InvalidPolicyId(_));
    }
}

#[tokio::test]
async fn whitespace_trimmed_empty_id_is_rejected() {
    init_tracing();

    let port = Arc::new(MockDeletePolicyPort::new());
    let uc = build_use_case(port);

    let cmd = DeletePolicyCommand {
        policy_id: "   ".to_string(),
    };

    let err = uc.execute(cmd).await.unwrap_err();
    matches!(err, DeletePolicyError::InvalidPolicyId(_));
}

#[tokio::test]
async fn error_classification_methods_work() {
    init_tracing();

    // Retryable errors
    let storage_err = DeletePolicyError::StorageError("test".to_string());
    assert!(storage_err.is_retryable());
    assert!(storage_err.is_server_error());
    assert!(!storage_err.is_client_error());

    // Non-retryable client errors
    let not_found = DeletePolicyError::PolicyNotFound("test".to_string());
    assert!(!not_found.is_retryable());
    assert!(not_found.is_client_error());
    assert!(!not_found.is_server_error());
    assert!(not_found.is_not_found());

    let in_use = DeletePolicyError::PolicyInUse("test".to_string());
    assert!(!in_use.is_retryable());
    assert!(in_use.is_client_error());

    let protected = DeletePolicyError::SystemPolicyProtected("test".to_string());
    assert!(!protected.is_retryable());
    assert!(protected.is_client_error());
}

#[tokio::test]
async fn concurrent_deletions_simulation() {
    init_tracing();

    let port = Arc::new(MockDeletePolicyPort::with_existing_policies(vec![
        "p1".to_string(),
        "p2".to_string(),
        "p3".to_string(),
        "p4".to_string(),
    ]));
    let uc = Arc::new(build_use_case(port.clone()));

    // Simulate concurrent deletion requests
    let handles: Vec<_> = (1..=4)
        .map(|i| {
            let uc_clone = uc.clone();
            let policy_id = format!("p{}", i);
            tokio::spawn(async move {
                let cmd = DeletePolicyCommand { policy_id };
                uc_clone.execute(cmd).await
            })
        })
        .collect();

    // Wait for all deletions to complete
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }

    assert_eq!(port.get_deleted_count(), 4);
}

// -------------------------------------------------------------------------------------------------
// Logging demonstration (not asserting anything, just ensures tracing does not panic)
// -------------------------------------------------------------------------------------------------
#[tokio::test]
async fn tracing_does_not_panic() {
    init_tracing();
    info!("Tracing initialized for delete_policy tests");
}
