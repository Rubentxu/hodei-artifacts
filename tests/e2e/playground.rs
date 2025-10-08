//! End-to-End Tests for Playground API
//!
//! This module contains comprehensive tests for the playground evaluate endpoint,
//! verifying the complete policy evaluation workflow from HTTP request to response.

use super::test_utils::{TestClient, test_action, test_hrn};
use serde_json::json;
use std::time::Duration;

/// Test basic playground evaluation with inline schema
#[tokio::test]
async fn test_playground_evaluate_basic() {
    let client = TestClient::new();

    let request = json!({
        "inline_schema": "{}",
        "inline_policies": [
            "permit(principal, action, resource);"
        ],
        "request": {
            "principal": test_hrn("iam", "User", "alice"),
            "action": test_action("api", "read"),
            "resource": test_hrn("storage", "Document", "doc1"),
            "context": {}
        }
    });

    let mut response = client.post("/api/v1/playground/evaluate", request).await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;

    // Verify response structure
    assert!(body.get("decision").is_some());
    assert!(body.get("determining_policies").is_some());
    assert!(body.get("diagnostics").is_some());
    assert!(body.get("errors").is_some());

    // Verify decision is ALLOW for permit policy
    let decision = body.get("decision").unwrap().as_str().unwrap();
    assert_eq!(decision, "ALLOW");

    // Verify diagnostics
    let diagnostics = body.get("diagnostics").unwrap();
    assert_eq!(
        diagnostics.get("total_policies").unwrap().as_u64().unwrap(),
        1
    );
    assert_eq!(
        diagnostics
            .get("matched_policies")
            .unwrap()
            .as_u64()
            .unwrap(),
        1
    );
    assert_eq!(
        diagnostics
            .get("schema_validated")
            .unwrap()
            .as_bool()
            .unwrap(),
        true
    );
}

/// Test playground evaluation with context attributes
#[tokio::test]
async fn test_playground_evaluate_with_context() {
    let client = TestClient::new();

    let request = json!({
        "inline_schema": "{}",
        "inline_policies": [
            "permit(principal, action, resource) when { context.ip == \"192.168.1.1\" };"
        ],
        "request": {
            "principal": test_hrn("iam", "User", "alice"),
            "action": test_action("api", "read"),
            "resource": test_hrn("storage", "Document", "doc1"),
            "context": {
                "ip": {
                    "type": "String",
                    "value": "192.168.1.1"
                }
            }
        }
    });

    let mut response = client.post("/api/v1/playground/evaluate", request).await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;
    let decision = body.get("decision").unwrap().as_str().unwrap();
    assert_eq!(decision, "ALLOW");
}

/// Test playground evaluation with complex context attributes
#[tokio::test]
async fn test_playground_evaluate_complex_context() {
    let client = TestClient::new();

    let request = json!({
        "inline_schema": "{}",
        "inline_policies": [
            "permit(principal, action, resource) when { context.tags.contains(\"public\") };"
        ],
        "request": {
            "principal": test_hrn("iam", "User", "alice"),
            "action": test_action("api", "read"),
            "resource": test_hrn("storage", "Document", "doc1"),
            "context": {
                "tags": {
                    "type": "Set",
                    "value": [
                        {
                            "type": "String",
                            "value": "public"
                        },
                        {
                            "type": "String",
                            "value": "document"
                        }
                    ]
                },
                "metadata": {
                    "type": "Record",
                    "value": {
                        "owner": {
                            "type": "String",
                            "value": "alice"
                        },
                        "created": {
                            "type": "Long",
                            "value": 1234567890
                        }
                    }
                }
            }
        }
    });

    let mut response = client.post("/api/v1/playground/evaluate", request).await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;
    let decision = body.get("decision").unwrap().as_str().unwrap();
    assert_eq!(decision, "ALLOW");
}

/// Test playground evaluation with forbid policy
#[tokio::test]
async fn test_playground_evaluate_forbid_policy() {
    let client = TestClient::new();

    let request = json!({
        "inline_schema": "{}",
        "inline_policies": [
            "forbid(principal, action, resource);"
        ],
        "request": {
            "principal": test_hrn("iam", "User", "alice"),
            "action": test_action("api", "read"),
            "resource": test_hrn("storage", "Document", "doc1"),
            "context": {}
        }
    });

    let mut response = client.post("/api/v1/playground/evaluate", request).await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;
    let decision = body.get("decision").unwrap().as_str().unwrap();
    assert_eq!(decision, "DENY");
}

/// Test playground evaluation with multiple policies
#[tokio::test]
async fn test_playground_evaluate_multiple_policies() {
    let client = TestClient::new();

    let request = json!({
        "inline_schema": "{}",
        "inline_policies": [
            "permit(principal, action, resource) when { context.role == \"admin\" };",
            "forbid(principal, action, resource) when { context.role == \"guest\" };"
        ],
        "request": {
            "principal": test_hrn("iam", "User", "alice"),
            "action": test_action("api", "read"),
            "resource": test_hrn("storage", "Document", "doc1"),
            "context": {
                "role": {
                    "type": "String",
                    "value": "admin"
                }
            }
        }
    });

    let mut response = client.post("/api/v1/playground/evaluate", request).await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;
    let decision = body.get("decision").unwrap().as_str().unwrap();
    assert_eq!(decision, "ALLOW");
}

/// Test playground evaluation with invalid policy syntax
#[tokio::test]
async fn test_playground_evaluate_invalid_policy() {
    let client = TestClient::new();

    let request = json!({
        "inline_schema": "{}",
        "inline_policies": [
            "invalid policy syntax"
        ],
        "request": {
            "principal": test_hrn("iam", "User", "alice"),
            "action": test_action("api", "read"),
            "resource": test_hrn("storage", "Document", "doc1"),
            "context": {}
        }
    });

    let mut response = client.post("/api/v1/playground/evaluate", request).await;

    // Should still return 200 OK with errors in response
    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;
    let errors = body.get("errors").unwrap().as_array().unwrap();
    assert!(!errors.is_empty(), "Should have validation errors");

    let diagnostics = body.get("diagnostics").unwrap();
    let validation_errors = diagnostics
        .get("validation_errors")
        .unwrap()
        .as_array()
        .unwrap();
    assert!(
        !validation_errors.is_empty(),
        "Should have validation errors in diagnostics"
    );
}

/// Test playground evaluation with invalid HRN format
#[tokio::test]
async fn test_playground_evaluate_invalid_hrn() {
    let client = TestClient::new();

    let request = json!({
        "inline_schema": "{}",
        "inline_policies": [
            "permit(principal, action, resource);"
        ],
        "request": {
            "principal": "invalid-hrn-format",
            "action": test_action("api", "read"),
            "resource": test_hrn("storage", "Document", "doc1"),
            "context": {}
        }
    });

    let response = client.post("/api/v1/playground/evaluate", request).await;

    // Should return 400 Bad Request for invalid HRN
    response.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

/// Test playground evaluation with missing required fields
#[tokio::test]
async fn test_playground_evaluate_missing_fields() {
    let client = TestClient::new();

    let request = json!({
        "inline_policies": [
            "permit(principal, action, resource);"
        ],
        "request": {
            "principal": test_hrn("iam", "User", "alice"),
            "action": test_action("api", "read"),
            "resource": test_hrn("storage", "Document", "doc1"),
            "context": {}
        }
    });

    let response = client.post("/api/v1/playground/evaluate", request).await;

    // Should return 400 Bad Request for missing schema
    response.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

/// Test playground evaluation with empty policies
#[tokio::test]
async fn test_playground_evaluate_empty_policies() {
    let client = TestClient::new();

    let request = json!({
        "inline_schema": "{}",
        "inline_policies": [],
        "request": {
            "principal": test_hrn("iam", "User", "alice"),
            "action": test_action("api", "read"),
            "resource": test_hrn("storage", "Document", "doc1"),
            "context": {}
        }
    });

    let response = client.post("/api/v1/playground/evaluate", request).await;

    // Should return 400 Bad Request for empty policies
    response.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

/// Test playground evaluation with entity reference in context
#[tokio::test]
async fn test_playground_evaluate_entity_ref_context() {
    let client = TestClient::new();

    let request = json!({
        "inline_schema": "{}",
        "inline_policies": [
            "permit(principal, action, resource) when { context.owner == User::\"alice\" };"
        ],
        "request": {
            "principal": test_hrn("iam", "User", "alice"),
            "action": test_action("api", "read"),
            "resource": test_hrn("storage", "Document", "doc1"),
            "context": {
                "owner": {
                    "type": "EntityRef",
                    "value": test_hrn("iam", "User", "alice")
                }
            }
        }
    });

    let mut response = client.post("/api/v1/playground/evaluate", request).await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;
    let decision = body.get("decision").unwrap().as_str().unwrap();
    assert_eq!(decision, "ALLOW");
}

/// Test playground evaluation performance with many policies
#[tokio::test]
async fn test_playground_evaluate_performance() {
    let client = TestClient::new();

    // Create 10 simple policies
    let policies: Vec<String> = (0..10)
        .map(|i| {
            format!(
                "permit(principal, action, resource) when {{ context.id == {} }};",
                i
            )
        })
        .collect();

    let request = json!({
        "inline_schema": "{}",
        "inline_policies": policies,
        "request": {
            "principal": test_hrn("iam", "User", "alice"),
            "action": test_action("api", "read"),
            "resource": test_hrn("storage", "Document", "doc1"),
            "context": {
                "id": {
                    "type": "Long",
                    "value": 5
                }
            }
        }
    });

    let start = std::time::Instant::now();
    let mut response = client.post("/api/v1/playground/evaluate", request).await;
    let duration = start.elapsed();

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;
    let decision = body.get("decision").unwrap().as_str().unwrap();
    assert_eq!(decision, "ALLOW");

    // Verify performance - should complete within 1 second
    assert!(
        duration < Duration::from_secs(1),
        "Evaluation took too long: {:?}",
        duration
    );
}
