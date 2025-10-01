#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::{delete, get, post},
        Router,
    };
    use serde_json::json;
    use tower::ServiceExt; // for `call` method on `Router`

    // Import the necessary modules from our application
    use hodei_artifacts_api::{
        api::{authorize, create_policy, delete_policy, health, list_policies, metrics, readiness},
        app_state::{AppMetrics, AppState, HealthStatus},
        config::Config,
    };

    // Test helper to create a mock AppState
    fn create_mock_app_state() -> AppState {
        // This would need to be implemented with mock versions of the components
        // For now, we'll just create a placeholder
        unimplemented!("Create mock app state for integration testing")
    }

    #[tokio::test]
    async fn test_full_api_integration() {
        // Create a mock app state
        let state = create_mock_app_state();
        
        // Build our application with the mock state
        let app = Router::new()
            // Policy management routes
            .route("/policies", post(create_policy))
            .route("/policies", get(list_policies))
            .route("/policies/:id", delete(delete_policy))
            // Authorization route
            .route("/authorize", post(authorize))
            // Health routes
            .route("/health", get(health))
            .route("/ready", get(readiness))
            // Metrics route
            .route("/metrics", get(metrics))
            .with_state(state);
        
        // Test health endpoint
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/health")
                    .body(Body::from(""))
                    .unwrap(),
            )
            .await
            .unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
        
        // Test readiness endpoint
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/ready")
                    .body(Body::from(""))
                    .unwrap(),
            )
            .await
            .unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
        
        // Test metrics endpoint
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/metrics")
                    .body(Body::from(""))
                    .unwrap(),
            )
            .await
            .unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
        
        // Test create policy endpoint
        let request_body = json!({
            "name": "integration-test-policy",
            "description": "An integration test policy",
            "policy_content": "permit(principal, action, resource);",
            "enabled": true
        });
        
        let response = app
            .clone()
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
        
        assert_eq!(response.status(), StatusCode::OK);
        
        // Test list policies endpoint
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/policies")
                    .body(Body::from(""))
                    .unwrap(),
            )
            .await
            .unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
        
        // Test authorize endpoint
        let request_body = json!({
            "principal": "admin",
            "action": "read",
            "resource": "document/123",
            "context": {}
        });
        
        let response = app
            .clone()
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
        
        assert_eq!(response.status(), StatusCode::OK);
    }
}
