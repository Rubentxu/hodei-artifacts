//! HSP-326 (HST-11): the read-only tiering advisor, through the real router over
//! the real Dgraph catalog, a real file CAS, the real Cedar engine and the real
//! garbage-collection service.
//!
//! What this suite proves (each case is a behaviour of
//! `GET /api/v1/storage/tiering/advice`):
//!
//! - it recommends a configured tier per digest and cites the facts it used
//!   (size, age, references, cost) and declares the facts the platform lacks;
//! - it writes nothing: Dgraph (every node), the CAS tree (bytes and dates) and
//!   the store's write calls are identical before and after many calls;
//! - Cedar decides before any read (a denied caller costs no ledger or store
//!   read) and no other storage action implies the dedicated one;
//! - another account gets the 404 of absence and never sees foreign digests;
//! - every bound holds (digests, ledger rows, response bytes, deadline,
//!   concurrency) and every failure is closed and audited, even when the client
//!   hangs up;
//! - without a configured ladder the endpoint answers 501.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` (with `HODEI_REQUIRE_IT=1` a missing
//! harness fails instead of skipping); serialize with `--test-threads=1`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use cedar_policy::Entities;
use chrono::Utc;
use hodei_artifacts_api::audit::AuditResult;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::gc::GcService;
use hodei_artifacts_api::tiering::{TierConfig, TieringAdvisor, TieringConfig};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use hodei_catalog::{
    AllowAllAuthorizer, CatalogReader, CatalogWriter, ContentLedger, DigestScope, RepositoryConfig,
    RepositoryDescriptor, RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_catalog_storage::DgraphCatalogStore;
use hodei_evidence::ContentDigest;
use hodei_graph_client::{GraphClient, GraphClientConfig};
use hodei_hub::blob::{BlobError, BlobStore, FileBlobStore, ReadPolicy};
use hodei_hub::reclaim::{BlobReclaim, ConditionalBlob};
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

const DAY: u64 = 86_400;

/// alice holds the tiering action; bob only reads and publishes; carol holds
/// the other storage actions but not the tiering one; mallory belongs to
/// another account and holds the tiering action there.
fn policy(account: &str, other: &str) -> String {
    format!(
        r#"
permit(principal == Iam::User::"hrn:hodei:iam::{account}:User/alice",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset",
                  Artifacts::Action::"CreateRepository",
                  Artifacts::Action::"ExploreStorageTiering"],
       resource);
permit(principal == Iam::User::"hrn:hodei:iam::{account}:User/bob",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset"],
       resource);
permit(principal == Iam::User::"hrn:hodei:iam::{account}:User/carol",
       action in [Artifacts::Action::"ManageStorageRepair",
                  Artifacts::Action::"ManageStorageMigration",
                  Artifacts::Action::"ManageStorageRelocation",
                  Artifacts::Action::"DownloadAsset"],
       resource);
permit(principal == Iam::User::"hrn:hodei:iam::{other}:User/mallory",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset",
                  Artifacts::Action::"ExploreStorageTiering"],
       resource);
"#
    )
}

/// The real file CAS with counters of what the advisor does to it.
struct Counting {
    inner: Arc<FileBlobStore>,
    /// `length`/`age` calls (the advisor's reads).
    reads: Arc<AtomicU64>,
    /// `put`/`begin`/`resume` calls (any write).
    writes: Arc<AtomicU64>,
    /// A delay (ms) before every `length`.
    delay_ms: Arc<AtomicU64>,
    /// `length` fails as a store that went away.
    fail: Arc<AtomicBool>,
}

#[async_trait]
impl BlobStore for Counting {
    async fn put(&self, bytes: &[u8]) -> Result<ContentDigest, BlobError> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.inner.put(bytes).await
    }
    async fn get(&self, digest: &ContentDigest) -> Result<Vec<u8>, BlobError> {
        self.inner.get(digest).await
    }
    async fn exists(&self, digest: &ContentDigest) -> Result<bool, BlobError> {
        self.inner.exists(digest).await
    }
    async fn begin(&self) -> Result<Box<dyn hodei_hub::blob::BlobWriteSession>, BlobError> {
        self.writes.fetch_add(1, Ordering::SeqCst);
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
        self.reads.fetch_add(1, Ordering::SeqCst);
        let delay = self.delay_ms.load(Ordering::SeqCst);
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        if self.fail.load(Ordering::SeqCst) {
            return Err(BlobError::Unavailable("the store went away".to_string()));
        }
        self.inner.length(digest).await
    }
    async fn age(&self, digest: &ContentDigest) -> Result<Option<Duration>, BlobError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.inner.age(digest).await
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
    fn reclaimer(&self) -> Option<&dyn BlobReclaim> {
        self.inner.reclaimer()
    }
    fn conditional(&self) -> Option<&dyn ConditionalBlob> {
        self.inner.conditional()
    }
}

struct Vertical {
    app: Router,
    store: Arc<DgraphCatalogStore>,
    client: Arc<GraphClient>,
    cas: Arc<FileBlobStore>,
    cas_root: std::path::PathBuf,
    reads: Arc<AtomicU64>,
    writes: Arc<AtomicU64>,
    delay_ms: Arc<AtomicU64>,
    fail: Arc<AtomicBool>,
    audit: Arc<hodei_artifacts_api::audit::InMemoryAuditSink>,
    #[allow(dead_code)]
    tmp: TempDir,
}

fn tier(name: &str, cost: u64, age: u64, size: u64, refs: Option<u32>) -> TierConfig {
    TierConfig {
        name: name.to_string(),
        monthly_cost_micro_per_gib: cost,
        min_age_secs: age,
        min_size_bytes: size,
        max_references: refs,
    }
}

fn ladder() -> TieringConfig {
    TieringConfig {
        cost_unit: "USD".to_string(),
        tiers: vec![
            tier("hot", 23_000_000, 0, 0, None),
            tier("warm", 12_500_000, DAY, 16, None),
            tier("cold", 4_000_000, 30 * DAY, 16, Some(1)),
        ],
        max_digests: 200,
        max_rows: 10_000,
        max_response_bytes: 256 * 1024,
        timeout_secs: 20,
        max_concurrent: 4,
    }
}

/// `tiering`: the ladder and bounds; `None` mounts no advisor.
async fn mount(account: &str, other: &str, tiering: Option<TieringConfig>) -> Option<Vertical> {
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
    let cas_root = tmp.path().join("cas");
    let cas = Arc::new(FileBlobStore::new(&cas_root).expect("the CAS"));
    let reads = Arc::new(AtomicU64::new(0));
    let writes = Arc::new(AtomicU64::new(0));
    let delay_ms = Arc::new(AtomicU64::new(0));
    let fail = Arc::new(AtomicBool::new(false));
    let counting: Arc<dyn BlobStore> = Arc::new(Counting {
        inner: cas.clone(),
        reads: reads.clone(),
        writes: writes.clone(),
        delay_ms: delay_ms.clone(),
        fail: fail.clone(),
    });
    let ledger: Arc<dyn ContentLedger> = store.clone();
    let gc = Arc::new(GcService::new(ledger, cas.clone(), Duration::ZERO).expect("the GC service"));
    let audit = Arc::new(hodei_artifacts_api::audit::InMemoryAuditSink::new());
    let mut state = state
        .with_catalog(reader, writer)
        .with_content_store(counting)
        .with_gc(gc)
        .with_audit_sink(audit.clone())
        .with_authenticator(Arc::new(TokenAuthenticator))
        .with_authorization(cedar(&policy(account, other)));
    if let Some(tiering) = tiering {
        state = state.with_tiering(Arc::new(
            TieringAdvisor::new(tiering).expect("the ladder validates"),
        ));
    }
    Some(Vertical {
        app: build_router(state, &config),
        store,
        client,
        cas,
        cas_root,
        reads,
        writes,
        delay_ms,
        fail,
        audit,
        tmp,
    })
}

async fn seed(v: &Vertical, account: &str, key: &str) {
    let mut config = RepositoryConfig::new(DigestScope::Global);
    config.config_settings = json!({});
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
    uri: &str,
    token: &str,
    method: Method,
    body: Vec<u8>,
) -> (StatusCode, Vec<u8>) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "127.0.0.1")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(body))
        .unwrap();
    let response = app.clone().oneshot(request).await.expect("a response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, bytes.to_vec())
}

fn json_of(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or(Value::Null)
}

fn unique_account() -> String {
    format!("t{}a", &nonce()[8..])
}

async fn upload(v: &Vertical, repo: &str, token: &str, name: &str, bytes: &[u8]) {
    let (status, body) = send(
        &v.app,
        &format!("/repository/{repo}/{name}"),
        token,
        Method::PUT,
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

fn digest_of(bytes: &[u8]) -> String {
    ContentDigest::of_bytes(bytes).to_string()
}

/// Makes a blob look `days` old on disk.
fn age_blob(v: &Vertical, bytes: &[u8], days: u64) {
    let path = v.cas.the_path_of(&ContentDigest::of_bytes(bytes));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("the blob opens");
    file.set_modified(SystemTime::now() - Duration::from_secs(days * DAY))
        .expect("the date is set");
}

async fn advice(v: &Vertical, token: &str, query: &str) -> (StatusCode, Value) {
    let (status, bytes) = send(
        &v.app,
        &format!("/api/v1/storage/tiering/advice?{query}"),
        token,
        Method::GET,
        Vec::new(),
    )
    .await;
    (status, json_of(&bytes))
}

const ACTION: &str = "Artifacts::Action::\"ExploreStorageTiering\"";

fn results(v: &Vertical) -> Vec<AuditResult> {
    v.audit
        .events()
        .iter()
        .filter(|event| event.action == ACTION)
        .map(|event| event.result)
        .collect()
}

fn entry<'a>(answer: &'a Value, bytes: &[u8]) -> &'a Value {
    let digest = digest_of(bytes);
    answer["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .find(|entry| entry["digest"] == digest)
        .unwrap_or_else(|| panic!("{digest} is not in {answer}"))
}

/// The same value with every object's keys in order (Dgraph answers them in any order).
fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .collect::<std::collections::BTreeMap<_, _>>()
                .into_iter()
                .map(|(key, value)| (key.clone(), sorted(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

/// Everything the advisor could possibly write: every node of Dgraph and every
/// file of the CAS tree, with its bytes and its date.
async fn world(v: &Vertical) -> String {
    let dump = v
        .client
        .execute_dql("{ q(func: has(dgraph.type)) { uid expand(_all_) } }")
        .await
        .expect("the dump");
    let mut nodes: Vec<String> = dump.json["q"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|node| sorted(node).to_string())
        .collect();
    nodes.sort();
    let mut files = Vec::new();
    let mut pending = vec![v.cas_root.clone()];
    while let Some(dir) = pending.pop() {
        for item in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = item.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let meta = std::fs::metadata(&path).unwrap();
                files.push(format!(
                    "{} {} {:?} {}",
                    path.display(),
                    meta.len(),
                    meta.modified().unwrap(),
                    digest_of(&std::fs::read(&path).unwrap())
                ));
            }
        }
    }
    files.sort();
    format!("{}\n--\n{}", nodes.join("\n"), files.join("\n"))
}

/// A repository `reg` with three blobs: a young one, a 10-day one and a
/// 400-day one, plus a tiny old one.
struct Blobs {
    young: Vec<u8>,
    middle: Vec<u8>,
    old: Vec<u8>,
    tiny: Vec<u8>,
}

async fn published(v: &Vertical, account: &str) -> Blobs {
    seed(v, account, "reg").await;
    let alice = format!("alice@{account}");
    let big = |tag: &str| format!("{tag} {account} {}", "x".repeat(200)).into_bytes();
    let blobs = Blobs {
        young: big("young"),
        middle: big("middle"),
        old: big("old"),
        tiny: b"t".to_vec(),
    };
    upload(v, "reg", &alice, "young.bin", &blobs.young).await;
    upload(v, "reg", &alice, "middle.bin", &blobs.middle).await;
    upload(v, "reg", &alice, "old.bin", &blobs.old).await;
    upload(v, "reg", &alice, "tiny.bin", &blobs.tiny).await;
    age_blob(v, &blobs.middle, 10);
    age_blob(v, &blobs.old, 400);
    age_blob(v, &blobs.tiny, 400);
    blobs
}

#[tokio::test]
async fn it_recommends_a_tier_per_digest_citing_the_facts_and_the_missing_ones() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(ladder())).await else {
        return;
    };
    let blobs = published(&v, &account).await;
    let alice = format!("alice@{account}");
    let (status, answer) = advice(&v, &alice, "repository=reg").await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["read_only"], true);
    assert_eq!(answer["truncated"], false);
    assert_eq!(answer["entries"].as_array().unwrap().len(), 4, "{answer}");
    assert_eq!(answer["tiers"], json!(["hot", "warm", "cold"]));
    assert_eq!(
        answer["missing_facts"],
        json!(["access_frequency", "last_access"])
    );

    let young = entry(&answer, &blobs.young);
    assert_eq!(young["recommended_tier"], "hot");
    assert_eq!(young["colder_tier_blocked_by"]["fact"], "age_secs");
    let middle = entry(&answer, &blobs.middle);
    assert_eq!(middle["recommended_tier"], "warm", "{middle}");
    assert_eq!(middle["facts"]["size_bytes"], blobs.middle.len());
    assert_eq!(middle["facts"]["references"], 1);
    assert!(middle["facts"]["age_secs"].as_u64().unwrap() >= 10 * DAY);
    let old = entry(&answer, &blobs.old);
    assert_eq!(old["recommended_tier"], "cold", "{old}");
    let cited: Vec<_> = old["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["fact"].as_str().unwrap())
        .collect();
    assert_eq!(
        cited,
        ["age_secs", "size_bytes", "references", "cost"],
        "{old}"
    );
    assert!(
        old["monthly_cost_micro"].as_u64().unwrap()
            < old["hottest_monthly_cost_micro"].as_u64().unwrap()
    );
    let tiny = entry(&answer, &blobs.tiny);
    assert_eq!(
        tiny["recommended_tier"], "hot",
        "a tiny blob is not worth moving"
    );
    assert_eq!(tiny["colder_tier_blocked_by"]["fact"], "size_bytes");
    assert_eq!(results(&v), vec![AuditResult::Allowed]);
}

#[tokio::test]
async fn a_digest_shared_by_two_paths_cites_its_references_and_stays_out_of_the_coldest_tier() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(ladder())).await else {
        return;
    };
    let blobs = published(&v, &account).await;
    let alice = format!("alice@{account}");
    upload(&v, "reg", &alice, "old-copy.bin", &blobs.old).await;
    age_blob(&v, &blobs.old, 400);
    let (status, answer) = advice(&v, &alice, "repository=reg").await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let old = entry(&answer, &blobs.old);
    assert_eq!(old["facts"]["references"], 2, "{old}");
    assert_eq!(old["recommended_tier"], "warm");
    assert_eq!(old["colder_tier_blocked_by"]["fact"], "references");
}

#[tokio::test]
async fn it_writes_nothing_dgraph_the_cas_and_the_store_are_identical_after_many_calls() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(ladder())).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    let bob = format!("bob@{account}");
    let mallory = "mallory@zz".to_string();
    // The world as the uploads left it; the audit sink is memory, not state.
    let before = world(&v).await;
    assert!(before.len() > 100, "the snapshot is not empty");
    v.writes.store(0, Ordering::SeqCst);
    for _ in 0..3 {
        assert_eq!(advice(&v, &alice, "repository=reg").await.0, StatusCode::OK);
        assert_eq!(
            advice(&v, &alice, "repository=reg&limit=1").await.0,
            StatusCode::OK
        );
        assert_eq!(
            advice(&v, &bob, "repository=reg").await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            advice(&v, &mallory, "repository=reg").await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            advice(&v, &alice, "repository=nope").await.0,
            StatusCode::NOT_FOUND
        );
    }
    v.fail.store(true, Ordering::SeqCst);
    assert_eq!(
        advice(&v, &alice, "repository=reg").await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    v.fail.store(false, Ordering::SeqCst);
    let after = world(&v).await;
    assert_eq!(
        before, after,
        "Dgraph and the CAS are byte-for-byte as they were"
    );
    assert_eq!(
        v.writes.load(Ordering::SeqCst),
        0,
        "the store was never asked to write"
    );
}

#[tokio::test]
async fn cedar_decides_before_any_read_and_no_other_storage_action_implies_it() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(ladder())).await else {
        return;
    };
    published(&v, &account).await;
    v.reads.store(0, Ordering::SeqCst);
    for who in ["bob", "carol"] {
        let (status, answer) = advice(&v, &format!("{who}@{account}"), "repository=reg").await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{who}: {answer}");
        assert!(
            answer["entries"].is_null(),
            "no data leaks to a denied caller"
        );
    }
    assert_eq!(
        v.reads.load(Ordering::SeqCst),
        0,
        "a denied caller costs no store read"
    );
    assert_eq!(results(&v), vec![AuditResult::Denied, AuditResult::Denied]);
    // Without credentials: no data either.
    let (status, _) = send(
        &v.app,
        "/api/v1/storage/tiering/advice?repository=reg",
        "nobody",
        Method::GET,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn another_account_gets_the_404_of_absence_and_never_sees_foreign_digests() {
    let account = unique_account();
    let other = unique_account();
    let Some(v) = mount(&account, &other, Some(ladder())).await else {
        return;
    };
    let blobs = published(&v, &account).await;
    // mallory owns a repository with the same key and different content.
    seed(&v, &other, "reg").await;
    let mallory = format!("mallory@{other}");
    let theirs = format!("their own bytes {other}").into_bytes();
    // mallory has no CreateRepository/Upload on its own? she does: UploadAsset.
    upload(&v, "reg", &mallory, "mine.bin", &theirs).await;

    let (status, answer) = advice(&v, &mallory, "repository=reg").await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let digests: Vec<_> = answer["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["digest"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        digests,
        vec![digest_of(&theirs)],
        "only her own digests: {answer}"
    );
    // Her cursor cannot be used to reach alice's digests either.
    let (_, paged) = advice(
        &v,
        &mallory,
        &format!("repository=reg&after={}", digest_of(&theirs)),
    )
    .await;
    assert_eq!(paged["entries"], json!([]), "{paged}");
    assert!(!answer.to_string().contains(&digest_of(&blobs.old)));

    // A repository that is only alice's is the same 404 as one that never existed.
    seed(&v, &account, "only-alice").await;
    let (hidden, hidden_body) = advice(&v, &mallory, "repository=only-alice").await;
    let (absent, absent_body) = advice(&v, &mallory, "repository=never-existed").await;
    assert_eq!(
        (hidden, absent),
        (StatusCode::NOT_FOUND, StatusCode::NOT_FOUND)
    );
    assert_eq!(hidden_body, absent_body, "indistinguishable from absence");
}

#[tokio::test]
async fn the_digest_bound_pages_the_answer_without_losing_or_repeating_a_digest() {
    let account = unique_account();
    let mut config = ladder();
    config.max_digests = 2;
    let Some(v) = mount(&account, "zz", Some(config)).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    let mut seen = Vec::new();
    let mut after = None::<String>;
    for _ in 0..5 {
        let query = match &after {
            Some(after) => format!("repository=reg&after={after}"),
            None => "repository=reg".to_string(),
        };
        let (status, answer) = advice(&v, &alice, &query).await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        assert!(answer["entries"].as_array().unwrap().len() <= 2, "{answer}");
        seen.extend(
            answer["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["digest"].as_str().unwrap().to_string()),
        );
        if answer["truncated"] == false {
            assert!(answer["next_after"].is_null());
            break;
        }
        assert_eq!(answer["truncated_by"], "digests", "{answer}");
        after = Some(answer["next_after"].as_str().expect("a cursor").to_string());
    }
    let mut sorted = seen.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(seen.len(), 4, "{seen:?}");
    assert_eq!(sorted, seen, "ordered, no repeats");
    // A limit above the configured bound does not raise it.
    let (_, answer) = advice(&v, &alice, "repository=reg&limit=100").await;
    assert_eq!(answer["entries"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn the_row_bound_stops_the_ledger_walk_and_says_where_to_resume() {
    let account = unique_account();
    let mut config = ladder();
    config.max_rows = 2;
    let Some(v) = mount(&account, "zz", Some(config)).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    let (status, answer) = advice(&v, &alice, "repository=reg").await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["truncated"], true, "{answer}");
    assert_eq!(answer["truncated_by"], "rows");
    assert!(
        answer["rows_read"].as_u64().unwrap() <= 3,
        "ties may add one: {answer}"
    );
    assert!(answer["next_after"].is_string());
}

#[tokio::test]
async fn the_byte_bound_cuts_the_answer_and_says_where_to_resume() {
    let account = unique_account();
    let mut config = ladder();
    config.max_response_bytes = 1_500;
    let Some(v) = mount(&account, "zz", Some(config)).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    let (status, answer) = advice(&v, &alice, "repository=reg").await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["truncated_by"], "bytes", "{answer}");
    assert!(answer["entries"].as_array().unwrap().len() < 4);
    assert!(answer["next_after"].is_string());
}

#[tokio::test]
async fn the_deadline_fails_closed_with_a_504_and_an_audited_failure() {
    let account = unique_account();
    let mut config = ladder();
    config.timeout_secs = 1;
    let Some(v) = mount(&account, "zz", Some(config)).await else {
        return;
    };
    published(&v, &account).await;
    v.delay_ms.store(3_000, Ordering::SeqCst);
    let alice = format!("alice@{account}");
    let (status, answer) = advice(&v, &alice, "repository=reg").await;
    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT, "{answer}");
    assert_eq!(answer["error"], "E_TIERING_TIMEOUT");
    assert_eq!(results(&v), vec![AuditResult::Failed]);
}

#[tokio::test]
async fn the_semaphore_refuses_the_request_that_does_not_fit_with_a_429() {
    let account = unique_account();
    let mut config = ladder();
    config.max_concurrent = 1;
    let Some(v) = mount(&account, "zz", Some(config)).await else {
        return;
    };
    published(&v, &account).await;
    v.delay_ms.store(600, Ordering::SeqCst);
    let alice = format!("alice@{account}");
    let (first, second) = tokio::join!(advice(&v, &alice, "repository=reg"), async {
        tokio::time::sleep(Duration::from_millis(300)).await;
        advice(&v, &alice, "repository=reg").await
    });
    assert_eq!(first.0, StatusCode::OK, "{}", first.1);
    assert_eq!(second.0, StatusCode::TOO_MANY_REQUESTS, "{}", second.1);
    assert_eq!(second.1["error"], "E_TIERING_BUSY");
    let mut got: Vec<String> = results(&v).iter().map(|r| format!("{r:?}")).collect();
    got.sort();
    assert_eq!(got, ["Allowed", "Failed"]);
}

#[tokio::test]
async fn a_client_that_hangs_up_still_leaves_its_audit_trail() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(ladder())).await else {
        return;
    };
    published(&v, &account).await;
    v.delay_ms.store(500, Ordering::SeqCst);
    let alice = format!("alice@{account}");
    let hung = tokio::time::timeout(
        Duration::from_millis(100),
        advice(&v, &alice, "repository=reg"),
    )
    .await;
    assert!(hung.is_err(), "the client gave up");
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert_eq!(
        results(&v),
        vec![AuditResult::Allowed],
        "the task finished and audited"
    );
}

#[tokio::test]
async fn without_a_ladder_the_endpoint_answers_closed_after_cedar() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", None).await else {
        return;
    };
    published(&v, &account).await;
    v.reads.store(0, Ordering::SeqCst);
    let (status, answer) = advice(&v, &format!("alice@{account}"), "repository=reg").await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED, "{answer}");
    assert_eq!(answer["error"], "E_TIERING_UNAVAILABLE");
    // Cedar still decides first: a caller without the action is a 403, not a 501.
    let (status, _) = advice(&v, &format!("bob@{account}"), "repository=reg").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(v.reads.load(Ordering::SeqCst), 0);
    assert_eq!(results(&v), vec![AuditResult::Failed, AuditResult::Denied]);
}

#[tokio::test]
async fn a_store_failure_is_a_503_never_a_partial_answer_and_a_missing_blob_is_declared() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(ladder())).await else {
        return;
    };
    let blobs = published(&v, &account).await;
    let alice = format!("alice@{account}");
    v.fail.store(true, Ordering::SeqCst);
    let (status, answer) = advice(&v, &alice, "repository=reg").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{answer}");
    assert!(answer["entries"].is_null());
    v.fail.store(false, Ordering::SeqCst);

    // The ledger names bytes the store lost: reported, not advised.
    std::fs::remove_file(v.cas.the_path_of(&ContentDigest::of_bytes(&blobs.old))).unwrap();
    let (status, answer) = advice(&v, &alice, "repository=reg").await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let lost = entry(&answer, &blobs.old);
    assert_eq!(lost["status"], "blob_missing");
    assert!(lost["recommended_tier"].is_null());
    assert_eq!(results(&v), vec![AuditResult::Failed, AuditResult::Allowed]);
}

#[tokio::test]
async fn bad_input_is_a_400_before_any_read() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(ladder())).await else {
        return;
    };
    published(&v, &account).await;
    v.reads.store(0, Ordering::SeqCst);
    let alice = format!("alice@{account}");
    for query in ["repository=reg&after=nonsense", "repository=reg&limit=0"] {
        let (status, _) = advice(&v, &alice, query).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
    }
    let (status, _) = advice(&v, &alice, "repository=reg&surprise=1").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(v.reads.load(Ordering::SeqCst), 0);
}
