//! End-to-End Tests for Policies API
//!
//! This module contains comprehensive tests for the policy management endpoints,
//! verifying policy validation and evaluation workflows.

use super::test_utils::{TestClient, test_action, test_hrn};
use serde_json::json;

/// Test policy validation endpoint with valid policy
#[tokio::test]
async fn test_validate_policy_valid() {
    let client = TestClient::new();

    let request = json!({
        "content": "permit(principal, action, resource);",
        "use_schema": false
    });

    let mut response = client.post("/api/v1/policies/validate", request).await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;

    // Verify response structure
    assert!(body.get("is_valid").is_some());
    assert!(body.get("errors").is_some());

    let is_valid = body.get("is_valid").unwrap().as_bool().unwrap();
    let errors = body.get("errors").unwrap().as_array().unwrap();

    assert!(is_valid, "Valid policy should return is_valid=true");
    assert!(errors.is_empty(), "Valid policy should have no errors");
}

/// Test policy validation endpoint with invalid policy
#[tokio::test]
async fn test_validate_policy_invalid() {
    let client = TestClient::new();

    let request = json!({
        "content": "invalid policy syntax",
        "use_schema": false
    });

    let mut response = client.post("/api/v1/policies/validate", request).await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;

    let is_valid = body.get("is_valid").unwrap().as_bool().unwrap();
    let errors = body.get("errors").unwrap().as_array().unwrap();

    assert!(!is_valid, "Invalid policy should return is_valid=false");
    assert!(!errors.is_empty(), "Invalid policy should have errors");
}

/// Test policy validation with schema validation disabled
#[tokio::test]
async fn test_validate_policy_no_schema() {
    let client = TestClient::new();

    let request = json!({
        "content": "permit(principal, action, resource) when { context.role == \"admin\" };",
        "use_schema": false
    });

    let mut response = client.post("/api/v1/policies/validate", request).await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;

    let is_valid = body.get("is_valid").unwrap().as_bool().unwrap();
    let errors = body.get("errors").unwrap().as_array().unwrap();

    // Should be valid for basic syntax checking without schema
    assert!(is_valid, "Policy should be syntactically valid");
    assert!(errors.is_empty(), "Should have no validation errors");
}

/// Test policy validation with empty policy
#[tokio::test]
async fn test_validate_policy_empty() {
    let client = TestClient::new();

    let request = json!({
        "content": "",
        "use_schema": false
    });

    let mut response = client.post("/api/v1/policies/validate", request).await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;

    let is_valid = body.get("is_valid").unwrap().as_bool().unwrap();
    let errors = body.get("errors").unwrap().as_array().unwrap();

    assert!(!is_valid, "Empty policy should be invalid");
    assert!(!errors.is_empty(), "Empty policy should have errors");
}

/// Test policy validation with complex policy
#[tokio::test]
async fn test_validate_policy_complex() {
    let client = TestClient::new();

    let request = json!({
        "content": r#"
            permit(
                principal in Group::"admins",
                action == Action::"read",
                resource
            ) when {
                resource.owner == principal &&
                context.ip_range == "192.168.1.0/24" &&
                context.time_of_day >= "09:00" &&
                context.time_of_day <= "17:00"
            };
        "#,
        "use_schema": false
    });

    let mut response = client.post("/api/v1/policies/validate", request).await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;

    let is_valid = body.get("is_valid").unwrap().as_bool().unwrap();
    let errors = body.get("errors").unwrap().as_array().unwrap();

    // Complex policy should be syntactically valid
    assert!(is_valid, "Complex policy should be syntactically valid");
    assert!(
        errors.is_empty(),
        "Complex policy should have no syntax errors"
    );
}

/// Test policy evaluation endpoint (stub implementation)
#[tokio::test]
async fn test_evaluate_policies_stub() {
    let client = TestClient::new();

    let request = json!({
        "principal_hrn": test_hrn("iam", "User", "alice"),
        "action": test_action("api", "read"),
        "resource_hrn": test_hrn("storage", "Document", "doc1"),
        "policies": [
            "permit(principal, action, resource);"
        ],
        "context": {},
        "schema_version": null,
        "evaluation_mode": "NoSchema"
    });

    let mut response = client.post("/api/v1/policies/evaluate", request).await;

    // The endpoint currently returns a stub response
    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;

    // Verify stub response structure
    assert!(body.get("decision").is_some());
    assert!(body.get("determining_policies").is_some());
    assert!(body.get("reasons").is_some());
    assert!(body.get("used_schema_version").is_some());
    assert!(body.get("policy_ids_evaluated").is_some());
    assert!(body.get("diagnostics").is_some());

    let decision = body.get("decision").unwrap().as_str().unwrap();
    let reasons = body.get("reasons").unwrap().as_array().unwrap();

    // Stub implementation currently returns "Deny"
    assert_eq!(decision, "Deny");
    assert!(!reasons.is_empty(), "Should have reasons for decision");
}

/// Test policy evaluation with context
#[tokio::test]
async fn test_evaluate_policies_with_context() {
    let client = TestClient::new();

    let request = json!({
        "principal_hrn": test_hrn("iam", "User", "alice"),
        "action": test_action("api", "read"),
        "resource_hrn": test_hrn("storage", "Document", "doc1"),
        "policies": [
            "permit(principal, action, resource) when { context.role == \"admin\" };"
        ],
        "context": {
            "role": "admin",
            "ip": "192.168.1.1",
            "timestamp": 1234567890
        },
        "schema_version": null,
        "evaluation_mode": "NoSchema"
    });

    let mut response = client.post("/api/v1/policies/evaluate", request).await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;

    // Verify response structure even for stub implementation
    assert!(body.get("decision").is_some());
    assert!(body.get("determining_policies").is_some());
}

/// Test policy evaluation with multiple policies
#[tokio::test]
async fn test_evaluate_policies_multiple() {
    let client = TestClient::new();

    let request = json!({
        "principal_hrn": test_hrn("iam", "User", "alice"),
        "action": test_action("api", "read"),
        "resource_hrn": test_hrn("storage", "Document", "doc1"),
        "policies": [
            "permit(principal, action, resource) when { context.access_level >= 5 };",
            "forbid(principal, action, resource) when { context.restricted == true };",
            "permit(principal, action, resource) when { principal == User::\"admin\" };"
        ],
        "context": {
            "access_level": 7,
            "restricted": false
        },
        "schema_version": null,
        "evaluation_mode": "NoSchema"
    });

    let mut response = client.post("/api/v1/policies/evaluate", request).await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;

    // Verify response structure
    assert!(body.get("decision").is_some());
    assert!(body.get("policy_ids_evaluated").is_some());

    let policy_ids = body
        .get("policy_ids_evaluated")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(policy_ids.len(), 3, "Should evaluate all 3 policies");
}

/// Test policy evaluation with invalid HRN
#[tokio::test]
async fn test_evaluate_policies_invalid_hrn() {
    let client = TestClient::new();

    let request = json!({
        "principal_hrn": "invalid-hrn-format",
        "action": test_action("api", "read"),
        "resource_hrn": test_hrn("storage", "Document", "doc1"),
        "policies": [
            "permit(principal, action, resource);"
        ],
        "context": {},
        "schema_version": null,
        "evaluation_mode": "NoSchema"
    });

    let response = client.post("/api/v1/policies/evaluate", request).await;

    // Should return error for invalid HRN
    response.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

/// Test policy evaluation with empty policies
#[tokio::test]
async fn test_evaluate_policies_empty() {
    let client = TestClient::new();

    let request = json!({
        "principal_hrn": test_hrn("iam", "User", "alice"),
        "action": test_action("api", "read"),
        "resource_hrn": test_hrn("storage", "Document", "doc1"),
        "policies": [],
        "context": {},
        "schema_version": null,
        "evaluation_mode": "NoSchema"
    });

    let response = client.post("/api/v1/policies/evaluate", request).await;

    // Should handle empty policies gracefully
    // Current stub implementation returns 200 OK with stub response
    response.assert_status(axum::http::StatusCode::OK);
}

/// Test policy evaluation performance with many policies
#[tokio::test]
async fn test_evaluate_policies_performance() {
    let client = TestClient::new();

    // Create 20 simple policies
    let policies: Vec<String> = (0..20)
        .map(|i| {
            format!(
                "permit(principal, action, resource) when {{ context.id == {} }};",
                i
            )
        })
        .collect();

    let request = json!({
        "principal_hrn": test_hrn("iam", "User", "alice"),
        "action": test_action("api", "read"),
        "resource_hrn": test_hrn("storage", "Document", "doc1"),
        "policies": policies,
        "context": {
            "id": 10
        },
        "schema_version": null,
        "evaluation_mode": "NoSchema"
    });

    let start = std::time::Instant::now();
    let mut response = client.post("/api/v1/policies/evaluate", request).await;
    let duration = start.elapsed();

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;
    let policy_ids = body
        .get("policy_ids_evaluated")
        .unwrap()
        .as_array()
        .unwrap();

    assert_eq!(policy_ids.len(), 20, "Should evaluate all 20 policies");

    // Verify performance - should complete within reasonable time
    assert!(
        duration < std::time::Duration::from_secs(2),
        "Evaluation took too long: {:?}",
        duration
    );
}

/// Test policy validation with special characters
#[tokio::test]
async fn test_validate_policy_special_characters() {
    let client = TestClient::new();

    let request = json!({
        "content": r#"
            permit(
                principal,
                action == Action::"read-data",
                resource in Document::"confidential-report_2024"
            ) when {
                context.department == "R&D" &&
                context."security-level" >= 3 &&
                context.tags.contains("top-secret")
            };
        "#,
        "use_schema": false
    });

    let mut response = client.post("/api/v1/policies/validate", request).await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;

    let is_valid = body.get("is_valid").unwrap().as_bool().unwrap();
    let errors = body.get("errors").unwrap().as_array().unwrap();

    // Policy with special characters should be syntactically valid
    assert!(is_valid, "Policy with special characters should be valid");
    assert!(errors.is_empty(), "Should have no syntax errors");
}

/// Test policy endpoints with concurrent requests
#[tokio::test]
async fn test_policies_concurrent_requests() {
    let client = TestClient::new();

    let validation_futures: Vec<_> = (0..5)
        .map(|i| {
            let client = &client;
            let request = json!({
                "content": format!("permit(principal, action, resource) when {{ context.id == {} }};", i),
                "use_schema": false
            });

            async move {
                let mut response = client.post("/api/v1/policies/validate", request).await;
                response.assert_status(axum::http::StatusCode::OK);

                let body = response.json().await;
                let is_valid = body.get("is_valid").unwrap().as_bool().unwrap();
                assert!(is_valid, "Concurrent validation should succeed");
            }
        })
        .collect();

    // Execute all validations concurrently
    futures::future::join_all(validation_futures).await;
}

/// Test policy validation error messages
#[tokio::test]
async fn test_validate_policy_error_messages() {
    let client = TestClient::new();

    let request = json!({
        "content": "permit(principal action resource);", // Missing comma
        "use_schema": false
    });

    let mut response = client.post("/api/v1/policies/validate", request).await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;

    let is_valid = body.get("is_valid").unwrap().as_bool().unwrap();
    let errors = body.get("errors").unwrap().as_array().unwrap();

    assert!(!is_valid, "Invalid policy should return false");
    assert!(!errors.is_empty(), "Should have error messages");

    // Verify error messages are descriptive
    let first_error = errors[0].as_str().unwrap();
    assert!(first_error.len() > 0, "Error message should not be empty");
    assert!(
        first_error.to_lowercase().contains("error"),
        "Error message should indicate an error"
    );
}
