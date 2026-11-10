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
    body::Body,
    extract::State,
    http::{HeaderValue, Method, Request, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use std::time::Duration;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    timeout::TimeoutLayer,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use tracing::Level;
use utoipa_swagger_ui::SwaggerUi;

/// Build the Axum router with all routes and middleware.
pub fn build_router(app_state: AppState, config: &AppConfig) -> Router {
    // The administrative surface is closed unless it is explicitly enabled.
    // This service does not authenticate requests yet, so the default must
    // deny before any handler runs.
    let admin_api = admin_v1_routes(app_state.clone()).layer(middleware::from_fn_with_state(
        config.security.admin_api_enabled,
        admin_gate,
    ));
    let repositories = repository_v1_routes(app_state);
    let api = admin_api.merge(repositories);

    Router::new()
        // Health check endpoints
        .route("/health", get(health_check))
        .route("/health/ready", get(health_check))
        .route("/health/live", get(health_check))
        // API v1 routes
        .nest("/api/v1", api)
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
        .layer(cors_layer(config))
}

/// Fail closed on the administrative surface unless it was explicitly enabled.
///
/// CORS preflight (`OPTIONS`) is answered by the outer CORS layer and does not
/// reach this gate; no handler runs in that path either.
async fn admin_gate(State(enabled): State<bool>, req: Request<Body>, next: Next) -> Response {
    if enabled {
        next.run(req).await
    } else {
        (StatusCode::FORBIDDEN, "administrative API disabled").into_response()
    }
}

/// Build the CORS policy from the explicit origin allowlist.
///
/// An empty allowlist means no cross-origin access. `*` is rejected by
/// `SecurityConfig::validate`, so the permissive layer cannot be reintroduced
/// by configuration.
fn cors_layer(config: &AppConfig) -> CorsLayer {
    if config.security.cors_allowed_origins.is_empty() {
        return CorsLayer::new();
    }

    // Normalize first so a value such as `"*/"` cannot slip past the wildcard
    // guard and reach `AllowOrigin::list`, which panics on a wildcard.
    let normalized: Vec<String> = config
        .security
        .cors_allowed_origins
        .iter()
        .map(|origin| origin.trim_end_matches('/').to_string())
        .collect();

    // `build_router` is public and callers may skip `AppConfig::validate`.
    // Enforce the same invariant here so a wildcard cannot reintroduce the
    // permissive policy through an unvalidated config.
    if normalized.iter().any(|origin| origin.trim() == "*") {
        tracing::error!("CORS wildcard is not allowed; cross-origin access stays closed");
        return CorsLayer::new();
    }

    let origins: Vec<HeaderValue> = normalized
        .iter()
        .filter_map(|origin| match HeaderValue::from_str(origin) {
            Ok(value) => Some(value),
            Err(_) => {
                tracing::error!(
                    origin = %origin,
                    "ignoring invalid CORS origin; cross-origin access stays closed for it"
                );
                None
            }
        })
        .collect();

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
}

/// API v1 routes
fn admin_v1_routes(app_state: AppState) -> Router {
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

/// Repository routes have their own credential/authorization gate; they are
/// deliberately not behind `admin_api_enabled`.
fn repository_v1_routes(app_state: AppState) -> Router {
    Router::new()
        .route(
            "/repositories",
            get(crate::handlers::repositories::get_repository),
        )
        .with_state(app_state)
}
