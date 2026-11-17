//! The console's view of the repositories of the catalog.
//!
//! What is measured here is the surface: that the list says which
//! repositories of the account the principal may read, that the creation
//! refuses what the release would not serve, and that a decision that
//! could not be taken is not answered with an empty list.
//!
//! The capabilities endpoint is measured too, because it is what the
//! console offers: if it said a modality that the API does not serve, the
//! console would offer a repository that cannot work.

use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use cedar_policy::Entities;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, AuthorizationOutcome, AuthorizationRequest, Credentials,
    IndeterminateReason, PrincipalAuthenticator, PrincipalClaims, PrincipalId,
};
use hodei_catalog::{
    AllowAllAuthorizer, CatalogReader, CatalogWriter, DigestScope, RepositoryConfig,
    RepositoryDescriptor, RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_catalog_storage::SurrealCatalogStore;
use serde_json::{Value, json};
use surrealdb::Surreal;
use surrealdb::engine::local::RocksDb;
use tower::ServiceExt;

/// The first account.
const ACCOUNT: &str = "acme";
/// The second account.
const OTHER: &str = "other";
/// The repository the first account may read.
const READABLE: &str = "libs";
/// The repository the first account **cannot** read.
const UNREADABLE: &str = "private";
/// The repository of the second account.
const THEIRS: &str = "secrets";

/// The credential verifier of the test.
///
/// The token is the name of the person: there is no secret here, and the
/// account comes from the token and not from a header.
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
            "alice" => (ACCOUNT, "alice"),
            "bob" => (ACCOUNT, "bob"),
            "carol" => (OTHER, "carol"),
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        Ok(PrincipalClaims::new(
            PrincipalId::new(format!("hrn:hodei:iam::{account}:User/{name}")).unwrap(),
            AccountId::new(account).unwrap(),
        ))
    }
}

/// The policy of the test.
///
/// Alice may read one repository of her account and not the other, and she
/// may create repositories. Bob may read nothing and create nothing.
///
/// The action of the list is `ReadRepository`: the list says which
/// repositories exist, not what is inside them.
fn cedar() -> Arc<dyn AuthorizerPort> {
    let policy = r#"permit(
        principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
        action == Artifacts::Action::"ReadRepository",
        resource == Artifacts::Repository::"hrn:hodei:artifacts:eu:acme:repository/libs"
    );
    permit(
        principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
        action == Artifacts::Action::"CreateRepository",
        resource
    );
    permit(
        principal == Iam::User::"hrn:hodei:iam::other:User/carol",
        action == Artifacts::Action::"ReadRepository",
        resource == Artifacts::Repository::"hrn:hodei:artifacts:eu:other:repository/secrets"
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

/// An authorizer that cannot decide.
struct IndeterminateAuthorizer;

impl AuthorizerPort for IndeterminateAuthorizer {
    fn authorize(&self, _request: &AuthorizationRequest) -> AuthorizationOutcome {
        AuthorizationOutcome::indeterminate(IndeterminateReason::EvaluationError)
    }
}

/// A repository of the catalog.
fn descriptor(account: &str, key: &str, format: RepositoryFormat) -> RepositoryDescriptor {
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", account, key),
        account: account.to_string(),
        repo_key: key.to_string(),
        format,
        mode: RepositoryMode::Hosted,
        config: RepositoryConfig::new(DigestScope::Global),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

/// The application with a real catalog behind it.
async fn app_with(
    authorizer: Arc<dyn AuthorizerPort>,
) -> (axum::Router, Arc<SurrealCatalogStore>, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = Surreal::new::<RocksDb>(tmp.path().join("db"))
        .await
        .expect("rocksdb");
    db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
    let store = Arc::new(SurrealCatalogStore::with_authorizer(
        db,
        Arc::new(AllowAllAuthorizer),
    ));
    store.initialize().await.expect("initialize");

    let mut config = AppConfig::default();
    config.security.admin_api_enabled = false;
    config.rocksdb.path = tmp
        .path()
        .join("schema.rocksdb")
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
        .with_repository_auth(
            Arc::new(TestAuthenticator),
            Arc::new(hodei_artifacts_api::repository::InMemoryRepositoryReader::empty()),
        )
        .with_catalog(store.clone(), store.clone())
        .with_catalog_search(store.clone())
        .with_trust_ledger(store.clone());

    (build_router(state, &config), store, tmp)
}

/// The application with the policy of the test.
async fn app() -> (axum::Router, Arc<SurrealCatalogStore>, tempfile::TempDir) {
    app_with(cedar()).await
}

/// A catalog with two accounts and a repository that alice cannot read.
async fn a_catalog(store: &SurrealCatalogStore) {
    for (account, key, format) in [
        (ACCOUNT, READABLE, RepositoryFormat::PyPi),
        (ACCOUNT, UNREADABLE, RepositoryFormat::Raw),
        (OTHER, THEIRS, RepositoryFormat::PyPi),
    ] {
        store
            .create_repository(account, descriptor(account, key, format))
            .await
            .expect("the repository is created");
    }
}

/// A request with a credential.
fn request(method: &str, path: &str, token: &str, body: Option<Value>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {token}"));
    match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("a request"),
        None => builder.body(Body::empty()).expect("a request"),
    }
}

/// The names of the repositories of the list.
fn the_names(body: &Value) -> Vec<String> {
    body["repositories"]
        .as_array()
        .expect("the repositories")
        .iter()
        .map(|repository| repository["name"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn the_list_names_the_repositories_of_the_account_that_can_be_read() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    let response = app
        .oneshot(request(
            "GET",
            "/api/v1/catalog/repositories",
            "alice",
            None,
        ))
        .await
        .expect("the response");
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("the body"),
    )
    .expect("json");

    let names = the_names(&body);
    assert_eq!(names, vec![READABLE.to_string()], "the readable one");
    assert!(
        !names.contains(&UNREADABLE.to_string()),
        "the one Cedar does not grant does not come out"
    );
    assert!(
        !names.contains(&THEIRS.to_string()),
        "the one of the other account does not come out"
    );
}

#[tokio::test]
async fn the_list_says_the_format_the_modality_and_the_channel() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    let response = app
        .oneshot(request(
            "GET",
            "/api/v1/catalog/repositories",
            "alice",
            None,
        ))
        .await
        .expect("the response");
    let body: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("the body"),
    )
    .expect("json");
    let repository = &body["repositories"][0];
    assert_eq!(repository["format"], "pypi");
    assert_eq!(repository["mode"], "hosted");
    assert_eq!(repository["channel"], "open");
}

#[tokio::test]
async fn the_list_of_the_other_account_names_only_what_it_may_read() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    let response = app
        .oneshot(request(
            "GET",
            "/api/v1/catalog/repositories",
            "carol",
            None,
        ))
        .await
        .expect("the response");
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("the body"),
    )
    .expect("json");
    assert_eq!(the_names(&body), vec![THEIRS.to_string()]);
}

#[tokio::test]
async fn a_list_that_cannot_be_decided_is_not_an_empty_list() {
    let (app, store, _tmp) = app_with(Arc::new(IndeterminateAuthorizer)).await;
    a_catalog(&store).await;

    let response = app
        .oneshot(request(
            "GET",
            "/api/v1/catalog/repositories",
            "alice",
            None,
        ))
        .await
        .expect("the response");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("the body"),
    )
    .expect("json");
    assert_eq!(
        body["error"], "indeterminate",
        "the console has to be able to tell it apart from an empty list"
    );
}

#[tokio::test]
async fn a_repository_is_created_in_the_modality_that_the_format_serves() {
    let (app, store, _tmp) = app().await;

    let response = app
        .oneshot(request(
            "POST",
            "/api/v1/catalog/repositories",
            "alice",
            Some(json!({
                "name": "releases",
                "format": "npm",
                "mode": "proxy",
                "region": "eu",
                "settings": {
                    "proxy": {
                        "upstream": {
                            "id": "central",
                            "url": "https://registry.example.org/",
                            "scopes": ["/"]
                        }
                    }
                }
            })),
        ))
        .await
        .expect("the response");
    assert_eq!(response.status(), StatusCode::CREATED);
    let body: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("the body"),
    )
    .expect("json");
    assert_eq!(body["name"], "releases");
    assert_eq!(body["format"], "npm");
    assert_eq!(body["mode"], "proxy");

    let created = store
        .repository(ACCOUNT, "releases")
        .await
        .expect("the repository is in the catalog");
    assert_eq!(created.mode, RepositoryMode::Proxy);
}

#[tokio::test]
async fn a_format_that_does_not_serve_the_modality_is_refused() {
    let (app, _store, _tmp) = app().await;

    let response = app
        .oneshot(request(
            "POST",
            "/api/v1/catalog/repositories",
            "alice",
            Some(json!({ "name": "images", "format": "oci", "mode": "proxy" })),
        ))
        .await
        .expect("the response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("the body"),
    )
    .expect("json");
    assert_eq!(body["error"], "invalid");
}

#[tokio::test]
async fn a_proxy_without_an_upstream_is_refused() {
    let (app, _store, _tmp) = app().await;

    let response = app
        .oneshot(request(
            "POST",
            "/api/v1/catalog/repositories",
            "alice",
            Some(json!({ "name": "mirror", "format": "raw", "mode": "proxy" })),
        ))
        .await
        .expect("the response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_group_without_members_is_refused() {
    let (app, _store, _tmp) = app().await;

    let response = app
        .oneshot(request(
            "POST",
            "/api/v1/catalog/repositories",
            "alice",
            Some(json!({ "name": "everything", "format": "raw", "mode": "group" })),
        ))
        .await
        .expect("the response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_creation_without_the_permission_is_refused() {
    let (app, _store, _tmp) = app().await;

    let response = app
        .oneshot(request(
            "POST",
            "/api/v1/catalog/repositories",
            "bob",
            Some(json!({ "name": "releases", "format": "raw", "mode": "hosted" })),
        ))
        .await
        .expect("the response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_capabilities_say_what_can_be_created() {
    let (app, _store, _tmp) = app().await;

    let response = app
        .oneshot(request("GET", "/api/v1/capabilities", "alice", None))
        .await
        .expect("the response");
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("the body"),
    )
    .expect("json");

    let formats = body["formats"].as_array().expect("the formats");
    let of = |name: &str| {
        formats
            .iter()
            .find(|format| format["name"] == name)
            .unwrap_or_else(|| panic!("the format {name} is not offered"))
    };
    assert_eq!(
        of("raw")["modes"],
        json!(["hosted", "proxy", "group"]),
        "the raw surface serves the three modalities"
    );
    assert_eq!(
        of("oci")["modes"],
        json!(["hosted"]),
        "the OCI surface serves only the hosted one"
    );
    assert_eq!(of("npm")["modes"], json!(["hosted", "proxy", "group"]));
    assert!(
        formats.iter().all(|format| format["name"] != "cargo"),
        "a format that this release does not serve is not offered"
    );
}
