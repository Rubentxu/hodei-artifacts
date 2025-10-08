//! End-to-End Tests for Health API
//!
//! This module contains comprehensive tests for the health check endpoints,
//! verifying that the API is properly running and responding to health checks.

use super::test_utils::TestClient;
use serde_json::json;

/// Test basic health check endpoint
#[tokio::test]
async fn test_health_check() {
    let client = TestClient::new();
    let mut response = client.get("/health").await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;

    // Verify response structure
    assert!(body.get("status").is_some());
    assert_eq!(body.get("status").unwrap().as_str().unwrap(), "healthy");
}

/// Test ready health check endpoint
#[tokio::test]
async fn test_health_ready() {
    let client = TestClient::new();
    let mut response = client.get("/health/ready").await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;
    assert!(body.get("status").is_some());
    assert_eq!(body.get("status").unwrap().as_str().unwrap(), "ready");
}

/// Test live health check endpoint
#[tokio::test]
async fn test_health_live() {
    let client = TestClient::new();
    let mut response = client.get("/health/live").await;

    response.assert_status(axum::http::StatusCode::OK);

    let body = response.json().await;
    assert!(body.get("status").is_some());
    assert_eq!(body.get("status").unwrap().as_str().unwrap(), "live");
}

/// Test that health endpoints return consistent responses
#[tokio::test]
async fn test_health_consistency() {
    let client = TestClient::new();

    // Test all health endpoints in sequence
    let mut response1 = client.get("/health").await;
    response1.assert_status(axum::http::StatusCode::OK);
    let body1 = response1.json().await;

    let mut response2 = client.get("/health/ready").await;
    response2.assert_status(axum::http::StatusCode::OK);
    let body2 = response2.json().await;

    let mut response3 = client.get("/health/live").await;
    response3.assert_status(axum::http::StatusCode::OK);
    let body3 = response3.json().await;

    // All endpoints should return the same status structure
    assert_eq!(body1.get("status").unwrap(), body2.get("status").unwrap());
    assert_eq!(body2.get("status").unwrap(), body3.get("status").unwrap());
}

/// Test health endpoints with different HTTP methods
#[tokio::test]
async fn test_health_methods() {
    let client = TestClient::new();

    // Health endpoints should only accept GET requests
    // Note: The current test client only supports GET and POST
    // This test verifies that GET works correctly
    let response = client.get("/health").await;
    response.assert_status(axum::http::StatusCode::OK);
}

/// Test health endpoint response time
#[tokio::test]
async fn test_health_response_time() {
    let client = TestClient::new();

    let start = std::time::Instant::now();
    let response = client.get("/health").await;
    let duration = start.elapsed();

    response.assert_status(axum::http::StatusCode::OK);

    // Health check should be very fast (under 100ms)
    assert!(
        duration < std::time::Duration::from_millis(100),
        "Health check took too long: {:?}",
        duration
    );
}

/// Test health endpoint with concurrent requests
#[tokio::test]
async fn test_health_concurrent() {
    let client = TestClient::new();

    let futures: Vec<_> = (0..10)
        .map(|_| {
            let client = &client;
            async move {
                let response = client.get("/health").await;
                response.assert_status(axum::http::StatusCode::OK);
            }
        })
        .collect();

    // Execute all health checks concurrently
    futures::future::join_all(futures).await;
}

/// Test health endpoint error handling
#[tokio::test]
async fn test_health_error_handling() {
    let client = TestClient::new();

    // Test non-existent endpoint - should return 404
    let response = client.get("/health/nonexistent").await;
    response.assert_status(axum::http::StatusCode::NOT_FOUND);

    // Test malformed health endpoint
    let response = client.get("/health/").await;
    // This might return 404 or redirect, but should not crash
    assert_ne!(
        response.response.status(),
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    );
}

/// Test health endpoint after application bootstrap
#[tokio::test]
async fn test_health_after_bootstrap() {
    // This test verifies that health endpoints work correctly
    // after the application has been fully bootstrapped
    let client = TestClient::new();

    // Make multiple requests to ensure stability
    for _ in 0..5 {
        let response = client.get("/health").await;
        response.assert_status(axum::http::StatusCode::OK);
    }
}

/// Test health endpoint with different paths
#[tokio::test]
async fn test_health_path_variations() {
    let client = TestClient::new();

    // Test with trailing slash
    let response = client.get("/health/").await;
    // Should handle gracefully (might redirect or return 404)
    assert_ne!(
        response.response.status(),
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    );

    // Test with uppercase (should be case-sensitive in typical setups)
    let response = client.get("/HEALTH").await;
    assert_eq!(
        response.response.status(),
        axum::http::StatusCode::NOT_FOUND
    );
}
