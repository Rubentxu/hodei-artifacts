//! End-to-End Tests for Schemas API
//!
//! This module contains comprehensive tests for the schema management endpoints,
//! verifying schema building, loading, and IAM schema registration workflows.

use super::test_utils::TestClient;
use serde_json::json;

/// Test schema building endpoint
#[tokio::test]
async fn test_schema_build() {
    let client = TestClient::new();

    let request = json!({
        "entity_types": [
            {
                "name": "User",
                "attributes": {
                    "name": { "type": "String" },
                    "email": { "type": "String" }
                }
            }
        ],
        "action_types": [
            {
                "name": "Read",
                "applies_to": {
                    "principal_types": ["User"],
                    "resource_types": ["Document"]
                }
            }
        ]
    });

    let mut response = client.post("/api/v1/schemas/build", request).await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;

    // Verify response structure
    assert!(body.get("schema_id").is_some());
    assert!(body.get("schema_version").is_some());
    assert!(body.get("entity_types_registered").is_some());
    assert!(body.get("action_types_registered").is_some());

    let entity_types = body
        .get("entity_types_registered")
        .unwrap()
        .as_u64()
        .unwrap();
    let action_types = body
        .get("action_types_registered")
        .unwrap()
        .as_u64()
        .unwrap();

    assert_eq!(entity_types, 1);
    assert_eq!(action_types, 1);
}

/// Test schema loading endpoint
#[tokio::test]
async fn test_schema_load() {
    let client = TestClient::new();

    // First build a schema
    let build_request = json!({
        "entity_types": [
            {
                "name": "TestEntity",
                "attributes": {
                    "test_attr": { "type": "String" }
                }
            }
        ],
        "action_types": [
            {
                "name": "TestAction",
                "applies_to": {
                    "principal_types": ["TestEntity"],
                    "resource_types": ["TestEntity"]
                }
            }
        ]
    });

    let mut build_response = client.post("/api/v1/schemas/build", build_request).await;
    build_response.assert_status(axum::http::StatusCode::OK);

    let build_body = build_response.json().await;
    let schema_version = build_body.get("schema_version").unwrap().as_str().unwrap();

    // Then try to load it
    let mut load_response = client
        .get(&format!("/api/v1/schemas/load?version={}", schema_version))
        .await;

    load_response.assert_status(axum::http::StatusCode::OK);

    let load_body = load_response.json().await;
    assert!(load_body.get("schema").is_some());
    assert!(load_body.get("version").is_some());
    assert_eq!(
        load_body.get("version").unwrap().as_str().unwrap(),
        schema_version
    );
}

/// Test IAM schema registration endpoint
#[tokio::test]
async fn test_register_iam_schema() {
    let client = TestClient::new();

    let request = json!({
        "validate": true,
        "version": "test-iam-v1"
    });

    let mut response = client.post("/api/v1/schemas/register-iam", request).await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;

    // Verify response structure
    assert!(body.get("schema_version").is_some());
    assert!(body.get("entity_types_registered").is_some());
    assert!(body.get("action_types_registered").is_some());

    let entity_types = body
        .get("entity_types_registered")
        .unwrap()
        .as_u64()
        .unwrap();
    let action_types = body
        .get("action_types_registered")
        .unwrap()
        .as_u64()
        .unwrap();

    // IAM schema should register multiple entity and action types
    assert!(entity_types > 0);
    assert!(action_types > 0);
}

/// Test schema building with invalid input
#[tokio::test]
async fn test_schema_build_invalid_input() {
    let client = TestClient::new();

    let request = json!({
        "entity_types": "invalid_type_structure"
    });

    let response = client.post("/api/v1/schemas/build", request).await;

    // Should return 400 Bad Request for invalid input
    response.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

/// Test schema loading with non-existent version
#[tokio::test]
async fn test_schema_load_not_found() {
    let client = TestClient::new();

    let response = client.get("/api/v1/schemas/load?version=nonexistent").await;

    // Should return appropriate error status
    // This might be 404 or 400 depending on implementation
    let status = response.response.status();
    assert!(
        status.is_client_error(),
        "Expected client error, got: {}",
        status
    );
}

/// Test schema building with complex entity structure
#[tokio::test]
async fn test_schema_build_complex_entities() {
    let client = TestClient::new();

    let request = json!({
        "entity_types": [
            {
                "name": "User",
                "attributes": {
                    "profile": {
                        "type": "Record",
                        "attributes": {
                            "firstName": { "type": "String" },
                            "lastName": { "type": "String" },
                            "age": { "type": "Long" },
                            "active": { "type": "Bool" }
                        }
                    },
                    "roles": {
                        "type": "Set",
                        "element": { "type": "String" }
                    },
                    "metadata": {
                        "type": "Record",
                        "attributes": {
                            "createdAt": { "type": "Long" },
                            "updatedAt": { "type": "Long" }
                        }
                    }
                }
            },
            {
                "name": "Document",
                "attributes": {
                    "title": { "type": "String" },
                    "content": { "type": "String" },
                    "tags": {
                        "type": "Set",
                        "element": { "type": "String" }
                    },
                    "permissions": {
                        "type": "Record",
                        "attributes": {
                            "read": { "type": "Bool" },
                            "write": { "type": "Bool" },
                            "delete": { "type": "Bool" }
                        }
                    }
                }
            }
        ],
        "action_types": [
            {
                "name": "ReadDocument",
                "applies_to": {
                    "principal_types": ["User"],
                    "resource_types": ["Document"]
                }
            },
            {
                "name": "WriteDocument",
                "applies_to": {
                    "principal_types": ["User"],
                    "resource_types": ["Document"]
                }
            },
            {
                "name": "DeleteDocument",
                "applies_to": {
                    "principal_types": ["User"],
                    "resource_types": ["Document"]
                }
            }
        ]
    });

    let mut response = client.post("/api/v1/schemas/build", request).await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;

    let entity_types = body
        .get("entity_types_registered")
        .unwrap()
        .as_u64()
        .unwrap();
    let action_types = body
        .get("action_types_registered")
        .unwrap()
        .as_u64()
        .unwrap();

    assert_eq!(entity_types, 2);
    assert_eq!(action_types, 3);
}

/// Test schema building with empty schema
#[tokio::test]
async fn test_schema_build_empty() {
    let client = TestClient::new();

    let request = json!({
        "entity_types": [],
        "action_types": []
    });

    let mut response = client.post("/api/v1/schemas/build", request).await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;

    let entity_types = body
        .get("entity_types_registered")
        .unwrap()
        .as_u64()
        .unwrap();
    let action_types = body
        .get("action_types_registered")
        .unwrap()
        .as_u64()
        .unwrap();

    assert_eq!(entity_types, 0);
    assert_eq!(action_types, 0);
}

/// Test IAM schema registration without validation
#[tokio::test]
async fn test_register_iam_schema_no_validation() {
    let client = TestClient::new();

    let request = json!({
        "validate": false,
        "version": "test-iam-no-validation"
    });

    let mut response = client.post("/api/v1/schemas/register-iam", request).await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;

    // Should still succeed without validation
    assert!(body.get("schema_version").is_some());
    assert!(body.get("entity_types_registered").is_some());
    assert!(body.get("action_types_registered").is_some());
}

/// Test schema endpoints with concurrent requests
#[tokio::test]
async fn test_schema_concurrent_requests() {
    let client = TestClient::new();

    let futures: Vec<_> = (0..3)
        .map(|i| {
            let client = &client;
            let request = json!({
                "entity_types": [
                    {
                        "name": format!("ConcurrentEntity{}", i),
                        "attributes": {
                            "id": { "type": "String" }
                        }
                    }
                ],
                "action_types": [
                    {
                        "name": format!("ConcurrentAction{}", i),
                        "applies_to": {
                            "principal_types": [format!("ConcurrentEntity{}", i)],
                            "resource_types": [format!("ConcurrentEntity{}", i)]
                        }
                    }
                ]
            });

            async move {
                let mut response = client.post("/api/v1/schemas/build", request).await;
                response.assert_status(axum::http::StatusCode::OK);

                let body = response.json().await;
                let entity_types = body
                    .get("entity_types_registered")
                    .unwrap()
                    .as_u64()
                    .unwrap();
                let action_types = body
                    .get("action_types_registered")
                    .unwrap()
                    .as_u64()
                    .unwrap();

                assert_eq!(entity_types, 1);
                assert_eq!(action_types, 1);
            }
        })
        .collect();

    // Execute all schema builds concurrently
    futures::future::join_all(futures).await;
}

/// Test schema building with special characters in names
#[tokio::test]
async fn test_schema_build_special_characters() {
    let client = TestClient::new();

    let request = json!({
        "entity_types": [
            {
                "name": "User_With_Underscore",
                "attributes": {
                    "email_address": { "type": "String" }
                }
            },
            {
                "name": "User-With-Dash",
                "attributes": {
                    "phone-number": { "type": "String" }
                }
            }
        ],
        "action_types": [
            {
                "name": "Read_Data",
                "applies_to": {
                    "principal_types": ["User_With_Underscore", "User-With-Dash"],
                    "resource_types": ["User_With_Underscore", "User-With-Dash"]
                }
            }
        ]
    });

    let mut response = client.post("/api/v1/schemas/build", request).await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;

    let entity_types = body
        .get("entity_types_registered")
        .unwrap()
        .as_u64()
        .unwrap();
    let action_types = body
        .get("action_types_registered")
        .unwrap()
        .as_u64()
        .unwrap();

    assert_eq!(entity_types, 2);
    assert_eq!(action_types, 1);
}
