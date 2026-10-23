//! End-to-End Tests for Hodei Artifacts API
//!
//! This module contains comprehensive end-to-end tests that verify the complete
//! system behavior from HTTP requests through business logic to responses.
//! Tests use real services and infrastructure to validate integration.

pub mod health;
pub mod playground;
pub mod policies;
pub mod schemas;

// Re-export commonly used test utilities
pub use test_utils::*;

/// Test utilities module
pub mod test_utils {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use hodei_artifacts_api::{bootstrap, config::Config};
    use serde_json::{Value, json};
    use std::time::Duration;
    use tower::ServiceExt;

    /// Test client for making HTTP requests to the API
    pub struct TestClient {
        pub app: axum::Router,
    }

    impl TestClient {
        /// Create a new test client with the application router
        pub fn new() -> Self {
            let config = Config::default();
            let app_state =
                tokio_test::block_on(bootstrap::bootstrap(bootstrap::BootstrapConfig::default()))
                    .expect("Failed to bootstrap application");

            let app = hodei_artifacts_api::build_router(app_state, &config);
            Self { app }
        }

        /// Make a GET request to the API
        pub async fn get(&self, path: &str) -> TestResponse {
            let request = Request::builder().uri(path).body(Body::empty()).unwrap();

            let response = self.app.clone().oneshot(request).await.unwrap();
            TestResponse { response }
        }

        /// Make a POST request to the API with JSON body
        pub async fn post(&self, path: &str, body: Value) -> TestResponse {
            let request = Request::builder()
                .method("POST")
                .uri(path)
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap();

            let response = self.app.clone().oneshot(request).await.unwrap();
            TestResponse { response }
        }
    }

    /// Test response wrapper for easier assertions
    pub struct TestResponse {
        pub response: axum::response::Response,
    }

    impl TestResponse {
        /// Assert that the response has the expected status code
        pub fn assert_status(&self, expected: StatusCode) -> &Self {
            assert_eq!(self.response.status(), expected);
            self
        }

        /// Get the response body as JSON
        pub async fn json(&mut self) -> Value {
            let bytes = hyper::body::to_bytes(self.response.body_mut())
                .await
                .unwrap();
            serde_json::from_slice(&bytes).unwrap()
        }

        /// Assert that the response body contains the expected JSON structure
        pub async fn assert_json_contains(&mut self, expected: Value) -> &mut Self {
            let actual = self.json().await;
            assert_json_contains(&actual, &expected);
            self
        }

        /// Assert that the response body matches the expected JSON exactly
        pub async fn assert_json(&mut self, expected: Value) -> &mut Self {
            let actual = self.json().await;
            assert_eq!(actual, expected);
            self
        }
    }

    /// Helper function to assert that actual JSON contains expected JSON
    pub fn assert_json_contains(actual: &Value, expected: &Value) {
        match (actual, expected) {
            (Value::Object(actual_obj), Value::Object(expected_obj)) => {
                for (key, expected_value) in expected_obj {
                    if let Some(actual_value) = actual_obj.get(key) {
                        assert_json_contains(actual_value, expected_value);
                    } else {
                        panic!("Missing key in JSON: {}", key);
                    }
                }
            }
            (Value::Array(actual_arr), Value::Array(expected_arr)) => {
                for (i, expected_value) in expected_arr.iter().enumerate() {
                    if i < actual_arr.len() {
                        assert_json_contains(&actual_arr[i], expected_value);
                    } else {
                        panic!("Array too short: expected at least {} elements", i + 1);
                    }
                }
            }
            (actual, expected) => {
                assert_eq!(actual, expected, "JSON values don't match");
            }
        }
    }

    /// Helper function to create a valid HRN string for testing
    pub fn test_hrn(service: &str, resource_type: &str, resource_id: &str) -> String {
        format!(
            "hodei::{}::default::{}::{}",
            service, resource_type, resource_id
        )
    }

    /// Helper function to create a valid action HRN for testing
    pub fn test_action(service: &str, action: &str) -> String {
        format!("hodei::{}::Action::{}", service, action)
    }

    /// Helper function to wait for async operations to complete
    pub async fn wait_for<F, T>(mut f: F, timeout: Duration) -> T
    where
        F: FnMut() -> Option<T>,
    {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if let Some(result) = f() {
                return result;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("Timeout waiting for condition");
    }
}

/// Main test runner setup
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_endpoint() {
        let client = TestClient::new();
        let mut response = client.get("/health").await;

        response.assert_status(StatusCode::OK);
        let body = response.json().await;
        assert!(body.get("status").is_some());
    }
}
