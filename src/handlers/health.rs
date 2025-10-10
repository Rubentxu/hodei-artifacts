//! Health check handlers
//!
//! This module provides health check endpoints for monitoring and orchestration.
//! Health checks are used by load balancers, Kubernetes, and monitoring systems
//! to determine if the service is healthy and ready to accept traffic.

use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Health check response
/// Alias for HealthStatus to maintain backward compatibility
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "status": "healthy",
    "version": "0.1.0",
    "service": "hodei-artifacts-api",
    "timestamp": "2024-01-15T10:30:00Z"
}))]
pub struct HealthResponse {
    /// Service status
    pub status: String,
    /// Service version
    pub version: String,
    /// Service name
    pub service: String,
    /// Timestamp of the health check
    pub timestamp: String,
}

/// Health check handler
///
/// This endpoint returns a simple health status indicating that the service
/// is running and responsive. It's used for basic liveness probes.
///
/// # Returns
///
/// A JSON response with health status
///
/// # Example Response
///
/// ```json
/// {
///   "status": "healthy",
///   "version": "0.1.0",
///   "service": "hodei-artifacts-api",
///   "timestamp": "2024-01-15T10:30:00Z"
/// }
/// ```
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses(
        (status = 200, description = "Service is healthy", body = HealthResponse)
    )
)]
pub async fn health_check() -> impl IntoResponse {
    let response = HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        service: "hodei-artifacts-api".to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };

    (StatusCode::OK, Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_check_returns_healthy_status() {
        let response = health_check().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
