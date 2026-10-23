//! Test Helpers
//!
//! This module provides utility functions and helpers for integration
//! and E2E tests, including HTTP client setup, assertion helpers,
//! and common test workflows.

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use hodei_iam::features::create_policy::dto::CreatePolicyCommand;
use hodei_iam::features::get_policy::dto::GetPolicyQuery;
use hodei_iam::features::list_policies::dto::ListPoliciesQuery;
use hodei_iam::infrastructure::surreal::policy_adapter::SurrealPolicyAdapter;
use hodei_policies::features::validate_policy::use_case::ValidatePolicyUseCase;
use kernel::{HodeiPolicy, Hrn};
use serde_json::Value;
use std::sync::Arc;
use surrealdb::Surreal;
use surrealdb::engine::remote::ws::Client;
use tower::ServiceExt;

/// HTTP test client wrapper
pub struct TestClient {
    router: Router,
}

impl TestClient {
    /// Create a new test client with the given router
    pub fn new(router: Router) -> Self {
        Self { router }
    }

    /// Send a GET request
    pub async fn get(&self, uri: &str) -> TestResponse {
        let request = Request::builder()
            .uri(uri)
            .method("GET")
            .body(Body::empty())
            .unwrap();

        self.send_request(request).await
    }

    /// Send a POST request with JSON body
    pub async fn post(&self, uri: &str, body: Value) -> TestResponse {
        let request = Request::builder()
            .uri(uri)
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        self.send_request(request).await
    }

    /// Send a PUT request with JSON body
    pub async fn put(&self, uri: &str, body: Value) -> TestResponse {
        let request = Request::builder()
            .uri(uri)
            .method("PUT")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        self.send_request(request).await
    }

    /// Send a DELETE request
    pub async fn delete(&self, uri: &str) -> TestResponse {
        let request = Request::builder()
            .uri(uri)
            .method("DELETE")
            .body(Body::empty())
            .unwrap();

        self.send_request(request).await
    }

    /// Send a DELETE request with JSON body
    pub async fn delete_with_body(&self, uri: &str, body: Value) -> TestResponse {
        let request = Request::builder()
            .uri(uri)
            .method("DELETE")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        self.send_request(request).await
    }

    /// Send a raw request
    async fn send_request(&self, request: Request<Body>) -> TestResponse {
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("Failed to send request");

        TestResponse::new(response).await
    }
}

/// Test response wrapper
pub struct TestResponse {
    pub status: StatusCode,
    pub body: Value,
}

impl TestResponse {
    /// Create a new test response
    async fn new(response: axum::response::Response) -> Self {
        let status = response.status();
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("Failed to read response body");

        let body = if body_bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body_bytes).unwrap_or(Value::Null)
        };

        Self { status, body }
    }

    /// Assert the response status code
    pub fn assert_status(&self, expected: StatusCode) -> &Self {
        assert_eq!(
            self.status, expected,
            "Expected status {}, got {}. Body: {}",
            expected, self.status, self.body
        );
        self
    }

    /// Assert the response is successful (2xx)
    pub fn assert_success(&self) -> &Self {
        assert!(
            self.status.is_success(),
            "Expected success status, got {}. Body: {}",
            self.status,
            self.body
        );
        self
    }

    /// Assert the response is a client error (4xx)
    pub fn assert_client_error(&self) -> &Self {
        assert!(
            self.status.is_client_error(),
            "Expected client error status, got {}",
            self.status
        );
        self
    }

    /// Assert the response is a server error (5xx)
    pub fn assert_server_error(&self) -> &Self {
        assert!(
            self.status.is_server_error(),
            "Expected server error status, got {}",
            self.status
        );
        self
    }

    /// Assert the response body contains a field with expected value
    pub fn assert_json_field(&self, field: &str, expected: Value) -> &Self {
        let actual = &self.body[field];
        assert_eq!(
            actual, &expected,
            "Expected field '{}' to be {:?}, got {:?}",
            field, expected, actual
        );
        self
    }

    /// Assert the response body contains an error message
    pub fn assert_error_contains(&self, text: &str) -> &Self {
        let error_msg = self.body["error"]
            .as_str()
            .expect("Response should have 'error' field");
        assert!(
            error_msg.contains(text),
            "Expected error to contain '{}', got '{}'",
            text,
            error_msg
        );
        self
    }

    /// Get a field from the response body
    pub fn get_field(&self, field: &str) -> &Value {
        &self.body[field]
    }

    /// Parse the response body as a specific type
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> T {
        serde_json::from_value(self.body.clone()).expect("Failed to parse response body")
    }
}

/// Create a test policy adapter with the given database
pub fn create_policy_adapter(db: Surreal<Client>) -> Arc<SurrealPolicyAdapter> {
    Arc::new(SurrealPolicyAdapter::new(Arc::new(db)))
}

/// Create a test validator
pub fn create_validator<S>(schema_storage: Arc<S>) -> Arc<ValidatePolicyUseCase<S>>
where
    S: hodei_policies::features::build_schema::ports::SchemaStoragePort + Clone + 'static,
{
    Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage))
}

/// Insert a policy directly into the database for testing
pub async fn insert_test_policy(
    db: &Surreal<Client>,
    policy: HodeiPolicy,
) -> Result<(), Box<dyn std::error::Error>> {
    let _: Option<HodeiPolicy> = db.create(("policy", policy.id())).content(policy).await?;
    Ok(())
}

/// Delete a policy directly from the database
pub async fn delete_test_policy(
    db: &Surreal<Client>,
    policy_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let _: Option<HodeiPolicy> = db.delete(("policy", policy_id)).await?;
    Ok(())
}

/// Count policies in the database
pub async fn count_policies(db: &Surreal<Client>) -> Result<usize, Box<dyn std::error::Error>> {
    let policies: Vec<HodeiPolicy> = db.select("policy").await?;
    Ok(policies.len())
}

/// Get a policy from the database by ID
pub async fn get_policy_from_db(
    db: &Surreal<Client>,
    policy_id: &str,
) -> Result<Option<HodeiPolicy>, Box<dyn std::error::Error>> {
    let policy: Option<HodeiPolicy> = db.select(("policy", policy_id)).await?;
    Ok(policy)
}

/// Assert a policy exists in the database
pub async fn assert_policy_exists(db: &Surreal<Client>, policy_id: &str) {
    let policy = get_policy_from_db(db, policy_id)
        .await
        .expect("Failed to query database");
    assert!(
        policy.is_some(),
        "Expected policy '{}' to exist in database",
        policy_id
    );
}

/// Assert a policy does not exist in the database
pub async fn assert_policy_not_exists(db: &Surreal<Client>, policy_id: &str) {
    let policy = get_policy_from_db(db, policy_id)
        .await
        .expect("Failed to query database");
    assert!(
        policy.is_none(),
        "Expected policy '{}' to not exist in database",
        policy_id
    );
}

/// Assert policy count in database
pub async fn assert_policy_count(db: &Surreal<Client>, expected: usize) {
    let count = count_policies(db).await.expect("Failed to count policies");
    assert_eq!(
        count, expected,
        "Expected {} policies in database, found {}",
        expected, count
    );
}

/// Wait for a condition with timeout
pub async fn wait_for<F, Fut>(condition: F, timeout_ms: u64) -> bool
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_millis(timeout_ms);

    while start.elapsed() < timeout {
        if condition().await {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    false
}

/// Create multiple test policies in parallel
pub async fn create_policies_parallel(
    db: &Surreal<Client>,
    count: usize,
    prefix: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut handles = vec![];
    let mut policy_ids = vec![];

    for i in 0..count {
        let policy_id = format!("{}-{}", prefix, i);
        policy_ids.push(policy_id.clone());

        let db_clone = db.clone();
        let policy = HodeiPolicy::new(
            policy_id,
            "permit(principal, action, resource);".to_string(),
        );

        let handle = tokio::spawn(async move { insert_test_policy(&db_clone, policy).await });
        handles.push(handle);
    }

    for handle in handles {
        handle.await??;
    }

    Ok(policy_ids)
}

/// Measure execution time of an async operation
pub async fn measure_time<F, Fut, T>(operation: F) -> (T, std::time::Duration)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    let start = std::time::Instant::now();
    let result = operation().await;
    let duration = start.elapsed();
    (result, duration)
}

/// Generate a unique test ID based on test name and timestamp
pub fn unique_test_id(prefix: &str) -> String {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    format!("{}-{}", prefix, timestamp)
}

/// Create a test HRN with unique ID
pub fn unique_test_hrn(resource_type: &str) -> Hrn {
    let id = unique_test_id(resource_type);
    Hrn::new(
        "hodei".to_string(),
        "iam".to_string(),
        "test".to_string(),
        resource_type.to_string(),
        id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unique_test_id() {
        let id1 = unique_test_id("test");
        let id2 = unique_test_id("test");
        assert_ne!(id1, id2, "IDs should be unique");
    }

    #[test]
    fn test_unique_test_hrn() {
        let hrn1 = unique_test_hrn("Policy");
        let hrn2 = unique_test_hrn("Policy");
        assert_ne!(hrn1, hrn2, "HRNs should be unique");
    }

    #[tokio::test]
    async fn test_measure_time() {
        let (result, duration) = measure_time(|| async {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            42
        })
        .await;

        assert_eq!(result, 42);
        assert!(duration.as_millis() >= 100);
    }

    #[tokio::test]
    async fn test_wait_for_success() {
        let mut counter = 0;
        let result = wait_for(
            || async {
                counter += 1;
                counter >= 3
            },
            5000,
        )
        .await;

        assert!(result, "Condition should be met");
    }

    #[tokio::test]
    async fn test_wait_for_timeout() {
        let result = wait_for(|| async { false }, 100).await;
        assert!(!result, "Should timeout");
    }
}

/// Mock SchemaStorage for testing
pub struct MockSchemaStorage {
    schemas: Arc<tokio::sync::RwLock<std::collections::HashMap<String, Vec<u8>>>>,
}

impl MockSchemaStorage {
    pub fn new() -> Self {
        Self {
            schemas: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }
}

impl Default for MockSchemaStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl hodei_policies::features::build_schema::ports::SchemaStoragePort for MockSchemaStorage {
    async fn save_schema(
        &self,
        version: String,
        schema: Vec<u8>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut schemas = self.schemas.write().await;
        schemas.insert(version, schema);
        Ok(())
    }

    async fn get_latest_schema(
        &self,
    ) -> Result<Option<(String, Vec<u8>)>, Box<dyn std::error::Error + Send + Sync>> {
        let schemas = self.schemas.read().await;
        Ok(schemas.iter().next().map(|(k, v)| (k.clone(), v.clone())))
    }

    async fn get_schema_by_version(
        &self,
        version: String,
    ) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
        let schemas = self.schemas.read().await;
        Ok(schemas.get(&version).cloned())
    }

    async fn delete_schema(
        &self,
        version: String,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut schemas = self.schemas.write().await;
        schemas.remove(&version);
        Ok(())
    }

    async fn list_schema_versions(
        &self,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        let schemas = self.schemas.read().await;
        Ok(schemas.keys().cloned().collect())
    }
}

impl Clone for MockSchemaStorage {
    fn clone(&self) -> Self {
        Self {
            schemas: Arc::clone(&self.schemas),
        }
    }
}
