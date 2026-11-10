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
use hodei_authz::{ActionId, Credentials, VerifiedContext, verify_principal};
use kernel::Hrn;
use std::time::Duration;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    timeout::TimeoutLayer,
    trace::{DefaultOnResponse, TraceLayer},
};
use tracing::Level;
use utoipa_swagger_ui::SwaggerUi;

/// Build the Axum router with all routes and middleware.
pub fn build_router(app_state: AppState, config: &AppConfig) -> Router {
    // The administrative surface is closed unless it is explicitly enabled.
    // This service does not authenticate requests yet, so the default must
    // deny before any handler runs.
    let admin_api = admin_v1_routes(app_state.clone()).layer(middleware::from_fn_with_state(
        (config.security.admin_api_enabled, app_state.clone()),
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
                .make_span_with(|request: &Request<Body>| {
                    let correlation_id = request
                        .headers()
                        .get("x-request-id")
                        .and_then(|value| value.to_str().ok())
                        .filter(|value| {
                            !value.is_empty()
                                && value.len() <= 128
                                && value.chars().all(|character| {
                                    !character.is_control() && !character.is_whitespace()
                                })
                        })
                        .unwrap_or("-");
                    tracing::info_span!(
                        "http_request",
                        method = %request.method(),
                        uri = %request.uri(),
                        correlation_id = %correlation_id,
                    )
                })
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(config.server.request_timeout_secs),
        ))
        .layer(cors_layer(config))
}

/// Fail closed on the administrative surface.
///
/// The opt-in only opens the surface when the state has both a real credential
/// verifier and a non-default authorizer. Even then every request must present
/// a bearer credential and receive a Cedar Permit for the concrete route.
/// CORS preflight (`OPTIONS`) is answered by the outer CORS layer and does not
/// reach this gate; no handler runs in that path either.
async fn admin_gate(
    State((enabled, app_state)): State<(bool, AppState)>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if !enabled {
        return (StatusCode::FORBIDDEN, "administrative API disabled").into_response();
    }
    if !app_state.admin_surface_ready() {
        tracing::error!(
            "administrative API requested but no authenticator/authorizer is configured; keeping it closed"
        );
        return (
            StatusCode::FORBIDDEN,
            "administrative API requires authentication",
        )
            .into_response();
    }

    let token = match bearer_token(req.headers()) {
        Some(token) => token,
        None => return unauthorized(),
    };
    let principal = match verify_principal(
        app_state.authenticator.as_ref(),
        &Credentials::bearer(token),
    )
    .await
    {
        Ok(principal) => principal,
        Err(_) => return unauthorized(),
    };
    let context = VerifiedContext::from_principal(principal);
    let action = match ActionId::new("admin", "AccessAdminApi") {
        Ok(action) => action,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "invalid admin action").into_response();
        }
    };

    let path = req
        .uri()
        .path()
        .trim_start_matches("/api/v1/")
        .trim_start_matches('/');
    // Include the HTTP method so a policy cannot grant "list" and silently
    // grant "create" on the same path.
    let resource_path = format!(
        "{}/{}",
        req.method().as_str().to_ascii_lowercase(),
        if path.is_empty() { "surface" } else { path }
    );
    let resource = Hrn::with_region(
        "hodei".to_string(),
        "admin".to_string(),
        "global".to_string(),
        context.account().as_str().to_string(),
        "AdminApi".to_string(),
        resource_path,
    );
    let outcome = match app_state
        .authorize_resource
        .execute_canonical(context, action, &resource)
    {
        Ok(outcome) => outcome,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "authorization unavailable",
            )
                .into_response();
        }
    };
    if !outcome.allows_access() {
        return (StatusCode::FORBIDDEN, "administrative permission denied").into_response();
    }

    next.run(req).await
}

fn unauthorized() -> Response {
    let mut response = (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    response
        .headers_mut()
        .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    response
}

fn bearer_token(headers: &axum::http::HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    let token = token.trim();
    if !scheme.eq_ignore_ascii_case("Bearer") || token.is_empty() {
        return None;
    }
    Some(token.to_string())
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
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::HeaderName::from_static("x-request-id"),
        ])
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
            get(crate::handlers::repositories::get_repository)
                .post(crate::handlers::repositories::create_repository),
        )
        .with_state(app_state)
}
