//! HSP-314 P2: the Cedar schema of the graph profile is stored, enforced and
//! durable.
//!
//! Before the schema store existed over Dgraph, the graph profile persisted no
//! schema: the version came back `null`, a policy that violated the IAM schema
//! was accepted, and the schema was gone the moment the service stopped. This
//! suite pins the three observable guarantees against a REAL Dgraph namespace:
//!
//! - S1 After the IAM schema is registered the active schema is reported with
//!   its version (not `null`).
//! - S2 A policy over a type or an action the schema does not declare is
//!   rejected with a `400`, and a policy over declared types is accepted (the
//!   discriminating counterpart: the rejection comes from the schema).
//! - S3 A restart keeps the schema: a second application on the same
//!   namespace, which registers nothing, still reports the schema and still
//!   rejects the same policy. The restart is a second `bootstrap` over the
//!   same namespace after the first application is dropped (the service
//!   binary does exactly that on boot, and nothing in the process survives
//!   between the two).
//! - S4 Another namespace (another deployment) does not see the schema.

use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use cedar_policy::Entities;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use serde_json::{Value, json};
use tower::ServiceExt;

// The graph profile of the harness: a REAL Dgraph namespace per application.
#[path = "support/graph_profile.rs"]
mod graph_profile;
use graph_profile::Workdir;

const VERSION: &str = "p2-v1";

/// A policy over an entity type no schema declares.
const POLICY_OUTSIDE_SCHEMA: &str =
    r#"permit(principal == Nope::Ghost::"alice", action == Action::"CreateUser", resource);"#;

/// A policy that only uses what the IAM schema declares.
const POLICY_INSIDE_SCHEMA: &str =
    r#"permit(principal == Iam::User::"alice", action == Action::"CreateUser", resource);"#;

struct Alice;

#[async_trait]
impl PrincipalAuthenticator for Alice {
    async fn authenticate(
        &self,
        _credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        Ok(PrincipalClaims::new(
            PrincipalId::new("hrn:hodei:iam::acme:User/alice").unwrap(),
            AccountId::new("acme").unwrap(),
        ))
    }
}

fn admin_authorizer() -> Arc<dyn AuthorizerPort> {
    let policy = r#"permit(principal, action == Admin::Action::"AccessAdminApi", resource);"#;
    Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .unwrap(),
    )))
}

/// One application over the namespace of `work`, registering the IAM schema
/// only when asked to.
async fn application(work: &Workdir, register: bool) -> Router {
    let mut config = AppConfig::default();
    config.security.admin_api_enabled = true;
    work.apply(&mut config);
    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: register,
            schema_version: register.then(|| VERSION.to_string()),
            validate_schemas: true,
        },
    )
    .await
    .expect("the graph profile boots")
    .with_authenticator(Arc::new(Alice))
    .with_authorization(admin_authorizer());
    build_router(state, &config)
}

async fn call(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", "Bearer alice");
    let response = app
        .clone()
        .oneshot(match body {
            Some(body) => builder
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
            None => builder.body(Body::empty()).unwrap(),
        })
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn create_policy(app: &Router, id: &str, content: &str) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        "/api/v1/iam/policies",
        Some(json!({ "policy_id": id, "policy_content": content })),
    )
    .await
}

#[tokio::test]
async fn the_registered_schema_is_reported_enforced_and_survives_a_restart() {
    let work = Workdir::new();

    // The first boot registers the schema.
    let first = application(&work, true).await;

    // S1: the active schema carries its version.
    let (status, loaded) = call(&first, "GET", "/api/v1/schemas/load", None).await;
    assert_eq!(status, StatusCode::OK, "load: {loaded}");
    assert_eq!(loaded["status"], json!("active"), "load: {loaded}");
    assert_eq!(loaded["version"], json!(VERSION), "load: {loaded}");

    // S2: the schema decides. The same request is accepted inside it and
    // rejected outside it.
    let (status, body) = create_policy(&first, "inside", POLICY_INSIDE_SCHEMA).await;
    assert_eq!(status, StatusCode::OK, "a policy inside the schema: {body}");
    let (status, body) = create_policy(&first, "outside", POLICY_OUTSIDE_SCHEMA).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a policy outside the schema must be rejected: {body}"
    );

    // S3: the restart. Nothing of the first application survives but the
    // namespace; the second one registers nothing.
    drop(first);
    let second = application(&work, false).await;
    let (status, loaded) = call(&second, "GET", "/api/v1/schemas/load", None).await;
    assert_eq!(status, StatusCode::OK, "load after restart: {loaded}");
    assert_eq!(
        loaded["status"],
        json!("active"),
        "the schema must survive the restart: {loaded}"
    );
    assert_eq!(loaded["version"], json!(VERSION), "load: {loaded}");
    let (status, body) = create_policy(&second, "outside-again", POLICY_OUTSIDE_SCHEMA).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "the restarted service must still reject a policy outside the schema: {body}"
    );
    let (status, body) = create_policy(&second, "inside-again", POLICY_INSIDE_SCHEMA).await;
    assert_eq!(status, StatusCode::OK, "inside the schema: {body}");
}

#[tokio::test]
async fn another_deployment_does_not_see_the_schema() {
    let registered = Workdir::new();
    let _first = application(&registered, true).await;

    // A different namespace never registered anything.
    let other = Workdir::new();
    let app = application(&other, false).await;
    let (status, loaded) = call(&app, "GET", "/api/v1/schemas/load", None).await;
    assert_eq!(status, StatusCode::OK, "load: {loaded}");
    assert_eq!(
        loaded["status"],
        json!("no_active_schema"),
        "a schema registered in one deployment must not be active in another: {loaded}"
    );
}
