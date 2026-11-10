//! Audit events at the create effect edge, without credentials.

use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use cedar_policy::Entities;
use hodei_artifacts_api::audit::{AuditResult, AuditSink, FailingAuditSink, InMemoryAuditSink};
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::repository::{
    InMemoryRepositoryStore, RepositoryView, RepositoryWriteError, RepositoryWriter,
};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

struct TestAuthenticator;

#[async_trait]
impl PrincipalAuthenticator for TestAuthenticator {
    async fn authenticate(
        &self,
        credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        let token = match credentials {
            Credentials::Bearer(token) => token.as_str(),
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        let (account, name) = match token {
            "tok-audit-secret" => ("acme", "alice"),
            "bob" => ("acme", "bob"),
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        Ok(PrincipalClaims::new(
            PrincipalId::new(format!("hrn:hodei:iam::{account}:User/{name}")).unwrap(),
            AccountId::new(account).unwrap(),
        ))
    }
}

fn cedar() -> Arc<dyn AuthorizerPort> {
    let policy = r#"permit(
        principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
        action == Artifacts::Action::"CreateRepository",
        resource
    );"#;
    Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .expect("authorizer"),
    )))
}

async fn app(sink: Arc<dyn AuditSink>) -> (axum::Router, Arc<InMemoryRepositoryStore>, TempDir) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let mut config = AppConfig::default();
    config.rocksdb.path = tmp.path().join("db").to_string_lossy().into_owned();
    let store = Arc::new(InMemoryRepositoryStore::empty());
    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .expect("bootstrap")
    .with_authorization(cedar())
    .with_repository_auth(Arc::new(TestAuthenticator), store.clone())
    .with_repository_writer(store.clone())
    .with_audit_sink(sink);
    (build_router(state, &config), store, tmp)
}

async fn post(
    app: &axum::Router,
    token: Option<&str>,
    correlation: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/api/v1/repositories")
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if let Some(correlation) = correlation {
        builder = builder.header("x-request-id", correlation);
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(Body::from(json!({"name": "releases"}).to_string()))
                .unwrap(),
        )
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

#[tokio::test]
async fn allowed_create_emits_correlated_identity_without_token() {
    let sink = Arc::new(InMemoryAuditSink::new());
    let (app, store, _tmp) = app(sink.clone()).await;
    let (status, body) = post(&app, Some("tok-audit-secret"), Some("corr-1")).await;

    assert_eq!(status, StatusCode::CREATED, "body: {body:?}");
    assert_eq!(store.len(), 1);
    let events = sink.events();
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event.actor, "hrn:hodei:iam::acme:User/alice");
    assert_eq!(event.action, "Artifacts::Action::\"CreateRepository\"");
    assert_eq!(
        event.resource,
        "hrn:hodei:artifacts:global:acme:repository/releases"
    );
    assert_eq!(event.result, AuditResult::Allowed);
    assert_eq!(event.correlation_id, "corr-1");

    let serialized = serde_json::to_string(event).unwrap();
    assert!(!serialized.contains("Bearer"));
    assert!(!serialized.contains("tok-audit-secret"));
    assert!(!serialized.contains("tok-audit"));
}

#[tokio::test]
async fn denied_create_emits_denied_without_writing() {
    let sink = Arc::new(InMemoryAuditSink::new());
    let (app, store, _tmp) = app(sink.clone()).await;
    let (status, _) = post(&app, Some("bob"), Some("corr-denied")).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(store.len(), 0);
    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].result, AuditResult::Denied);
    assert_eq!(events[0].actor, "hrn:hodei:iam::acme:User/bob");
    assert_eq!(events[0].correlation_id, "corr-denied");
}

#[tokio::test]
async fn duplicate_and_success_are_both_audited() {
    let sink = Arc::new(InMemoryAuditSink::new());
    let (app, store, _tmp) = app(sink.clone()).await;

    assert_eq!(
        post(&app, Some("tok-audit-secret"), Some("c1")).await.0,
        StatusCode::CREATED
    );
    assert_eq!(
        post(&app, Some("tok-audit-secret"), Some("c2")).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(store.len(), 1);

    let results: Vec<_> = sink
        .events()
        .into_iter()
        .map(|event| event.result)
        .collect();
    assert_eq!(results, vec![AuditResult::Allowed, AuditResult::Conflict]);
}

#[tokio::test]
async fn no_token_has_no_authenticated_actor_and_no_event() {
    let sink = Arc::new(InMemoryAuditSink::new());
    let (app, store, _tmp) = app(sink.clone()).await;
    let (status, _) = post(&app, None, Some("c3")).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(store.len(), 0);
    assert!(sink.is_empty());
}

#[tokio::test]
async fn audit_sink_failure_does_not_change_the_decision() {
    let failing = Arc::new(FailingAuditSink::new());
    let (app, store, _tmp) = app(failing.clone()).await;
    let (status, _) = post(&app, Some("tok-audit-secret"), Some("c4")).await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(store.len(), 1);
    assert_eq!(failing.attempts(), 1, "the sink must have been attempted");
}

struct FailingWriter;

#[async_trait]
impl RepositoryWriter for FailingWriter {
    async fn create(
        &self,
        _account: &str,
        _hrn: &kernel::Hrn,
        _view: RepositoryView,
    ) -> Result<RepositoryView, RepositoryWriteError> {
        Err(RepositoryWriteError::Unavailable)
    }
}

#[tokio::test]
async fn writer_failure_emits_failed() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = AppConfig::default();
    config.rocksdb.path = tmp.path().join("db").to_string_lossy().into_owned();
    let sink = Arc::new(InMemoryAuditSink::new());
    let store = Arc::new(InMemoryRepositoryStore::empty());
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
    .with_authorization(cedar())
    .with_repository_auth(Arc::new(TestAuthenticator), store)
    .with_repository_writer(Arc::new(FailingWriter))
    .with_audit_sink(sink.clone());
    let app = build_router(state, &config);

    let (status, _) = post(&app, Some("tok-audit-secret"), Some("c-failed")).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].result, AuditResult::Failed);
    assert_eq!(events[0].correlation_id, "c-failed");
}

#[tokio::test]
async fn correlation_id_is_echoed_and_used_in_the_event() {
    let sink = Arc::new(InMemoryAuditSink::new());
    let (app, _store, _tmp) = app(sink.clone()).await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/repositories")
                .header("content-type", "application/json")
                .header("authorization", "Bearer tok-audit-secret")
                .header("x-request-id", "c-echo")
                .body(Body::from(json!({"name": "releases"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(response.headers().get("x-request-id").unwrap(), "c-echo");
    assert_eq!(sink.events()[0].correlation_id, "c-echo");
}
