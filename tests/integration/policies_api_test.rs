use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::json;
use tower::ServiceExt;

// Helper function to build the test app
async fn build_test_app() -> axum::Router {
    // This would need to be implemented based on your app structure
    // For now, this is a placeholder showing the structure
    todo!("Implement test app builder - requires refactoring main.rs to expose build_app function")
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
