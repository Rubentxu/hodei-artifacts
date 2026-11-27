//! HSP-323 slice 7 (origin HSP-113 AC3): deleting an asset keeps the search, the
//! download and the garbage collection consistent, through the real router over
//! the real Dgraph catalog, the real Cedar engine, the real GC service and the
//! real file CAS. Every test owns an ACL namespace.
//!
//! What this suite proves (each case is a behaviour of
//! `DELETE /repository/{repository}/{*path}`):
//!
//! - after a deletion the path is gone from the search and from the download,
//!   the blob is left in the CAS for the collector, and the collector reclaims
//!   it only when nothing else names it: a dependency freeze (a hold) keeps the
//!   bytes of what it pinned and releasing the freeze hands them back; the
//!   control is a path that was not deleted, whose bytes are never touched;
//! - without the dedicated Cedar action `DeleteAsset` there is no deletion
//!   (403, audited) and the asset is still found and served;
//! - another account's repository is the same 404 as one that never existed,
//!   nothing of the owner changes and the stranger's search is byte-identical
//!   before and after the owner deletes (no name, no total, no timing field);
//! - repeating a deletion is harmless (`deleted: false`, no new audit fact), a
//!   path can be published again and deleted again, and the history of a path
//!   replaced under `raw.replace: allow` goes with it;
//! - a store failure is closed: the deletion answers 503 and nothing is hidden;
//! - promotion and re-evaluation (grant and revoke in the trust ledger) are
//!   reflected by the search of a trusted repository at once, and a deleted
//!   asset stays gone even when a grant is still active.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` (with `HODEI_REQUIRE_IT=1` a missing
//! harness fails instead of skipping); serialize with `--test-threads=1`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use cedar_policy::Entities;
use chrono::Utc;
use hodei_artifacts_api::audit::{AuditResult, InMemoryAuditSink};
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::gc::GcService;
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use hodei_catalog::{
    AssetDeletion, AssetRecord, CatalogError, CatalogReader, CatalogSearch, CatalogWriter,
    ContentLedger, DigestScope, MutableTag, PackageCoordinates, Publication, PublishOutcome,
    RepositoryConfig, RepositoryDescriptor, RepositoryFormat, RepositoryMode, TrustGrant,
    TrustLedger, repository_hrn,
};
use hodei_catalog_storage::DgraphCatalogStore;
use hodei_evidence::ContentDigest;
use hodei_hub::blob::{BlobStore, FileBlobStore};
use hodei_trust_policy_storage::DgraphTrustStore;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

#[path = "support/graph_profile.rs"]
mod graph_profile;
use graph_profile::GraphTenant;

const ACCOUNT: &str = "acme";
const OTHER: &str = "other";

/// Without the harness the suite skips, unless the run demands it.
fn harness_available() -> bool {
    if graph_profile::endpoint().is_some() {
        return true;
    }
    assert!(
        std::env::var("HODEI_REQUIRE_IT").as_deref() != Ok("1"),
        "HODEI_REQUIRE_IT=1 but HODEI_GRAPH_IT_ENDPOINT is not set: a skipped IT is a false green"
    );
    eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
    false
}

/// Tokens are `<user>@<account>`; the service derives the account from the
/// verified principal.
struct TokenAuthenticator;

#[async_trait]
impl PrincipalAuthenticator for TokenAuthenticator {
    async fn authenticate(
        &self,
        credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        let Credentials::Bearer(token) = credentials else {
            return Err(AuthenticationError::InvalidCredentials);
        };
        let (user, account) = token
            .split_once('@')
            .ok_or(AuthenticationError::InvalidCredentials)?;
        Ok(PrincipalClaims::new(
            PrincipalId::new(format!("hrn:hodei:iam::{account}:User/{user}")).unwrap(),
            AccountId::new(account).unwrap(),
        ))
    }
}

fn cedar(policy: &str) -> Arc<dyn AuthorizerPort> {
    Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .expect("the policy parses"),
    )))
}

/// alice: reads, publishes, deletes and operates freezes. bob can read and
/// publish but does NOT hold `DeleteAsset`. mallory belongs to another account
/// and holds the deletion in hers.
fn policy() -> String {
    format!(
        r#"
permit(principal == Iam::User::"hrn:hodei:iam::{ACCOUNT}:User/alice",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset",
                  Artifacts::Action::"DeleteAsset",
                  Artifacts::Action::"ManageDependencyFreeze"],
       resource);
permit(principal == Iam::User::"hrn:hodei:iam::{ACCOUNT}:User/bob",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset"],
       resource);
permit(principal == Iam::User::"hrn:hodei:iam::{OTHER}:User/mallory",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset",
                  Artifacts::Action::"DeleteAsset"],
       resource);
"#
    )
}

/// The real Dgraph catalog writer whose deletion can be made to fail, as a
/// graph that goes down between the authorization and the write would.
struct Flaky {
    inner: Arc<DgraphCatalogStore>,
    down: Arc<AtomicBool>,
}

#[async_trait]
impl CatalogWriter for Flaky {
    async fn create_repository(
        &self,
        account: &str,
        descriptor: RepositoryDescriptor,
    ) -> Result<RepositoryDescriptor, CatalogError> {
        self.inner.create_repository(account, descriptor).await
    }
    async fn update_config(
        &self,
        account: &str,
        repo_key: &str,
        config: RepositoryConfig,
    ) -> Result<RepositoryDescriptor, CatalogError> {
        self.inner.update_config(account, repo_key, config).await
    }
    async fn publish(
        &self,
        account: &str,
        publication: Publication,
    ) -> Result<PublishOutcome, CatalogError> {
        self.inner.publish(account, publication).await
    }
    async fn move_tag(
        &self,
        account: &str,
        repository: &str,
        tag_name: &str,
        coordinates: PackageCoordinates,
    ) -> Result<MutableTag, CatalogError> {
        self.inner
            .move_tag(account, repository, tag_name, coordinates)
            .await
    }
    async fn move_asset(
        &self,
        account: &str,
        repository: &str,
        asset_path: &str,
        coordinates: PackageCoordinates,
    ) -> Result<AssetRecord, CatalogError> {
        self.inner
            .move_asset(account, repository, asset_path, coordinates)
            .await
    }
    async fn delete_asset(
        &self,
        account: &str,
        repository: &str,
        asset_path: &str,
    ) -> Result<AssetDeletion, CatalogError> {
        if self.down.load(Ordering::SeqCst) {
            return Err(CatalogError::unavailable("the graph is down"));
        }
        self.inner
            .delete_asset(account, repository, asset_path)
            .await
    }
}

struct Vertical {
    app: Router,
    store: Arc<DgraphCatalogStore>,
    trust: Arc<DgraphTrustStore>,
    cas: Arc<FileBlobStore>,
    gc: Arc<GcService>,
    audit: Arc<InMemoryAuditSink>,
    down: Arc<AtomicBool>,
    #[allow(dead_code)]
    tmp: TempDir,
    #[allow(dead_code)]
    tenant: GraphTenant,
}

async fn mount() -> Option<Vertical> {
    if !harness_available() {
        return None;
    }
    let tenant = GraphTenant::create();
    let tmp = tempfile::tempdir().expect("temp dir");
    let mut config = AppConfig::default();
    config.security.raw_api_enabled = true;
    config.security.admin_api_enabled = true;
    tenant.apply(&mut config, &tmp.path().join("bootstrap-cas"));
    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .expect("bootstrap must succeed");

    let trust = Arc::new(tenant.trust_store().await);
    let store = Arc::new(
        tenant
            .catalog_store()
            .await
            .with_trust_ledger(trust.clone()),
    );
    let down = Arc::new(AtomicBool::new(false));
    let reader: Arc<dyn CatalogReader> = store.clone();
    let writer: Arc<dyn CatalogWriter> = Arc::new(Flaky {
        inner: store.clone(),
        down: down.clone(),
    });
    let searcher: Arc<dyn CatalogSearch> = store.clone();
    let cas = Arc::new(FileBlobStore::new(tmp.path().join("cas")).expect("the CAS"));
    let ledger: Arc<dyn ContentLedger> = store.clone();
    let gc = Arc::new(GcService::new(ledger, cas.clone(), Duration::ZERO).expect("the GC service"));
    let audit = Arc::new(InMemoryAuditSink::new());
    let content: Arc<dyn BlobStore> = cas.clone();
    let state = state
        .with_catalog(reader, writer)
        .with_catalog_search(searcher)
        .with_trust_ledger(trust.clone())
        .with_content_store(content)
        .with_gc(gc.clone())
        .with_audit_sink(audit.clone())
        .with_authenticator(Arc::new(TokenAuthenticator))
        .with_authorization(cedar(&policy()));
    Some(Vertical {
        app: build_router(state, &config),
        store,
        trust,
        cas,
        gc,
        audit,
        down,
        tmp,
        tenant,
    })
}

async fn seed(v: &Vertical, account: &str, key: &str, settings: Value) {
    let mut config = RepositoryConfig::new(DigestScope::Global);
    config.config_settings = settings;
    v.store
        .create_repository(
            account,
            RepositoryDescriptor {
                hrn: repository_hrn("hodei", "artifacts", "eu", account, key),
                account: account.to_string(),
                repo_key: key.to_string(),
                format: RepositoryFormat::Raw,
                mode: RepositoryMode::Hosted,
                config,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        )
        .await
        .expect("the repository is seeded");
}

async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Vec<u8>,
) -> (StatusCode, Vec<u8>) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "127.0.0.1");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if !body.is_empty() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::from(body)).unwrap())
        .await
        .expect("a response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, bytes.to_vec())
}

fn json_of(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or(Value::Null)
}

fn alice() -> String {
    format!("alice@{ACCOUNT}")
}

async fn upload(v: &Vertical, repo: &str, token: &str, path: &str, bytes: &[u8]) {
    let (status, body) = send(
        &v.app,
        Method::PUT,
        &format!("/repository/{repo}/{path}"),
        Some(token),
        bytes.to_vec(),
    )
    .await;
    // 201 creates the path, 200 replaces it (`raw.replace: allow`).
    assert!(
        matches!(status, StatusCode::CREATED | StatusCode::OK),
        "{status}: {}",
        String::from_utf8_lossy(&body)
    );
}

async fn download(v: &Vertical, token: &str, repo: &str, path: &str) -> (StatusCode, Vec<u8>) {
    send(
        &v.app,
        Method::GET,
        &format!("/repository/{repo}/{path}"),
        Some(token),
        Vec::new(),
    )
    .await
}

async fn delete(v: &Vertical, token: &str, repo: &str, path: &str) -> (StatusCode, Vec<u8>) {
    send(
        &v.app,
        Method::DELETE,
        &format!("/repository/{repo}/{path}"),
        Some(token),
        Vec::new(),
    )
    .await
}

/// The raw answer of a search of the repository, and the paths it names.
async fn search_raw(v: &Vertical, token: &str, query: &str) -> (StatusCode, Vec<u8>) {
    send(
        &v.app,
        Method::GET,
        &format!("/api/v1/search?{query}"),
        Some(token),
        Vec::new(),
    )
    .await
}

async fn found(v: &Vertical, token: &str, query: &str) -> Vec<String> {
    let (status, body) = search_raw(v, token, query).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let answer = json_of(&body);
    let mut paths: Vec<String> = answer["results"]
        .as_array()
        .expect("results")
        .iter()
        .map(|hit| hit["asset_path"].as_str().unwrap().to_string())
        .collect();
    paths.sort();
    paths
}

fn digest_of(bytes: &[u8]) -> ContentDigest {
    ContentDigest::of_bytes(bytes)
}

async fn in_cas(v: &Vertical, bytes: &[u8]) -> bool {
    v.cas
        .exists(&digest_of(bytes))
        .await
        .expect("the CAS answers")
}

fn results_of(v: &Vertical) -> Vec<AuditResult> {
    v.audit
        .events()
        .iter()
        .filter(|event| event.action == ACTION)
        .map(|event| event.result)
        .collect()
}

const ACTION: &str = "Artifacts::Action::\"DeleteAsset\"";

const KEEP: &[u8] = b"keep: pinned by a dependency freeze, so the collector must not take it";
const DROP: &[u8] = b"drop: deleted and not pinned by anything, the collector takes it";
const STAY: &[u8] = b"stay: never deleted, the control of the whole case";

#[tokio::test]
async fn a_deleted_asset_leaves_the_search_and_the_download_and_the_collector_honors_holds() {
    let Some(v) = mount().await else { return };
    seed(&v, ACCOUNT, "reg", json!({})).await;
    upload(&v, "reg", &alice(), "keep/k.bin", KEEP).await;
    upload(&v, "reg", &alice(), "drop/d.bin", DROP).await;
    upload(&v, "reg", &alice(), "stay/s.bin", STAY).await;

    // Before: everything is found, served and kept by its own reference.
    assert_eq!(
        found(&v, &alice(), "repository=reg").await,
        ["drop/d.bin", "keep/k.bin", "stay/s.bin"]
    );
    v.gc.sweep_platform("alice", ACCOUNT)
        .await
        .expect("a sweep");
    assert!(in_cas(&v, KEEP).await && in_cas(&v, DROP).await && in_cas(&v, STAY).await);

    // A freeze pins what is under `keep/` (a hold), nothing else.
    let (status, body) = send(
        &v.app,
        Method::POST,
        "/api/v1/freezes",
        Some(&alice()),
        serde_json::to_vec(&json!({"repository": "reg", "prefix": "keep/"})).unwrap(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let freeze = json_of(&body)["id"].as_str().unwrap().to_string();

    // Deleting answers what it released and never touches a byte.
    let (status, body) = delete(&v, &alice(), "reg", "keep/k.bin").await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let answer = json_of(&body);
    assert_eq!(answer["deleted"], true, "{answer}");
    assert_eq!(answer["released"], json!([digest_of(KEEP).to_string()]));
    let (status, _) = delete(&v, &alice(), "reg", "drop/d.bin").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        in_cas(&v, KEEP).await && in_cas(&v, DROP).await,
        "the deletion is logical: the bytes wait for the collector"
    );

    // After: gone from the search (by path listing, text, package and digest)
    // and from the download; the control is untouched.
    assert_eq!(found(&v, &alice(), "repository=reg").await, ["stay/s.bin"]);
    assert!(found(&v, &alice(), "text=d.bin").await.is_empty());
    assert!(found(&v, &alice(), "package=drop/d.bin").await.is_empty());
    assert!(
        found(&v, &alice(), &format!("digest={}", digest_of(DROP)))
            .await
            .is_empty()
    );
    assert_eq!(
        download(&v, &alice(), "reg", "drop/d.bin").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        download(&v, &alice(), "reg", "keep/k.bin").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        download(&v, &alice(), "reg", "stay/s.bin").await,
        (StatusCode::OK, STAY.to_vec())
    );

    // The collector reclaims what nothing names and keeps what a hold names.
    let report =
        v.gc.sweep_platform("alice", ACCOUNT)
            .await
            .expect("a sweep");
    assert!(report.deleted >= 1, "{report:?}");
    assert!(!in_cas(&v, DROP).await, "no root and no hold: reclaimed");
    assert!(in_cas(&v, KEEP).await, "the freeze's hold keeps the bytes");
    assert!(in_cas(&v, STAY).await, "the control is never reclaimed");

    // Releasing the freeze hands the bytes back to the general rule.
    let (status, body) = send(
        &v.app,
        Method::POST,
        "/api/v1/freezes/unpin",
        Some(&alice()),
        serde_json::to_vec(&json!({"id": freeze})).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    v.gc.sweep_platform("alice", ACCOUNT)
        .await
        .expect("a sweep");
    assert!(!in_cas(&v, KEEP).await, "released: reclaimed");
    assert!(in_cas(&v, STAY).await);

    // Two deletions were facts and were said; nothing else was.
    assert_eq!(results_of(&v), vec![AuditResult::Allowed; 2]);
}

#[tokio::test]
async fn without_the_dedicated_action_nothing_is_deleted_and_the_denial_is_audited() {
    let Some(v) = mount().await else { return };
    seed(&v, ACCOUNT, "reg", json!({})).await;
    upload(&v, "reg", &alice(), "a/one.bin", KEEP).await;
    let bob = format!("bob@{ACCOUNT}");

    // bob uploads and downloads, but neither implies deleting.
    let (status, _) = delete(&v, &bob, "reg", "a/one.bin").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send(
        &v.app,
        Method::DELETE,
        "/repository/reg/a/one.bin",
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    assert_eq!(found(&v, &alice(), "repository=reg").await, ["a/one.bin"]);
    assert_eq!(
        download(&v, &bob, "reg", "a/one.bin").await,
        (StatusCode::OK, KEEP.to_vec())
    );
    assert_eq!(results_of(&v), vec![AuditResult::Denied]);

    // Control: the same request with the action is the deletion.
    let (status, _) = delete(&v, &alice(), "reg", "a/one.bin").await;
    assert_eq!(status, StatusCode::OK);
    assert!(found(&v, &alice(), "repository=reg").await.is_empty());
}

#[tokio::test]
async fn another_account_neither_deletes_nor_learns_anything() {
    let Some(v) = mount().await else { return };
    seed(&v, ACCOUNT, "reg", json!({})).await;
    upload(&v, "reg", &alice(), "secret/plan.bin", KEEP).await;
    let mallory = format!("mallory@{OTHER}");

    // The stranger's search is empty and stays byte-identical when the owner
    // deletes: not a name, not a total, not a timestamp moves.
    let before = search_raw(&v, &mallory, "").await;
    assert_eq!(before.0, StatusCode::OK);
    assert_eq!(json_of(&before.1)["total"], 0);
    assert!(
        !String::from_utf8_lossy(&before.1).contains("plan.bin"),
        "no name of another account"
    );

    // A repository of the owner is, for the stranger, the same 404 as one that
    // never existed, whatever she holds in her own account.
    let foreign = delete(&v, &mallory, "reg", "secret/plan.bin").await;
    let unknown = delete(&v, &mallory, "never-existed", "secret/plan.bin").await;
    assert_eq!(foreign.0, StatusCode::NOT_FOUND);
    assert_eq!(
        (foreign.0, &foreign.1),
        (unknown.0, &unknown.1),
        "indistinguishable from absence"
    );
    // Same-named repository in HER account: it is hers, the path is not there,
    // and the owner's asset is untouched.
    seed(&v, OTHER, "reg", json!({})).await;
    let (status, body) = delete(&v, &mallory, "reg", "secret/plan.bin").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json_of(&body)["deleted"], false);
    assert_eq!(
        download(&v, &alice(), "reg", "secret/plan.bin").await,
        (StatusCode::OK, KEEP.to_vec())
    );

    let (status, _) = delete(&v, &alice(), "reg", "secret/plan.bin").await;
    assert_eq!(status, StatusCode::OK);
    let after = search_raw(&v, &mallory, "").await;
    assert_eq!(
        before, after,
        "the owner's deletion is invisible to the stranger"
    );
}

#[tokio::test]
async fn deleting_twice_is_harmless_and_the_path_can_be_published_and_deleted_again() {
    let Some(v) = mount().await else { return };
    // `raw.replace: allow` leaves the replaced content as a version of the path.
    seed(&v, ACCOUNT, "reg", json!({"raw": {"replace": "allow"}})).await;
    let first: &[u8] = b"first release of the path";
    let second: &[u8] = b"second release of the path";
    upload(&v, "reg", &alice(), "lib/x.bin", first).await;
    upload(&v, "reg", &alice(), "lib/x.bin", second).await;
    upload(&v, "reg", &alice(), "lib/y.bin", STAY).await;

    let (status, body) = delete(&v, &alice(), "reg", "lib/x.bin").await;
    assert_eq!(status, StatusCode::OK);
    let answer = json_of(&body);
    assert_eq!(answer["deleted"], true);
    assert_eq!(
        answer["versions_removed"], 2,
        "the history goes with the path"
    );
    assert_eq!(found(&v, &alice(), "repository=reg").await, ["lib/y.bin"]);

    // Again: nothing to remove, nothing written, no new fact.
    let (status, body) = delete(&v, &alice(), "reg", "lib/x.bin").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json_of(&body)["deleted"], false);
    assert_eq!(results_of(&v), vec![AuditResult::Allowed]);

    // Both versions are reclaimable (no row names them); the sibling is not.
    v.gc.sweep_platform("alice", ACCOUNT)
        .await
        .expect("a sweep");
    assert!(!in_cas(&v, first).await && !in_cas(&v, second).await);
    assert!(in_cas(&v, STAY).await);

    // The path is free again: publish the same bytes and delete them again
    // (the once-only marker of the first deletion must not swallow this one).
    upload(&v, "reg", &alice(), "lib/x.bin", first).await;
    assert_eq!(
        found(&v, &alice(), "repository=reg").await,
        ["lib/x.bin", "lib/y.bin"]
    );
    assert_eq!(
        download(&v, &alice(), "reg", "lib/x.bin").await,
        (StatusCode::OK, first.to_vec())
    );
    let (_, body) = delete(&v, &alice(), "reg", "lib/x.bin").await;
    assert_eq!(json_of(&body)["deleted"], true);
    assert_eq!(found(&v, &alice(), "repository=reg").await, ["lib/y.bin"]);
}

#[tokio::test]
async fn a_failing_store_fails_closed_and_hides_nothing() {
    let Some(v) = mount().await else { return };
    seed(&v, ACCOUNT, "reg", json!({})).await;
    upload(&v, "reg", &alice(), "a/one.bin", KEEP).await;

    v.down.store(true, Ordering::SeqCst);
    let (status, _) = delete(&v, &alice(), "reg", "a/one.bin").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        found(&v, &alice(), "repository=reg").await,
        ["a/one.bin"],
        "a deletion that failed does not hide the asset"
    );
    assert_eq!(
        download(&v, &alice(), "reg", "a/one.bin").await,
        (StatusCode::OK, KEEP.to_vec())
    );
    assert_eq!(results_of(&v), vec![AuditResult::Failed]);

    // The graph comes back: the same request now does what it says.
    v.down.store(false, Ordering::SeqCst);
    let (status, _) = delete(&v, &alice(), "reg", "a/one.bin").await;
    assert_eq!(status, StatusCode::OK);
    assert!(found(&v, &alice(), "repository=reg").await.is_empty());
}

fn grant(digest: &ContentDigest, decision: &str) -> TrustGrant {
    TrustGrant {
        account: ACCOUNT.to_string(),
        repository: "tools".to_string(),
        digest: digest.clone(),
        decision_id: decision.to_string(),
        actor: format!("hrn:hodei:iam::{ACCOUNT}:User/alice"),
        granted_at: Utc::now(),
        revoked_at: None,
        revoked_by: None,
        revoke_reason: None,
        subject: None,
        policy_version: Some("1.0.0".to_string()),
        capsule_hash: None,
        risk_version: None,
        valid_until: Some(Utc::now() + chrono::Duration::hours(1)),
    }
}

#[tokio::test]
async fn promotion_and_reevaluation_are_reflected_by_the_search_and_a_deleted_asset_stays_gone() {
    let Some(v) = mount().await else { return };
    seed(&v, ACCOUNT, "tools", json!({"trust": {"serve": "trusted"}})).await;
    upload(&v, "tools", &alice(), "bin/tool", KEEP).await;
    upload(&v, "tools", &alice(), "bin/other", DROP).await;
    let (kept, other) = (digest_of(KEEP), digest_of(DROP));
    let trusted = "repository=tools&attribute=channel%3Dtrusted";

    // Nobody promoted anything: the trusted channel hides both, in the search
    // and in the download.
    assert!(found(&v, &alice(), trusted).await.is_empty());
    assert_eq!(
        download(&v, &alice(), "tools", "bin/tool").await.0,
        StatusCode::NOT_FOUND
    );

    // Promotion: the next search lists it, the next download serves it.
    v.trust
        .grant(&[grant(&kept, "dec-1"), grant(&other, "dec-1")])
        .await
        .expect("the promotion");
    assert_eq!(
        found(&v, &alice(), trusted).await,
        ["bin/other", "bin/tool"]
    );
    assert_eq!(
        download(&v, &alice(), "tools", "bin/tool").await,
        (StatusCode::OK, KEEP.to_vec())
    );

    // Re-evaluation (the revocation of a decision for the account, the call
    // the reaction to a new advisory makes): both vanish at once.
    let revoked = v
        .trust
        .revoke_decision_for_account(ACCOUNT, "dec-1", "reactor", "re-evaluated", Utc::now())
        .await
        .expect("the re-evaluation");
    assert_eq!(revoked, 2);
    assert!(found(&v, &alice(), trusted).await.is_empty());
    assert_eq!(
        download(&v, &alice(), "tools", "bin/tool").await.0,
        StatusCode::NOT_FOUND
    );

    // Promoted again under a new decision, then deleted: a live grant does not
    // resurrect a deleted asset, and the other one is untouched.
    v.trust
        .grant(&[grant(&kept, "dec-2"), grant(&other, "dec-2")])
        .await
        .expect("the second promotion");
    assert_eq!(found(&v, &alice(), trusted).await.len(), 2);
    let (status, _) = delete(&v, &alice(), "tools", "bin/tool").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(found(&v, &alice(), trusted).await, ["bin/other"]);
}
