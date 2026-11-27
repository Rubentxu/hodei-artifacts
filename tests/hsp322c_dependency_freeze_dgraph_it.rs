//! HSP-322 tramo c (from HSP-161): the dependency freeze, through the real
//! router over the real Dgraph catalog, a real file CAS, the real Cedar engine
//! and the real garbage collector.
//!
//! A freeze is an immutable view named by the digest of its manifest. What this
//! suite proves, with the origin moving underneath it:
//!
//! - the freeze answers the same collection and the same bytes after the origin
//!   publishes a new version and moves `latest`; a path the freeze does not name
//!   is an explicit `404 E_MISSING_DEPENDENCY`, never the newer version;
//! - without the dedicated Cedar action nobody can create or release a freeze,
//!   and a freeze of another account does not exist for the caller;
//! - the pin holds: a blob only the freeze still references is retained by a
//!   real sweep while the freeze is in force, and is reclaimed after `unpin`
//!   (and after expiry) while a blob the catalog still references is not;
//! - a digest the trusted channel of the origin stops admitting (revoked) is not
//!   served by the freeze, and one it never admitted is not frozen.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` (with `HODEI_REQUIRE_IT=1` a missing
//! harness fails instead of skipping); serialize with `--test-threads=1`.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use base64::Engine as _;
use cedar_policy::Entities;
use chrono::Utc;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::freeze::FreezeManifest;
use hodei_artifacts_api::gc::GcService;
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use hodei_catalog::{
    AllowAllAuthorizer, CatalogReader, CatalogWriter, ContentLedger, DigestScope, RepositoryConfig,
    RepositoryDescriptor, RepositoryFormat, RepositoryMode, TrustGrant, TrustLedger,
    repository_hrn,
};
use hodei_catalog_storage::DgraphCatalogStore;
use hodei_evidence::ContentDigest;
use hodei_graph_client::{GraphClient, GraphClientConfig, Tenant};
use hodei_hub::blob::{BlobStore, FileBlobStore};
use hodei_trust_policy_storage::DgraphTrustStore;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

fn config_from_env() -> Option<GraphClientConfig> {
    let endpoint = std::env::var("HODEI_GRAPH_IT_ENDPOINT").ok()?;
    let user = std::env::var("HODEI_GRAPH_IT_USER").unwrap_or_default();
    let password = std::env::var("HODEI_GRAPH_IT_PASSWORD").unwrap_or_default();
    let mut text =
        format!("endpoint = \"{endpoint}\"\n\n[tls]\nenabled = false\nca_cert_path = \"\"\n");
    if !user.is_empty() {
        text.push_str(&format!(
            "\n[credentials]\nuserid = \"{user}\"\npassword = \"{password}\"\n"
        ));
    }
    Some(GraphClientConfig::from_toml_str(&text).expect("the IT configuration validates"))
}

/// Without the harness the suite skips, unless the run demands it.
fn skip_or_fail() {
    assert!(
        std::env::var("HODEI_REQUIRE_IT").as_deref() != Ok("1"),
        "HODEI_REQUIRE_IT=1 but HODEI_GRAPH_IT_ENDPOINT is not set: a skipped IT is a false green"
    );
    eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
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

/// alice: reads, publishes, operates freezes and the GC. bob: reads and
/// publishes only (he does NOT hold `ManageDependencyFreeze`).
fn policy(account: &str) -> String {
    format!(
        r#"
permit(principal == Iam::User::"hrn:hodei:iam::{account}:User/alice",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset",
                  Artifacts::Action::"CreateRepository",
                  Artifacts::Action::"ManageDependencyFreeze"],
       resource);
permit(principal == Iam::User::"hrn:hodei:iam::{account}:User/alice",
       action == Admin::Action::"AccessAdminApi", resource);
permit(principal == Iam::User::"hrn:hodei:iam::{account}:User/bob",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset"],
       resource);
"#
    )
}

struct Vertical {
    app: Router,
    store: Arc<DgraphCatalogStore>,
    cas: Arc<FileBlobStore>,
    trust: Arc<DgraphTrustStore>,
    #[allow(dead_code)]
    tmp: TempDir,
}

async fn mount(account: &str) -> Option<Vertical> {
    let Some(graph) = config_from_env() else {
        skip_or_fail();
        return None;
    };
    let mut client = GraphClient::connect(graph.clone())
        .await
        .expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    let client = Arc::new(client);
    let store = Arc::new(
        DgraphCatalogStore::open(client.clone(), Arc::new(AllowAllAuthorizer))
            .await
            .expect("the dgraph catalog store opens"),
    );
    let tenant = Tenant::new(&format!("hsp322c-{}", nonce())).expect("a valid tenant");
    let trust = Arc::new(
        DgraphTrustStore::open(client, tenant)
            .await
            .expect("the trust ledger opens"),
    );
    let tmp = tempfile::tempdir().expect("temp dir");
    let mut config = AppConfig::default();
    config.security.raw_api_enabled = true;
    config.security.admin_api_enabled = true;
    config.graph = Some(graph);
    config.content.cas_path = tmp
        .path()
        .join("bootstrap-cas")
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
    .expect("bootstrap must succeed");
    let reader: Arc<dyn CatalogReader> = store.clone();
    let writer: Arc<dyn CatalogWriter> = store.clone();
    let cas = Arc::new(FileBlobStore::new(tmp.path().join("cas")).expect("the CAS"));
    let content: Arc<dyn BlobStore> = cas.clone();
    let ledger: Arc<dyn ContentLedger> = store.clone();
    // Zero grace: what the pin does not protect is reclaimable at once, so a
    // retained blob is retained BECAUSE of the freeze and not of the grace.
    let gc = GcService::new(ledger, content.clone(), Duration::ZERO).expect("the GC service");
    let ledger: Arc<dyn TrustLedger> = trust.clone();
    let state = state
        .with_catalog(reader, writer)
        .with_content_store(content)
        .with_gc(Arc::new(gc))
        .with_trust_ledger(ledger)
        .with_authenticator(Arc::new(TokenAuthenticator))
        .with_authorization(cedar(&policy(account)));
    Some(Vertical {
        app: build_router(state, &config),
        store,
        cas,
        trust,
        tmp,
    })
}

async fn seed(v: &Vertical, account: &str, key: &str, format: RepositoryFormat, settings: Value) {
    let mut config = RepositoryConfig::new(DigestScope::Global);
    config.config_settings = settings;
    v.store
        .create_repository(
            account,
            RepositoryDescriptor {
                hrn: repository_hrn("hodei", "artifacts", "eu", account, key),
                account: account.to_string(),
                repo_key: key.to_string(),
                format,
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
    token: &str,
    body: Vec<u8>,
) -> (StatusCode, Vec<u8>) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "127.0.0.1")
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
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

/// The body of an `npm publish`: a version and its tarball in base64.
fn publication(name: &str, version: &str) -> Vec<u8> {
    let tarball = format!("tarball of {name}@{version}");
    let file = format!("{name}-{version}.tgz");
    serde_json::to_vec(&json!({
        "_id": name,
        "name": name,
        "dist-tags": { "latest": version },
        "versions": { version: { "name": name, "version": version, "_id": format!("{name}@{version}") } },
        "_attachments": { file: {
            "content_type": "application/octet-stream",
            "data": base64::engine::general_purpose::STANDARD.encode(tarball.as_bytes()),
            "length": tarball.len(),
        } },
    }))
    .unwrap()
}

async fn publish(v: &Vertical, repo: &str, token: &str, name: &str, version: &str) {
    let (status, body) = send(
        &v.app,
        Method::PUT,
        &format!("/npm/{repo}/{name}"),
        token,
        publication(name, version),
    )
    .await;
    assert!(
        status.is_success(),
        "publish {name}@{version}: {status} {}",
        String::from_utf8_lossy(&body)
    );
}

async fn freeze(v: &Vertical, token: &str, body: Value) -> (StatusCode, Value) {
    let (status, bytes) = send(
        &v.app,
        Method::POST,
        "/api/v1/freezes",
        token,
        serde_json::to_vec(&body).unwrap(),
    )
    .await;
    (status, json_of(&bytes))
}

async fn unpin(v: &Vertical, token: &str, id: &str) -> StatusCode {
    send(
        &v.app,
        Method::POST,
        "/api/v1/freezes/unpin",
        token,
        serde_json::to_vec(&json!({"id": id})).unwrap(),
    )
    .await
    .0
}

async fn frozen(v: &Vertical, token: &str, id: &str, path: &str) -> (StatusCode, Vec<u8>) {
    send(
        &v.app,
        Method::GET,
        &format!("/api/v1/freezes/{id}/content/{path}"),
        token,
        Vec::new(),
    )
    .await
}

async fn sweep(v: &Vertical, token: &str) -> Value {
    let (status, bytes) = send(
        &v.app,
        Method::POST,
        "/api/v1/gc/sweep",
        token,
        br#"{"execute":true}"#.to_vec(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    json_of(&bytes)
}

async fn plan_verdict(v: &Vertical, token: &str, digest: &str) -> Value {
    let (status, bytes) = send(&v.app, Method::GET, "/api/v1/gc/plan", token, Vec::new()).await;
    assert_eq!(status, StatusCode::OK);
    json_of(&bytes)["entries"]
        .as_array()
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry["digest"] == digest)
                .cloned()
        })
        .unwrap_or(Value::Null)
}

/// The digest the catalog says the packument of `left-pad` has right now: the
/// stored blob, not the rewritten document the origin answers to clients.
async fn stored_packument(v: &Vertical, account: &str) -> ContentDigest {
    v.store
        .asset(account, "reg", PACKUMENT)
        .await
        .expect("the packument asset")
        .digest
}

fn digest_of(bytes: &[u8]) -> ContentDigest {
    ContentDigest::of_bytes(bytes)
}

const PACKUMENT: &str = "left-pad";
const TARBALL_1: &str = "left-pad/-/left-pad-1.0.0.tgz";
const TARBALL_2: &str = "left-pad/-/left-pad-1.1.0.tgz";

#[tokio::test]
async fn the_freeze_answers_the_same_collection_after_the_origin_moves() {
    let account = format!("f{}a", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    seed(&v, &account, "reg", RepositoryFormat::Npm, json!({})).await;
    let alice = format!("alice@{account}");
    publish(&v, "reg", &alice, "left-pad", "1.0.0").await;

    let packument_before = stored_packument(&v, &account).await;
    let tarball_before = send(
        &v.app,
        Method::GET,
        &format!("/npm/reg/{TARBALL_1}"),
        &alice,
        Vec::new(),
    )
    .await
    .1;
    let (status, created) = freeze(&v, &alice, json!({"repository": "reg"})).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().expect("an id").to_string();
    assert_eq!(created["entries"], 2, "packument + tarball: {created}");

    // The origin moves: a new version and `latest` follows it.
    publish(&v, "reg", &alice, "left-pad", "1.1.0").await;
    let origin_after = send(&v.app, Method::GET, "/npm/reg/left-pad", &alice, Vec::new())
        .await
        .1;
    assert_ne!(
        stored_packument(&v, &account).await,
        packument_before,
        "the origin packument moved"
    );
    assert!(String::from_utf8_lossy(&origin_after).contains("1.1.0"));

    // The freeze did not: same bytes, same digests, same collection.
    let (status, manifest_bytes) = send(
        &v.app,
        Method::GET,
        &format!("/api/v1/freezes/{id}"),
        &alice,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let manifest = json_of(&manifest_bytes);
    let paths: Vec<&str> = manifest["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, [PACKUMENT, TARBALL_1]);
    let (status, packument_bytes) = frozen(&v, &alice, &id, PACKUMENT).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        digest_of(&packument_bytes),
        packument_before,
        "the frozen packument is the stored one"
    );
    assert_eq!(
        frozen(&v, &alice, &id, TARBALL_1).await,
        (StatusCode::OK, tarball_before.clone())
    );
    let entry = &manifest["entries"][1];
    assert_eq!(
        entry["digest"],
        format!("sha256:{}", digest_of(&tarball_before).value)
    );
    assert_eq!(entry["version"], "1.0.0");
    assert!(
        !String::from_utf8_lossy(&frozen(&v, &alice, &id, PACKUMENT).await.1).contains("1.1.0")
    );

    // Absent is an explicit error, never the newer version nor `latest`.
    let (status, body) = frozen(&v, &alice, &id, TARBALL_2).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        json_of(&body)["error"],
        "E_MISSING_DEPENDENCY",
        "{}",
        String::from_utf8_lossy(&body)
    );
    let (status, body) = frozen(&v, &alice, &id, "left-pad/latest").await;
    assert_eq!(
        (status, json_of(&body)["error"].as_str()),
        (StatusCode::NOT_FOUND, Some("E_MISSING_DEPENDENCY"))
    );

    // A freeze of this account does not exist for another account.
    let other = format!("alice@x{account}");
    assert_eq!(
        frozen(&v, &other, &id, TARBALL_1).await.0,
        StatusCode::NOT_FOUND
    );
    // Nor for a malformed id.
    assert_eq!(
        frozen(&v, &alice, "not-an-id", TARBALL_1).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn nobody_creates_or_releases_a_freeze_without_the_dedicated_action() {
    let account = format!("f{}a", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    seed(&v, &account, "reg", RepositoryFormat::Npm, json!({})).await;
    let alice = format!("alice@{account}");
    let bob = format!("bob@{account}");
    publish(&v, "reg", &alice, "left-pad", "1.0.0").await;

    // Control: bob can publish and read, and still cannot freeze.
    publish(&v, "reg", &bob, "other", "1.0.0").await;
    assert_eq!(
        freeze(&v, &bob, json!({"repository": "reg"})).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        freeze(&v, "nobody", json!({"repository": "reg"})).await.0,
        StatusCode::UNAUTHORIZED
    );
    // The same request with the action is created (the mutation control).
    let (status, created) = freeze(&v, &alice, json!({"repository": "reg"})).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().unwrap().to_string();

    // Bob may read the frozen bytes (DownloadAsset) but not release the pin.
    assert_eq!(frozen(&v, &bob, &id, TARBALL_1).await.0, StatusCode::OK);
    assert_eq!(unpin(&v, &bob, &id).await, StatusCode::FORBIDDEN);
    assert_eq!(
        frozen(&v, &alice, &id, TARBALL_1).await.0,
        StatusCode::OK,
        "still pinned"
    );
    assert_eq!(unpin(&v, &alice, &id).await, StatusCode::OK);
}

#[tokio::test]
async fn the_pin_stops_the_gc_until_unpin_and_the_catalog_references_keep_what_they_name() {
    let account = format!("f{}a", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    seed(&v, &account, "reg", RepositoryFormat::Npm, json!({})).await;
    let alice = format!("alice@{account}");
    publish(&v, "reg", &alice, "left-pad", "1.0.0").await;
    let old_packument = stored_packument(&v, &account).await;
    let tarball = digest_of(
        &send(
            &v.app,
            Method::GET,
            &format!("/npm/reg/{TARBALL_1}"),
            &alice,
            Vec::new(),
        )
        .await
        .1,
    );

    let (status, created) = freeze(&v, &alice, json!({"repository": "reg"})).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().unwrap().to_string();
    // The origin publishes again and moves `latest`.
    publish(&v, "reg", &alice, "left-pad", "1.1.0").await;

    let name = format!("sha256:{}", old_packument.value);
    let pinned = plan_verdict(&v, &alice, &name).await;
    assert_eq!(pinned["verdict"], "retained", "{pinned}");
    assert!(
        pinned["reasons"].to_string().contains("\"kind\":\"hold\""),
        "the plan says the freeze is why: {pinned}"
    );
    sweep(&v, &alice).await;
    assert!(v.cas.exists(&old_packument).await.unwrap());
    assert!(v.cas.exists(&tarball).await.unwrap());
    assert_eq!(frozen(&v, &alice, &id, PACKUMENT).await.0, StatusCode::OK);

    // Release: only the freeze's holds go. The census decides the rest, and it
    // still finds the catalog's own reference to the content: kept.
    assert_eq!(unpin(&v, &alice, &id).await, StatusCode::OK);
    let released = plan_verdict(&v, &alice, &name).await;
    assert!(
        !released["reasons"]
            .to_string()
            .contains("\"kind\":\"hold\""),
        "the freeze no longer pins: {released}"
    );
    assert_eq!(
        released["verdict"], "retained",
        "other reference: {released}"
    );
    sweep(&v, &alice).await;
    assert!(
        v.cas.exists(&old_packument).await.unwrap(),
        "referenced by the catalog"
    );
    assert!(
        v.cas.exists(&tarball).await.unwrap(),
        "referenced by the catalog"
    );
    // The freeze itself is over: its manifest was only pinned by the freeze.
    assert_eq!(
        frozen(&v, &alice, &id, PACKUMENT).await.0,
        StatusCode::NOT_FOUND,
        "the freeze is gone"
    );
    assert_eq!(unpin(&v, &alice, &id).await, StatusCode::NOT_FOUND);
}

/// The catalog has no deletion yet, so a blob that only a freeze references is
/// built by hand: bytes in the CAS that no catalog row names, pinned by the
/// holds of two overlapping freezes, through the real GC service and ledger.
#[tokio::test]
async fn a_blob_only_the_freezes_reference_is_reclaimed_after_the_last_one_releases() {
    let account = format!("f{}a", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    let alice = format!("alice@{account}");
    let bytes = format!("only a freeze names me {}", nonce()).into_bytes();
    let digest = v.cas.put(&bytes).await.expect("the orphan is stored");
    let name = format!("sha256:{}", digest.value);
    let record = hodei_catalog::AssetRecord {
        account: account.clone(),
        repository: "reg".to_string(),
        asset_path: "x".to_string(),
        coordinates: hodei_catalog::PackageCoordinates {
            package_name: "x".to_string(),
            package_version: "1".to_string(),
        },
        digest: digest.clone(),
        size_bytes: bytes.len() as u64,
        subject: None,
        updated_at: Utc::now(),
    };
    let manifest = |by: &str| {
        FreezeManifest::build(
            &account,
            "reg",
            by,
            "alice",
            Utc::now(),
            None,
            std::slice::from_ref(&record),
        )
        .expect("a manifest")
    };
    let (first, second) = (manifest("a"), manifest("b"));
    assert_ne!(first.id(), second.id());
    let ledger: Arc<dyn ContentLedger> = v.store.clone();
    let gc =
        Arc::new(GcService::new(ledger, v.cas.clone(), Duration::ZERO).expect("the GC service"));
    gc.place_holds(&first.holds()).await.unwrap();
    gc.place_holds(&second.holds()).await.unwrap();

    let verdict = |plan: &hodei_artifacts_api::gc::Plan| {
        let entry = plan.entries.iter().find(|entry| entry.digest == name);
        format!("{:?}", entry.map(|entry| &entry.verdict))
    };
    assert!(verdict(&gc.dry_run().await.unwrap()).contains("Retained"));
    gc.sweep_platform("alice", &account).await.unwrap();
    assert!(v.cas.exists(&digest).await.unwrap(), "pinned twice: kept");

    // Releasing one freeze leaves the other's pin: the other reference holds.
    assert_eq!(
        gc.release_holds_with_prefix(&hodei_artifacts_api::freeze::hold_prefix(&first.id()))
            .await
            .unwrap(),
        2,
        "manifest hold + content hold, and nothing of the other freeze"
    );
    assert!(verdict(&gc.dry_run().await.unwrap()).contains("Retained"));
    gc.sweep_platform("alice", &account).await.unwrap();
    assert!(
        v.cas.exists(&digest).await.unwrap(),
        "still pinned by the second freeze"
    );

    // The last release hands the blob back to the general rule: reclaimed.
    gc.release_holds_with_prefix(&hodei_artifacts_api::freeze::hold_prefix(&second.id()))
        .await
        .unwrap();
    assert!(verdict(&gc.dry_run().await.unwrap()).contains("Reclaimable"));
    let report = gc.sweep_platform("alice", &account).await.unwrap();
    assert!(report.deleted >= 1, "{report:?}");
    assert!(
        !v.cas.exists(&digest).await.unwrap(),
        "no pin and no root: reclaimed"
    );
    let _ = alice;
}

#[tokio::test]
async fn an_expired_freeze_stops_pinning_and_serving() {
    let account = format!("f{}a", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    seed(&v, &account, "reg", RepositoryFormat::Npm, json!({})).await;
    let alice = format!("alice@{account}");
    publish(&v, "reg", &alice, "left-pad", "1.0.0").await;
    let old_packument = stored_packument(&v, &account).await;

    assert_eq!(
        freeze(&v, &alice, json!({"repository": "reg", "ttl_secs": 0}))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let (status, created) = freeze(&v, &alice, json!({"repository": "reg", "ttl_secs": 2})).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().unwrap().to_string();
    publish(&v, "reg", &alice, "left-pad", "1.1.0").await;
    sweep(&v, &alice).await;
    assert!(
        v.cas.exists(&old_packument).await.unwrap(),
        "in force: pinned"
    );

    tokio::time::sleep(Duration::from_millis(2300)).await;
    assert_eq!(
        frozen(&v, &alice, &id, PACKUMENT).await.0,
        StatusCode::GONE,
        "past its lifetime"
    );
    let after = plan_verdict(&v, &alice, &format!("sha256:{}", old_packument.value)).await;
    assert!(
        !after["reasons"].to_string().contains("\"kind\":\"hold\""),
        "expired: the freeze no longer pins: {after}"
    );
    sweep(&v, &alice).await;
    assert_eq!(
        frozen(&v, &alice, &id, PACKUMENT).await.0,
        StatusCode::GONE,
        "the lapsed freeze lets its manifest be reclaimed and stays expired"
    );
}

#[tokio::test]
async fn a_revoked_digest_is_not_served_in_trusted_and_an_ungranted_one_is_not_frozen() {
    let account = format!("f{}a", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    seed(
        &v,
        &account,
        "gated",
        RepositoryFormat::Raw,
        json!({"trust": {"serve": "trusted"}}),
    )
    .await;
    let alice = format!("alice@{account}");
    let files = [
        ("a.txt", b"alpha".as_slice()),
        ("b.txt", b"bravo"),
        ("c.txt", b"charlie"),
    ];
    for (name, bytes) in files {
        let (status, body) = send(
            &v.app,
            Method::PUT,
            &format!("/repository/gated/{name}"),
            &alice,
            bytes.to_vec(),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "{}",
            String::from_utf8_lossy(&body)
        );
    }
    // Promote a and b only.
    let grant = |bytes: &[u8], decision: &str| TrustGrant {
        account: account.clone(),
        repository: "gated".to_string(),
        digest: digest_of(bytes),
        decision_id: decision.to_string(),
        actor: "alice".to_string(),
        granted_at: Utc::now(),
        revoked_at: None,
        revoked_by: None,
        revoke_reason: None,
        subject: Some("hrn:hodei:artifacts:eu:acme:Artifact/x".to_string()),
        policy_version: Some("1".to_string()),
        capsule_hash: Some("sha256:cap".to_string()),
        risk_version: Some(1),
        valid_until: Some(Utc::now() + chrono::Duration::hours(1)),
    };
    v.trust
        .grant(&[grant(b"alpha", "d-a"), grant(b"bravo", "d-b")])
        .await
        .unwrap();

    let (status, created) = freeze(&v, &alice, json!({"repository": "gated"})).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(
        created["entries"], 2,
        "the ungranted digest is not frozen: {created}"
    );
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(
        frozen(&v, &alice, &id, "a.txt").await,
        (StatusCode::OK, b"alpha".to_vec())
    );
    assert_eq!(
        frozen(&v, &alice, &id, "b.txt").await,
        (StatusCode::OK, b"bravo".to_vec())
    );
    let (status, body) = frozen(&v, &alice, &id, "c.txt").await;
    assert_eq!(
        (status, json_of(&body)["error"].as_str()),
        (StatusCode::NOT_FOUND, Some("E_MISSING_DEPENDENCY"))
    );

    // Revoke b: the freeze keeps the identity but does not serve it.
    let revoked = v
        .trust
        .revoke_digest(
            &account,
            "gated",
            &digest_of(b"bravo"),
            "alice",
            "withdrawn",
            Utc::now(),
        )
        .await
        .unwrap();
    assert_eq!(revoked, 1);
    let (status, body) = frozen(&v, &alice, &id, "b.txt").await;
    assert_eq!(
        (status, json_of(&body)["error"].as_str()),
        (StatusCode::FORBIDDEN, Some("E_WITHHELD")),
        "{}",
        String::from_utf8_lossy(&body)
    );
    assert_eq!(
        frozen(&v, &alice, &id, "a.txt").await,
        (StatusCode::OK, b"alpha".to_vec()),
        "the rest is untouched"
    );
    // The manifest still names it: identity is historical, permission is today's.
    let (_, manifest) = send(
        &v.app,
        Method::GET,
        &format!("/api/v1/freezes/{id}"),
        &alice,
        Vec::new(),
    )
    .await;
    assert_eq!(json_of(&manifest)["entries"].as_array().unwrap().len(), 2);
}
