//! In-process HTTP contract for the authorized repository read.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use cedar_policy::Entities;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::repository::{
    InMemoryRepositoryReader, RepositoryReadError, RepositoryReader, RepositoryView,
};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, AuthorizationOutcome, AuthorizationRequest, Credentials,
    IndeterminateReason, PrincipalAuthenticator, PrincipalClaims, PrincipalId,
};
use kernel::Hrn;
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
            "alice" => ("acme", "alice"),
            "bob" => ("acme", "bob"),
            "carol" => ("other", "carol"),
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        Ok(PrincipalClaims::new(
            PrincipalId::new(format!("hrn:hodei:iam::{account}:User/{name}")).unwrap(),
            AccountId::new(account).unwrap(),
        ))
    }
}

struct CountingReader {
    inner: InMemoryRepositoryReader,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl RepositoryReader for CountingReader {
    async fn find_by_hrn(
        &self,
        account: &str,
        hrn: &Hrn,
    ) -> Result<Option<RepositoryView>, RepositoryReadError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.find_by_hrn(account, hrn).await
    }
}

struct FailingReader;

#[async_trait]
impl RepositoryReader for FailingReader {
    async fn find_by_hrn(
        &self,
        _account: &str,
        _hrn: &Hrn,
    ) -> Result<Option<RepositoryView>, RepositoryReadError> {
        Err(RepositoryReadError::Unavailable)
    }
}

struct IndeterminateAuthorizer;

impl AuthorizerPort for IndeterminateAuthorizer {
    fn authorize(&self, _request: &AuthorizationRequest) -> AuthorizationOutcome {
        AuthorizationOutcome::indeterminate(IndeterminateReason::EvaluationError)
    }
}

fn hrn() -> Hrn {
    Hrn::with_region(
        "hodei".to_string(),
        "artifacts".to_string(),
        "eu".to_string(),
        "acme".to_string(),
        "repository".to_string(),
        "releases".to_string(),
    )
}

fn view() -> RepositoryView {
    RepositoryView::new(
        hrn().to_string(),
        "acme".to_string(),
        "releases".to_string(),
        Some("release repository".to_string()),
    )
}

fn cedar() -> Arc<dyn AuthorizerPort> {
    let policy = r#"permit(
        principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
        action == Artifacts::Action::"ReadRepository",
        resource == Artifacts::Repository::"hrn:hodei:artifacts:eu:acme:repository/releases"
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

fn cedar_lax() -> Arc<dyn AuthorizerPort> {
    let policy = r#"permit(
        principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
        action == Artifacts::Action::"ReadRepository",
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

async fn app_with_reader(
    authorizer: Arc<dyn AuthorizerPort>,
    reader: Arc<dyn RepositoryReader>,
) -> (axum::Router, TempDir) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let mut config = AppConfig::default();
    config.security.admin_api_enabled = false;
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
    .expect("bootstrap");
    let state = state
        .with_authorization(authorizer)
        .with_repository_auth(Arc::new(TestAuthenticator), reader);
    (build_router(state, &config), tmp)
}

async fn app(
    authorizer: Arc<dyn AuthorizerPort>,
    reader: Arc<CountingReader>,
) -> (axum::Router, TempDir, Arc<AtomicUsize>) {
    let calls = reader.calls.clone();
    let (app, tmp) = app_with_reader(authorizer, reader).await;
    (app, tmp, calls)
}

fn reader(calls: Arc<AtomicUsize>) -> Arc<CountingReader> {
    Arc::new(CountingReader {
        inner: InMemoryRepositoryReader::empty().with(hrn(), view()),
        calls,
    })
}

async fn call(
    app: &axum::Router,
    token: Option<&str>,
    hrn: &str,
) -> (StatusCode, serde_json::Value) {
    let uri = format!("/api/v1/repositories?hrn={hrn}");
    let mut builder = Request::builder().uri(uri);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, body)
}

async fn raw_call(app: &axum::Router, token: &str, hrn: &str) -> (StatusCode, String) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/repositories?hrn={hrn}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn foreign_and_missing_repositories_share_the_same_404_body() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (app, _tmp, _calls) = app(cedar_lax(), reader(calls)).await;
    let foreign = Hrn::with_region(
        "hodei".to_string(),
        "artifacts".to_string(),
        "eu".to_string(),
        "other".to_string(),
        "repository".to_string(),
        "releases".to_string(),
    );
    let missing = Hrn::with_region(
        "hodei".to_string(),
        "artifacts".to_string(),
        "eu".to_string(),
        "acme".to_string(),
        "repository".to_string(),
        "missing".to_string(),
    );

    let (foreign_status, foreign_body) = raw_call(&app, "alice", &foreign.to_string()).await;
    let (missing_status, missing_body) = raw_call(&app, "alice", &missing.to_string()).await;

    assert_eq!(foreign_status, StatusCode::NOT_FOUND);
    assert_eq!(missing_status, StatusCode::NOT_FOUND);
    assert_eq!(foreign_body, missing_body);
}

#[tokio::test]
async fn permitted_read_returns_the_real_repository() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (app, _tmp, calls) = app(cedar(), reader(calls)).await;
    let (status, body) = call(&app, Some("alice"), &hrn().to_string()).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "releases");
    assert_eq!(body["account"], "acme");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn missing_or_invalid_token_is_401_and_never_reads() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (app, _tmp, calls) = app(cedar(), reader(calls)).await;

    let (status, _) = call(&app, None, &hrn().to_string()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = call(&app, Some("invalid"), &hrn().to_string()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn valid_identity_without_permission_is_403_and_never_reads() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (app, _tmp, calls) = app(cedar(), reader(calls)).await;
    let (status, _) = call(&app, Some("bob"), &hrn().to_string()).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn foreign_account_is_404_and_never_reads() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (app, _tmp, calls) = app(cedar(), reader(calls)).await;
    let foreign = Hrn::new(
        "hodei".to_string(),
        "artifacts".to_string(),
        "other".to_string(),
        "repository".to_string(),
        "releases".to_string(),
    );
    let (status, body) = call(&app, Some("alice"), &foreign.to_string()).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, serde_json::Value::Null);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn same_account_but_not_permitted_repository_is_403_and_never_reads() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (app, _tmp, calls) = app(cedar(), reader(calls)).await;
    let other = Hrn::with_region(
        "hodei".to_string(),
        "artifacts".to_string(),
        "eu".to_string(),
        "acme".to_string(),
        "repository".to_string(),
        "other".to_string(),
    );
    let (status, _) = call(&app, Some("alice"), &other.to_string()).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn non_repository_hrn_is_404_and_never_reads() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (app, _tmp, calls) = app(cedar_lax(), reader(calls)).await;
    let (status, _) = call(&app, Some("alice"), "hrn:hodei:iam::acme:User/alice").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn reader_failure_is_503() {
    let (app, _tmp) = app_with_reader(cedar_lax(), Arc::new(FailingReader)).await;
    let (status, _) = call(&app, Some("alice"), &hrn().to_string()).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn missing_query_is_400_without_reading() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (app, _tmp, calls) = app(cedar_lax(), reader(calls)).await;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/repositories")
                .header("authorization", "Bearer alice")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn unauthorized_response_has_a_bearer_challenge() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (app, _tmp, _calls) = app(cedar_lax(), reader(calls)).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/repositories?hrn={}", hrn()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response.headers().get("www-authenticate").unwrap(),
        "Bearer"
    );
}

#[tokio::test]
async fn missing_repository_is_404() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (app, _tmp, calls) = app(cedar_lax(), reader(calls)).await;
    let missing = Hrn::new(
        "hodei".to_string(),
        "artifacts".to_string(),
        "acme".to_string(),
        "repository".to_string(),
        "missing".to_string(),
    );
    let (status, _) = call(&app, Some("alice"), &missing.to_string()).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn malformed_hrn_is_400_and_never_reads() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (app, _tmp, calls) = app(cedar(), reader(calls)).await;
    let (status, _) = call(&app, Some("alice"), "not-an-hrn").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn indeterminate_authorizer_is_403_and_never_reads() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (app, _tmp, calls) = app(Arc::new(IndeterminateAuthorizer), reader(calls)).await;
    let (status, _) = call(&app, Some("alice"), &hrn().to_string()).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
