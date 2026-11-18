//! The search and the detail view, against the real router.
//!
//! What is measured here is not the SQL: that is in the contract of the
//! port, against the real database. What is measured is the **surface**:
//! that the account comes from the verified principal, that Cedar narrows
//! what is seen, and that the detail view does not confuse "nobody said
//! it is good" with "it is good".
//!
//! The two accounts are the hard case: the other account has an asset
//! with the same name and the same path, and the search of one account
//! cannot return neither the name nor the total of the other.

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
    AllowAllAuthorizer, CatalogWriter, DigestScope, PackageCoordinates, Publication,
    RepositoryConfig, RepositoryDescriptor, RepositoryFormat, RepositoryMode, TrustGrant,
    TrustLedger, repository_hrn,
};
use hodei_catalog_storage::SurrealCatalogStore;
use hodei_evidence::ContentDigest;
use serde_json::json;
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
/// The repository served on the trusted channel.
const TOOLS: &str = "tools";

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
/// Alice may read the content of two repositories of her account and not
/// the third: what Cedar does not grant, the search neither names nor
/// counts. Carol may only read hers.
///
/// The action is `DownloadAsset`, the one of the content: a listing says
/// what is inside, and that is what the search says.
fn cedar() -> Arc<dyn AuthorizerPort> {
    let policy = r#"permit(
        principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
        action == Artifacts::Action::"DownloadAsset",
        resource == Artifacts::Repository::"hrn:hodei:artifacts:eu:acme:repository/libs"
    );
    permit(
        principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
        action == Artifacts::Action::"DownloadAsset",
        resource == Artifacts::Repository::"hrn:hodei:artifacts:eu:acme:repository/apps"
    );
    permit(
        principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
        action == Artifacts::Action::"DownloadAsset",
        resource == Artifacts::Repository::"hrn:hodei:artifacts:eu:acme:repository/tools"
    );
    permit(
        principal == Iam::User::"hrn:hodei:iam::other:User/carol",
        action == Artifacts::Action::"DownloadAsset",
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
///
/// The evaluation failed: it is not a denial, and it is not a permit
/// either. The surface must not answer as if it were a denial, because
/// an empty page says that there is nothing there, and nobody asked.
struct IndeterminateAuthorizer;

impl AuthorizerPort for IndeterminateAuthorizer {
    fn authorize(&self, _request: &AuthorizationRequest) -> AuthorizationOutcome {
        AuthorizationOutcome::indeterminate(IndeterminateReason::EvaluationError)
    }
}

/// A repository of the catalog.
fn descriptor(account: &str, key: &str, format: RepositoryFormat) -> RepositoryDescriptor {
    descriptor_with(account, key, format, serde_json::json!({}))
}

/// A repository of the catalog, with its settings.
fn descriptor_with(
    account: &str,
    key: &str,
    format: RepositoryFormat,
    settings: serde_json::Value,
) -> RepositoryDescriptor {
    let mut config = RepositoryConfig::new(DigestScope::Global);
    config.config_settings = settings;
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", account, key),
        account: account.to_string(),
        repo_key: key.to_string(),
        format,
        mode: RepositoryMode::Hosted,
        config,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

/// The application with a real catalog behind it.
async fn app() -> (axum::Router, Arc<SurrealCatalogStore>, tempfile::TempDir) {
    app_with(cedar()).await
}

/// The application with the authorizer that is given.
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

/// Publishes an asset and returns its digest.
async fn publish(
    store: &SurrealCatalogStore,
    account: &str,
    repository: &str,
    package: &str,
    version: &str,
    path: &str,
    subject: Option<ContentDigest>,
) -> ContentDigest {
    let digest = ContentDigest::of_bytes(path.as_bytes());
    store
        .publish(
            account,
            Publication {
                repository: repository.to_string(),
                asset_path: path.to_string(),
                coordinates: PackageCoordinates {
                    package_name: package.to_string(),
                    package_version: version.to_string(),
                },
                digest: digest.clone(),
                size_bytes: path.len() as u64,
                tags: Vec::new(),
                actor: format!("hrn:hodei:iam::{account}:User/alice"),
                correlation_id: "corr-hsp113".to_string(),
                overwrite: false,
                precondition: None,
                subject,
            },
        )
        .await
        .expect("published");
    digest
}

/// A catalog with two accounts, a repository that alice cannot read and
/// an asset with the same name in both.
async fn a_catalog(store: &SurrealCatalogStore) {
    for (account, key, format) in [
        (ACCOUNT, READABLE, RepositoryFormat::PyPi),
        (ACCOUNT, "apps", RepositoryFormat::Npm),
        (ACCOUNT, UNREADABLE, RepositoryFormat::Raw),
        (OTHER, THEIRS, RepositoryFormat::PyPi),
    ] {
        store
            .create_repository(account, descriptor(account, key, format))
            .await
            .expect("the repository is created");
    }
    // The repository served on the trusted channel: what is not granted
    // there does not come out.
    store
        .create_repository(
            ACCOUNT,
            descriptor_with(
                ACCOUNT,
                TOOLS,
                RepositoryFormat::Npm,
                serde_json::json!({ "trust": { "serve": "trusted" } }),
            ),
        )
        .await
        .expect("the repository is created");

    publish(
        store,
        ACCOUNT,
        READABLE,
        "labpkg",
        "1.0",
        "labpkg-1.0.whl",
        None,
    )
    .await;
    publish(
        store,
        ACCOUNT,
        READABLE,
        "labpkg",
        "1.1",
        "labpkg-1.1.whl",
        None,
    )
    .await;
    publish(
        store,
        ACCOUNT,
        "apps",
        "left-pad",
        "1.0.0",
        "left-pad-1.0.0.tgz",
        None,
    )
    .await;
    publish(
        store,
        ACCOUNT,
        UNREADABLE,
        "secretpkg",
        "1.0",
        "secretpkg-1.0.tar.gz",
        None,
    )
    .await;
    // The other account has an asset with the same name and the same path.
    publish(
        store,
        OTHER,
        THEIRS,
        "labpkg",
        "1.0",
        "labpkg-1.0.whl",
        None,
    )
    .await;
}

/// An asset seeded by hand, for the cases that need many.
#[derive(Debug, serde::Serialize)]
struct SeededAsset {
    account: String,
    repository: String,
    asset_path: String,
    package_name: String,
    package_version: String,
    digest_algorithm: String,
    digest: String,
    size_bytes: i64,
    subject: Option<String>,
    updated_at: surrealdb::sql::Datetime,
}

/// A search request.
fn search_request(token: &str, query: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(format!("/api/v1/search{query}"))
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .expect("a request")
}

/// A request for the detail of a package.
fn package_request(token: &str, repository: &str, path: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(format!("/api/v1/packages/{repository}/{path}"))
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .expect("a request")
}

/// The body of the response, as JSON.
async fn the_body(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .expect("the body"),
    )
    .expect("the body is JSON")
}

/// The search of an account finds its own and not the foreign one.
#[tokio::test]
async fn the_search_finds_its_own_and_not_the_other_account() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    let response = app
        .clone()
        .oneshot(search_request("alice", "?package=labpkg"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;

    assert_eq!(body["total"], json!(2), "the two versions of the package");
    let results = body["results"].as_array().expect("the results");
    assert_eq!(results.len(), 2);
    for result in results {
        assert_eq!(result["repository"], READABLE);
        assert_eq!(result["package_name"], "labpkg");
    }
}

/// The other account does not come out in the names nor in the totals.
#[tokio::test]
async fn the_other_account_does_not_appear_in_the_names_nor_in_the_totals() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    let response = app
        .clone()
        .oneshot(search_request("carol", "?package=labpkg"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;

    // The total is the one of its account: the same name in the other
    // account does not add up, because a total that added up what is
    // foreign would say there is something where there is nothing to
    // see.
    assert_eq!(body["total"], json!(1), "the count of the other account");
    let results = body["results"].as_array().expect("the results");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["repository"], THEIRS);

    // And the body names nothing of the other account.
    let text = serde_json::to_string(&body).expect("the body");
    assert!(!text.contains(READABLE), "the other repository is named");
    assert!(!text.contains("apps"), "the other repository is named");
    assert!(!text.contains(UNREADABLE), "the other repository is named");
}

/// The repository that Cedar does not grant neither comes out nor is counted.
#[tokio::test]
async fn a_repository_without_permission_neither_appears_nor_is_counted() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    let response = app
        .clone()
        .oneshot(search_request("alice", "?text=secretpkg"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;

    assert_eq!(
        body["total"],
        json!(0),
        "the unreadable repository is counted"
    );
    assert!(body["results"].as_array().expect("the results").is_empty());
}

/// A search without a credential is not a search.
#[tokio::test]
async fn the_search_needs_a_credential() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/search?package=labpkg")
        .body(Body::empty())
        .expect("a request");
    let response = app.oneshot(request).await.expect("the request");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// A query that does not say what it says is a 400.
#[tokio::test]
async fn a_query_that_does_not_say_what_it_says_is_a_400() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    for query in [
        "?pakcage=labpkg",
        "?package=labpkg&package=left-pad",
        "?limit=0",
        "?limit=not-a-number",
        "?digest=sha256:zz",
        "?attribute=colour=red",
    ] {
        let response = app
            .clone()
            .oneshot(search_request("alice", query))
            .await
            .expect("the request");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{query}");
    }
}

/// A request without filters is the walk over what there is, and the walk
/// does not name what the principal cannot read.
#[tokio::test]
async fn a_request_without_filters_is_the_walk_of_what_there_is() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    let response = app
        .clone()
        .oneshot(search_request("alice", ""))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;

    // The two repositories that Cedar grants, and not the third.
    assert_eq!(
        body["total"],
        json!(3),
        "the assets of the readable repositories"
    );
    let repositories: Vec<&str> = body["results"]
        .as_array()
        .expect("the results")
        .iter()
        .map(|result| result["repository"].as_str().unwrap_or(""))
        .collect();
    assert!(repositories.contains(&READABLE));
    assert!(repositories.contains(&"apps"));
    assert!(!repositories.contains(&UNREADABLE));
}

/// The detail view says where the bytes are, what was published, what
/// it hangs from, what the evidence says and what has been decided.
#[tokio::test]
async fn the_detail_view_links_the_blob_the_metadata_and_the_decision() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    let response = app
        .clone()
        .oneshot(package_request("alice", READABLE, "labpkg-1.0.whl"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;

    assert_eq!(body["repository"], READABLE);
    assert_eq!(body["asset_path"], "labpkg-1.0.whl");
    assert_eq!(body["package_name"], "labpkg");
    assert_eq!(body["package_version"], "1.0");

    // The blob: where the bytes are.
    assert_eq!(body["blob"]["scope"], "global");
    assert!(body["blob"]["digest"]["value"].is_string());
    assert_eq!(body["blob"]["size_bytes"], body["size_bytes"]);

    // The original metadata: the version as it was published.
    assert_eq!(body["published"]["package_name"], "labpkg");
    assert_eq!(body["published"]["package_version"], "1.0");
    assert!(body["published"]["published_at"].is_string());

    // The line: what it hangs from.
    assert!(body["lineage"]["subject"].is_null());
    assert!(
        body["lineage"]["referrers"]
            .as_array()
            .expect("the referrers")
            .is_empty()
    );

    // The capsule: what the evidence says.
    assert!(
        !body["capsule"]["content_hash"]
            .as_str()
            .unwrap_or("")
            .is_empty()
    );

    // And the decision: without a grant, quarantined. There are no
    // findings, but that is not a trust: it is a silence.
    assert_eq!(body["trust"]["state"], "quarantined");
    assert!(body["trust"]["decision"].is_null());
}

/// A referrer hangs from the asset and the view says so.
#[tokio::test]
async fn the_view_says_whose_the_content_is() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;
    let subject = ContentDigest::of_bytes(b"labpkg-1.0.whl");
    publish(
        &store,
        ACCOUNT,
        READABLE,
        "sbom",
        "1.0",
        "sbom-1.0.spdx.json",
        Some(subject.clone()),
    )
    .await;

    let response = app
        .clone()
        .oneshot(package_request("alice", READABLE, "labpkg-1.0.whl"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;

    let referrers = body["lineage"]["referrers"]
        .as_array()
        .expect("the referrers");
    assert_eq!(referrers.len(), 1, "the referrer is not there");
    assert_eq!(referrers[0]["asset_path"], "sbom-1.0.spdx.json");
    assert_eq!(referrers[0]["subject"]["value"], subject.value);
}

/// The grant is what says the content is trusted, and it says it with
/// its decision.
#[tokio::test]
async fn the_grant_is_the_decision_that_says_it_is_trusted() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;
    let digest = ContentDigest::of_bytes(b"labpkg-1.0.whl");
    store
        .grant(&[TrustGrant {
            account: ACCOUNT.to_string(),
            repository: READABLE.to_string(),
            digest: digest.clone(),
            decision_id: "dec-hsp113".to_string(),
            actor: "hrn:hodei:iam::acme:User/alice".to_string(),
            granted_at: chrono::Utc::now(),
            revoked_at: None,
            revoked_by: None,
            revoke_reason: None,
            subject: None,
            policy_version: Some("1.0.0".to_string()),
            capsule_hash: None,
            risk_version: None,
            valid_until: Some(chrono::Utc::now() + chrono::Duration::hours(1)),
        }])
        .await
        .expect("the grant");

    let response = app
        .clone()
        .oneshot(package_request("alice", READABLE, "labpkg-1.0.whl"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;

    assert_eq!(body["trust"]["state"], "trusted");
    assert_eq!(body["trust"]["decision"]["decision_id"], "dec-hsp113");

    // And the search says it too, without rewriting anything.
    let response = app
        .oneshot(search_request("alice", "?package=labpkg"))
        .await
        .expect("the request");
    let body = the_body(response).await;
    let states: Vec<&str> = body["results"]
        .as_array()
        .expect("the results")
        .iter()
        .map(|result| result["trust_state"].as_str().unwrap_or(""))
        .collect();
    assert!(
        states.contains(&"trusted"),
        "the promotion is not in the search: {states:?}"
    );
}

/// The detail of an asset of another account does not exist.
#[tokio::test]
async fn the_detail_of_another_account_asset_does_not_exist() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    // The repository is not of carol's account: the same 404 as if it did
    // not exist, because answering differently would say it does.
    let response = app
        .clone()
        .oneshot(package_request("carol", READABLE, "labpkg-1.0.whl"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // And hers, which exists, is seen.
    let response = app
        .oneshot(package_request("carol", THEIRS, "labpkg-1.0.whl"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
}

/// The detail of an asset of a repository without permission is not seen.
#[tokio::test]
async fn the_detail_of_a_repository_without_permission_is_not_seen() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    let response = app
        .clone()
        .oneshot(package_request("alice", UNREADABLE, "secretpkg-1.0.tar.gz"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

/// The detail needs a credential.
#[tokio::test]
async fn the_detail_needs_a_credential() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/packages/{READABLE}/labpkg-1.0.whl"))
        .body(Body::empty())
        .expect("a request");
    let response = app.oneshot(request).await.expect("the request");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// The paths of the results of a page.
fn the_paths(body: &serde_json::Value) -> Vec<String> {
    body["results"]
        .as_array()
        .expect("the results")
        .iter()
        .map(|result| result["asset_path"].as_str().expect("a path").to_string())
        .collect()
}

/// The search pages, and says whether there is more behind the page.
#[tokio::test]
async fn the_search_pages_and_says_whether_there_is_more() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    let first = app
        .clone()
        .oneshot(search_request("alice", "?limit=2"))
        .await
        .expect("the request");
    assert_eq!(first.status(), StatusCode::OK);
    let body = the_body(first).await;
    assert_eq!(body["total"], json!(3), "the three assets she can read");
    assert_eq!(body["more"], json!(true), "and there is one behind");
    assert_eq!(body["limit"], json!(2));
    assert_eq!(body["offset"], json!(0));
    let first_paths = the_paths(&body);

    let last = app
        .clone()
        .oneshot(search_request("alice", "?limit=2&offset=2"))
        .await
        .expect("the request");
    assert_eq!(last.status(), StatusCode::OK);
    let body = the_body(last).await;
    assert_eq!(body["more"], json!(false), "the last page is the last page");
    assert_eq!(body["offset"], json!(2));
    let last_paths = the_paths(&body);

    // No asset is repeated and none is lost.
    let mut all: Vec<String> = first_paths
        .iter()
        .chain(last_paths.iter())
        .cloned()
        .collect();
    let seen = all.len();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), seen, "no asset is repeated");
    assert_eq!(seen, 3, "and none is lost");
}

/// A total that does not fit is not said, and the page says there is
/// more.
#[tokio::test]
async fn a_total_that_does_not_fit_is_not_said() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    // More assets than are counted: the total does not fit, and a number
    // that was not counted is not said.
    let now = surrealdb::sql::Datetime::from(chrono::Utc::now());
    let rows: Vec<SeededAsset> = (0..300)
        .map(|index| SeededAsset {
            account: ACCOUNT.to_string(),
            repository: READABLE.to_string(),
            asset_path: format!("cargado-{index:05}.whl"),
            package_name: format!("cargado-{index:05}"),
            package_version: "1.0".to_string(),
            digest_algorithm: "sha256".to_string(),
            digest: format!("{index:064x}"),
            size_bytes: 10,
            subject: None,
            updated_at: now.clone(),
        })
        .collect();
    store
        .db()
        .query("INSERT INTO catalog_asset $rows")
        .bind(("rows", rows))
        .await
        .expect("the assets are seeded")
        .check()
        .expect("the assets are seeded");

    let response = app
        .clone()
        .oneshot(search_request("alice", "?limit=10"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;

    assert_eq!(
        body["total"],
        serde_json::Value::Null,
        "the total is not said"
    );
    assert_eq!(body["more"], json!(true), "and there is more");
    assert_eq!(body["results"].as_array().expect("the results").len(), 10);
}

/// The filters of the search are the ones the client writes.
#[tokio::test]
async fn the_filters_of_the_search_are_the_ones_the_client_writes() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    // The attribute of the repository.
    let response = app
        .clone()
        .oneshot(search_request("alice", "?attribute=format%3Dpypi"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;
    assert_eq!(
        body["total"],
        json!(2),
        "the two assets of the PyPI repository"
    );

    // The digest of the content.
    let digest = ContentDigest::of_bytes(b"labpkg-1.0.whl");
    let response = app
        .clone()
        .oneshot(search_request(
            "alice",
            &format!("?digest=sha256:{}", digest.value),
        ))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;
    assert_eq!(body["total"], json!(1), "the asset of that digest");
    assert_eq!(body["results"][0]["asset_path"], "labpkg-1.0.whl");

    // And the tag, which narrows by its coordinates.
    store
        .move_tag(
            ACCOUNT,
            READABLE,
            "latest",
            PackageCoordinates {
                package_name: "labpkg".to_string(),
                package_version: "1.1".to_string(),
            },
        )
        .await
        .expect("the tag moves");
    let response = app
        .clone()
        .oneshot(search_request("alice", "?tag=latest"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;
    assert_eq!(body["total"], json!(1), "the tag points at one version");
    assert_eq!(body["results"][0]["package_version"], "1.1");
}

/// A repository served on the trusted channel does not show what has no
/// grant, and its detail answers that it does not exist.
#[tokio::test]
async fn the_trusted_channel_does_not_show_what_has_no_grant() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;
    let digest = publish(
        &store,
        ACCOUNT,
        TOOLS,
        "toolpkg",
        "1.0.0",
        "toolpkg-1.0.0.tgz",
        None,
    )
    .await;

    // The search does not name it.
    let response = app
        .clone()
        .oneshot(search_request("alice", "?repository=tools"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;
    assert_eq!(
        body["total"],
        json!(0),
        "what is not granted is not counted"
    );
    assert!(body["results"].as_array().expect("the results").is_empty());

    // And the detail answers that it does not exist: a 403 would say
    // that the content exists.
    let response = app
        .clone()
        .oneshot(package_request("alice", TOOLS, "toolpkg-1.0.0.tgz"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // Granted, it comes out and its detail comes with it.
    store
        .grant(&[TrustGrant {
            account: ACCOUNT.to_string(),
            repository: TOOLS.to_string(),
            digest: digest.clone(),
            decision_id: "dec-hsp113-e2e".to_string(),
            actor: "hrn:hodei:iam:acme:User/alice".to_string(),
            granted_at: chrono::Utc::now(),
            revoked_at: None,
            revoked_by: None,
            revoke_reason: None,
            subject: None,
            policy_version: Some("1.0.0".to_string()),
            capsule_hash: None,
            risk_version: None,
            valid_until: Some(chrono::Utc::now() + chrono::Duration::hours(1)),
        }])
        .await
        .expect("the grant");
    let response = app
        .clone()
        .oneshot(search_request("alice", "?repository=tools"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;
    assert_eq!(body["total"], json!(1), "the granted asset comes out");
    assert_eq!(body["results"][0]["trust_state"], "trusted");

    let response = app
        .clone()
        .oneshot(package_request("alice", TOOLS, "toolpkg-1.0.0.tgz"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;
    assert_eq!(body["trust"]["state"], "trusted");
    assert_eq!(body["trust"]["decision"]["decision_id"], "dec-hsp113-e2e");
}

/// An authorizer that cannot decide answers that it cannot answer.
///
/// A denial is a decision and the page leaves the repository out. An
/// authorizer that could not decide did not deny anything: an empty
/// page would say that there is nothing there, and nobody asked.
#[tokio::test]
async fn an_authorizer_that_cannot_decide_is_not_an_empty_page() {
    let (app, store, _tmp) = app_with(Arc::new(IndeterminateAuthorizer)).await;
    a_catalog(&store).await;

    let response = app
        .clone()
        .oneshot(search_request("alice", ""))
        .await
        .expect("the request");
    assert_eq!(
        response.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "the search says that it cannot answer"
    );

    let response = app
        .clone()
        .oneshot(package_request("alice", READABLE, "labpkg-1.0.whl"))
        .await
        .expect("the request");
    assert_eq!(
        response.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "and so does the detail"
    );
}
