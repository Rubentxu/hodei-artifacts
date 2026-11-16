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
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
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
/// Alice may read two repositories of her account and not the third:
/// what Cedar does not grant, the search neither names nor counts. Carol
/// may only read hers.
fn cedar() -> Arc<dyn AuthorizerPort> {
    let policy = r#"permit(
        principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
        action == Artifacts::Action::"ReadRepository",
        resource == Artifacts::Repository::"hrn:hodei:artifacts:eu:acme:repository/libs"
    );
    permit(
        principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
        action == Artifacts::Action::"ReadRepository",
        resource == Artifacts::Repository::"hrn:hodei:artifacts:eu:acme:repository/apps"
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
async fn app() -> (axum::Router, Arc<SurrealCatalogStore>, tempfile::TempDir) {
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
        .with_authorization(cedar())
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

    assert_eq!(body["total"], json!(0), "the unreadable repository is counted");
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
    assert_eq!(body["total"], json!(3), "the assets of the readable repositories");
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
