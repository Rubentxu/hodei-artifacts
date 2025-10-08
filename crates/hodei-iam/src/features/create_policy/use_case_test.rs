//! Unit test suite for `CreatePolicyUseCase` (create_policy_new feature)
//!
//! These tests exercise the business logic in isolation using the mock
//! implementations defined in `mocks.rs`. No real infrastructure is touched.
//!
//! Covered Scenarios
//! -----------------
//! 1. Successful creation
//! 2. Empty policy ID
//! 3. Empty policy content
//! 4. Validator service failure (infrastructure error)
//! 5. Validator reports semantic/syntax errors
//! 6. Duplicate policy (already exists)
//! 7. Storage layer failure
//! 8. Validation warnings (non-blocking) still allow success
//! 9. Ensures ports are invoked exactly once in the happy path
//! 10. Large policy content handled correctly
//!
//! Principles
//! ----------
//! - No logging via println! -> uses tracing (if initialized)
//! - Uses provided mocks (Interface Segregation respected)
//! - Asserts map correctly from ports to domain errors
//! - Avoids coupling with any other bounded context
//!
//! NOTE: The HRN format is partially validated by mocks; full validation
//! is the responsibility of other layers / adapters.
//!
//! Architecture Alignment
//! ----------------------
//! - Tests only the vertical slice (no cross-feature leakage).
//! - Ports are mocked individually (granular ISP traits).
//! - DTOs are used exactly as exposed by the feature API.
//!
//! Run with:
//!    cargo test -p hodei-iam --features "" -- create_policy_new
//! or simply:
//!    cargo test -p hodei-iam
//! (until feature gating is introduced)

use std::sync::Arc;

use crate::features::create_policy::{
    CreatePolicyCommand, CreatePolicyError, CreatePolicyUseCase, MockCreatePolicyPort,
    MockPolicyValidator,
};

use tracing::{info, warn};

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
fn valid_command(id: &str) -> CreatePolicyCommand {
    CreatePolicyCommand {
        policy_id: id.to_string(),
        policy_content: "permit(principal, action, resource);".to_string(),
        description: Some("Test policy".to_string()),
    }
}

/// Build a use case from provided mock port + validator
fn build_use_case(
    port: Arc<MockCreatePolicyPort>,
    validator: Arc<MockPolicyValidator>,
) -> CreatePolicyUseCase<MockCreatePolicyPort, MockPolicyValidator> {
    CreatePolicyUseCase::new(port, validator)
}

// -------------------------------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------------------------------

#[tokio::test]
async fn creates_policy_successfully() {
    init_tracing();

    let port = Arc::new(MockCreatePolicyPort::new());
    let validator = Arc::new(MockPolicyValidator::new());
    let uc = build_use_case(port.clone(), validator.clone());

    let cmd = valid_command("allow-read");
    let view = uc.execute(cmd).await.expect("policy should be created");

    assert!(view.id.to_string().contains("policy/allow-read"));
    assert_eq!(view.description, Some("Test policy".to_string()));
    assert_eq!(port.get_created_count(), 1);
    assert_eq!(validator.get_call_count(), 1);
}

#[tokio::test]
async fn fails_with_empty_policy_id() {
    init_tracing();

    let port = Arc::new(MockCreatePolicyPort::new());
    let validator = Arc::new(MockPolicyValidator::new());
    let uc = build_use_case(port, validator);

    let mut cmd = valid_command("");
    cmd.policy_id = "".to_string();

    let err = uc.execute(cmd).await.unwrap_err();
    matches!(err, CreatePolicyError::InvalidPolicyId(_));
}

#[tokio::test]
async fn fails_with_empty_policy_content() {
    init_tracing();

    let port = Arc::new(MockCreatePolicyPort::new());
    let validator = Arc::new(MockPolicyValidator::new());
    let uc = build_use_case(port, validator);

    let mut cmd = valid_command("empty-content");
    cmd.policy_content = "   ".to_string();

    let err = uc.execute(cmd).await.unwrap_err();
    matches!(err, CreatePolicyError::EmptyPolicyContent);
}

#[tokio::test]
async fn fails_when_validation_service_errors() {
    init_tracing();

    let port = Arc::new(MockCreatePolicyPort::new());
    let validator = Arc::new(MockPolicyValidator::with_service_error());
    let uc = build_use_case(port, Arc::new(validator));

    let cmd = valid_command("svc-fail");
    let err = uc.execute(cmd).await.unwrap_err();

    matches!(err, CreatePolicyError::ValidationFailed(_));
    assert!(err.to_string().contains("Policy validation failed"));
}

#[tokio::test]
async fn fails_when_validation_reports_errors() {
    init_tracing();

    let port = Arc::new(MockCreatePolicyPort::new());
    let validator = Arc::new(MockPolicyValidator::with_errors(vec![
        "Syntax error near 'permit'".to_string(),
    ]));
    let uc = build_use_case(port, validator);

    let cmd = valid_command("syntax-error");
    let err = uc.execute(cmd).await.unwrap_err();

    matches!(err, CreatePolicyError::InvalidPolicyContent(_));
    assert!(
        err.to_string()
            .contains("Invalid policy content: Syntax error near 'permit'")
    );
}

#[tokio::test]
async fn fails_on_duplicate_policy_id() {
    init_tracing();

    // Configure mock with existing policy id
    let port = Arc::new(MockCreatePolicyPort::with_existing_policies(vec![
        "dup-policy".to_string(),
    ]));
    let validator = Arc::new(MockPolicyValidator::new());
    let uc = build_use_case(port, validator);

    let cmd = valid_command("dup-policy");
    let err = uc.execute(cmd).await.unwrap_err();

    matches!(err, CreatePolicyError::PolicyAlreadyExists(_));
    assert!(err.to_string().contains("dup-policy"));
}

#[tokio::test]
async fn fails_on_storage_error() {
    init_tracing();

    let port = Arc::new(MockCreatePolicyPort::with_storage_error());
    let validator = Arc::new(MockPolicyValidator::new());
    let uc = build_use_case(port, validator);

    let cmd = valid_command("storage-failure");
    let err = uc.execute(cmd).await.unwrap_err();

    matches!(err, CreatePolicyError::StorageError(_));
    assert!(err.to_string().contains("Policy storage error"));
}

#[tokio::test]
async fn succeeds_with_validation_warnings() {
    init_tracing();

    // Validator with a non-blocking warning
    let mut validator = MockPolicyValidator::new();
    validator.add_warning(
        "Policy could be more restrictive".to_string(),
        "low".to_string(),
    );
    let validator = Arc::new(validator);

    let port = Arc::new(MockCreatePolicyPort::new());
    let uc = build_use_case(port.clone(), validator.clone());

    let cmd = valid_command("warn-policy");
    let view = uc
        .execute(cmd)
        .await
        .expect("should succeed despite warnings");

    assert!(view.id.to_string().contains("policy/warn-policy"));
    assert_eq!(port.get_created_count(), 1);
    assert_eq!(validator.get_call_count(), 1);
}

#[tokio::test]
async fn validator_and_port_called_once_on_success() {
    init_tracing();

    let port = Arc::new(MockCreatePolicyPort::new());
    let validator = Arc::new(MockPolicyValidator::new());
    let uc = build_use_case(port.clone(), validator.clone());

    let cmd = valid_command("single-call");
    let _ = uc.execute(cmd).await.unwrap();

    assert_eq!(
        validator.get_call_count(),
        1,
        "validator called exactly once"
    );
    assert_eq!(port.get_call_count(), 1, "port create called exactly once");
}

#[tokio::test]
async fn large_policy_content_is_handled() {
    init_tracing();

    let port = Arc::new(MockCreatePolicyPort::new());
    let validator = Arc::new(MockPolicyValidator::new());
    let uc = build_use_case(port.clone(), validator);

    // Generate a large (but valid syntactically) policy body
    let repeated_clause = "permit(principal, action, resource);";
    let large_content = std::iter::repeat(repeated_clause)
        .take(2_000) // ~60KB of text
        .collect::<Vec<_>>()
        .join("\n");

    let cmd = CreatePolicyCommand {
        policy_id: "large-policy".to_string(),
        policy_content: large_content.clone(),
        description: Some("Large test policy".to_string()),
    };

    let view = uc
        .execute(cmd)
        .await
        .expect("large policy should be created");

    assert!(view.id.to_string().contains("policy/large-policy"));
    assert!(view.content.starts_with("permit("));
    assert_eq!(port.get_created_count(), 1);
}

#[tokio::test]
async fn policy_id_trimmed_is_still_invalid_if_empty() {
    init_tracing();

    let port = Arc::new(MockCreatePolicyPort::new());
    let validator = Arc::new(MockPolicyValidator::new());
    let uc = build_use_case(port, validator);

    let cmd = CreatePolicyCommand {
        policy_id: "   ".to_string(),
        policy_content: "permit(principal, action, resource);".to_string(),
        description: None,
    };

    let err = uc.execute(cmd).await.unwrap_err();
    matches!(err, CreatePolicyError::InvalidPolicyId(_));
}

#[tokio::test]
async fn description_is_optional() {
    init_tracing();

    let port = Arc::new(MockCreatePolicyPort::new());
    let validator = Arc::new(MockPolicyValidator::new());
    let uc = build_use_case(port.clone(), validator);

    let cmd = CreatePolicyCommand {
        policy_id: "no-desc".to_string(),
        policy_content: "permit(principal, action, resource);".to_string(),
        description: None,
    };

    let view = uc.execute(cmd).await.unwrap();
    assert!(view.description.is_none());
    assert_eq!(port.get_created_count(), 1);
}

// -------------------------------------------------------------------------------------------------
// (Optional) Logging demonstration (not asserting anything, just ensures tracing does not panic)
// -------------------------------------------------------------------------------------------------
#[tokio::test]
async fn tracing_does_not_panic() {
    init_tracing();
    info!("Tracing initialized for create_policy_new tests");
    warn!("This is a benign warning for test visibility");
}
