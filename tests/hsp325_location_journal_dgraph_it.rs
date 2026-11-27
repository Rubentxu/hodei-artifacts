//! HSP-325: location journal per digest with lease and fencing, reads that
//! follow the journal, and a verified retirement of the origin copy, through the
//! real router over the real Dgraph catalog, real file CAS directories, the real
//! Cedar engine, the real GC service and the real repairer.
//!
//! What this suite proves (each case is a behaviour of `POST /api/v1/storage/relocate`
//! or of the journal itself):
//!
//! - the reads follow the journal: switch sends them to the destination, a
//!   retirement moves the origin copy aside (never deletes it) and the registry
//!   keeps serving, and a rollback puts the origin back and the reads with it;
//! - lease and fencing: a writer whose lease expired and was taken over cannot
//!   advance the journal, even if it resumes mid-operation;
//! - a failure half way never loses bytes: the origin stays intact and readable;
//! - a hold of ANY scope (digest, repository, account) blocks the retirement,
//!   including one placed while the origin is being moved (the origin is put
//!   back); an unrelated hold does not;
//! - a destination that does not verify blocks the switch and the retirement,
//!   and a corrupt destination never shadows a good origin;
//! - races with the real GC sweep and the real repair lose nothing;
//! - account isolation (uniform 404), the dedicated Cedar action (403, audited),
//!   bounds and fail-closed behaviour.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` (with `HODEI_REQUIRE_IT=1` a missing
//! harness fails instead of skipping); serialize with `--test-threads=1`.

use std::sync::Arc;
use std::sync::Mutex;
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
use hodei_artifacts_api::recovery::{
    LocatedStore, Migrator, RelocateOperation, Relocated, Relocator, Repairer,
};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use hodei_catalog::{
    AllowAllAuthorizer, CatalogReader, CatalogWriter, ContentLedger, DigestScope, HoldScope,
    LocationJournal, Phase, RepositoryConfig, RepositoryDescriptor, RepositoryFormat,
    RepositoryMode, repository_hrn,
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

/// alice: reads, publishes and holds the relocation (and migration, repair).
/// bob can read and publish but does NOT hold the relocation action. mallory
/// belongs to another account and holds every repository action there.
fn policy(account: &str, other: &str) -> String {
    format!(
        r#"
permit(principal == Iam::User::"hrn:hodei:iam::{account}:User/alice",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset",
                  Artifacts::Action::"CreateRepository",
                  Artifacts::Action::"ManageStorageRepair",
                  Artifacts::Action::"ManageStorageMigration",
                  Artifacts::Action::"ManageStorageRelocation"],
       resource);
permit(principal == Iam::User::"hrn:hodei:iam::{account}:User/bob",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset",
                  Artifacts::Action::"ManageStorageMigration"],
       resource);
permit(principal == Iam::User::"hrn:hodei:iam::{other}:User/mallory",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset",
                  Artifacts::Action::"CreateRepository",
                  Artifacts::Action::"ManageStorageMigration",
                  Artifacts::Action::"ManageStorageRelocation"],
       resource);
"#
    )
}

type Hook = Arc<Mutex<Option<(Arc<GcService>, HoldScope)>>>;

/// The conditional view of a file CAS with a hook: right before a version is
/// moved aside it can place a hold, as a concurrent operator would.
struct HookedConditional {
    inner: Arc<FileBlobStore>,
    hook: Hook,
    /// The whole store is down: the conditional view fails as well.
    down: Arc<AtomicBool>,
    /// The origin's `put` flag and whether the hook, once it fires, flips it:
    /// a store that stops accepting writes right after the hold appears.
    put_fails: Arc<AtomicBool>,
    break_puts_after_hold: Arc<AtomicBool>,
}

#[async_trait]
impl ConditionalBlob for HookedConditional {
    fn supports_conditional_delete(&self) -> bool {
        true
    }
    async fn head_marker(
        &self,
        digest: &ContentDigest,
    ) -> Result<Option<hodei_hub::reclaim::VersionMarker>, BlobError> {
        if self.down.load(Ordering::SeqCst) {
            return Err(BlobError::Unavailable("the store is down".to_string()));
        }
        self.inner.conditional().unwrap().head_marker(digest).await
    }
    fn quarantine_key_of(&self, digest: &ContentDigest, run_id: &str) -> String {
        self.inner
            .conditional()
            .unwrap()
            .quarantine_key_of(digest, run_id)
    }
    async fn delete_if_untouched(
        &self,
        digest: &ContentDigest,
        marker: &hodei_hub::reclaim::VersionMarker,
    ) -> Result<hodei_hub::reclaim::ConditionalOutcome, BlobError> {
        self.inner
            .conditional()
            .unwrap()
            .delete_if_untouched(digest, marker)
            .await
    }
    async fn quarantine_if_untouched(
        &self,
        digest: &ContentDigest,
        marker: &hodei_hub::reclaim::VersionMarker,
        quarantine_key: &str,
    ) -> Result<hodei_hub::reclaim::ConditionalOutcome, BlobError> {
        let pending = self.hook.lock().unwrap().take();
        if let Some((gc, scope)) = pending {
            gc.place_hold("ops", scope, "placed mid-retirement", None)
                .await
                .expect("the hold is placed");
            if self.break_puts_after_hold.load(Ordering::SeqCst) {
                self.put_fails.store(true, Ordering::SeqCst);
            }
        }
        self.inner
            .conditional()
            .unwrap()
            .quarantine_if_untouched(digest, marker, quarantine_key)
            .await
    }
}

/// A real file CAS that can be made to misbehave.
struct Wrapper {
    inner: Arc<FileBlobStore>,
    cond: HookedConditional,
    /// `put` fails for every digest.
    put_fails: Arc<AtomicBool>,
    /// Every read, length and verify fails.
    down: Arc<AtomicBool>,
    /// `get` answers other bytes without saying so: a store that lies.
    lies: Arc<AtomicBool>,
    /// A delay (ms) added to every `get`, to keep an operation in flight.
    get_delay_ms: Arc<AtomicU64>,
    /// How many times the store was asked for anything about content.
    calls: Arc<AtomicU64>,
}

impl Wrapper {
    fn new(inner: Arc<FileBlobStore>) -> Arc<Self> {
        let down = Arc::new(AtomicBool::new(false));
        let put_fails = Arc::new(AtomicBool::new(false));
        Arc::new(Self {
            cond: HookedConditional {
                inner: inner.clone(),
                hook: Arc::new(Mutex::new(None)),
                down: down.clone(),
                put_fails: put_fails.clone(),
                break_puts_after_hold: Arc::new(AtomicBool::new(false)),
            },
            inner,
            put_fails,
            down,
            lies: Arc::new(AtomicBool::new(false)),
            get_delay_ms: Arc::new(AtomicU64::new(0)),
            calls: Arc::new(AtomicU64::new(0)),
        })
    }
    fn touch(&self) -> Result<(), BlobError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.down.load(Ordering::SeqCst) {
            Err(BlobError::Unavailable("the store is down".to_string()))
        } else {
            Ok(())
        }
    }
    fn calls(&self) -> u64 {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl BlobStore for Wrapper {
    async fn put(&self, bytes: &[u8]) -> Result<ContentDigest, BlobError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.put_fails.load(Ordering::SeqCst) {
            return Err(BlobError::Unavailable("the store is full".to_string()));
        }
        self.inner.put(bytes).await
    }
    async fn get(&self, digest: &ContentDigest) -> Result<Vec<u8>, BlobError> {
        self.touch()?;
        let delay = self.get_delay_ms.load(Ordering::SeqCst);
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        if self.lies.load(Ordering::SeqCst) {
            return Ok(b"bytes that are not the content".to_vec());
        }
        self.inner.get(digest).await
    }
    async fn exists(&self, digest: &ContentDigest) -> Result<bool, BlobError> {
        self.touch()?;
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
        self.touch()?;
        self.inner.get_with(digest, policy).await
    }
    async fn length(&self, digest: &ContentDigest) -> Result<u64, BlobError> {
        self.touch()?;
        self.inner.length(digest).await
    }
    async fn read_range(
        &self,
        digest: &ContentDigest,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, BlobError> {
        self.touch()?;
        self.inner.read_range(digest, offset, length).await
    }
    async fn verify(&self, digest: &ContentDigest) -> Result<(), BlobError> {
        self.touch()?;
        self.inner.verify(digest).await
    }
    fn reclaimer(&self) -> Option<&dyn BlobReclaim> {
        self.inner.reclaimer()
    }
    fn conditional(&self) -> Option<&dyn ConditionalBlob> {
        Some(&self.cond)
    }
}

struct Vertical {
    app: Router,
    store: Arc<DgraphCatalogStore>,
    /// The raw origin directory (what the GC sweeps and the repair heals).
    cas: Arc<FileBlobStore>,
    origin: Arc<Wrapper>,
    /// The destination backend.
    dest: Arc<FileBlobStore>,
    destination: Arc<Wrapper>,
    /// The replica the repair reads from.
    replica: Arc<FileBlobStore>,
    gc: Arc<GcService>,
    audit: Arc<hodei_artifacts_api::audit::InMemoryAuditSink>,
    root: std::path::PathBuf,
    /// The five contents of this test: unique per test, because the journal
    /// is keyed by digest and the database outlives the test.
    a: &'static [u8],
    b: &'static [u8],
    c: &'static [u8],
    d: &'static [u8],
    e: &'static [u8],
    #[allow(dead_code)]
    tmp: TempDir,
}

/// Content that no other test (or earlier run) has ever published.
fn unique_blob(text: &str) -> &'static [u8] {
    Box::leak(
        format!("release {text} #{}", nonce())
            .into_bytes()
            .into_boxed_slice(),
    )
}

/// `lease`: the lease of one digest; `None` mounts no destination at all.
async fn mount(account: &str, other: &str, lease: Option<Duration>) -> Option<Vertical> {
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
    let root = tmp.path().join("cas");
    let cas = Arc::new(FileBlobStore::new(&root).expect("the CAS"));
    let dest = Arc::new(FileBlobStore::new(tmp.path().join("dest")).expect("the destination"));
    let replica = Arc::new(FileBlobStore::new(tmp.path().join("replica")).expect("the replica"));
    let origin = Wrapper::new(cas.clone());
    let destination = Wrapper::new(dest.clone());
    let ledger: Arc<dyn ContentLedger> = store.clone();
    let gc = Arc::new(GcService::new(ledger.clone(), cas.clone(), Duration::ZERO).expect("the GC"));
    let audit = Arc::new(hodei_artifacts_api::audit::InMemoryAuditSink::new());
    let origin_view: Arc<dyn BlobStore> = origin.clone();
    let destination_view: Arc<dyn BlobStore> = destination.clone();
    let journal: Arc<dyn LocationJournal> = store.clone();
    let mut state = state
        .with_catalog(reader, writer)
        .with_gc(gc.clone())
        .with_audit_sink(audit.clone())
        .with_authenticator(Arc::new(TokenAuthenticator))
        .with_authorization(cedar(&policy(account, other)))
        .with_repairer(Arc::new(Repairer::new(
            origin_view.clone(),
            replica.clone(),
            1 << 20,
        )));
    if let Some(lease) = lease {
        state = state
            .with_content_store(Arc::new(LocatedStore::new(
                origin_view.clone(),
                destination_view.clone(),
                journal.clone(),
            )))
            .with_migrator(Arc::new(
                Migrator::new(
                    origin_view.clone(),
                    destination_view.clone(),
                    1 << 20,
                    1 << 24,
                )
                .with_io_timeout(Duration::from_secs(5)),
            ))
            .with_relocator(Arc::new(
                Relocator::new(origin_view, destination_view, journal, ledger, 1 << 20)
                    .with_io_timeout(Duration::from_secs(5))
                    .with_lease(lease),
            ));
    } else {
        state = state.with_content_store(origin_view);
    }
    Some(Vertical {
        app: build_router(state, &config),
        store,
        cas,
        origin,
        dest,
        destination,
        replica,
        gc,
        audit,
        root,
        a: unique_blob("A, the first artifact to move between backends"),
        b: unique_blob("B, the second artifact, a bit longer than the first one"),
        c: unique_blob("C, the third"),
        d: unique_blob("D, the fourth, held by nobody"),
        e: unique_blob("E, the fifth, held while it is being retired"),
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

fn unique_account() -> String {
    format!("h{}a", &nonce()[8..])
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

async fn download(v: &Vertical, token: &str, repo: &str, name: &str) -> (StatusCode, Vec<u8>) {
    send(
        &v.app,
        Method::GET,
        &format!("/repository/{repo}/{name}"),
        token,
        Vec::new(),
    )
    .await
}

const ACTION: &str = "Artifacts::Action::\"ManageStorageRelocation\"";

fn dg(bytes: &[u8]) -> ContentDigest {
    ContentDigest::of_bytes(bytes)
}

fn digest_of(bytes: &[u8]) -> String {
    dg(bytes).to_string()
}

fn origin_path(v: &Vertical, bytes: &[u8]) -> std::path::PathBuf {
    v.cas.the_path_of(&dg(bytes))
}

fn dest_path(v: &Vertical, bytes: &[u8]) -> std::path::PathBuf {
    v.dest.the_path_of(&dg(bytes))
}

fn in_origin(v: &Vertical, bytes: &[u8]) -> Option<Vec<u8>> {
    std::fs::read(origin_path(v, bytes)).ok()
}

fn in_dest(v: &Vertical, bytes: &[u8]) -> Option<Vec<u8>> {
    std::fs::read(dest_path(v, bytes)).ok()
}

/// Whether the origin copy of `bytes` is kept aside under a quarantine name.
fn quarantined(v: &Vertical, bytes: &[u8]) -> bool {
    let hex = dg(bytes).value;
    std::fs::read_dir(v.root.join("quarantine"))
        .map(|runs| {
            runs.flatten()
                .any(|run| std::fs::read(run.path().join(&hex)).ok().as_deref() == Some(bytes))
        })
        .unwrap_or(false)
}

/// The phase and fence of a digest in the journal.
async fn phase(v: &Vertical, bytes: &[u8]) -> Option<(String, u64)> {
    v.store
        .location_of(&dg(bytes))
        .await
        .expect("the journal answers")
        .map(|entry| (entry.phase.as_str().to_string(), entry.fence))
}

async fn phase_name(v: &Vertical, bytes: &[u8]) -> String {
    phase(v, bytes)
        .await
        .map(|(name, _)| name)
        .unwrap_or_else(|| "origin".to_string())
}

fn results(v: &Vertical) -> Vec<AuditResult> {
    v.audit
        .events()
        .iter()
        .filter(|event| event.action == ACTION)
        .map(|event| event.result)
        .collect()
}

/// v.a repository with v.a, v.b and v.c published.
async fn published(v: &Vertical, account: &str) {
    seed(v, account, "reg").await;
    let alice = format!("alice@{account}");
    upload(v, "reg", &alice, "a.bin", v.a).await;
    upload(v, "reg", &alice, "b.bin", v.b).await;
    upload(v, "reg", &alice, "c.bin", v.c).await;
}

async fn migrate(v: &Vertical, token: &str, blobs: &[&[u8]]) {
    let digests: Vec<String> = blobs.iter().map(|b| digest_of(b)).collect();
    let (status, bytes) = send(
        &v.app,
        Method::POST,
        "/api/v1/storage/migrate",
        token,
        serde_json::to_vec(&json!({"repository": "reg", "digests": digests})).unwrap(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
}

async fn relocate(v: &Vertical, token: &str, op: &str, blobs: &[&[u8]]) -> (StatusCode, Value) {
    let digests: Vec<String> = blobs.iter().map(|b| digest_of(b)).collect();
    relocate_digests(v, token, "reg", op, &digests).await
}

async fn relocate_digests(
    v: &Vertical,
    token: &str,
    repo: &str,
    op: &str,
    digests: &[String],
) -> (StatusCode, Value) {
    let (status, bytes) = send(
        &v.app,
        Method::POST,
        "/api/v1/storage/relocate",
        token,
        serde_json::to_vec(&json!({"repository": repo, "operation": op, "digests": digests}))
            .unwrap(),
    )
    .await;
    (status, json_of(&bytes))
}

fn outcome_of<'a>(answer: &'a Value, bytes: &[u8]) -> &'a str {
    let digest = digest_of(bytes);
    answer["entries"]
        .as_array()
        .and_then(|entries| entries.iter().find(|e| e["digest"] == digest))
        .and_then(|e| e["outcome"].as_str())
        .unwrap_or("<missing>")
}

async fn serves(v: &Vertical, token: &str, name: &str, bytes: &[u8]) {
    let (status, served) = download(v, token, "reg", name).await;
    assert_eq!(
        (status, served.as_slice()),
        (StatusCode::OK, bytes),
        "the registry must keep serving {name}"
    );
}

async fn place(v: &Vertical, scope: HoldScope) -> String {
    v.gc.place_hold("ops", scope, "retention for the test", None)
        .await
        .expect("the hold is placed")
        .id
}

async fn release(v: &Vertical, id: &str) {
    assert!(v.gc.release_hold(id).await.expect("the hold is released"));
}

#[tokio::test]
async fn reads_follow_the_journal_and_the_origin_is_retired_reversibly() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(Duration::from_secs(60))).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    migrate(&v, &alice, &[v.a]).await;
    assert_eq!(phase(&v, v.a).await, None, "no row: the origin serves");

    // Before the switch the reads go to the origin and only the origin.
    let (o0, d0) = (v.origin.calls(), v.destination.calls());
    serves(&v, &alice, "a.bin", v.a).await;
    assert!(v.origin.calls() > o0);
    assert_eq!(v.destination.calls(), d0, "the destination is not read yet");

    // Switch: the reads go to the destination; the origin copy is intact.
    let (status, answer) = relocate(&v, &alice, "switch", &[v.a]).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(outcome_of(&answer, v.a), "switched");
    assert_eq!(answer["origin_deleted"], false);
    assert_eq!(
        phase(&v, v.a).await.map(|p| p.0).as_deref(),
        Some("switched")
    );
    assert_eq!(in_origin(&v, v.a).as_deref(), Some(v.a));
    let (o1, d1) = (v.origin.calls(), v.destination.calls());
    serves(&v, &alice, "a.bin", v.a).await;
    assert!(v.destination.calls() > d1, "the reads follow the journal");
    assert_eq!(v.origin.calls(), o1, "the origin is no longer read");

    // Retire: the origin copy is moved aside, never deleted; still served.
    let (status, answer) = relocate(&v, &alice, "retire", &[v.a]).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(outcome_of(&answer, v.a), "retired");
    assert_eq!(in_origin(&v, v.a), None, "the origin key is free");
    assert!(quarantined(&v, v.a), "the origin bytes are kept aside");
    assert_eq!(in_dest(&v, v.a).as_deref(), Some(v.a));
    assert_eq!(phase_name(&v, v.a).await, "retired");
    serves(&v, &alice, "a.bin", v.a).await;
    // Retiring again is the same as retiring once.
    let (status, answer) = relocate(&v, &alice, "retire", &[v.a]).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(outcome_of(&answer, v.a), "unchanged");

    // Rollback: the origin gets its verified bytes back and the reads follow.
    let (status, answer) = relocate(&v, &alice, "rollback", &[v.a]).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(outcome_of(&answer, v.a), "rolled_back");
    assert_eq!(in_origin(&v, v.a).as_deref(), Some(v.a));
    assert_eq!(phase_name(&v, v.a).await, "origin");
    let (o2, d2) = (v.origin.calls(), v.destination.calls());
    serves(&v, &alice, "a.bin", v.a).await;
    assert!(v.origin.calls() > o2, "the reads are back in the origin");
    assert_eq!(v.destination.calls(), d2);
    assert!(
        results(&v).iter().all(|r| *r == AuditResult::Allowed),
        "{:?}",
        results(&v)
    );

    // v.a switch asked for a digest the destination never received is refused.
    let (status, answer) = relocate(&v, &alice, "switch", &[v.b]).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(outcome_of(&answer, v.b), "destination_absent");
    let (status, answer) = relocate(&v, &alice, "retire", &[v.b]).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(outcome_of(&answer, v.b), "not_switched");
    assert_eq!(in_origin(&v, v.b).as_deref(), Some(v.b));
    assert_eq!(results(&v).last(), Some(&AuditResult::Failed));
}

#[tokio::test]
async fn a_writer_whose_lease_expired_is_fenced_out_by_the_monotonic_token() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", None).await else {
        return;
    };
    let d = dg(format!("fencing-{}", nonce()).as_bytes());
    let journal: &dyn LocationJournal = v.store.as_ref();

    let first = journal
        .acquire_location(&d, "writer-1", Duration::from_millis(400), Utc::now())
        .await
        .expect("the first claim");
    assert_eq!(first.fence, 1);
    // v.a live lease of another owner is exclusion, not a take-over.
    let busy = journal
        .acquire_location(&d, "writer-2", Duration::from_secs(60), Utc::now())
        .await;
    assert!(
        matches!(busy, Err(hodei_catalog::CatalogError::Conflict { .. })),
        "{busy:?}"
    );
    tokio::time::sleep(Duration::from_millis(600)).await;

    // The lease expired: the second writer takes over with a LARGER fence.
    let second = journal
        .acquire_location(&d, "writer-2", Duration::from_secs(60), Utc::now())
        .await
        .expect("the take-over");
    assert_eq!(second.fence, 2);
    assert_ne!(second.token, first.token);
    // Its own renewal keeps token and fence.
    let renewed = journal
        .acquire_location(&d, "writer-2", Duration::from_secs(60), Utc::now())
        .await
        .expect("the renewal");
    assert_eq!((renewed.fence, &renewed.token), (2, &second.token));

    // The old writer cannot advance, whatever it asks for.
    let stale = journal
        .advance_location(&first, Phase::Origin, Phase::Switched)
        .await;
    assert!(
        matches!(
            stale,
            Err(hodei_catalog::CatalogError::PreconditionFailed { .. })
        ),
        "{stale:?}"
    );
    let row = journal.location_of(&d).await.unwrap().expect("the row");
    assert_eq!((row.phase, row.fence), (Phase::Origin, 2));

    // A holder whose lease ran out cannot advance even if nobody took over:
    // the token is right but the lease is not live.
    let lonely = dg(format!("expired-{}", nonce()).as_bytes());
    let short = journal
        .acquire_location(&lonely, "writer-1", Duration::from_millis(300), Utc::now())
        .await
        .expect("a claim");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let expired = journal
        .advance_location(&short, Phase::Origin, Phase::Switched)
        .await;
    assert!(
        matches!(
            expired,
            Err(hodei_catalog::CatalogError::PreconditionFailed { .. })
        ),
        "an expired lease does not advance: {expired:?}"
    );
    assert_eq!(
        journal.location_of(&lonely).await.unwrap().unwrap().phase,
        Phase::Origin
    );

    // The current writer is bound by the phase it expects to leave...
    let wrong_from = journal
        .advance_location(&second, Phase::Switched, Phase::Retired)
        .await;
    assert!(
        matches!(
            wrong_from,
            Err(hodei_catalog::CatalogError::PreconditionFailed { .. })
        ),
        "{wrong_from:?}"
    );
    // ... and by the transitions the journal allows.
    let skipped = journal
        .advance_location(&second, Phase::Origin, Phase::Retired)
        .await;
    assert!(
        matches!(
            skipped,
            Err(hodei_catalog::CatalogError::InvalidRequest { .. })
        ),
        "{skipped:?}"
    );
    journal
        .advance_location(&second, Phase::Origin, Phase::Switched)
        .await
        .expect("the live holder advances");

    // Releasing ends the lease; the next claim still moves the fence UP.
    journal.release_location(&second).await.unwrap();
    let third = journal
        .acquire_location(&d, "writer-3", Duration::from_secs(60), Utc::now())
        .await
        .expect("a claim after the release");
    assert_eq!(third.fence, 3, "the fence never goes down");
    let late = journal
        .advance_location(&second, Phase::Switched, Phase::Origin)
        .await;
    assert!(late.is_err(), "a released holder cannot advance: {late:?}");
    let row = journal.location_of(&d).await.unwrap().expect("the row");
    assert_eq!((row.phase, row.fence), (Phase::Switched, 3));
}

#[tokio::test]
async fn a_stalled_operation_that_loses_its_lease_does_nothing_when_it_resumes() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(Duration::from_secs(60))).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    migrate(&v, &alice, &[v.a]).await;

    // v.a relocator with a short lease whose destination read stalls.
    let origin: Arc<dyn BlobStore> = v.origin.clone();
    let destination: Arc<dyn BlobStore> = v.destination.clone();
    let journal: Arc<dyn LocationJournal> = v.store.clone();
    let ledger: Arc<dyn ContentLedger> = v.store.clone();
    let slow = Arc::new(
        Relocator::new(origin, destination, journal.clone(), ledger, 1 << 20)
            .with_lease(Duration::from_millis(700))
            .with_io_timeout(Duration::from_secs(10)),
    );
    v.destination.get_delay_ms.store(2000, Ordering::SeqCst);
    let task = {
        let slow = slow.clone();
        tokio::spawn(async move {
            let place = slow.reserve("stalled").expect("a place");
            slow.relocate(&place, RelocateOperation::Switch, &[dg(v.a)])
                .await
                .expect("a report")
        })
    };
    // While it is stalled its lease expires and an intruder takes over.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let intruder = journal
        .acquire_location(&dg(v.a), "intruder", Duration::from_secs(60), Utc::now())
        .await
        .expect("the take-over of the expired lease");

    let report = task.await.expect("the task finished");
    assert_eq!(
        report.entries[0].result,
        Relocated::Fenced,
        "the resumed operation must not advance"
    );
    v.destination.get_delay_ms.store(0, Ordering::SeqCst);
    // Nothing moved: the intruder's claim is intact and the origin serves.
    let row = journal.location_of(&dg(v.a)).await.unwrap().expect("a row");
    assert_eq!((row.phase, row.fence), (Phase::Origin, intruder.fence));
    assert_eq!(in_origin(&v, v.a).as_deref(), Some(v.a));
    serves(&v, &alice, "a.bin", v.a).await;
    // The stalled operation's release did not end the intruder's lease.
    journal
        .advance_location(&intruder, Phase::Origin, Phase::Switched)
        .await
        .expect("the live holder still advances");

    // The same for a RETIREMENT. What this proves, exactly: the intruder holds
    // a LIVE lease when the stalled retirement resumes, so the renewal it does
    // right before moving the origin aside is refused (the Conflict branch)
    // and nothing is moved. It does NOT prove the fence-mismatch branch (an
    // intruder whose lease also expired, or a released one), which the
    // journal-level test above covers for `advance_location` only, and it does
    // not cover a zombie that stalls AFTER the renewal (see the docs).
    journal.release_location(&intruder).await.unwrap();
    let (status, _) = relocate(&v, &alice, "rollback", &[v.a]).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = relocate(&v, &alice, "switch", &[v.a]).await;
    assert_eq!(status, StatusCode::OK);
    v.destination.get_delay_ms.store(2000, Ordering::SeqCst);
    let task = {
        let slow = slow.clone();
        let a = v.a;
        tokio::spawn(async move {
            let place = slow.reserve("stalled").expect("a place");
            slow.relocate(&place, RelocateOperation::Retire, &[dg(a)])
                .await
                .expect("a report")
        })
    };
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let second = journal
        .acquire_location(&dg(v.a), "intruder-2", Duration::from_secs(60), Utc::now())
        .await
        .expect("the take-over of the expired lease");
    let report = task.await.expect("the task finished");
    v.destination.get_delay_ms.store(0, Ordering::SeqCst);
    assert_eq!(report.entries[0].result, Relocated::Fenced, "{report:?}");
    assert_eq!(
        in_origin(&v, v.a).as_deref(),
        Some(v.a),
        "the origin stayed"
    );
    assert!(!quarantined(&v, v.a), "nothing was moved aside");
    let row = journal.location_of(&dg(v.a)).await.unwrap().expect("a row");
    assert_eq!((row.phase, row.fence), (Phase::Switched, second.fence));
    serves(&v, &alice, "a.bin", v.a).await;
}

#[tokio::test]
async fn a_failure_half_way_never_costs_the_origin_its_bytes() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(Duration::from_secs(60))).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    migrate(&v, &alice, &[v.a, v.b]).await;

    // The destination goes down while switching: nothing moves.
    v.destination.down.store(true, Ordering::SeqCst);
    let (status, answer) = relocate(&v, &alice, "switch", &[v.a]).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{answer}");
    assert_eq!(outcome_of(&answer, v.a), "destination_failed");
    v.destination.down.store(false, Ordering::SeqCst);
    assert_eq!(phase_name(&v, v.a).await, "origin");
    assert_eq!(in_origin(&v, v.a).as_deref(), Some(v.a));
    serves(&v, &alice, "a.bin", v.a).await;
    assert_eq!(results(&v), vec![AuditResult::Failed]);

    // The process dies between moving the origin aside and writing the
    // journal: the journal still says `switched`, the bytes are kept aside
    // and in the destination, and the registry serves.
    let (status, _) = relocate(&v, &alice, "switch", &[v.a, v.b]).await;
    assert_eq!(status, StatusCode::OK);
    let conditional = v.cas.conditional().expect("a conditional CAS");
    let marker = conditional
        .head_marker(&dg(v.a))
        .await
        .unwrap()
        .expect("a version");
    let key = conditional.quarantine_key_of(&dg(v.a), "crashed");
    conditional
        .quarantine_if_untouched(&dg(v.a), &marker, &key)
        .await
        .expect("the origin is moved aside");
    assert_eq!(phase_name(&v, v.a).await, "switched");
    assert_eq!(in_origin(&v, v.a), None);
    serves(&v, &alice, "a.bin", v.a).await;
    // The retry completes what was left, without inventing an origin copy.
    let (status, answer) = relocate(&v, &alice, "retire", &[v.a]).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(
        answer["entries"][0]["origin_already_absent"], true,
        "{answer}"
    );
    assert_eq!(phase_name(&v, v.a).await, "retired");

    // v.a rollback whose origin cannot be restored leaves the reads where they
    // are (served by the destination) instead of pointing them at nothing.
    v.origin.put_fails.store(true, Ordering::SeqCst);
    let (status, answer) = relocate(&v, &alice, "rollback", &[v.a]).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{answer}");
    assert_eq!(outcome_of(&answer, v.a), "origin_failed");
    assert_eq!(phase_name(&v, v.a).await, "retired");
    serves(&v, &alice, "a.bin", v.a).await;
    v.origin.put_fails.store(false, Ordering::SeqCst);
    let (status, answer) = relocate(&v, &alice, "rollback", &[v.a]).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(in_origin(&v, v.a).as_deref(), Some(v.a));
    assert_eq!(phase_name(&v, v.a).await, "origin");

    // v.a rollback with neither an origin copy nor a verified destination copy
    // refuses: reads never go back to nothing.
    let (status, _) = relocate(&v, &alice, "switch", &[v.b]).await;
    assert_eq!(status, StatusCode::OK);
    std::fs::remove_file(origin_path(&v, v.b)).unwrap();
    std::fs::remove_file(dest_path(&v, v.b)).unwrap();
    let (status, answer) = relocate(&v, &alice, "rollback", &[v.b]).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(outcome_of(&answer, v.b), "nothing_to_restore");
    assert_eq!(phase_name(&v, v.b).await, "switched");
}

#[tokio::test]
async fn a_hold_of_any_scope_blocks_the_retirement_and_an_unrelated_one_does_not() {
    let account = unique_account();
    let other = format!("{account}x");
    let Some(v) = mount(&account, &other, Some(Duration::from_secs(60))).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    upload(&v, "reg", &alice, "d.bin", v.d).await;
    upload(&v, "reg", &alice, "e.bin", v.e).await;
    migrate(&v, &alice, &[v.a, v.b, v.c, v.d, v.e]).await;
    let (status, _) = relocate(&v, &alice, "switch", &[v.a, v.b, v.c, v.d, v.e]).await;
    assert_eq!(status, StatusCode::OK);

    let scopes = [
        (v.a, "digest", HoldScope::Digest { digest: dg(v.a) }),
        (
            v.b,
            "repository",
            HoldScope::Repository {
                account: account.clone(),
                repository: "reg".to_string(),
            },
        ),
        (
            v.c,
            "account",
            HoldScope::Account {
                account: account.clone(),
            },
        ),
    ];
    for (blob, name, scope) in scopes {
        let id = place(&v, scope).await;
        let (status, answer) = relocate(&v, &alice, "retire", &[blob]).await;
        assert_eq!(status, StatusCode::CONFLICT, "{name}: {answer}");
        assert_eq!(outcome_of(&answer, blob), "held", "{name}: {answer}");
        // The id of a hold is shown only to the account it belongs to: the
        // repository and account holds of the caller name themselves, a
        // digest hold (no account) stays opaque.
        let shown = &answer["entries"][0]["hold"];
        if name == "digest" {
            assert!(shown.is_null(), "{name}: {answer}");
        } else {
            assert_eq!(shown, &id, "{name}");
        }
        assert_eq!(in_origin(&v, blob).as_deref(), Some(blob), "{name}");
        assert_eq!(phase_name(&v, blob).await, "switched", "{name}");
        release(&v, &id).await;
    }

    // Holds that do not reach the digest do not block it: another account's
    // repository and account, and a digest hold on another blob.
    let unrelated = [
        place(
            &v,
            HoldScope::Repository {
                account: other.clone(),
                repository: "reg".to_string(),
            },
        )
        .await,
        place(
            &v,
            HoldScope::Account {
                account: other.clone(),
            },
        )
        .await,
        place(&v, HoldScope::Digest { digest: dg(v.b) }).await,
    ];
    let (status, answer) = relocate(&v, &alice, "retire", &[v.d]).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(outcome_of(&answer, v.d), "retired");
    for id in &unrelated {
        release(&v, id).await;
    }

    // v.a hold placed while the origin is being moved aside puts the origin
    // back and leaves the reads where they were.
    let hold_scope = HoldScope::Digest { digest: dg(v.e) };
    *v.origin.cond.hook.lock().unwrap() = Some((v.gc.clone(), hold_scope));
    let (status, answer) = relocate(&v, &alice, "retire", &[v.e]).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(outcome_of(&answer, v.e), "held", "{answer}");
    assert_eq!(
        in_origin(&v, v.e).as_deref(),
        Some(v.e),
        "the origin is back"
    );
    assert_eq!(phase_name(&v, v.e).await, "switched");
    serves(&v, &alice, "e.bin", v.e).await;
    let mid =
        v.gc.holds()
            .await
            .unwrap()
            .into_iter()
            .find(|h| {
                h.reason == "placed mid-retirement"
                    && matches!(&h.scope, HoldScope::Digest { digest } if *digest == dg(v.e))
            })
            .expect("the hold stayed");
    release(&v, &mid.id).await;

    // With no hold left, everything retires.
    let (status, answer) = relocate(&v, &alice, "retire", &[v.a, v.b, v.c, v.e]).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    for blob in [v.a, v.b, v.c, v.d, v.e] {
        assert_eq!(in_origin(&v, blob), None);
        assert_eq!(in_dest(&v, blob).as_deref(), Some(blob));
    }
    serves(&v, &alice, "a.bin", v.a).await;
}

#[tokio::test]
async fn a_destination_that_does_not_verify_blocks_the_switch_and_the_retirement() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(Duration::from_secs(60))).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    migrate(&v, &alice, &[v.a, v.b]).await;

    // Same length would pass a length check: only hashing can tell.
    std::fs::write(dest_path(&v, v.a), vec![b'x'; v.a.len()]).unwrap();
    let (status, answer) = relocate(&v, &alice, "switch", &[v.a]).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(outcome_of(&answer, v.a), "destination_corrupt");
    assert_eq!(phase_name(&v, v.a).await, "origin");
    assert_eq!(in_origin(&v, v.a).as_deref(), Some(v.a));

    // v.a store that lies (hands back other bytes without an error) is caught
    // by the hash made here, not by the store's own check.
    v.destination.lies.store(true, Ordering::SeqCst);
    let (status, answer) = relocate(&v, &alice, "switch", &[v.b]).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(outcome_of(&answer, v.b), "destination_corrupt");
    v.destination.lies.store(false, Ordering::SeqCst);
    assert_eq!(phase_name(&v, v.b).await, "origin");

    // Once the destination is sound the switch goes through.
    std::fs::write(dest_path(&v, v.a), v.a).unwrap();
    let (status, _) = relocate(&v, &alice, "switch", &[v.a, v.b]).await;
    assert_eq!(status, StatusCode::OK);

    // v.a destination that rots AFTER the switch never shadows the good origin
    // and never lets the origin be retired.
    std::fs::write(dest_path(&v, v.a), vec![b'y'; v.a.len()]).unwrap();
    serves(&v, &alice, "a.bin", v.a).await;
    let (status, answer) = relocate(&v, &alice, "retire", &[v.a]).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(outcome_of(&answer, v.a), "destination_corrupt");
    assert_eq!(in_origin(&v, v.a).as_deref(), Some(v.a));
    assert!(!quarantined(&v, v.a));
    assert_eq!(phase_name(&v, v.a).await, "switched");

    // v.a destination that lies at retirement time blocks it as well.
    v.destination.lies.store(true, Ordering::SeqCst);
    let (status, answer) = relocate(&v, &alice, "retire", &[v.b]).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(outcome_of(&answer, v.b), "destination_corrupt");
    v.destination.lies.store(false, Ordering::SeqCst);
    assert_eq!(in_origin(&v, v.b).as_deref(), Some(v.b));

    // v.a destination that lost the copy blocks it too.
    std::fs::remove_file(dest_path(&v, v.b)).unwrap();
    let (status, answer) = relocate(&v, &alice, "retire", &[v.b]).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(outcome_of(&answer, v.b), "destination_absent");
    assert_eq!(in_origin(&v, v.b).as_deref(), Some(v.b));
    serves(&v, &alice, "b.bin", v.b).await;

    // The destination down: not a verification, so no retirement.
    v.destination.down.store(true, Ordering::SeqCst);
    let (status, answer) = relocate(&v, &alice, "retire", &[v.a]).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{answer}");
    assert_eq!(in_origin(&v, v.a).as_deref(), Some(v.a));
    v.destination.down.store(false, Ordering::SeqCst);
}

#[tokio::test]
async fn a_race_with_the_garbage_collection_loses_no_bytes() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(Duration::from_secs(60))).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    migrate(&v, &alice, &[v.a, v.b, v.c]).await;
    let (status, _) = relocate(&v, &alice, "switch", &[v.a, v.b, v.c]).await;
    assert_eq!(status, StatusCode::OK);

    // Several rounds of retire / rollback, each run CONCURRENTLY with a real
    // GC sweep. The interleaving is not forced (no barrier): it proves that
    // the invariants hold whichever order the scheduler picked, not that a
    // particular order (sweep between the hold read and the move) was hit.
    let trio = [v.a, v.b, v.c];
    for round in 0..3 {
        let (retired, swept) = tokio::join!(
            relocate(&v, &alice, "retire", &trio),
            v.gc.sweep_platform("alice", &account)
        );
        swept.expect("the sweep runs");
        assert!(
            retired.0 == StatusCode::OK || retired.0 == StatusCode::MULTI_STATUS,
            "round {round}: {}",
            retired.1
        );
        for (name, blob) in [("a.bin", v.a), ("b.bin", v.b), ("c.bin", v.c)] {
            serves(&v, &alice, name, blob).await;
            assert_eq!(in_dest(&v, blob).as_deref(), Some(blob));
        }
        // The sweep after a retirement finds nothing to delete either: the
        // referenced blobs are rooted, and the aside copies are not its to
        // touch.
        v.gc.sweep_platform("alice", &account).await.unwrap();
        for blob in [v.a, v.b, v.c] {
            if phase_name(&v, blob).await == "retired" {
                assert!(quarantined(&v, blob), "round {round}: bytes kept aside");
            }
            assert!(
                v.store.deletion_of(&dg(blob)).await.unwrap().is_none(),
                "the GC deleted nothing that is referenced"
            );
        }
        let (rolled, swept) = tokio::join!(
            relocate(&v, &alice, "rollback", &trio),
            v.gc.sweep_platform("alice", &account)
        );
        swept.expect("the sweep runs");
        assert_eq!(rolled.0, StatusCode::OK, "round {round}: {}", rolled.1);
        for (name, blob) in [("a.bin", v.a), ("b.bin", v.b), ("c.bin", v.c)] {
            serves(&v, &alice, name, blob).await;
            assert_eq!(in_origin(&v, blob).as_deref(), Some(blob));
        }
        let (status, _) = relocate(&v, &alice, "switch", &[v.a, v.b, v.c]).await;
        assert_eq!(status, StatusCode::OK);
    }
}

#[tokio::test]
async fn a_race_with_the_repair_loses_no_bytes_and_the_journal_keeps_the_reads() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(Duration::from_secs(60))).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    migrate(&v, &alice, &[v.a]).await;
    v.replica.put(v.a).await.expect("the replica is seeded");
    let (status, _) = relocate(&v, &alice, "switch", &[v.a]).await;
    assert_eq!(status, StatusCode::OK);

    // The origin copy rots and the repair and the retirement run together.
    // The interleaving is not forced: either may win, and the assertions
    // below hold for every branch (the branch that happened is checked for
    // consistency with the journal, not chosen).
    let only_a = [v.a];
    std::fs::write(origin_path(&v, v.a), vec![b'x'; v.a.len()]).unwrap();
    let repair_body =
        serde_json::to_vec(&json!({"repository": "reg", "digest": digest_of(v.a)})).unwrap();
    let (retired, repaired) = tokio::join!(
        relocate(&v, &alice, "retire", &only_a),
        send(
            &v.app,
            Method::POST,
            "/api/v1/repair",
            &alice,
            repair_body.clone()
        )
    );
    assert!(
        ["retired", "raced", "origin_failed"].contains(&outcome_of(&retired.1, v.a)),
        "{}",
        retired.1
    );
    assert!(
        matches!(
            repaired.0,
            StatusCode::OK | StatusCode::CONFLICT | StatusCode::SERVICE_UNAVAILABLE
        ),
        "{}",
        repaired.0
    );
    // The branch that happened must agree with the journal: a retirement
    // that reports `retired` moved the journal to `retired`; any other
    // outcome left it in `switched`.
    let branch = outcome_of(&retired.1, v.a).to_string();
    let expected = if branch == "retired" {
        "retired"
    } else {
        "switched"
    };
    assert_eq!(
        phase_name(&v, v.a).await,
        expected,
        "branch {branch} (repair answered {})",
        repaired.0
    );
    eprintln!("repair race: retire={branch} repair={}", repaired.0);
    // Whoever won, nothing was lost: the registry serves and the destination
    // is intact.
    serves(&v, &alice, "a.bin", v.a).await;
    assert_eq!(in_dest(&v, v.a).as_deref(), Some(v.a));

    // The repair after a retirement may resurrect the origin copy; the
    // journal, not the origin, still decides where reads go.
    let (status, _) = relocate(&v, &alice, "retire", &[v.a]).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(&v.app, Method::POST, "/api/v1/repair", &alice, repair_body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(in_origin(&v, v.a).as_deref(), Some(v.a));
    assert_eq!(phase_name(&v, v.a).await, "retired");
    let o = v.origin.calls();
    serves(&v, &alice, "a.bin", v.a).await;
    assert_eq!(v.origin.calls(), o, "the reads still follow the journal");

    // And the rollback then ends with an origin that verifies.
    let (status, answer) = relocate(&v, &alice, "rollback", &[v.a]).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    v.cas.verify(&dg(v.a)).await.expect("the origin verifies");
    assert_eq!(phase_name(&v, v.a).await, "origin");
}

#[tokio::test]
async fn other_accounts_see_absence_and_the_action_is_dedicated() {
    let account = unique_account();
    let other = format!("{account}x");
    let Some(v) = mount(&account, &other, Some(Duration::from_secs(60))).await else {
        return;
    };
    published(&v, &account).await;
    seed(&v, &other, "mine").await;
    let alice = format!("alice@{account}");
    let mallory = format!("mallory@{other}");
    let bob = format!("bob@{account}");
    migrate(&v, &alice, &[v.a, v.b]).await;

    // mallory names alice's repository: the 404 of absence, the same as a
    // repository that does not exist.
    let (status_foreign, body_foreign) =
        relocate_digests(&v, &mallory, "reg", "switch", &[digest_of(v.a)]).await;
    let (status_missing, body_missing) =
        relocate_digests(&v, &mallory, "nope", "switch", &[digest_of(v.a)]).await;
    assert_eq!(status_foreign, StatusCode::NOT_FOUND);
    assert_eq!(
        (status_foreign, &body_foreign),
        (status_missing, &body_missing)
    );
    // mallory names her own repository and alice's digest: also absence.
    let (status, _) = relocate_digests(&v, &mallory, "mine", "switch", &[digest_of(v.a)]).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(phase(&v, v.a).await, None, "nothing moved");

    // One stranger digest refuses the whole request: the valid one stays put.
    let stranger = digest_of(b"nobody references this");
    let (status, _) =
        relocate_digests(&v, &alice, "reg", "switch", &[digest_of(v.a), stranger]).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        phase(&v, v.a).await,
        None,
        "not even the valid digest moved"
    );

    // bob can read, publish and migrate but does not hold the relocation.
    let (status, _) = relocate(&v, &bob, "switch", &[v.a]).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(phase(&v, v.a).await, None);
    let seen = results(&v);
    assert!(seen.contains(&AuditResult::Denied), "{seen:?}");

    // alice can.
    let (status, answer) = relocate(&v, &alice, "switch", &[v.a]).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(results(&v).last(), Some(&AuditResult::Allowed));
    let trace = v
        .audit
        .events()
        .iter()
        .rev()
        .find(|e| e.action == ACTION)
        .map(|e| e.resource.clone())
        .unwrap_or_default();
    assert!(
        trace.contains("op=switch") && trace.contains("switched=1") && trace.contains("set="),
        "{trace}"
    );
}

#[tokio::test]
async fn the_request_is_bounded_and_fails_closed() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(Duration::from_secs(60))).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    migrate(&v, &alice, &[v.a, v.b]).await;

    // Malformed requests are 400 and touch nothing.
    for body in [
        json!({"repository": "reg", "operation": "switch", "digests": []}),
        json!({"repository": "reg", "operation": "switch", "digests": ["nope"]}),
        json!({"repository": "reg", "operation": "delete", "digests": [digest_of(v.a)]}),
        json!({"repository": "reg", "operation": "switch", "digests": [digest_of(v.a)], "x": 1}),
    ] {
        let (status, _) = send(
            &v.app,
            Method::POST,
            "/api/v1/storage/relocate",
            &alice,
            serde_json::to_vec(&body).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }
    let many: Vec<String> = (0..65)
        .map(|i| digest_of(format!("many-{i}").as_bytes()))
        .collect();
    let (status, _) = relocate_digests(&v, &alice, "reg", "switch", &many).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(phase(&v, v.a).await, None);

    // One relocation per account: the second request is refused while the
    // first is in flight, and the place comes back afterwards.
    v.destination.get_delay_ms.store(1500, Ordering::SeqCst);
    let first = {
        let app = v.app.clone();
        let token = alice.clone();
        let body = serde_json::to_vec(
            &json!({"repository": "reg", "operation": "switch", "digests": [digest_of(v.a)]}),
        )
        .unwrap();
        tokio::spawn(async move {
            send(&app, Method::POST, "/api/v1/storage/relocate", &token, body).await
        })
    };
    tokio::time::sleep(Duration::from_millis(500)).await;
    let (status, answer) = relocate(&v, &alice, "switch", &[v.b]).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{answer}");
    assert_eq!(answer["error"], "E_RELOCATE_BUSY");
    let (status, _) = first.await.unwrap();
    assert_eq!(status, StatusCode::OK);
    v.destination.get_delay_ms.store(0, Ordering::SeqCst);
    let (status, _) = relocate(&v, &alice, "switch", &[v.b]).await;
    assert_eq!(status, StatusCode::OK, "the place came back");

    // v.a journal read that cannot be proven fails closed as well: with the
    // origin store down a retirement moves nothing.
    v.origin.down.store(true, Ordering::SeqCst);
    let (status, answer) = relocate(&v, &alice, "retire", &[v.a]).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{answer}");
    assert_eq!(outcome_of(&answer, v.a), "origin_failed");
    v.origin.down.store(false, Ordering::SeqCst);
    assert_eq!(in_origin(&v, v.a).as_deref(), Some(v.a));
}

#[tokio::test]
async fn without_a_destination_there_is_no_relocation() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", None).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    let (status, _) = relocate(&v, &alice, "switch", &[v.a]).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(results(&v), vec![AuditResult::Failed]);
    assert_eq!(phase(&v, v.a).await, None);
    serves(&v, &alice, "a.bin", v.a).await;
}

/// The graph profile itself wires the relocation from the configured
/// destination: no test injection of the relocator or the content store.
#[tokio::test]
async fn the_graph_profile_wires_the_journal_from_the_configured_destination() {
    let Some(graph) = config_from_env() else {
        skip_or_fail();
        return;
    };
    let account = unique_account();
    let a = unique_blob("A, the profile artifact");
    let tmp = tempfile::tempdir().expect("temp dir");
    let cas_dir = tmp.path().join("cas");
    let dest_dir = tmp.path().join("dest");
    std::fs::create_dir_all(&dest_dir).unwrap();
    let mut config = AppConfig::default();
    config.security.raw_api_enabled = true;
    config.security.admin_api_enabled = true;
    config.graph = Some(graph);
    config.content.cas_path = cas_dir.to_string_lossy().into_owned();
    config.content.migration_destination_path = Some(dest_dir.to_string_lossy().into_owned());
    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .expect("bootstrap must succeed")
    .with_authenticator(Arc::new(TokenAuthenticator))
    .with_authorization(cedar(&policy(&account, "zz")));
    let app = build_router(state, &config);
    let alice = format!("alice@{account}");
    let client = Arc::new({
        let mut c = GraphClient::connect(config_from_env().unwrap())
            .await
            .unwrap();
        c.login().await.unwrap();
        c
    });
    let store = DgraphCatalogStore::open(client, Arc::new(AllowAllAuthorizer))
        .await
        .unwrap();
    let mut repo_config = RepositoryConfig::new(DigestScope::Global);
    repo_config.config_settings = json!({});
    store
        .create_repository(
            &account,
            RepositoryDescriptor {
                hrn: repository_hrn("hodei", "artifacts", "eu", &account, "reg"),
                account: account.clone(),
                repo_key: "reg".to_string(),
                format: RepositoryFormat::Raw,
                mode: RepositoryMode::Hosted,
                config: repo_config,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        )
        .await
        .unwrap();
    let call = |method: Method, uri: &'static str, body: Value| {
        let app = app.clone();
        let alice = alice.clone();
        async move {
            send(
                &app,
                method,
                uri,
                &alice,
                serde_json::to_vec(&body).unwrap(),
            )
            .await
        }
    };
    let (status, _) = send(
        &app,
        Method::PUT,
        "/repository/reg/a.bin",
        &alice,
        a.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let body = |op: &str| json!({"repository": "reg", "operation": op, "digests": [digest_of(a)]});
    let (status, _) = call(
        Method::POST,
        "/api/v1/storage/migrate",
        json!({"repository": "reg", "digests": [digest_of(a)]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for op in ["switch", "retire"] {
        let (status, bytes) = call(Method::POST, "/api/v1/storage/relocate", body(op)).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "{op}: {}",
            String::from_utf8_lossy(&bytes)
        );
    }
    let origin = FileBlobStore::new(&cas_dir).unwrap();
    assert!(!origin.the_path_of(&dg(a)).exists(), "the origin is aside");
    let (status, served) = send(
        &app,
        Method::GET,
        "/repository/reg/a.bin",
        &alice,
        Vec::new(),
    )
    .await;
    assert_eq!((status, served.as_slice()), (StatusCode::OK, a));
    let (status, _) = call(Method::POST, "/api/v1/storage/relocate", body("rollback")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(origin.the_path_of(&dg(a)).exists(), "the origin is back");
}

#[tokio::test]
async fn a_hold_of_another_account_blocks_without_naming_itself() {
    let account = unique_account();
    let other = format!("{account}x");
    let Some(v) = mount(&account, &other, Some(Duration::from_secs(60))).await else {
        return;
    };
    published(&v, &account).await;
    seed(&v, &other, "mine").await;
    let alice = format!("alice@{account}");
    let mallory = format!("mallory@{other}");
    // mallory publishes the very same bytes in her own repository, so the
    // blob has two owners.
    upload(&v, "mine", &mallory, "copy.bin", v.a).await;
    migrate(&v, &alice, &[v.a]).await;
    let (status, _) = relocate(&v, &alice, "switch", &[v.a]).await;
    assert_eq!(status, StatusCode::OK);

    let id = place(
        &v,
        HoldScope::Account {
            account: other.clone(),
        },
    )
    .await;
    let (status, answer) = relocate(&v, &alice, "retire", &[v.a]).await;
    // Safe by default: a hold of ANY owner blocks the retirement...
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(outcome_of(&answer, v.a), "held", "{answer}");
    assert_eq!(in_origin(&v, v.a).as_deref(), Some(v.a));
    // ...but alice learns nothing about the other account's hold.
    assert!(answer["entries"][0]["hold"].is_null(), "{answer}");
    assert!(!answer.to_string().contains(&id), "the id leaked: {answer}");
    assert!(!answer.to_string().contains(&other), "{answer}");

    release(&v, &id).await;
    let (status, answer) = relocate(&v, &alice, "retire", &[v.a]).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
}

#[tokio::test]
async fn a_hold_that_appears_when_the_origin_cannot_be_put_back_is_declared() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(Duration::from_secs(60))).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    migrate(&v, &alice, &[v.a]).await;
    let (status, _) = relocate(&v, &alice, "switch", &[v.a]).await;
    assert_eq!(status, StatusCode::OK);

    // A hold appears right after the origin was moved aside, and the origin
    // store stops accepting writes: the copy cannot be put back.
    v.origin
        .cond
        .break_puts_after_hold
        .store(true, Ordering::SeqCst);
    *v.origin.cond.hook.lock().unwrap() =
        Some((v.gc.clone(), HoldScope::Digest { digest: dg(v.a) }));
    let (status, answer) = relocate(&v, &alice, "retire", &[v.a]).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{answer}");
    // The result says it plainly: a hold is in force AND the origin is aside.
    assert_eq!(outcome_of(&answer, v.a), "held_origin_aside", "{answer}");
    assert_eq!(results(&v).last(), Some(&AuditResult::Failed));
    // Nothing is lost: the journal stays on `switched`, the destination
    // serves, and the origin bytes are kept aside, not deleted.
    assert_eq!(phase_name(&v, v.a).await, "switched");
    assert_eq!(in_origin(&v, v.a), None);
    assert!(quarantined(&v, v.a), "the bytes are kept aside");
    assert_eq!(in_dest(&v, v.a).as_deref(), Some(v.a));
    serves(&v, &alice, "a.bin", v.a).await;

    let mid =
        v.gc.holds()
            .await
            .unwrap()
            .into_iter()
            .find(|h| matches!(&h.scope, HoldScope::Digest { digest } if *digest == dg(v.a)))
            .expect("the hold stayed");
    release(&v, &mid.id).await;

    // The way back: the store recovers and the rollback restores the origin
    // from the verified destination, hold or not.
    v.origin.put_fails.store(false, Ordering::SeqCst);
    v.origin
        .cond
        .break_puts_after_hold
        .store(false, Ordering::SeqCst);
    let (status, answer) = relocate(&v, &alice, "rollback", &[v.a]).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(in_origin(&v, v.a).as_deref(), Some(v.a));
    assert_eq!(phase_name(&v, v.a).await, "origin");
    serves(&v, &alice, "a.bin", v.a).await;
}

/// A journal whose answers can be slowed down or refused.
struct Flaky {
    inner: Arc<DgraphCatalogStore>,
    delay_ms: AtomicU64,
    down: AtomicBool,
    asked: AtomicU64,
}

impl Flaky {
    async fn gate(&self) -> Result<(), hodei_catalog::CatalogError> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let delay = self.delay_ms.load(Ordering::SeqCst);
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        if self.down.load(Ordering::SeqCst) {
            return Err(hodei_catalog::CatalogError::Unavailable {
                detail: "the journal is down".to_string(),
            });
        }
        Ok(())
    }
}

#[async_trait]
impl LocationJournal for Flaky {
    async fn location_of(
        &self,
        digest: &ContentDigest,
    ) -> Result<Option<hodei_catalog::LocationEntry>, hodei_catalog::CatalogError> {
        self.gate().await?;
        self.inner.location_of(digest).await
    }
    async fn acquire_location(
        &self,
        digest: &ContentDigest,
        owner: &str,
        lease_for: Duration,
        now: chrono::DateTime<Utc>,
    ) -> Result<hodei_catalog::LocationLease, hodei_catalog::CatalogError> {
        self.inner
            .acquire_location(digest, owner, lease_for, now)
            .await
    }
    async fn advance_location(
        &self,
        lease: &hodei_catalog::LocationLease,
        from: Phase,
        to: Phase,
    ) -> Result<hodei_catalog::LocationEntry, hodei_catalog::CatalogError> {
        self.inner.advance_location(lease, from, to).await
    }
    async fn release_location(
        &self,
        lease: &hodei_catalog::LocationLease,
    ) -> Result<(), hodei_catalog::CatalogError> {
        self.inner.release_location(lease).await
    }
    async fn digest_owners(
        &self,
        digest: &ContentDigest,
    ) -> Result<Vec<(String, String)>, hodei_catalog::CatalogError> {
        self.inner.digest_owners(digest).await
    }
}

#[tokio::test]
async fn a_slow_or_down_journal_degrades_reads_to_the_origin_in_bounded_time() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(Duration::from_secs(60))).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    // a: retired (only the destination holds it live); b: never moved.
    migrate(&v, &alice, &[v.a]).await;
    let (status, _) = relocate(&v, &alice, "switch", &[v.a]).await;
    assert_eq!(status, StatusCode::OK);
    let (status, answer) = relocate(&v, &alice, "retire", &[v.a]).await;
    assert_eq!(status, StatusCode::OK, "{answer}");

    let flaky = Arc::new(Flaky {
        inner: v.store.clone(),
        delay_ms: AtomicU64::new(0),
        down: AtomicBool::new(false),
        asked: AtomicU64::new(0),
    });
    let origin: Arc<dyn BlobStore> = v.origin.clone();
    let destination: Arc<dyn BlobStore> = v.destination.clone();
    let journal: Arc<dyn LocationJournal> = flaky.clone();
    // The limits are validated, never clamped.
    assert!(
        LocatedStore::new(origin.clone(), destination.clone(), journal.clone())
            .with_journal_limits(Duration::from_millis(10), Duration::from_secs(5))
            .is_err()
    );
    assert!(
        LocatedStore::new(origin.clone(), destination.clone(), journal.clone())
            .with_journal_limits(Duration::from_secs(60), Duration::from_secs(5))
            .is_err()
    );
    assert!(
        LocatedStore::new(origin.clone(), destination.clone(), journal.clone())
            .with_journal_limits(Duration::from_millis(300), Duration::from_secs(120))
            .is_err()
    );
    let located = LocatedStore::new(origin, destination, journal)
        .with_journal_limits(Duration::from_millis(300), Duration::from_secs(2))
        .expect("limits in range");

    // Healthy: follows the journal (a is read from the destination).
    let before = v.origin.calls();
    assert_eq!(located.get(&dg(v.a)).await.unwrap(), v.a);
    assert_eq!(
        v.origin.calls(),
        before,
        "a retired digest skips the origin"
    );

    // A journal that hangs: the first read pays one short timeout, not the
    // hang, and still answers correctly (b from the origin, a from the
    // destination after the origin missed).
    flaky.delay_ms.store(5000, Ordering::SeqCst);
    let started = std::time::Instant::now();
    assert_eq!(located.get(&dg(v.b)).await.unwrap(), v.b);
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "the first read waited {:?}",
        started.elapsed()
    );
    let asked = flaky.asked.load(Ordering::SeqCst);
    // The breaker is open: the next reads do not wait for the journal at all.
    let started = std::time::Instant::now();
    assert_eq!(located.get(&dg(v.a)).await.unwrap(), v.a);
    assert_eq!(located.get(&dg(v.b)).await.unwrap(), v.b);
    assert!(
        started.elapsed() < Duration::from_millis(250),
        "reads with the breaker open waited {:?}",
        started.elapsed()
    );
    assert_eq!(
        flaky.asked.load(Ordering::SeqCst),
        asked,
        "the journal is not asked while the breaker is open"
    );

    // A journal that errors fast behaves the same way.
    flaky.delay_ms.store(0, Ordering::SeqCst);
    flaky.down.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(2200)).await;
    assert_eq!(located.get(&dg(v.b)).await.unwrap(), v.b);
    assert_eq!(located.get(&dg(v.a)).await.unwrap(), v.a);

    // Once the journal is back and the cooldown has passed, reads follow it
    // again.
    flaky.down.store(false, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(2200)).await;
    let asked = flaky.asked.load(Ordering::SeqCst);
    let before = v.origin.calls();
    assert_eq!(located.get(&dg(v.a)).await.unwrap(), v.a);
    assert!(flaky.asked.load(Ordering::SeqCst) > asked, "asked again");
    assert_eq!(v.origin.calls(), before, "back to the destination first");
}
