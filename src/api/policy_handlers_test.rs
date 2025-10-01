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
    use crate::{app_state::{AppState, AppMetrics, HealthStatus}, config::Config, adapters::SurrealDbAdapter, ports::{AuthorizationEnginePort, PolicyStorePort, StorageAdapterPort}};

    async fn build_test_state() -> Arc<AppState> {
        let config = Config::from_env().expect("config");

        // Minimal adapters for required ports (placeholders with SurrealDbAdapter)
        let storage: Arc<dyn StorageAdapterPort> = Arc::new(SurrealDbAdapter::connect(&config.database.url).await.expect("db"));
        let engine: Arc<dyn AuthorizationEnginePort> = Arc::new(SurrealDbAdapter::new());
        let policy_store: Arc<dyn PolicyStorePort> = Arc::new(SurrealDbAdapter::new());

        // DI for policies create_policy use case
        #[cfg(feature = "embedded")]
        let (uc, _engine_uc) = policies::features::create_policy::di::embedded::make_use_case_embedded(&config.database.url)
            .await
            .expect("uc embedded");
        #[cfg(not(feature = "embedded"))]
        let (uc, _engine_uc) = policies::features::create_policy::di::make_use_case_mem()
            .await
            .expect("uc mem");

        Arc::new(AppState {
            engine,
            policy_store,
            storage,
            config,
            metrics: AppMetrics::new(),
            health: Arc::new(tokio::sync::RwLock::new(HealthStatus::new())),
            create_policy_uc: Some(Arc::new(uc)),
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
}
