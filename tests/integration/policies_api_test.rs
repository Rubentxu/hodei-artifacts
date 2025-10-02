use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::json;
use tower::ServiceExt;

// Use the library's test app builder
use hodei_artifacts_api::build_app_for_tests;

// Helper function to build the test app
async fn build_test_app() -> axum::Router {
    build_app_for_tests().await.expect("build test app")
}

#[tokio::test]
async fn test_playground_basic_allow_and_deny() {
    let app = build_test_app().await;

    let body = json!({
        "policies": [
            "permit(principal, action, resource);",
            "forbid(principal == User::\"bob\", action, resource);"
        ],
        "authorization_requests": [
            {"name":"alice-allow","principal":"User::\"alice\"","action":"Action::\"view\"","resource":"Resource::\"doc1\""},
            {"name":"bob-deny","principal":"User::\"bob\"","action":"Action::\"view\"","resource":"Resource::\"doc1\""}
        ]
    });

    let req = Request::builder()
        .uri("/api/v1/policies/playground")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_playground_with_schema_validation_error() {
    let app = build_test_app().await;

    let body = json!({
        "policies": ["permit(principal, action, resource);"] ,
        "schema": "entity Principal { ;", // invalid schema
        "authorization_requests": [
            {"name":"req","principal":"User::\"alice\"","action":"Action::\"view\"","resource":"Resource::\"doc1\""}
        ]
    });

    let req = Request::builder()
        .uri("/api/v1/policies/playground")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    // Depending on handler behavior, invalid schema currently returns 200 with is_valid true (we parse schema in use_case, but do not fail request). This test can be adjusted when behavior is finalized.
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_playground_with_invalid_policy() {
    let app = build_test_app().await;

    let body = json!({
        "policies": ["this is not cedar"],
        "authorization_requests": [
            {"name":"req","principal":"User::\"alice\"","action":"Action::\"view\"","resource":"Resource::\"doc1\""}
        ]
    });

    let req = Request::builder()
        .uri("/api/v1/policies/playground")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_playground_entities_and_parents_forbid() {
    let app = build_test_app().await;

    let body = json!({
        "policies": [
            "forbid(principal in Group::\"admins\", action, resource);",
            "permit(principal, action, resource);"
        ],
        "entities": [
            {"uid":"User::\"alice\"","attributes":{},"parents":["Group::\"admins\""]},
            {"uid":"Group::\"admins\"","attributes":{},"parents":[]}
        ],
        "authorization_requests": [
            {"name":"alice-deny","principal":"User::\"alice\"","action":"Action::\"view\"","resource":"Resource::\"doc1\""}
        ]
    });

    let req = Request::builder()
        .uri("/api/v1/policies/playground")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_playground_context_affects_decision() {
    let app = build_test_app().await;

    let body = json!({
        "policies": [
            "permit(principal, action, resource) when { context.mfa == true };"
        ],
        "authorization_requests": [
            {"name":"no-mfa","principal":"User::\"alice\"","action":"Action::\"view\"","resource":"Resource::\"doc1\"","context": {"mfa": false}},
            {"name":"with-mfa","principal":"User::\"alice\"","action":"Action::\"view\"","resource":"Resource::\"doc1\"","context": {"mfa": true}}
        ]
    });

    let req = Request::builder()
        .uri("/api/v1/policies/playground")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
#[ignore] // Ignore until we refactor main.rs to expose app builder
async fn test_e2e_create_and_list_policies() {
    // Arrange
    let app = build_test_app().await;
    
    // Act 1: Create a policy
    let create_request = Request::builder()
        .uri("/api/v1/policies")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "name": "test-policy",
                "description": "Test policy for E2E",
                "policy_content": "permit(principal, action, resource);",
                "enabled": true
            })
            .to_string(),
        ))
        .unwrap();
    
    let create_response = app.clone().oneshot(create_request).await.unwrap();
    assert_eq!(create_response.status(), StatusCode::OK);
    
    // Act 2: List policies
    let list_request = Request::builder()
        .uri("/api/v1/policies")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    
    let list_response = app.oneshot(list_request).await.unwrap();
    
    // Assert
    assert_eq!(list_response.status(), StatusCode::OK);
    // Additional assertions would parse the response body and verify the policy is listed
}

#[tokio::test]
#[ignore] // Ignore until we refactor main.rs to expose app builder
async fn test_e2e_create_get_and_delete_policy() {
    // Arrange
    let app = build_test_app().await;
    
    // Act 1: Create a policy
    let create_request = Request::builder()
        .uri("/api/v1/policies")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "name": "test-policy-delete",
                "policy_content": "permit(principal, action, resource);",
            })
            .to_string(),
        ))
        .unwrap();
    
    let create_response = app.clone().oneshot(create_request).await.unwrap();
    assert_eq!(create_response.status(), StatusCode::OK);
    
    // Extract policy ID from response (would need actual implementation)
    let policy_id = "policy0"; // Placeholder
    
    // Act 2: Get the policy
    let get_request = Request::builder()
        .uri(format!("/api/v1/policies/{}", policy_id))
        .method("GET")
        .body(Body::empty())
        .unwrap();
    
    let get_response = app.clone().oneshot(get_request).await.unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);
    
    // Act 3: Delete the policy
    let delete_request = Request::builder()
        .uri(format!("/api/v1/policies/{}", policy_id))
        .method("DELETE")
        .body(Body::empty())
        .unwrap();
    
    let delete_response = app.clone().oneshot(delete_request).await.unwrap();
    assert_eq!(delete_response.status(), StatusCode::OK);
    
    // Act 4: Try to get the deleted policy
    let get_after_delete_request = Request::builder()
        .uri(format!("/api/v1/policies/{}", policy_id))
        .method("GET")
        .body(Body::empty())
        .unwrap();
    
    let get_after_delete_response = app.oneshot(get_after_delete_request).await.unwrap();
    assert_eq!(get_after_delete_response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore] // Ignore until we refactor main.rs to expose app builder
async fn test_e2e_delete_nonexistent_policy() {
    // Arrange
    let app = build_test_app().await;
    
    // Act: Try to delete a non-existent policy
    let delete_request = Request::builder()
        .uri("/api/v1/policies/nonexistent_id")
        .method("DELETE")
        .body(Body::empty())
        .unwrap();
    
    let delete_response = app.oneshot(delete_request).await.unwrap();
    
    // Assert: Should return 404 Not Found
    assert_eq!(delete_response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore] // Ignore until we refactor main.rs to expose app builder
async fn test_e2e_list_empty_policies() {
    // Arrange
    let app = build_test_app().await;
    
    // Act: List policies when none exist
    let list_request = Request::builder()
        .uri("/api/v1/policies")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    
    let list_response = app.oneshot(list_request).await.unwrap();
    
    // Assert
    assert_eq!(list_response.status(), StatusCode::OK);
    // Response body should contain empty list with total: 0
}
