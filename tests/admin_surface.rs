//! Security contract for the administrative HTTP surface.
//!
//! The IAM/policy/schema routes are administrative. This service does not
//! authenticate requests yet, so the default configuration must not expose
//! them. The tests drive the real router in-process, without binding a port.

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use tempfile::TempDir;
use tower::ServiceExt;

const ADMIN_ROUTES: &[(&str, &str)] = &[
    ("POST", "/api/v1/schemas/build"),
    ("GET", "/api/v1/schemas/load"),
    ("POST", "/api/v1/schemas/register-iam"),
    ("POST", "/api/v1/policies/validate"),
    ("POST", "/api/v1/policies/evaluate"),
    ("POST", "/api/v1/iam/policies"),
    ("GET", "/api/v1/iam/policies"),
    ("POST", "/api/v1/iam/policies/get"),
    ("PUT", "/api/v1/iam/policies/update"),
    ("DELETE", "/api/v1/iam/policies/delete"),
    ("POST", "/api/v1/iam/evaluate"),
    ("POST", "/api/v1/playground/evaluate"),
];

const GATE_MARKER: &str = "administrative API disabled";

async fn test_app() -> (Router, TempDir) {
    test_app_with(false, Vec::new()).await
}

async fn test_app_with(
    admin_enabled: bool,
    cors_allowed_origins: Vec<String>,
) -> (Router, TempDir) {
    let tmp = tempfile::tempdir().expect("temp dir");

    let mut config = AppConfig::default();
    config.security.admin_api_enabled = admin_enabled;
    config.security.cors_allowed_origins = cors_allowed_origins;
    config.rocksdb.path = tmp
        .path()
        .join("hodei.rocksdb")
        .to_string_lossy()
        .into_owned();

    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .expect("bootstrap must succeed");

    let app = build_router(state, &config);
    (app, tmp)
}

async fn response_of(app: &Router, method: &str, uri: &str) -> (StatusCode, String) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .expect("request");

    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn administrative_routes_are_closed_by_default() {
    let (app, _tmp) = test_app().await;

    for (method, uri) in ADMIN_ROUTES {
        let (status, body) = response_of(&app, method, uri).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{method} {uri} must not be reachable without authentication"
        );
        assert_eq!(
            body, GATE_MARKER,
            "{method} {uri} must be stopped by the gate before the handler runs"
        );
    }
}

#[tokio::test]
async fn health_probes_remain_public() {
    let (app, _tmp) = test_app().await;

    for uri in ["/health", "/health/ready", "/health/live"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK, "{uri} must stay public");
    }
}

#[tokio::test]
async fn cors_does_not_allow_arbitrary_origins_by_default() {
    let (app, _tmp) = test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .header(header::ORIGIN, "https://evil.example")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none(),
        "the default CORS policy must not authorize arbitrary origins"
    );
}

#[tokio::test]
async fn administrative_routes_can_be_enabled_explicitly() {
    let (app, _tmp) = test_app_with(true, Vec::new()).await;

    let (status, _body) = response_of(&app, "GET", "/api/v1/schemas/load").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the explicit opt-in must restore the administrative surface"
    );
}

#[tokio::test]
async fn cors_allows_only_the_configured_origins() {
    // The trailing slash is normalized: browsers never send it in `Origin`.
    let (app, _tmp) = test_app_with(false, vec!["https://app.example/".to_string()]).await;

    let allowed = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .header(header::ORIGIN, "https://app.example")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(
        allowed
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .expect("allowed origin header"),
        "https://app.example"
    );

    let denied = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .header(header::ORIGIN, "https://evil.example")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert!(
        denied
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none(),
        "an origin outside the allowlist must not be authorized"
    );
}

#[tokio::test]
async fn wildcard_in_unvalidated_config_keeps_cors_closed() {
    // `build_router` is public and callers may skip `AppConfig::validate`.
    for wildcard in ["*", "*/"] {
        let (app, _tmp) = test_app_with(false, vec![wildcard.to_string()]).await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .header(header::ORIGIN, "https://evil.example")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none(),
            "an unvalidated wildcard ({wildcard}) must not reopen CORS"
        );
    }
}

#[tokio::test]
async fn allowed_cors_origin_does_not_open_the_admin_surface() {
    let (app, _tmp) = test_app_with(false, vec!["https://app.example".to_string()]).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/iam/policies")
                .header(header::ORIGIN, "https://app.example")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "https://app.example"
    );
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    assert_eq!(&body[..], GATE_MARKER.as_bytes());
}

#[tokio::test]
async fn repository_route_is_not_served() {
    for admin_enabled in [false, true] {
        let (app, _tmp) = test_app_with(admin_enabled, Vec::new()).await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repositories?hrn=hrn%3Ahodei%3Aartifacts%3Aeu%3Aacme%3Arepository%2Freleases")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        let status = response.status();
        if admin_enabled {
            assert_eq!(
                status,
                StatusCode::NOT_FOUND,
                "the registry route must remain unregistered even with admin enabled"
            );
        } else {
            assert!(
                matches!(status, StatusCode::FORBIDDEN | StatusCode::NOT_FOUND),
                "the registry route must stay closed (admin_enabled=false), got {status}"
            );
        }
    }
}
