//! HSP-322 tramo d (from HSP-165): the repository time machine (a read-only
//! comparison of two freezes), through the real router over the real Dgraph
//! catalog, a real file CAS, the real Cedar engine and the real garbage collector.
//!
//! What this suite proves:
//!
//! - two freezes of one repository explain their difference with the historical
//!   facts each manifest recorded; a digest whose bytes the collector reclaimed
//!   is reported as `gone` and nothing is reconstructed;
//! - the read writes nothing: holds, trust grants, catalog and content store are
//!   identical before and after (and the same probe DOES see a real write, so it
//!   is not blind);
//! - without the dedicated Cedar action there is no comparison; a freeze of
//!   another account does not exist for the caller; freezes of different
//!   repositories are not comparable;
//! - the listing is bounded while the totals stay exact;
//! - a content store that fails makes the whole answer a 503, never a partial
//!   one;
//! - today's trusted-channel permission (`admitted_now`) is reported apart from
//!   the historical identity.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` (with `HODEI_REQUIRE_IT=1` a missing
//! harness fails instead of skipping); serialize with `--test-threads=1`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
use hodei_hub::blob::{BlobError, BlobStore, FileBlobStore, ReadPolicy};
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

/// alice: reads, publishes, operates freezes, the history and the GC. bob can
/// freeze but does NOT hold `ExploreRepositoryHistory`. mallory belongs to
/// another account and holds every repository action there.
fn policy(account: &str, other: &str) -> String {
    format!(
        r#"
permit(principal == Iam::User::"hrn:hodei:iam::{account}:User/alice",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset",
                  Artifacts::Action::"CreateRepository",
                  Artifacts::Action::"ManageDependencyFreeze",
                  Artifacts::Action::"ExploreRepositoryHistory"],
       resource);
permit(principal == Iam::User::"hrn:hodei:iam::{account}:User/alice",
       action == Admin::Action::"AccessAdminApi", resource);
permit(principal == Iam::User::"hrn:hodei:iam::{account}:User/bob",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset",
                  Artifacts::Action::"ManageDependencyFreeze"],
       resource);
permit(principal == Iam::User::"hrn:hodei:iam::{other}:User/mallory",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset",
                  Artifacts::Action::"ManageDependencyFreeze",
                  Artifacts::Action::"ExploreRepositoryHistory"],
       resource);
"#
    )
}

struct Vertical {
    app: Router,
    store: Arc<DgraphCatalogStore>,
    cas: Arc<FileBlobStore>,
    gc: Arc<GcService>,
    trust: Arc<DgraphTrustStore>,
    /// While set, the content store the router sees fails every `exists`.
    broken: Arc<AtomicBool>,
    #[allow(dead_code)]
    tmp: TempDir,
}

/// The content store of the router: the real file CAS, except that `exists` can
/// be made to fail, as a store that goes away would.
struct FlakyStore {
    inner: Arc<FileBlobStore>,
    broken: Arc<AtomicBool>,
}

#[async_trait]
impl BlobStore for FlakyStore {
    async fn put(&self, bytes: &[u8]) -> Result<ContentDigest, BlobError> {
        self.inner.put(bytes).await
    }
    async fn get(&self, digest: &ContentDigest) -> Result<Vec<u8>, BlobError> {
        self.inner.get(digest).await
    }
    async fn exists(&self, digest: &ContentDigest) -> Result<bool, BlobError> {
        if self.broken.load(Ordering::SeqCst) {
            return Err(BlobError::Unavailable("the store went away".to_string()));
        }
        self.inner.exists(digest).await
    }
    async fn begin(&self) -> Result<Box<dyn hodei_hub::blob::BlobWriteSession>, BlobError> {
        self.inner.begin().await
    }
    async fn get_with(
        &self,
        digest: &ContentDigest,
        policy: ReadPolicy,
    ) -> Result<Vec<u8>, BlobError> {
        self.inner.get_with(digest, policy).await
    }
    async fn length(&self, digest: &ContentDigest) -> Result<u64, BlobError> {
        self.inner.length(digest).await
    }
    async fn read_range(
        &self,
        digest: &ContentDigest,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, BlobError> {
        self.inner.read_range(digest, offset, length).await
    }
    async fn verify(&self, digest: &ContentDigest) -> Result<(), BlobError> {
        self.inner.verify(digest).await
    }
}

async fn mount(account: &str, other: &str) -> Option<Vertical> {
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
    let tenant = Tenant::new(&format!("hsp322d-{}", nonce())).expect("a valid tenant");
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
    let broken = Arc::new(AtomicBool::new(false));
    let flaky: Arc<dyn BlobStore> = Arc::new(FlakyStore {
        inner: cas.clone(),
        broken: broken.clone(),
    });
    // The collector talks to the real CAS; only the router's reads can fail.
    let real: Arc<dyn BlobStore> = cas.clone();
    let ledger: Arc<dyn ContentLedger> = store.clone();
    // Zero grace: what no pin protects is reclaimable at once.
    let gc = Arc::new(GcService::new(ledger, real, Duration::ZERO).expect("the GC service"));
    let ledger: Arc<dyn TrustLedger> = trust.clone();
    let state = state
        .with_catalog(reader, writer)
        .with_content_store(flaky)
        .with_gc(gc.clone())
        .with_trust_ledger(ledger)
        .with_authenticator(Arc::new(TokenAuthenticator))
        .with_authorization(cedar(&policy(account, other)));
    Some(Vertical {
        app: build_router(state, &config),
        store,
        cas,
        gc,
        trust,
        broken,
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
fn digest_of(bytes: &[u8]) -> ContentDigest {
    ContentDigest::of_bytes(bytes)
}

fn unique_account() -> String {
    format!("f{}a", &nonce()[8..])
}

async fn upload(v: &Vertical, repo: &str, token: &str, name: &str, bytes: &[u8]) {
    let (status, body) = send(
        &v.app,
        Method::PUT,
        &format!("/repository/{repo}/{name}"),
        token,
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

async fn make_freeze(v: &Vertical, token: &str, repo: &str) -> String {
    let (status, created) = freeze(v, token, json!({"repository": repo})).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["id"].as_str().expect("an id").to_string()
}

async fn diff(v: &Vertical, token: &str, from: &str, to: &str) -> (StatusCode, Value) {
    let (status, bytes) = send(
        &v.app,
        Method::GET,
        &format!("/api/v1/freezes/diff?from={from}&to={to}"),
        token,
        Vec::new(),
    )
    .await;
    (status, json_of(&bytes))
}

/// Everything the time machine must not touch: the holds of the ledger, the
/// trust grants and the catalog of the repository, plus whether each digest is
/// still in the store.
async fn state_of(v: &Vertical, account: &str, repo: &str, digests: &[&str]) -> Value {
    let mut holds: Vec<String> =
        v.gc.holds()
            .await
            .expect("the holds")
            .into_iter()
            .map(|hold| hold.id)
            .collect();
    holds.sort();
    let grants = v.trust.grants_of(account, repo).await.expect("the grants");
    let mut grants: Vec<String> = grants
        .iter()
        .map(|g| {
            format!(
                "{}:{}:{}",
                g.digest.value,
                g.decision_id,
                g.revoked_at.is_some()
            )
        })
        .collect();
    grants.sort();
    let mut assets: Vec<String> = v
        .store
        .assets_under(account, repo, "")
        .await
        .expect("the assets")
        .into_iter()
        .map(|a| format!("{}={}", a.asset_path, a.digest.value))
        .collect();
    assets.sort();
    let mut present = Vec::new();
    for digest in digests {
        let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
        present.push(v.cas.exists(&ContentDigest::sha256(hex)).await.unwrap());
    }
    json!({"holds": holds, "grants": grants, "assets": assets, "present": present})
}

/// The digest the catalog says the packument of `left-pad` has right now.
async fn stored_packument(v: &Vertical, account: &str) -> ContentDigest {
    v.store
        .asset(account, "reg", PACKUMENT)
        .await
        .expect("the packument asset")
        .digest
}

const PACKUMENT: &str = "left-pad";
const TARBALL_1: &str = "left-pad/-/left-pad-1.0.0.tgz";
const TARBALL_2: &str = "left-pad/-/left-pad-1.1.0.tgz";

#[tokio::test]
async fn two_freezes_explain_their_difference_and_report_reclaimed_bytes_as_gone_without_writing() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz").await else {
        return;
    };
    seed(&v, &account, "reg", RepositoryFormat::Npm, json!({})).await;
    let alice = format!("alice@{account}");
    publish(&v, "reg", &alice, "left-pad", "1.0.0").await;
    let packument_a = stored_packument(&v, &account).await;
    let first = make_freeze(&v, &alice, "reg").await;
    publish(&v, "reg", &alice, "left-pad", "1.1.0").await;
    let packument_b = stored_packument(&v, &account).await;
    assert_ne!(packument_a, packument_b, "the origin moved");
    let second = make_freeze(&v, &alice, "reg").await;

    let cited = [
        format!("sha256:{}", packument_a.value),
        format!("sha256:{}", packument_b.value),
    ];
    let cited_refs: Vec<&str> = cited.iter().map(String::as_str).collect();
    let before = state_of(&v, &account, "reg", &cited_refs).await;
    let (status, body) = diff(&v, &alice, &first, &second).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let after = state_of(&v, &account, "reg", &cited_refs).await;
    assert_eq!(before, after, "the read changed the state it reads");

    // The moved packument names both historical digests; the new version is an
    // addition; the untouched tarball is not a difference.
    assert_eq!(body["from"]["id"], first.as_str());
    assert_eq!(body["to"]["id"], second.as_str());
    assert_eq!(body["total_changes"], 2, "{body}");
    assert_eq!(body["unchanged"], 1, "{body}");
    assert_eq!(body["truncated"], false);
    let changes = body["changes"].as_array().unwrap();
    let moved = changes.iter().find(|c| c["path"] == PACKUMENT).unwrap();
    assert_eq!(moved["kind"], "digest_changed");
    assert_eq!(moved["from"]["digest"], cited[0].as_str());
    assert_eq!(moved["to"]["digest"], cited[1].as_str());
    let added = changes.iter().find(|c| c["path"] == TARBALL_2).unwrap();
    assert_eq!(added["kind"], "added");
    assert!(added["from"].is_null());
    assert!(changes.iter().all(|c| c["path"] != TARBALL_1));
    for digest in &cited {
        assert_eq!(body["digests"][digest]["availability"], "present", "{body}");
        assert!(body["digests"][digest]["admitted_now"].is_null());
    }

    // A freeze of the same shape the service writes, whose only reference to
    // its content is its own pin (the catalog has no deletion yet, so a blob no
    // catalog row names is built by hand, as in the freeze suite). When the pin
    // is lost the collector reclaims it. The freeze still NAMES the digest; the
    // time machine says the bytes are gone and does not recover them.
    let lost_bytes = format!("a packument nobody references {}", nonce()).into_bytes();
    let lost = v.cas.put(&lost_bytes).await.expect("a blob");
    let record = hodei_catalog::AssetRecord {
        account: account.clone(),
        repository: "reg".to_string(),
        asset_path: PACKUMENT.to_string(),
        coordinates: hodei_catalog::PackageCoordinates {
            package_name: "left-pad".to_string(),
            package_version: "0.9.0".to_string(),
        },
        digest: lost.clone(),
        size_bytes: lost_bytes.len() as u64,
        subject: None,
        updated_at: Utc::now(),
    };
    let manifest = FreezeManifest::build(&account, "reg", "", "alice", Utc::now(), None, &[record])
        .expect("a manifest");
    let ghost = manifest.id();
    v.cas
        .put(&manifest.bytes())
        .await
        .expect("the manifest blob");
    v.gc.place_holds(&manifest.holds())
        .await
        .expect("the holds");
    let lost_name = format!("sha256:{}", lost.value);
    let mut watched: Vec<&str> = cited_refs.clone();
    watched.push(&lost_name);
    let (status, body) = diff(&v, &alice, &ghost, &second).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["digests"][&lost_name]["availability"], "present");

    let hold = format!("freeze-{ghost}-{}", lost.value);
    assert!(v.store.release_hold(&hold).await.unwrap(), "{hold}");
    sweep(&v, &alice).await;
    assert!(!v.cas.exists(&lost).await.unwrap(), "really reclaimed");
    let before = state_of(&v, &account, "reg", &watched).await;
    let (status, body) = diff(&v, &alice, &ghost, &second).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["digests"][&lost_name]["availability"], "gone",
        "{body}"
    );
    assert_eq!(body["digests"][&cited[1]]["availability"], "present");
    let moved = body["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["path"] == PACKUMENT)
        .unwrap();
    assert_eq!(moved["from"]["digest"], lost_name.as_str(), "still cited");
    assert!(
        !body.to_string().contains("a packument nobody references"),
        "no bytes are returned"
    );
    let after = state_of(&v, &account, "reg", &watched).await;
    assert_eq!(before, after, "the read did not resurrect the bytes");

    // Control: the probe is not blind. A real write (a third freeze) moves it.
    let third = make_freeze(&v, &alice, "reg").await;
    assert_ne!(
        state_of(&v, &account, "reg", &cited_refs).await,
        after,
        "a freeze writes holds and the probe sees them"
    );
    let (status, _) = diff(&v, &alice, &second, &third).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn the_history_needs_its_own_action_and_never_crosses_accounts_or_repositories() {
    let account = unique_account();
    let other = format!("{}o", unique_account());
    let Some(v) = mount(&account, &other).await else {
        return;
    };
    seed(&v, &account, "reg", RepositoryFormat::Npm, json!({})).await;
    seed(&v, &account, "reg2", RepositoryFormat::Npm, json!({})).await;
    seed(&v, &other, "reg", RepositoryFormat::Npm, json!({})).await;
    let alice = format!("alice@{account}");
    let bob = format!("bob@{account}");
    let mallory = format!("mallory@{other}");
    publish(&v, "reg", &alice, "left-pad", "1.0.0").await;
    let first = make_freeze(&v, &alice, "reg").await;
    publish(&v, "reg", &alice, "left-pad", "1.1.0").await;
    let second = make_freeze(&v, &alice, "reg").await;
    publish(&v, "reg2", &alice, "other-pkg", "1.0.0").await;
    let elsewhere = make_freeze(&v, &alice, "reg2").await;
    publish(&v, "reg", &mallory, "secret-pkg", "9.9.9").await;
    let foreign = make_freeze(&v, &mallory, "reg").await;

    // bob can freeze and download but does not hold the history action.
    let (status, body) = diff(&v, &bob, &first, &second).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(body.get("changes").is_none(), "nothing leaks on a 403");
    // Unauthenticated.
    let (status, _) = send(
        &v.app,
        Method::GET,
        &format!("/api/v1/freezes/diff?from={first}&to={second}"),
        "nobody",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(diff(&v, &alice, &first, &second).await.0, StatusCode::OK);

    // Another account: it holds the action over its OWN repositories, yet a
    // freeze of this account is indistinguishable from absence, whichever side.
    for (from, to) in [(&first, &second), (&first, &foreign), (&foreign, &second)] {
        let (status, body) = diff(&v, &mallory, from, to).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert!(body.get("changes").is_none() && body.get("digests").is_none());
    }
    // And mixing one of its own with a foreign one from the owner's side.
    let (status, body) = diff(&v, &alice, &first, &foreign).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(!body.to_string().contains("secret-pkg"));
    // A made-up id is the same 404.
    let (status, _) = diff(&v, &alice, &first, &"0".repeat(64)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = diff(&v, &alice, &first, "not-an-id").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Two repositories of one account are not comparable.
    let (status, body) = diff(&v, &alice, &first, &elsewhere).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

#[tokio::test]
async fn the_listing_is_bounded_but_the_totals_are_exact() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz").await else {
        return;
    };
    seed(&v, &account, "raw", RepositoryFormat::Raw, json!({})).await;
    let alice = format!("alice@{account}");
    upload(&v, "raw", &alice, "anchor.txt", b"anchor").await;
    let first = make_freeze(&v, &alice, "raw").await;
    let extra = hodei_artifacts_api::time_machine::MAX_CHANGES + 50;
    for n in 0..extra {
        upload(
            &v,
            "raw",
            &alice,
            &format!("f{n:04}.txt"),
            format!("c{n}").as_bytes(),
        )
        .await;
    }
    let second = make_freeze(&v, &alice, "raw").await;
    let (status, body) = diff(&v, &alice, &first, &second).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_changes"], extra);
    assert_eq!(body["truncated"], true);
    assert_eq!(
        body["changes"].as_array().unwrap().len(),
        hodei_artifacts_api::time_machine::MAX_CHANGES
    );
    assert_eq!(
        body["digests"].as_object().unwrap().len(),
        hodei_artifacts_api::time_machine::MAX_CHANGES,
        "only the listed digests are probed"
    );
    assert_eq!(body["unchanged"], 1);
    // Release the bulk pins: the ledger is shared by every run against one
    // Dgraph and the collector reads it whole.
    for id in [&first, &second] {
        v.gc.release_holds_with_prefix(
            &hodei_artifacts_api::freeze::hold_prefix(id),
            &hodei_artifacts_api::freeze::manifest_hold_id(id),
        )
        .await
        .expect("the pins are released");
    }
}

#[tokio::test]
async fn a_failing_store_is_a_503_and_never_a_partial_answer() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz").await else {
        return;
    };
    seed(&v, &account, "reg", RepositoryFormat::Npm, json!({})).await;
    let alice = format!("alice@{account}");
    publish(&v, "reg", &alice, "left-pad", "1.0.0").await;
    let first = make_freeze(&v, &alice, "reg").await;
    publish(&v, "reg", &alice, "left-pad", "1.1.0").await;
    let second = make_freeze(&v, &alice, "reg").await;

    v.broken.store(true, Ordering::SeqCst);
    let (status, body) = diff(&v, &alice, &first, &second).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(
        body.get("changes").is_none() && body.get("digests").is_none(),
        "no partial answer: {body}"
    );
    // Control: the same request with the store healthy answers.
    v.broken.store(false, Ordering::SeqCst);
    assert_eq!(diff(&v, &alice, &first, &second).await.0, StatusCode::OK);
}

#[tokio::test]
async fn present_permission_is_reported_apart_from_the_historical_identity() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz").await else {
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
    upload(&v, "gated", &alice, "a.txt", b"alpha").await;
    v.trust.grant(&[grant(b"alpha", "d-a")]).await.unwrap();
    let first = make_freeze(&v, &alice, "gated").await;
    for (name, bytes, decision) in [("b.txt", b"bravo", "d-b"), ("c.txt", b"charl", "d-c")] {
        upload(&v, "gated", &alice, name, bytes).await;
        v.trust.grant(&[grant(bytes, decision)]).await.unwrap();
    }
    let second = make_freeze(&v, &alice, "gated").await;
    // Today the channel withdraws `b`: the freeze still names it.
    v.trust
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
    let (status, body) = diff(&v, &alice, &first, &second).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total_changes"], 2, "{body}");
    let key = |bytes: &[u8]| format!("sha256:{}", digest_of(bytes).value);
    assert_eq!(
        body["digests"][key(b"bravo")]["admitted_now"],
        false,
        "{body}"
    );
    assert_eq!(
        body["digests"][key(b"charl")]["admitted_now"],
        true,
        "{body}"
    );
    assert_eq!(body["digests"][key(b"bravo")]["availability"], "present");
}
