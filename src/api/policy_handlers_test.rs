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
    use crate::{app_state::{AppState, AppMetrics, HealthStatus}, config::Config};

    async fn build_test_state() -> Arc<AppState> {
        let config = Config::from_env().expect("config");

        // Build use cases from policies crate via DI
        #[cfg(feature = "embedded")]
        let (create_uc, authorization_engine) = policies::features::create_policy::di::embedded::make_use_case_embedded(&config.database.url)
            .await
            .expect("create uc embedded");
        #[cfg(not(feature = "embedded"))]
        let (create_uc, authorization_engine) = policies::features::create_policy::di::make_use_case_mem()
            .await
            .expect("create uc mem");

        #[cfg(feature = "embedded")]
        let (get_uc, _) = policies::features::get_policy::di::embedded::make_use_case_embedded(&config.database.url)
            .await
            .expect("get uc embedded");
        #[cfg(not(feature = "embedded"))]
        let (get_uc, _) = policies::features::get_policy::di::make_use_case_mem()
            .await
            .expect("get uc mem");

        Arc::new(AppState {
            config,
            metrics: AppMetrics::new(),
            health: Arc::new(tokio::sync::RwLock::new(HealthStatus::new())),
            create_policy_uc: Arc::new(create_uc),
            get_policy_uc: Arc::new(get_uc),
            authorization_engine,
        })
    }

    #[tokio::test]
    async fn test_create_policy_success() {
        let state = build_test_state().await;
        let app = Router::new()
            .route("/api/v1/policies", post(create_policy))
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
                    .uri("/api/v1/policies")
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
        let state = build_test_state().await;
        let app = Router::new()
            .route("/api/v1/policies", post(create_policy))
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
        let state = build_test_state().await;
        let app = Router::new()
            .route("/api/v1/policies", post(create_policy))
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
        let state = build_test_state().await;
        let app = Router::new()
            .route("/api/v1/policies", get(list_policies))
            .with_state(state);
        
        // Send the request
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/policies")
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
        let state = build_test_state().await;
        let app = Router::new()
            .route("/api/v1/policies/:id", delete(delete_policy))
            .with_state(state);
        
        // Send the request
        let response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/policies/test-policy-id")
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
        let state = build_test_state().await;
        let app = Router::new()
            .route("/api/v1/policies/:id", delete(delete_policy))
            .with_state(state);
        
        // Send the request with empty policy ID
        let response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/policies/")
                    .header("content-type", "application/json")
                    .body(Body::from(""))
                    .unwrap(),
            )
            .await
            .unwrap();
        
        // Check the response status
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_get_policy_not_found() {
        let state = build_test_state().await;
        let app = Router::new()
            .route("/api/v1/policies/:id", get(crate::api::get_policy))
            .with_state(state);
        
        // Send the request for a non-existent policy
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/policies/nonexistent-policy-id")
                    .header("content-type", "application/json")
                    .body(Body::from(""))
                    .unwrap(),
            )
            .await
            .unwrap();
        
        // Check the response status - should be 404 Not Found
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        
        // Check the response body contains error information
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let response_body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        
        assert!(response_body["error"].is_object());
        assert_eq!(response_body["error"]["type"], "NOT_FOUND");
    }

    #[tokio::test]
    async fn test_get_policy_empty_id() {
        let state = build_test_state().await;
        let app = Router::new()
            .route("/api/v1/policies/:id", get(crate::api::get_policy))
            .with_state(state);
        
        // Send the request with empty policy ID (will be caught by validation)
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/policies/%20") // URL-encoded space
                    .header("content-type", "application/json")
                    .body(Body::from(""))
                    .unwrap(),
            )
            .await
            .unwrap();
        
        // Check the response status - should be 400 Bad Request
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
