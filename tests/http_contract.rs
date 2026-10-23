//! HTTP contract for the Hodei Artifacts API.
//!
//! # Why this file exists
//!
//! The root package used to ship as a binary only. The suites under
//! `tests/e2e/`, `tests/integration/` and `tests/common/` referenced
//! `hodei_artifacts_api::…` paths that did not exist, so Cargo never compiled
//! them: 109 tests had never been executed by anyone. Those suites are now
//! quarantined under `tests/legacy/` (see `tests/legacy/README.md` for the
//! drift report).
//!
//! This file is the permanent replacement: a small, deterministic, in-process
//! contract that asserts the guarantees a consumer of the HTTP API actually
//! depends on. It binds no port and spawns no process — the router is driven
//! directly through `tower::ServiceExt::oneshot`.
//!
//! # Guarantees
//!
//! - G1 `/health` reports `healthy` and identifies the service.
//! - G2 `/health/ready` and `/health/live` are served and report a status.
//! - G3 An unknown route is a `404`.
//! - G4 A syntactically valid Cedar policy validates.
//! - G5 A syntactically invalid Cedar policy is rejected with errors (the
//!   discriminating negative of G4).
//! - G6 The IAM policy lifecycle works end to end and the HRN returned by one
//!   endpoint is the identifier accepted by the others.
//! - G7 Creating the same policy twice is a conflict.
//! - G8 Creating a policy with empty content is a bad request.
//!
//! Each test bootstraps its own application on a throwaway RocksDB directory,
//! so tests never share mutable state.

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

/// A valid Cedar policy used across the contract.
const VALID_POLICY: &str = "permit(principal, action, resource);";

/// A syntactically broken Cedar policy (unterminated `permit`).
const INVALID_POLICY: &str = "permit(principal, action, resource";

/// Bootstraps the application on a throwaway RocksDB directory.
///
/// The returned `TempDir` must outlive the router: dropping it first would
/// remove the database directory while the embedded engine is still open.
async fn test_app() -> (Router, TempDir) {
    let tmp = tempfile::tempdir().expect("temp dir");

    let mut config = AppConfig::default();
    config.rocksdb.path = tmp
        .path()
        .join("hodei.rocksdb")
        .to_string_lossy()
        .into_owned();

    // The IAM schema is not registered on purpose: this contract asserts the
    // HTTP surface, and schema registration is covered by its own test.
    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .expect("bootstrap must succeed for the HTTP contract");

    let app = build_router(state, &config);
    (app, tmp)
}

/// Sends one request to the router and returns the status and the JSON body.
async fn request(
    app: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let builder = Request::builder().method(method).uri(uri);

    let request = match body {
        Some(value) => builder
            .header("content-type", "application/json")
            .body(Body::from(value.to_string()))
            .expect("request"),
        None => builder.body(Body::empty()).expect("request"),
    };

    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("the router must always produce a response");

    let status = response.status();
    let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("body");

    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };

    (status, json)
}

// ---------------------------------------------------------------------------
// G1 / G2 / G3 — health probes and routing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn g1_health_reports_healthy_and_identifies_the_service() {
    let (app, _tmp) = test_app().await;

    let (status, body) = request(&app, Method::GET, "/health", None).await;

    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["status"], json!("healthy"));
    assert_eq!(body["service"], json!("hodei-artifacts-api"));
    assert!(
        body["version"].as_str().is_some_and(|v| !v.is_empty()),
        "the health payload must carry a version: {body}"
    );
}

#[tokio::test]
async fn g2_readiness_and_liveness_probes_are_served() {
    let (app, _tmp) = test_app().await;

    for probe in ["/health/ready", "/health/live"] {
        let (status, body) = request(&app, Method::GET, probe, None).await;

        assert_eq!(status, StatusCode::OK, "{probe} body: {body}");
        assert!(
            body["status"].as_str().is_some_and(|s| !s.is_empty()),
            "{probe} must report a status: {body}"
        );
    }
}

#[tokio::test]
async fn g3_unknown_route_is_not_found() {
    let (app, _tmp) = test_app().await;

    let (status, _body) =
        request(&app, Method::GET, "/api/v1/this-route-does-not-exist", None).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// G4 / G5 — policy validation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn g4_valid_cedar_policy_is_accepted() {
    let (app, _tmp) = test_app().await;

    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/policies/validate",
        Some(json!({ "content": VALID_POLICY })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["is_valid"], json!(true), "body: {body}");
    assert_eq!(body["errors"], json!([]), "body: {body}");
}

#[tokio::test]
async fn g5_invalid_cedar_policy_is_rejected_with_errors() {
    let (app, _tmp) = test_app().await;

    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/policies/validate",
        Some(json!({ "content": INVALID_POLICY })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["is_valid"], json!(false), "body: {body}");
    assert!(
        body["errors"].as_array().is_some_and(|e| !e.is_empty()),
        "a rejected policy must report why: {body}"
    );
}

// ---------------------------------------------------------------------------
// G6 — IAM policy lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn g6_iam_policy_lifecycle_is_consistent_end_to_end() {
    let (app, _tmp) = test_app().await;

    let policy_id = "contract-lifecycle-policy";

    // create
    let (status, created) = request(
        &app,
        Method::POST,
        "/api/v1/iam/policies",
        Some(json!({
            "policy_id": policy_id,
            "policy_content": VALID_POLICY,
            "description": "contract policy",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create body: {created}");

    let hrn = created["hrn"].as_str().expect("create must return an HRN");
    assert_eq!(
        hrn,
        format!("hrn:hodei:iam::default:Policy/{policy_id}"),
        "the canonical HRN must be usable as an identifier by the other endpoints"
    );

    // get
    let (status, fetched) = request(
        &app,
        Method::POST,
        "/api/v1/iam/policies/get",
        Some(json!({ "policy_hrn": hrn })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "get body: {fetched}");
    assert_eq!(fetched["hrn"], json!(hrn));
    assert_eq!(fetched["name"], json!(policy_id));
    assert_eq!(fetched["content"], json!(VALID_POLICY));

    // list — the listed HRN must be the same identifier
    let (status, listed) = request(
        &app,
        Method::GET,
        "/api/v1/iam/policies?limit=50&offset=0",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "list body: {listed}");
    let listed_hrns: Vec<&str> = listed["policies"]
        .as_array()
        .expect("policies must be an array")
        .iter()
        .filter_map(|p| p["hrn"].as_str())
        .collect();
    assert!(
        listed_hrns.contains(&hrn),
        "list must expose the policy under the same HRN, got {listed_hrns:?}"
    );

    // update
    let updated_content = "permit(principal, action == \"Action::read\", resource);";
    let (status, updated) = request(
        &app,
        Method::PUT,
        "/api/v1/iam/policies/update",
        Some(json!({
            "policy_hrn": hrn,
            "policy_content": updated_content,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "update body: {updated}");
    assert_eq!(updated["hrn"], json!(hrn));
    assert_eq!(updated["content"], json!(updated_content));

    // get again — the update must be visible
    let (status, refetched) = request(
        &app,
        Method::POST,
        "/api/v1/iam/policies/get",
        Some(json!({ "policy_hrn": hrn })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "get body: {refetched}");
    assert_eq!(refetched["content"], json!(updated_content));

    // delete
    let (status, deleted) = request(
        &app,
        Method::DELETE,
        "/api/v1/iam/policies/delete",
        Some(json!({ "policy_hrn": hrn })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "delete body: {deleted}");
    assert_eq!(deleted["deleted_hrn"], json!(hrn));

    // get after delete — the policy must be gone
    let (status, _body) = request(
        &app,
        Method::POST,
        "/api/v1/iam/policies/get",
        Some(json!({ "policy_hrn": hrn })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// G7 / G8 — rejected writes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn g7_creating_the_same_policy_twice_is_a_conflict() {
    let (app, _tmp) = test_app().await;

    let payload = json!({
        "policy_id": "contract-duplicate-policy",
        "policy_content": VALID_POLICY,
    });

    let (status, _body) = request(
        &app,
        Method::POST,
        "/api/v1/iam/policies",
        Some(payload.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the first create must succeed");

    let (status, body) = request(&app, Method::POST, "/api/v1/iam/policies", Some(payload)).await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a duplicate policy must be a conflict: {body}"
    );
}

#[tokio::test]
async fn g8_creating_a_policy_without_content_is_a_bad_request() {
    let (app, _tmp) = test_app().await;

    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/iam/policies",
        Some(json!({
            "policy_id": "contract-empty-policy",
            "policy_content": "",
        })),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "an empty policy must be rejected: {body}"
    );
}
