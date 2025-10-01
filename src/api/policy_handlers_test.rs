#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::{post, get, delete},
        Router,
    };
    use serde_json::json;
    use std::sync::Arc;
    use tower::ServiceExt;

    // Test helper to create a mock AppState
    fn create_mock_app_state() -> Arc<AppState> {
        // This would need to be implemented with mock versions of the components
        // For now, we'll just create a placeholder
        unimplemented!("Create mock app state for testing")
    }

    #[tokio::test]
    async fn test_create_policy_success() {
        // Create a mock app state
        let state = create_mock_app_state();
        
        // Build our application with the mock state
        let app = Router::new()
            .route("/policies", post(create_policy))
            .with_state(state);
        
        // Create a request body
        let request_body = json!({
            "name": "test-policy",
            "description": "A test policy",
            "policy_content": "permit(principal, action, resource);",
            "enabled": true
        });
        
        // Send the request
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/policies")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        
        // Check the response status
        assert_eq!(response.status(), StatusCode::OK);
        
        // Check the response body
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let response_body: PolicyResponse = serde_json::from_slice(&body).unwrap();
        
        assert_eq!(response_body.name, "test-policy");
        assert_eq!(response_body.description, Some("A test policy".to_string()));
        assert_eq!(response_body.policy_content, "permit(principal, action, resource);");
        assert_eq!(response_body.enabled, true);
        assert!(response_body.id.len() > 0);
        assert!(response_body.created_at.len() > 0);
        assert!(response_body.updated_at.len() > 0);
    }

    #[tokio::test]
    async fn test_create_policy_missing_name() {
        // Create a mock app state
        let state = create_mock_app_state();
        
        // Build our application with the mock state
        let app = Router::new()
            .route("/policies", post(create_policy))
            .with_state(state);
        
        // Create a request body with missing name
        let request_body = json!({
            "description": "A test policy",
            "policy_content": "permit(principal, action, resource);",
            "enabled": true
        });
        
        // Send the request
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/policies")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        
        // Check the response status
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_policy_missing_content() {
        // Create a mock app state
        let state = create_mock_app_state();
        
        // Build our application with the mock state
        let app = Router::new()
            .route("/policies", post(create_policy))
            .with_state(state);
        
        // Create a request body with missing policy_content
        let request_body = json!({
            "name": "test-policy",
            "description": "A test policy",
            "enabled": true
        });
        
        // Send the request
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/policies")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        
        // Check the response status
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_list_policies() {
        // Create a mock app state
        let state = create_mock_app_state();
        
        // Build our application with the mock state
        let app = Router::new()
            .route("/policies", get(list_policies))
            .with_state(state);
        
        // Send the request
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/policies")
                    .header("content-type", "application/json")
                    .body(Body::from(""))
                    .unwrap(),
            )
            .await
            .unwrap();
        
        // Check the response status
        assert_eq!(response.status(), StatusCode::OK);
        
        // Check the response body
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let response_body: PolicyListResponse = serde_json::from_slice(&body).unwrap();
        
        // In our placeholder implementation, we return one sample policy
        assert_eq!(response_body.total, 1);
        assert_eq!(response_body.policies.len(), 1);
    }

    #[tokio::test]
    async fn test_delete_policy() {
        // Create a mock app state
        let state = create_mock_app_state();
        
        // Build our application with the mock state
        let app = Router::new()
            .route("/policies/:id", delete(delete_policy))
            .with_state(state);
        
        // Send the request
        let response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/policies/test-policy-id")
                    .header("content-type", "application/json")
                    .body(Body::from(""))
                    .unwrap(),
            )
            .await
            .unwrap();
        
        // Check the response status
        assert_eq!(response.status(), StatusCode::OK);
        
        // Check the response body
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let response_body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        
        assert_eq!(response_body["policy_id"], "test-policy-id");
        assert!(response_body["timestamp"].as_str().is_some());
    }

    #[tokio::test]
    async fn test_delete_policy_missing_id() {
        // Create a mock app state
        let state = create_mock_app_state();
        
        // Build our application with the mock state
        let app = Router::new()
            .route("/policies/:id", delete(delete_policy))
            .with_state(state);
        
        // Send the request with empty policy ID
        let response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/policies/")
                    .header("content-type", "application/json")
                    .body(Body::from(""))
                    .unwrap(),
            )
            .await
            .unwrap();
        
        // Check the response status
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
