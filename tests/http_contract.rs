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
//! - G9 `POST /api/v1/policies/evaluate` decides for real: a `permit` allows
//!   and names the policy that determined the decision.
//! - G10 An explicit `forbid` prevails over the `permit` (the discriminating
//!   negative of G9) and the default without policies is deny.
//! - G11 A malformed HRN is a bad request, not a silent decision (the
//!   discriminating negative of G9's input handling).
//! - G12 A policy scoped to one account does not reach a homonym of another
//!   account: the tenant boundary is structural.
//! - G13 The playground validates the inline policies against the inline
//!   schema it was given: a policy over a declared type is accepted.
//! - G14 (negative of G13) A policy over an undeclared type is rejected by
//!   that schema — and the same policy is not rejected when no schema is
//!   declared, so the rejection comes from the schema and not from the
//!   validator rejecting everything.
//! - G15 An inline schema that is not a readable schema envelope is a bad
//!   request, not an empty schema.
//! - G16 `GET /api/v1/schemas/load` says explicitly that there is no active
//!   schema when nothing is stored.
//! - G17 `GET /api/v1/schemas/load` returns the active schema — its version
//!   and its content — once one has been registered.
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

/// A schema envelope that declares the entity type `Iam::User`.
///
/// It is the `cedar-schema-fragments-v1` format, the very content the
/// persistence path stores, so the inline and the stored read paths speak
/// the same language.
const INLINE_SCHEMA_DECLARING_USER: &str = r#"{"format":"cedar-schema-fragments-v1","fragments":[{"":{"entityTypes":{},"actions":{}},"Iam":{"entityTypes":{"User":{"shape":{"type":"Record","attributes":{}}}},"actions":{}}}]}"#;

/// A policy over the entity type the inline schema declares.
const POLICY_OVER_DECLARED_TYPE: &str =
    r#"permit(principal == Iam::User::"hrn:aws:iam::account-a:User/alice", action, resource);"#;

/// A policy over an entity type no schema declares.
const POLICY_OVER_UNDECLARED_TYPE: &str =
    r#"permit(principal == Nope::Ghost::"alice", action, resource);"#;

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

// ---------------------------------------------------------------------------
// G9 / G10 / G11 / G12 — real policy evaluation
// ---------------------------------------------------------------------------

/// Builds the body of an evaluation request.
///
/// The mode is explicit (`NoSchema`) so the contract does not depend on
/// whether a schema happens to be stored: H1a evaluates schema-less.
fn evaluate_body(principal_hrn: &str, resource_hrn: &str, policies: &[&str]) -> Value {
    json!({
        "principal_hrn": principal_hrn,
        "action": "Read",
        "resource_hrn": resource_hrn,
        "policies": policies,
        "context": {},
        "evaluation_mode": "NoSchema",
    })
}

#[tokio::test]
async fn g9_a_permit_allows_and_names_the_determining_policy() {
    let (app, _tmp) = test_app().await;

    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/policies/evaluate",
        Some(evaluate_body(
            "hrn:aws:iam::account-a:User/alice",
            "hrn:aws:iam::account-a:User/bob",
            &[VALID_POLICY],
        )),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "evaluate body: {body}");
    assert_eq!(body["decision"], json!("Allow"));
    assert_eq!(body["determining_policies"], json!(["policy_0"]));
    assert_eq!(body["policy_ids_evaluated"], json!(["policy_0"]));
}

#[tokio::test]
async fn g10_an_explicit_forbid_prevails_over_the_permit() {
    let (app, _tmp) = test_app().await;

    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/policies/evaluate",
        Some(evaluate_body(
            "hrn:aws:iam::account-a:User/alice",
            "hrn:aws:iam::account-a:User/bob",
            &[VALID_POLICY, "forbid(principal, action, resource);"],
        )),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "evaluate body: {body}");
    assert_eq!(body["decision"], json!("Deny"));
    assert_eq!(
        body["determining_policies"],
        json!(["policy_1"]),
        "the forbid must be the policy that determined the decision"
    );

    // Without any policy the default is deny as well.
    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/policies/evaluate",
        Some(evaluate_body(
            "hrn:aws:iam::account-a:User/alice",
            "hrn:aws:iam::account-a:User/bob",
            &[],
        )),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "evaluate body: {body}");
    assert_eq!(body["decision"], json!("Deny"));
}

#[tokio::test]
async fn g11_a_malformed_hrn_is_a_bad_request() {
    let (app, _tmp) = test_app().await;

    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/policies/evaluate",
        Some(evaluate_body(
            "not-an-hrn",
            "hrn:aws:iam::account-a:User/bob",
            &[VALID_POLICY],
        )),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a malformed HRN must be rejected: {body}"
    );

    // The same holds for the resource side of the request.
    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/policies/evaluate",
        Some(evaluate_body(
            "hrn:aws:iam::account-a:User/alice",
            "hrn:aws:iam::account-a:User/",
            &[VALID_POLICY],
        )),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a malformed resource HRN must be rejected: {body}"
    );
}

/// El límite de tenant es estructural: una política que nombra al principal de
/// una cuenta no alcanza al homónimo de otra cuenta.
#[tokio::test]
async fn g12_a_policy_scoped_to_one_account_does_not_reach_another_account() {
    let (app, _tmp) = test_app().await;

    let policy = "permit(principal, action, resource) when { principal == Iam::User::\"hrn:aws:iam::account-a:User/alice\" };";

    // The principal of account A is the one the policy names: allowed.
    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/policies/evaluate",
        Some(evaluate_body(
            "hrn:aws:iam::account-a:User/alice",
            "hrn:aws:iam::account-a:User/bob",
            &[policy],
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "evaluate body: {body}");
    assert_eq!(body["decision"], json!("Allow"));

    // The homonym of account B is a different entity, so the same policy
    // does not grant it. The resource is the principal of account A, so the
    // entity the policy mentions exists in the request and the denial comes
    // from the policy, not from an evaluation error.
    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/policies/evaluate",
        Some(evaluate_body(
            "hrn:aws:iam::account-b:User/alice",
            "hrn:aws:iam::account-a:User/alice",
            &[policy],
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "evaluate body: {body}");
    assert_eq!(
        body["decision"],
        json!("Deny"),
        "a policy scoped to account-a must not grant account-b"
    );
    assert!(
        !body["diagnostics"]
            .as_array()
            .expect("diagnostics must be an array")
            .iter()
            .any(|d| d["level"] == json!("error")),
        "the denial must come from the policy, not from an evaluation error: {body}"
    );
}

// ---------------------------------------------------------------------------
// G13 / G14 / G15 — the playground validates against the schema it was given
// ---------------------------------------------------------------------------

/// Builds the body of a playground evaluation request.
fn playground_body(inline_schema: &str, policies: &[&str]) -> Value {
    json!({
        "inline_schema": inline_schema,
        "inline_policies": policies,
        "request": {
            "principal": "hrn:aws:iam::account-a:User/alice",
            "action": "hrn:aws:iam::account-a:Action/read",
            "resource": "hrn:aws:iam::account-a:User/bob",
            "context": {},
        }
    })
}

/// The validation errors the playground reported, if any.
fn validation_errors(body: &Value) -> Vec<String> {
    body["diagnostics"]["validation_errors"]
        .as_array()
        .expect("validation_errors must be an array")
        .iter()
        .filter_map(|e| e.as_str().map(str::to_string))
        .collect()
}

#[tokio::test]
async fn g13_the_playground_validates_against_the_inline_schema() {
    let (app, _tmp) = test_app().await;

    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/playground/evaluate",
        Some(playground_body(
            INLINE_SCHEMA_DECLARING_USER,
            &[POLICY_OVER_DECLARED_TYPE],
        )),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "playground body: {body}");
    assert!(
        validation_errors(&body).is_empty(),
        "a policy over the declared entity type must not be reported as invalid: {body}"
    );
    assert_eq!(body["decision"], json!("ALLOW"), "body: {body}");
}

#[tokio::test]
async fn g14_the_playground_rejects_a_policy_over_an_undeclared_type() {
    let (app, _tmp) = test_app().await;

    // With the schema that declares `Iam::User`, a policy over `Nope::Ghost`
    // is invalid against it.
    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/playground/evaluate",
        Some(playground_body(
            INLINE_SCHEMA_DECLARING_USER,
            &[POLICY_OVER_UNDECLARED_TYPE],
        )),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "playground body: {body}");
    let errors = validation_errors(&body);
    assert!(
        !errors.is_empty(),
        "a policy over an undeclared entity type must be reported: {body}"
    );
    assert!(
        errors.iter().any(|e| e.contains("Nope::Ghost")),
        "the report must name the undeclared type: {errors:?}"
    );

    // The same policy, with no schema declared, is not reported: the
    // rejection above comes from the schema that was supplied, not from the
    // validator rejecting everything.
    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/playground/evaluate",
        Some(playground_body("{}", &[POLICY_OVER_UNDECLARED_TYPE])),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "playground body: {body}");
    assert!(
        validation_errors(&body).is_empty(),
        "with no schema declared only the syntax can be checked: {body}"
    );
}

#[tokio::test]
async fn g15_an_inline_schema_that_is_not_an_envelope_is_a_bad_request() {
    let (app, _tmp) = test_app().await;

    // A Cedar schema in the human-readable format: it looks like a schema,
    // but it is not the envelope the read paths share.
    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/playground/evaluate",
        Some(playground_body(
            r#"{"entityTypes": {"User": {"shape": {"type": "Record", "attributes": {}}}}}"#,
            &[VALID_POLICY],
        )),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "an inline schema that cannot be read must be rejected: {body}"
    );
}

// ---------------------------------------------------------------------------
// G16 / G17 — the schema load endpoint reports the active schema
// ---------------------------------------------------------------------------

#[tokio::test]
async fn g16_loading_the_schema_says_when_there_is_none() {
    let (app, _tmp) = test_app().await;

    let (status, body) = request(&app, Method::GET, "/api/v1/schemas/load", None).await;

    assert_eq!(status, StatusCode::OK, "load body: {body}");
    assert!(
        matches!(
            body["status"].as_str(),
            Some("active") | Some("no_active_schema")
        ),
        "the endpoint must answer with a declared status, not with a placeholder: {body}"
    );
    assert_eq!(
        body["status"],
        json!("no_active_schema"),
        "the endpoint must say that there is no schema, not answer with a placeholder: {body}"
    );
    assert_eq!(body["version"], json!(null), "body: {body}");
    assert_eq!(body["content"], json!(null), "body: {body}");
}

#[tokio::test]
async fn g17_loading_the_schema_returns_the_active_schema() {
    let (app, _tmp) = test_app().await;

    let (status, registered) = request(
        &app,
        Method::POST,
        "/api/v1/schemas/register-iam",
        Some(json!({ "version": "v1.0.0", "validate": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "register body: {registered}");

    let (status, body) = request(&app, Method::GET, "/api/v1/schemas/load", None).await;

    assert_eq!(status, StatusCode::OK, "load body: {body}");
    assert_eq!(body["status"], json!("active"), "body: {body}");
    assert_eq!(body["version"], json!("v1.0.0"), "body: {body}");

    let content = body["content"]
        .as_str()
        .expect("the active schema must come with its content");

    let envelope: Value = serde_json::from_str(content).expect("the content must be a schema");
    assert_eq!(
        envelope["format"],
        json!("cedar-schema-fragments-v1"),
        "the content must be the envelope the read paths share: {content}"
    );
    assert!(
        envelope["fragments"]
            .as_array()
            .is_some_and(|f| !f.is_empty()),
        "the active schema must declare something: {content}"
    );
    assert!(
        body["entity_type_count"].as_u64().is_some_and(|c| c > 0),
        "the active schema must declare its entity types: {body}"
    );
}
