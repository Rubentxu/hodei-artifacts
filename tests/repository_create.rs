//! Authorized repository creation: one entity, account from the principal,
//! atomic uniqueness and no effects on rejection.

use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use cedar_policy::Entities;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::repository::{
    InMemoryRepositoryStore, RepositoryView, RepositoryWriteError, RepositoryWriter,
};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, AuthorizationOutcome, AuthorizationRequest, Credentials,
    IndeterminateReason, PrincipalAuthenticator, PrincipalClaims, PrincipalId,
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
        tokio::task::yield_now().await;
        let (account, name) = match token {
            "alice" => ("acme", "alice"),
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
    );
    permit(
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

async fn app(
    authorizer: Arc<dyn AuthorizerPort>,
) -> (axum::Router, InMemoryRepositoryStore, TempDir) {
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
    let store = InMemoryRepositoryStore::empty();
    let state = state
        .with_authorization(authorizer)
        .with_repository_auth(Arc::new(TestAuthenticator), Arc::new(store.clone()))
        .with_repository_writer(Arc::new(store.clone()));
    (build_router(state, &config), store, tmp)
}

async fn post(app: &axum::Router, token: Option<&str>, body: Value) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/api/v1/repositories")
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

async fn get(app: &axum::Router, hrn: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/repositories?hrn={hrn}"))
                .header("authorization", "Bearer alice")
                .body(Body::empty())
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

struct IndeterminateAuthorizer;

impl AuthorizerPort for IndeterminateAuthorizer {
    fn authorize(&self, _request: &AuthorizationRequest) -> AuthorizationOutcome {
        AuthorizationOutcome::indeterminate(IndeterminateReason::EvaluationError)
    }
}

async fn bootstrap_shared(authorizer: Arc<dyn AuthorizerPort>) -> (axum::Router, TempDir) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let mut config = AppConfig::default();
    config.security.admin_api_enabled = false;
    config.rocksdb.path = tmp.path().join("db").to_string_lossy().into_owned();
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
    .with_authorization(authorizer)
    .with_authenticator(Arc::new(TestAuthenticator));
    (build_router(state, &config), tmp)
}

#[tokio::test]
async fn bootstrap_reader_and_writer_share_the_same_store() {
    let (app, _tmp) = bootstrap_shared(cedar()).await;
    let (status, body) = post(&app, Some("alice"), json!({"name": "releases"})).await;
    assert_eq!(status, StatusCode::CREATED, "body: {body:?}");

    let (read_status, read_body) = get(&app, body["hrn"].as_str().unwrap()).await;
    assert_eq!(read_status, StatusCode::OK);
    assert_eq!(read_body["name"], "releases");
}

#[tokio::test]
async fn authorized_create_persists_one_entity_and_returns_its_hrn() {
    let (app, store, _tmp) = app(cedar()).await;
    let (status, body) = post(
        &app,
        Some("alice"),
        json!({"name": "releases", "description": "release repository"}),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "body: {body:?}");
    assert_eq!(
        body["hrn"],
        "hrn:hodei:artifacts:global:acme:repository/releases"
    );
    assert_eq!(body["account"], "acme");
    assert_eq!(store.len(), 1);

    let (read_status, read_body) = get(&app, body["hrn"].as_str().unwrap()).await;
    assert_eq!(read_status, StatusCode::OK);
    assert_eq!(read_body["name"], "releases");
}

#[tokio::test]
async fn the_body_cannot_choose_another_account() {
    let (app, store, _tmp) = app(cedar()).await;
    let (status, _) = post(
        &app,
        Some("alice"),
        json!({"name": "releases", "account": "other"}),
    )
    .await;

    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY,
        "unexpected status: {status}"
    );
    assert_eq!(store.len(), 0);
}

#[tokio::test]
async fn identity_without_create_permission_is_403_and_has_no_effect() {
    let (app, store, _tmp) = app(cedar()).await;
    let (status, _) = post(&app, Some("bob"), json!({"name": "releases"})).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(store.len(), 0);
}

#[tokio::test]
async fn duplicate_create_is_409_and_preserves_the_original() {
    let (app, store, _tmp) = app(cedar()).await;
    let (first, _) = post(
        &app,
        Some("alice"),
        json!({"name": "releases", "description": "original"}),
    )
    .await;
    let (second, _) = post(
        &app,
        Some("alice"),
        json!({"name": "releases", "description": "replacement"}),
    )
    .await;

    assert_eq!(first, StatusCode::CREATED);
    assert_eq!(second, StatusCode::CONFLICT);
    assert_eq!(store.len(), 1);
    let hrn = "hrn:hodei:artifacts:global:acme:repository/releases";
    let (status, body) = get(&app, hrn).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["description"], "original");
}

#[tokio::test]
async fn duplicate_create_is_409_and_keeps_one_entity() {
    let (app, store, _tmp) = app(cedar()).await;
    let (first, _) = post(&app, Some("alice"), json!({"name": "releases"})).await;
    let (second, _) = post(&app, Some("alice"), json!({"name": "releases"})).await;

    assert_eq!(first, StatusCode::CREATED);
    assert_eq!(second, StatusCode::CONFLICT);
    assert_eq!(store.len(), 1);
}

#[tokio::test]
async fn invalid_name_is_400_and_has_no_effect() {
    let (app, store, _tmp) = app(cedar()).await;
    for name in ["", "..", "a/b", "with space"] {
        let (status, _) = post(&app, Some("alice"), json!({"name": name})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "name {name:?}");
    }
    assert_eq!(store.len(), 0);
}

#[tokio::test]
async fn missing_token_is_401_and_has_no_effect() {
    let (app, store, _tmp) = app(cedar()).await;
    let (status, _) = post(&app, None, json!({"name": "releases"})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(store.len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_equivalent_requests_create_exactly_one_entity() {
    let (app, store, _tmp) = app(cedar()).await;
    let left = app.clone();
    let right = app.clone();

    let (a, b) = tokio::join!(
        post(&left, Some("alice"), json!({"name": "releases"})),
        post(&right, Some("alice"), json!({"name": "releases"})),
    );

    let statuses = [a.0, b.0];
    assert!(statuses.contains(&StatusCode::CREATED));
    assert!(statuses.contains(&StatusCode::CONFLICT));
    assert_eq!(store.len(), 1);
}

#[tokio::test]
async fn indeterminate_authorizer_is_403_and_has_no_effect() {
    let (app, store, _tmp) = app(Arc::new(IndeterminateAuthorizer)).await;
    let (status, _) = post(&app, Some("alice"), json!({"name": "releases"})).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(store.len(), 0);
}

#[tokio::test]
async fn missing_token_with_malformed_body_is_still_401() {
    let (app, store, _tmp) = app(cedar()).await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/repositories")
                .header("content-type", "application/json")
                .body(Body::from("{not-json"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(store.len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_store_itself_rejects_concurrent_duplicates() {
    let store = Arc::new(InMemoryRepositoryStore::empty());
    let hrn = kernel::Hrn::with_region(
        "hodei".to_string(),
        "artifacts".to_string(),
        "global".to_string(),
        "acme".to_string(),
        "repository".to_string(),
        "releases".to_string(),
    );
    let view = RepositoryView::new(
        hrn.to_string(),
        "acme".to_string(),
        "releases".to_string(),
        None,
    );

    let mut tasks = Vec::new();
    for _ in 0..16 {
        let store = store.clone();
        let hrn = hrn.clone();
        let view = view.clone();
        tasks.push(tokio::spawn(async move {
            store.create("acme", &hrn, view).await
        }));
    }
    let mut created = 0;
    let mut conflicts = 0;
    for task in tasks {
        match task.await.unwrap() {
            Ok(_) => created += 1,
            Err(RepositoryWriteError::AlreadyExists) => conflicts += 1,
            Err(other) => panic!("unexpected writer error: {other:?}"),
        }
    }

    assert_eq!(created, 1);
    assert_eq!(conflicts, 15);
    assert_eq!(store.len(), 1);
}

#[tokio::test]
async fn writer_failure_is_503_and_has_no_effect() {
    struct FailingWriter;
    #[async_trait]
    impl RepositoryWriter for FailingWriter {
        async fn create(
            &self,
            _account: &str,
            _hrn: &kernel::Hrn,
            _view: hodei_artifacts_api::repository::RepositoryView,
        ) -> Result<
            hodei_artifacts_api::repository::RepositoryView,
            hodei_artifacts_api::repository::RepositoryWriteError,
        > {
            Err(hodei_artifacts_api::repository::RepositoryWriteError::Unavailable)
        }
    }

    let tmp = tempfile::tempdir().unwrap();
    let mut config = AppConfig::default();
    config.rocksdb.path = tmp.path().join("db").to_string_lossy().into_owned();
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
    .with_repository_auth(
        Arc::new(TestAuthenticator),
        Arc::new(InMemoryRepositoryStore::empty()),
    )
    .with_repository_writer(Arc::new(FailingWriter));
    let app = build_router(state, &config);
    let (status, _) = post(&app, Some("alice"), json!({"name": "releases"})).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn reader_is_not_required_for_create() {
    // The writer performs the atomic uniqueness check; no pre-read is needed.
    let (app, store, _tmp) = app(cedar()).await;
    assert!(store.is_empty());
    let (status, _) = post(&app, Some("alice"), json!({"name": "releases"})).await;
    assert_eq!(status, StatusCode::CREATED);
}
