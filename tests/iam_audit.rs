//! The changes to the authorization configuration leave a trail.
//!
//! Creating, updating and deleting an IAM policy is a mutation of the
//! authorization configuration itself. Those operations were not audited:
//! the audit seam existed and was used by the repository routes, and the
//! policy routes -- the ones that decide who may do what -- left no trace at
//! all, neither of the successes nor of the refusals.
//!
//! The event identifies the principal, the operation, the policy and how it
//! ended. It never carries the token or the content of the policy.

use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use cedar_policy::Entities;
use hodei_artifacts_api::audit::InMemoryAuditSink;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use kernel::application::ports::clock::{CountingIdGenerator, FixedClock, Identity};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

const SECRET: &str = "a-token-that-must-never-be-audited";

struct Accounts;

#[async_trait]
impl PrincipalAuthenticator for Accounts {
    async fn authenticate(
        &self,
        credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        let token = match credentials {
            Credentials::Bearer(token) => token.as_str(),
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        let (account, name) = match token {
            "alice" => ("acme", "alice"),
            "bob" => ("other", "bob"),
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        Ok(PrincipalClaims::new(
            PrincipalId::new(format!("hrn:hodei:iam::{account}:User/{name}")).unwrap(),
            AccountId::new(account).unwrap(),
        ))
    }
}

async fn app() -> (axum::Router, TempDir, Arc<InMemoryAuditSink>) {
    app_with_identity(Identity::system()).await
}

/// The same application, with the clock and the identifier generator pinned.
async fn app_with_identity(identity: Identity) -> (axum::Router, TempDir, Arc<InMemoryAuditSink>) {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = AppConfig::default();
    config.security.admin_api_enabled = true;
    config.rocksdb.path = tmp.path().join("db").to_string_lossy().into_owned();
    let policy = r#"permit(principal, action == Admin::Action::"AccessAdminApi", resource);"#;
    let authorizer: Arc<dyn AuthorizerPort> = Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .unwrap(),
    )));
    let sink = Arc::new(InMemoryAuditSink::new());
    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .unwrap()
    .with_authenticator(Arc::new(Accounts))
    .with_authorization(authorizer)
    .with_audit_sink(sink.clone())
    .with_identity(identity);
    (build_router(state, &config), tmp, sink)
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("x-request-id", "correlation-1");
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

/// The mutations of the authorization configuration leave a trail, and the
/// trail says who did what, to which policy, and how it ended.
#[tokio::test]
async fn the_policy_mutations_are_audited() {
    let (app, _tmp, sink) = app().await;

    // Create.
    let (status, created) = call(
        &app,
        "POST",
        "/api/v1/iam/policies",
        "alice",
        Some(json!({
            "policy_id": "audited",
            "policy_content": "permit(principal, action, resource);",
            "attached_principals": ["hrn:hodei:iam::acme:User/alice"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create: {created:?}");

    // Update.
    let hrn = created["hrn"].as_str().unwrap().to_string();
    let (status, updated) = call(
        &app,
        "PUT",
        "/api/v1/iam/policies/update",
        "alice",
        Some(json!({
            "policy_hrn": hrn,
            "policy_content": "permit(principal, action == Admin::Action::\"AccessAdminApi\", resource);"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "update: {updated:?}");

    // Delete.
    let (status, deleted) = call(
        &app,
        "DELETE",
        "/api/v1/iam/policies/delete",
        "alice",
        Some(json!({"policy_hrn": hrn})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "delete: {deleted:?}");

    let events = sink.events();
    let actions: Vec<&str> = events.iter().map(|event| event.action.as_str()).collect();
    assert_eq!(
        actions,
        vec!["CreatePolicy", "UpdatePolicy", "DeletePolicy"],
        "the three mutations must be in the trail: {events:?}"
    );
    for event in &events {
        assert_eq!(event.actor, "hrn:hodei:iam::acme:User/alice");
        assert_eq!(
            event.result,
            hodei_artifacts_api::audit::AuditResult::Allowed
        );
        assert!(
            event.resource.contains("audited"),
            "the event must name the policy: {event:?}"
        );
        assert_eq!(event.correlation_id, "correlation-1");
    }
}

/// A refused mutation is part of the trail too.
///
/// A creation that names a principal of another account is denied, and a
/// denial that leaves no trace is indistinguishable from a request that never
/// happened.
#[tokio::test]
async fn a_refused_policy_mutation_is_audited_as_denied() {
    let (app, _tmp, sink) = app().await;

    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/iam/policies",
        "alice",
        Some(json!({
            "policy_id": "cross-account",
            "policy_content": "permit(principal, action, resource);",
            "attached_principals": ["hrn:hodei:iam::other:User/bob"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let events = sink.events();
    assert_eq!(
        events.len(),
        1,
        "the refusal must be in the trail: {events:?}"
    );
    assert_eq!(events[0].action, "CreatePolicy");
    assert_eq!(
        events[0].result,
        hodei_artifacts_api::audit::AuditResult::Denied,
        "a refusal is a denial, not a failure: {events:?}"
    );
    assert_eq!(events[0].actor, "hrn:hodei:iam::acme:User/alice");
}

/// The trail never carries the token.
#[tokio::test]
async fn the_trail_never_carries_the_token() {
    let (app, _tmp, sink) = app().await;

    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/iam/policies",
        "alice",
        Some(json!({
            "policy_id": "without-secrets",
            "policy_content": "permit(principal, action, resource);",
            "attached_principals": ["hrn:hodei:iam::acme:User/alice"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let events = sink.events();
    assert!(!events.is_empty(), "the creation must be in the trail");
    for event in &events {
        let rendered = format!("{event:?}");
        assert!(
            !rendered.contains(SECRET),
            "the event must not carry the token: {rendered}"
        );
        // And not the content of the policy either.
        assert!(
            !rendered.contains("permit(principal, action, resource)"),
            "the event must not carry the policy content: {rendered}"
        );
    }
}

/// The trail is reproducible when the clock and the identifier generator are
/// pinned.
///
/// A moment and an identifier are facts about the machine, not about the
/// operation. When the code reads them directly, two executions of the same
/// operation cannot be compared, and an event whose moment cannot be
/// reproduced cannot be checked.
#[tokio::test]
async fn the_trail_is_reproducible_with_pinned_seams() {
    // A fresh identity for each run: the same clock, the same generator, and
    // a counter that starts at zero, so the two runs are comparable.
    let identity = || {
        Identity::new(
            Arc::new(FixedClock::at("2026-01-02T03:04:05Z").expect("a valid moment")),
            Arc::new(CountingIdGenerator::with_prefix("event-")),
        )
    };
    let (app, _tmp, sink) = app_with_identity(identity()).await;

    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/iam/policies",
        "alice",
        Some(json!({
            "policy_id": "pinned",
            "policy_content": "permit(principal, action, resource);",
            "attached_principals": ["hrn:hodei:iam::acme:User/alice"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let events = sink.events();
    assert_eq!(events.len(), 1, "the creation must be in the trail");
    assert_eq!(
        events[0].timestamp.to_rfc3339(),
        "2026-01-02T03:04:05+00:00",
        "the moment must come from the clock, not from the machine"
    );
    assert!(
        events[0].event_id.starts_with("event-"),
        "the identifier must come from the generator, not from the machine: {}",
        events[0].event_id
    );
    let first_run_id = events[0].event_id.clone();

    // And a second run produces the same trail: the same operation, the same
    // moment and the same identifiers.
    let (app, _tmp, sink) = app_with_identity(identity()).await;
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/iam/policies",
        "alice",
        Some(json!({
            "policy_id": "pinned",
            "policy_content": "permit(principal, action, resource);",
            "attached_principals": ["hrn:hodei:iam::acme:User/alice"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].timestamp.to_rfc3339(),
        "2026-01-02T03:04:05+00:00"
    );
    assert_eq!(
        events[0].event_id, first_run_id,
        "the same operation must produce the same trail"
    );
}
