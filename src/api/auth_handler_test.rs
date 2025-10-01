#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::post,
        Router,
    };
    use serde_json::json;
    use std::sync::Arc;
    use tower::ServiceExt; // for `call` method on `Router`

    // Test helper to create a mock AppState
    fn create_mock_app_state() -> Arc<AppState> {
        // This would need to be implemented with mock versions of the components
        // For now, we'll just create a placeholder
        unimplemented!("Create mock app state for testing")
    }

    #[tokio::test]
    async fn test_authorize_success() {
        // Create a mock app state
        let state = create_mock_app_state();
        
        // Build our application with the mock state
        let app = Router::new()
            .route("/authorize", post(authorize))
            .with_state(state);
        
        // Create a request body
        let request_body = json!({
            "principal": "admin",
            "action": "read",
            "resource": "document/123",
            "context": {}
        });
        
        // Send the request
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/authorize")
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
        let response_body: AuthorizationResponse = serde_json::from_slice(&body).unwrap();
        
        assert_eq!(response_body.decision, "Allow");
        assert_eq!(response_body.reasons.len(), 1);
        assert!(response_body.request_id.len() > 0);
        assert!(response_body.timestamp.len() > 0);
    }

    #[tokio::test]
    async fn test_authorize_missing_principal() {
        // Create a mock app state
        let state = create_mock_app_state();
        
        // Build our application with the mock state
        let app = Router::new()
            .route("/authorize", post(authorize))
            .with_state(state);
        
        // Create a request body with missing principal
        let request_body = json!({
            "action": "read",
            "resource": "document/123",
            "context": {}
        });
        
        // Send the request
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/authorize")
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
    async fn test_authorize_missing_action() {
        // Create a mock app state
        let state = create_mock_app_state();
        
        // Build our application with the mock state
        let app = Router::new()
            .route("/authorize", post(authorize))
            .with_state(state);
        
        // Create a request body with missing action
        let request_body = json!({
            "principal": "user",
            "resource": "document/123",
            "context": {}
        });
        
        // Send the request
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/authorize")
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
    async fn test_authorize_missing_resource() {
        // Create a mock app state
        let state = create_mock_app_state();
        
        // Build our application with the mock state
        let app = Router::new()
            .route("/authorize", post(authorize))
            .with_state(state);
        
        // Create a request body with missing resource
        let request_body = json!({
            "principal": "user",
            "action": "read",
            "context": {}
        });
        
        // Send the request
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/authorize")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        
        // Check the response status
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
