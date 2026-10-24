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
//! - G18 The decision follows the schema that was given: a verb the schema
//!   does not declare is denied, and the reason names it.
//! - G19 The schema decides the namespace of the action: the same policy
//!   allows against a namespaced schema and does not against a flat one.
//! - G20 The context of the request reaches the evaluation: a policy
//!   conditioned on it holds or does not, accordingly.
//! - G21 A request that does not conform to the schema is denied, and the
//!   reason names what does not conform.
//! - G22 A schema that declares nothing constrains nothing (the counterpart
//!   of G21 that keeps the schema from rejecting what it does not know).
//! - G23 An action the schema declares twice is denied, not resolved by
//!   guessing one of the candidates.
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

/// A schema envelope that declares the entity type `Iam::User` **and** the
/// action `read`, for `Iam::User` principals and resources.
///
/// It is the world the request of the playground lives in: a schema that
/// declares a type but no action says that the verb asked for does not
/// exist.
const INLINE_SCHEMA_DECLARING_READ: &str = r#"{"format":"cedar-schema-fragments-v1","fragments":[{"":{"entityTypes":{},"actions":{"read":{"appliesTo":{"resourceTypes":["Iam::User"],"principalTypes":["Iam::User"]}}}},"Iam":{"entityTypes":{"User":{"shape":{"type":"Record","attributes":{"name":{"type":"EntityOrCommon","name":"String"}}}}},"actions":{}}}]}"#;

/// A schema envelope that declares the action `read` in the namespace
/// `Hodei`.
///
/// The namespace is what the schema declares; the request only names the
/// verb.
const INLINE_SCHEMA_DECLARING_NAMESPACED_READ: &str = r#"{"format":"cedar-schema-fragments-v1","fragments":[{"Hodei":{"entityTypes":{},"actions":{"read":{"appliesTo":{"resourceTypes":["Iam::User"],"principalTypes":["Iam::User"]}}}},"Iam":{"entityTypes":{"User":{"shape":{"type":"Record","attributes":{"name":{"type":"EntityOrCommon","name":"String"}}}}},"actions":{}}}]}"#;

/// A schema envelope that declares the action `read` with a context type
/// `Ctx` that holds the attribute `ip`.
const INLINE_SCHEMA_DECLARING_READ_WITH_CONTEXT: &str = r#"{"format":"cedar-schema-fragments-v1","fragments":[{"":{"commonTypes":{"Ctx":{"type":"Record","attributes":{"ip":{"type":"EntityOrCommon","name":"String"}}}},"entityTypes":{},"actions":{"read":{"appliesTo":{"resourceTypes":["Iam::User"],"principalTypes":["Iam::User"],"context":{"type":"Ctx"}}}}},"Iam":{"entityTypes":{"User":{"shape":{"type":"Record","attributes":{"name":{"type":"EntityOrCommon","name":"String"}}}}},"actions":{}}}]}"#;

/// A schema envelope that declares the action `read` twice, in the
/// namespaces `Hodei` and `Other`.
///
/// Cedar allows it: two namespaced actions may share an id. The request
/// names the verb, not the namespace, so the schema cannot say which one is
/// meant.
const INLINE_SCHEMA_DECLARING_AMBIGUOUS_READ: &str = r#"{"format":"cedar-schema-fragments-v1","fragments":[{"Hodei":{"entityTypes":{},"actions":{"read":{"appliesTo":{"resourceTypes":["Iam::User"],"principalTypes":["Iam::User"]}}}},"Iam":{"entityTypes":{"User":{"shape":{"type":"Record","attributes":{"name":{"type":"EntityOrCommon","name":"String"}}}}},"actions":{}},"Other":{"entityTypes":{},"actions":{"read":{"appliesTo":{"resourceTypes":["Iam::User"],"principalTypes":["Iam::User"]}}}}}]}"#;

/// A policy over the entity type the inline schema declares.
const POLICY_OVER_DECLARED_TYPE: &str =
    r#"permit(principal == Iam::User::"hrn:aws:iam::account-a:User/alice", action, resource);"#;

/// A policy over an entity type no schema declares.
const POLICY_OVER_UNDECLARED_TYPE: &str =
    r#"permit(principal == Nope::Ghost::"alice", action, resource);"#;

/// A policy that names the action of the `Hodei` namespace.
const POLICY_OVER_NAMESPACED_ACTION: &str =
    r#"permit(principal, action == Hodei::Action::"read", resource);"#;

/// A policy conditioned on the context attribute `ip`.
const POLICY_OVER_CONTEXT_IP: &str =
    r#"permit(principal, action, resource) when { context.ip == "10.0.0.1" };"#;

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
///
/// The verb, the resource and the context of the request are parameters, so
/// the same request can be presented to different schemas.
fn playground_request_body(
    inline_schema: &str,
    policies: &[&str],
    action: &str,
    resource: &str,
    context: Value,
) -> Value {
    json!({
        "inline_schema": inline_schema,
        "inline_policies": policies,
        "request": {
            "principal": "hrn:aws:iam::account-a:User/alice",
            "action": action,
            "resource": resource,
            "context": context,
        }
    })
}

/// Builds the body of a playground evaluation request with the default
/// request: the verb `read`, a resource of the type `Iam::User` and no
/// context.
fn playground_body(inline_schema: &str, policies: &[&str]) -> Value {
    playground_request_body(
        inline_schema,
        policies,
        "hrn:aws:iam::account-a:Action/read",
        "hrn:aws:iam::account-a:User/bob",
        json!({}),
    )
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

/// The errors the playground reported for the evaluation itself.
fn evaluation_errors(body: &Value) -> Vec<String> {
    body["errors"]
        .as_array()
        .expect("errors must be an array")
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
            INLINE_SCHEMA_DECLARING_READ,
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
            INLINE_SCHEMA_DECLARING_READ,
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

// ---------------------------------------------------------------------------
// G18 / G19 / G20 / G21 / G22 / G23 — the schema governs the decision
// ---------------------------------------------------------------------------

/// The decision follows the schema that was given
///
/// The request is the same in the three cases; only the schema changes.
/// The schema that declares the verb allows it, the schema that declares
/// another verb does not, and the schema that declares no verb at all does
/// not either: a verb that the world does not declare does not exist.
#[tokio::test]
async fn g18_the_decision_follows_the_schema_that_was_given() {
    let (app, _tmp) = test_app().await;

    // The schema declares the verb the request asks for: allowed.
    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/playground/evaluate",
        Some(playground_body(
            INLINE_SCHEMA_DECLARING_READ,
            &[VALID_POLICY],
        )),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "playground body: {body}");
    assert_eq!(body["decision"], json!("ALLOW"), "body: {body}");

    // The same schema, but the request asks for a verb it does not declare.
    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/playground/evaluate",
        Some(playground_request_body(
            INLINE_SCHEMA_DECLARING_READ,
            &[VALID_POLICY],
            "hrn:aws:iam::account-a:Action/write",
            "hrn:aws:iam::account-a:User/bob",
            json!({}),
        )),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "playground body: {body}");
    assert_eq!(
        body["decision"],
        json!("DENY"),
        "a verb the schema does not declare must not be authorized: {body}"
    );
    let errors = evaluation_errors(&body);
    assert!(
        errors.iter().any(|e| e.contains("write")),
        "the reason must name the verb that was refused: {errors:?}"
    );

    // A schema that declares the type but no verb at all: the verb the
    // request asks for does not exist in that world either.
    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/playground/evaluate",
        Some(playground_body(
            INLINE_SCHEMA_DECLARING_USER,
            &[VALID_POLICY],
        )),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "playground body: {body}");
    assert_eq!(
        body["decision"],
        json!("DENY"),
        "a schema that declares no verb must not authorize one: {body}"
    );
    assert!(
        evaluation_errors(&body).iter().any(|e| e.contains("read")),
        "the reason must name the verb that was refused: {body}"
    );
}

/// The schema decides the namespace of the action
///
/// The policy names `Hodei::Action::"read"`. The request names the verb
/// `read` and nothing else: the namespace is what the schema declares. The
/// same policy against a schema that declares the flat action is a policy
/// about another action, and it does not grant.
#[tokio::test]
async fn g19_the_schema_decides_the_namespace_of_the_action() {
    let (app, _tmp) = test_app().await;

    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/playground/evaluate",
        Some(playground_body(
            INLINE_SCHEMA_DECLARING_NAMESPACED_READ,
            &[POLICY_OVER_NAMESPACED_ACTION],
        )),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "playground body: {body}");
    assert_eq!(
        body["decision"],
        json!("ALLOW"),
        "the schema resolves the verb to the namespaced action: {body}"
    );

    // The same policy, against a schema that declares the flat action: the
    // verb denotes `Action::"read"`, which the policy does not mention.
    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/playground/evaluate",
        Some(playground_body(
            INLINE_SCHEMA_DECLARING_READ,
            &[POLICY_OVER_NAMESPACED_ACTION],
        )),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "playground body: {body}");
    assert_eq!(
        body["decision"],
        json!("DENY"),
        "the namespace of the action comes from the schema, not from the policy: {body}"
    );
}

/// The context of the request reaches the evaluation
///
/// The policy is conditioned on the context attribute `ip`. The context is
/// part of the request, so the schema that declares it and the policy that
/// reads it must both see it.
#[tokio::test]
async fn g20_the_context_of_the_request_reaches_the_evaluation() {
    let (app, _tmp) = test_app().await;

    let body_with_ip = playground_request_body(
        INLINE_SCHEMA_DECLARING_READ_WITH_CONTEXT,
        &[POLICY_OVER_CONTEXT_IP],
        "hrn:aws:iam::account-a:Action/read",
        "hrn:aws:iam::account-a:User/bob",
        json!({"ip": {"type": "String", "value": "10.0.0.1"}}),
    );

    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/playground/evaluate",
        Some(body_with_ip),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "playground body: {body}");
    assert_eq!(
        body["decision"],
        json!("ALLOW"),
        "a policy conditioned on the context must see it: {body}"
    );

    // The same request with another address: the policy does not hold.
    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/playground/evaluate",
        Some(playground_request_body(
            INLINE_SCHEMA_DECLARING_READ_WITH_CONTEXT,
            &[POLICY_OVER_CONTEXT_IP],
            "hrn:aws:iam::account-a:Action/read",
            "hrn:aws:iam::account-a:User/bob",
            json!({"ip": {"type": "String", "value": "10.0.0.2"}}),
        )),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "playground body: {body}");
    assert_eq!(
        body["decision"],
        json!("DENY"),
        "a policy conditioned on the context must not hold for another value: {body}"
    );
}

/// A request that does not conform to the schema is denied
///
/// The schema declares the type `Iam::User`; the request names a resource of
/// the type `Storage::Document`, which the schema does not declare. The
/// request is refused, and the reason is reported: the schema is the world
/// the request has to live in.
#[tokio::test]
async fn g21_a_request_that_does_not_conform_to_the_schema_is_denied() {
    let (app, _tmp) = test_app().await;

    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/playground/evaluate",
        Some(playground_request_body(
            INLINE_SCHEMA_DECLARING_READ,
            &[VALID_POLICY],
            "hrn:aws:iam::account-a:Action/read",
            "hrn:aws:storage::account-a:Document/doc1",
            json!({}),
        )),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "playground body: {body}");
    assert_eq!(
        body["decision"],
        json!("DENY"),
        "a request outside the world of the schema must not be authorized: {body}"
    );
    let errors = evaluation_errors(&body);
    assert!(
        !errors.is_empty(),
        "the reason of the denial must be reported: {body}"
    );
    assert!(
        errors.iter().any(|e| e.contains("Storage::Document")),
        "the reason must name what does not conform: {errors:?}"
    );
}

/// A schema that declares nothing constrains nothing
///
/// This is the counterpart of G21: the guard that keeps the schema from
/// rejecting requests it knows nothing about. An empty schema must not turn
/// a valid request into a denial.
#[tokio::test]
async fn g22_a_schema_that_declares_nothing_constrains_nothing() {
    let (app, _tmp) = test_app().await;

    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/playground/evaluate",
        Some(playground_body("{}", &[VALID_POLICY])),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "playground body: {body}");
    assert_eq!(
        body["decision"],
        json!("ALLOW"),
        "an empty schema must not reject a request it knows nothing about: {body}"
    );
    assert!(
        evaluation_errors(&body).is_empty(),
        "an empty schema must not report a reason: {body}"
    );
}

/// An action that the schema declares twice is not resolved by guessing
///
/// Cedar allows two namespaced actions to share an id. The request names the
/// verb and nothing else, so the schema cannot say which of them is meant.
/// Picking one of them would evaluate the request against an action the
/// caller did not ask for.
#[tokio::test]
async fn g23_an_action_the_schema_declares_twice_is_denied() {
    let (app, _tmp) = test_app().await;

    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/playground/evaluate",
        Some(playground_body(
            INLINE_SCHEMA_DECLARING_AMBIGUOUS_READ,
            &[VALID_POLICY],
        )),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "playground body: {body}");
    assert_eq!(
        body["decision"],
        json!("DENY"),
        "an ambiguous action must not be resolved by guessing: {body}"
    );
    let errors = evaluation_errors(&body);
    assert!(
        errors.iter().any(|e| e.contains("read")),
        "the reason must name the verb that was refused: {errors:?}"
    );
    assert!(
        errors.iter().any(|e| e.contains("Hodei")) && errors.iter().any(|e| e.contains("Other")),
        "the reason must name both candidates: {errors:?}"
    );
}
