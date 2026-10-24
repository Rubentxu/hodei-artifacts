//! HTTP routing for the Hodei Artifacts API.
//!
//! The router is built once during bootstrap and owns:
//! - the health probes (`/health`, `/health/ready`, `/health/live`)
//! - the versioned API surface (`/api/v1`)
//! - the OpenAPI / Swagger UI documentation
//! - the cross-cutting middleware (tracing, timeout, CORS)
//!
//! Keeping the router in the library target (instead of `main.rs`) is what
//! allows the HTTP contract to be asserted in-process without spawning a
//! server and binding a port.

use crate::app_state::AppState;
use crate::config::AppConfig;
use crate::handlers::health::health_check;
use crate::openapi::create_api_doc;
use axum::{
    Router,
    http::StatusCode,
    routing::{delete, get, post, put},
};
use std::time::Duration;
use tower_http::{
    cors::CorsLayer,
    timeout::TimeoutLayer,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use tracing::Level;
use utoipa_swagger_ui::SwaggerUi;

/// Build the Axum router with all routes and middleware.
pub fn build_router(app_state: AppState, config: &AppConfig) -> Router {
    Router::new()
        // Health check endpoints
        .route("/health", get(health_check))
        .route("/health/ready", get(health_check))
        .route("/health/live", get(health_check))
        // API v1 routes
        .nest("/api/v1", api_v1_routes(app_state))
        // Swagger UI - serve at /swagger-ui
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", create_api_doc()))
        // Middleware layers (applied in reverse order)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(config.server.request_timeout_secs),
        ))
        .layer(CorsLayer::permissive()) // TODO: Configure CORS properly for production
}

/// API v1 routes
fn api_v1_routes(app_state: AppState) -> Router {
    Router::new()
        // Schema management
        .route(
            "/schemas/build",
            post(crate::handlers::schemas::build_schema),
        )
        .route("/schemas/load", get(crate::handlers::schemas::load_schema))
        .route(
            "/schemas/register-iam",
            post(crate::handlers::schemas::register_iam_schema),
        )
        // Policy validation and evaluation
        .route(
            "/policies/validate",
            post(crate::handlers::policies::validate_policy),
        )
        .route(
            "/policies/evaluate",
            post(crate::handlers::policies::evaluate_policies),
        )
        // IAM Policy Management
        .route("/iam/policies", post(crate::handlers::iam::create_policy))
        .route("/iam/policies", get(crate::handlers::iam::list_policies))
        .route("/iam/policies/get", post(crate::handlers::iam::get_policy))
        .route(
            "/iam/policies/update",
            put(crate::handlers::iam::update_policy),
        )
        .route(
            "/iam/policies/delete",
            delete(crate::handlers::iam::delete_policy),
        )
        .route(
            "/iam/evaluate",
            post(crate::handlers::iam::evaluate_iam_policies),
        )
        // Playground routes
        .route(
            "/playground/evaluate",
            post(crate::handlers::playground::playground_evaluate),
        )
        // TODO: Add more routes as needed
        // .route("/users", post(crate::handlers::users::create_user))
        // .route("/users/:id", get(crate::handlers::users::get_user))
        // .route("/groups", post(crate::handlers::groups::create_group))
        .with_state(app_state)
}
