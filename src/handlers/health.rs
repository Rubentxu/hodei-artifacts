//! Health check handlers
//!
//! This module provides health check endpoints for monitoring and orchestration.
//! Health checks are used by load balancers, Kubernetes, and monitoring systems
//! to determine if the service is healthy and ready to accept traffic.

use crate::app_state::AppState;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
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

/// Liveness response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "status": "alive",
    "version": "0.1.0",
    "service": "hodei-artifacts-api",
    "timestamp": "2024-01-15T10:30:00Z"
}))]
pub struct LivenessResponse {
    /// Service status: `alive`
    pub status: String,
    /// Service version
    pub version: String,
    /// Service name
    pub service: String,
    /// Timestamp of the check
    pub timestamp: String,
}

/// The state of one dependency the active profile requires
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "name": "content_store",
    "status": "unreachable"
}))]
pub struct DependencyStatus {
    /// The name of the dependency: `catalog`, `content_store` or `trust_policy`
    pub name: String,
    /// The coarse state: `ok`, `unreachable`, `timeout` or `misconfigured`
    pub status: String,
}

/// Readiness response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "status": "not_ready",
    "dependencies": [
        { "name": "catalog", "status": "ok" },
        { "name": "content_store", "status": "unreachable" }
    ]
}))]
pub struct ReadinessResponse {
    /// The verdict: `ready` or `not_ready`
    pub status: String,
    /// The state of every dependency the active profile requires
    pub dependencies: Vec<DependencyStatus>,
}

/// Liveness probe handler
///
/// This endpoint says that the **process** lives. It never looks at the
/// catalog, never looks at the store of content and does no I/O: a
/// dependency that is down must not take the process out of the load
/// balancer, because the process is fine and the traffic it is offered
/// would fail anyway.
///
/// # Returns
///
/// A JSON response with the liveness status
#[utoipa::path(
    get,
    path = "/health/live",
    tag = "health",
    responses(
        (status = 200, description = "The process is alive", body = LivenessResponse)
    )
)]
pub async fn liveness() -> impl IntoResponse {
    let response = LivenessResponse {
        status: "alive".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        service: "hodei-artifacts-api".to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };

    (StatusCode::OK, Json(response))
}

/// Readiness probe handler
///
/// This endpoint says that the deployment can **serve**: that every
/// dependency the active profile requires is available. A dependency that
/// is not available makes the answer `503`, and the process keeps living.
///
/// The body is coarse on purpose. It names the dependency and the coarse
/// state of it, and nothing else: no URL, no bucket, no path and no
/// credential ever travel in it. The raw error of a driver can carry all
/// four --an S3 error carries the endpoint and sometimes the session
/// key--, so it never travels.
///
/// # Returns
///
/// A JSON response with the verdict and the state of every required
/// dependency
#[utoipa::path(
    get,
    path = "/health/ready",
    tag = "health",
    responses(
        (status = 200, description = "Every required dependency is available", body = ReadinessResponse),
        (status = 503, description = "A required dependency is not available", body = ReadinessResponse)
    )
)]
pub async fn readiness(State(state): State<AppState>) -> impl IntoResponse {
    let report = state.readiness.report().await;
    // Every required dependency is named, and not only the failing ones: a
    // verdict that says what is fine is as useful as one that says what is
    // not.
    let dependencies = state
        .readiness
        .required()
        .iter()
        .map(|dependency| {
            let status = report
                .failing
                .iter()
                .find(|(failing, _)| failing == dependency)
                .map(|(_, failure)| failure.as_str())
                .unwrap_or("ok");
            DependencyStatus {
                name: dependency.as_str().to_string(),
                status: status.to_string(),
            }
        })
        .collect();
    let response = ReadinessResponse {
        status: if report.ready {
            "ready".to_string()
        } else {
            "not_ready".to_string()
        },
        dependencies,
    };

    if report.ready {
        (StatusCode::OK, Json(response))
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(response))
    }
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
