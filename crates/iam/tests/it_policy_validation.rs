use axum::{
    body::Body,
    http::{self, Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use iam::features::validate_policy::dto::ValidatePolicyResponse;
use iam::features::validate_policy::ValidatePolicyDIContainer;
use std::path::PathBuf;
use tower::ServiceExt; // for `oneshot`

#[tokio::test]
async fn test_validate_policy_happy_path() {
    // --- Arrange ---
    let schema_path = PathBuf::from("crates/security/schema/policy_schema.cedarschema");
    let container = ValidatePolicyDIContainer::for_production(schema_path)
        .expect("Failed to create DI container for test");
    
    let app = Router::new()
        .route("/policies/validate", axum::routing::post(container.api.handle))
        .with_state(container.api);

    let valid_policy = r#"permit(principal, action, resource);"#;

    let request = Request::builder()
        .method(http::Method::POST)
        .uri("/policies/validate")
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(format!(r#"{{"policy": "{}"}}'#, valid_policy)))
        .unwrap();

    // --- Act ---
    let response = app.oneshot(request).await.unwrap();

    // --- Assert ---
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let result: ValidatePolicyResponse = serde_json::from_slice(&body).unwrap();
    assert!(result.is_valid);
    assert!(result.errors.is_empty());
}

#[tokio::test]
async fn test_validate_policy_syntax_error() {
    // --- Arrange ---
    let schema_path = PathBuf::from("crates/security/schema/policy_schema.cedarschema");
    let container = ValidatePolicyDIContainer::for_production(schema_path)
        .expect("Failed to create DI container for test");
    
    let app = Router::new()
        .route("/policies/validate", axum::routing::post(container.api.handle))
        .with_state(container.api);

    let invalid_policy = r#"permit(principal, action, resource"#; // Missing closing parenthesis

    let request = Request::builder()
        .method(http::Method::POST)
        .uri("/policies/validate")
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(format!(r#"{{"policy": "{}"}}'#, invalid_policy)))
        .unwrap();

    // --- Act ---
    let response = app.oneshot(request).await.unwrap();

    // --- Assert ---
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let result: ValidatePolicyResponse = serde_json::from_slice(&body).unwrap();
    assert!(!result.is_valid);
    assert!(!result.errors.is_empty());
    assert!(result.errors[0].contains("syntax error"));
}

#[tokio::test]
async fn test_validate_policy_semantic_error() {
    // --- Arrange ---
    let schema_path = PathBuf::from("crates/security/schema/policy_schema.cedarschema");
    let container = ValidatePolicyDIContainer::for_production(schema_path)
        .expect("Failed to create DI container for test");
    
    let app = Router::new()
        .route("/policies/validate", axum::routing::post(container.api.handle))
        .with_state(container.api);

    // Policy uses an entity type not defined in the schema
    let semantic_invalid_policy = r#"permit(principal == NonExistent::"user", action, resource);"#;

    let request = Request::builder()
        .method(http::Method::POST)
        .uri("/policies/validate")
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(format!(r#"{{"policy": "{}"}}'#, semantic_invalid_policy)))
        .unwrap();

    // --- Act ---
    let response = app.oneshot(request).await.unwrap();

    // --- Assert ---
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let result: ValidatePolicyResponse = serde_json::from_slice(&body).unwrap();
    assert!(!result.is_valid);
    assert!(!result.errors.is_empty());
    assert!(result.errors[0].contains("undefined entity type"));
}

#[tokio::test]
async fn test_validate_policy_empty_content() {
    // --- Arrange ---
    let schema_path = PathBuf::from("crates/security/schema/policy_schema.cedarschema");
    let container = ValidatePolicyDIContainer::for_production(schema_path)
        .expect("Failed to create DI container for test");
    
    let app = Router::new()
        .route("/policies/validate", axum::routing::post(container.api.handle))
        .with_state(container.api);

    let empty_policy = r#""#;

    let request = Request::builder()
        .method(http::Method::POST)
        .uri("/policies/validate")
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(format!(r#"{{"policy": "{}"}}'#, empty_policy)))
        .unwrap();

    // --- Act ---
    let response = app.oneshot(request).await.unwrap();

    // --- Assert ---
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let result: ValidatePolicyResponse = serde_json::from_slice(&body).unwrap();
    assert!(!result.is_valid);
    assert!(!result.errors.is_empty());
    assert!(result.errors[0].contains("empty"));
}
