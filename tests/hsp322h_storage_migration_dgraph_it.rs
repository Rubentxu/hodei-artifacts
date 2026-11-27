//! HSP-322 tramo h (from HSP-163): verified migration of blobs between two
//! content backends, through the real router over the real Dgraph catalog, two
//! real file CAS directories, the real Cedar engine and the real GC service.
//!
//! What this suite proves (each case is a behaviour of `POST /api/v1/storage/migrate`):
//!
//! - a migration copies the blobs, verifies them by re-hashing in the
//!   destination, leaves the origin byte-for-byte intact and the registry
//!   serving; the report says what was moved and verified;
//! - repeating it is the same as running it once (nothing is read again);
//! - bytes that do not hash in the origin are never written to the destination
//!   (a damaged origin does not spread) and the repair then heals it;
//! - a destination that fails half way (or lies about having stored) leaves the
//!   origin intact, is never reported verified, and the retry completes;
//! - a damaged copy already in the destination is left as it was, closed;
//! - without the dedicated Cedar action there is no migration (403, audited);
//! - another account, or a digest the repository does not reference, is the 404
//!   of absence and moves nothing, not even the valid digests of the request;
//! - the cost is bounded (digests per request, bytes per blob, bytes per
//!   request with resumable `deferred`, concurrent migrations);
//! - every store failure fails closed; no destination configured is a 501;
//! - the GC never loses what was migrated and the origin is never deleted.
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
use hodei_artifacts_api::recovery::{Migrator, Repairer};
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

/// alice: reads, publishes and holds `ManageStorageMigration` (and the repair).
/// bob can read and publish but does NOT hold the migration action. mallory belongs to another
/// account and holds every repository action there.
fn policy(account: &str, other: &str) -> String {
    format!(
        r#"
permit(principal == Iam::User::"hrn:hodei:iam::{account}:User/alice",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset",
                  Artifacts::Action::"CreateRepository",
                  Artifacts::Action::"ManageStorageRepair",
                  Artifacts::Action::"ManageStorageMigration"],
       resource);
permit(principal == Iam::User::"hrn:hodei:iam::{account}:User/bob",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset"],
       resource);
permit(principal == Iam::User::"hrn:hodei:iam::{other}:User/mallory",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset",
                  Artifacts::Action::"ManageStorageRepair",
                  Artifacts::Action::"ManageStorageMigration"],
       resource);
"#
    )
}

/// A real file CAS that can be made to misbehave, as a disk that fills up, a
/// network that goes down or a store that acknowledges without storing would.
struct Wrapper {
    inner: Arc<FileBlobStore>,
    /// `put` fails for every digest.
    put_fails: Arc<AtomicBool>,
    /// `put` fails for this digest (hex) only.
    put_fails_for: Arc<Mutex<Option<String>>>,
    /// `put` answers `Ok(digest)` without storing anything.
    put_swallows: Arc<AtomicBool>,
    /// `get` answers other bytes without saying so: an origin that lies about
    /// its own integrity (the file store would have said `Corrupted`).
    lies: Arc<AtomicBool>,
    /// Every read, length and verify fails.
    down: Arc<AtomicBool>,
    /// `put` of this digest (hex) never answers.
    put_stalls: Arc<Mutex<Option<String>>>,
    /// How many times the store was asked for bytes.
    reads: Arc<AtomicU64>,
}

impl Wrapper {
    fn new(inner: Arc<FileBlobStore>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            put_fails: Arc::new(AtomicBool::new(false)),
            put_fails_for: Arc::new(Mutex::new(None)),
            put_swallows: Arc::new(AtomicBool::new(false)),
            lies: Arc::new(AtomicBool::new(false)),
            down: Arc::new(AtomicBool::new(false)),
            put_stalls: Arc::new(Mutex::new(None)),
            reads: Arc::new(AtomicU64::new(0)),
        })
    }
    fn check_up(&self) -> Result<(), BlobError> {
        if self.down.load(Ordering::SeqCst) {
            Err(BlobError::Unavailable("the store is down".to_string()))
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl BlobStore for Wrapper {
    async fn put(&self, bytes: &[u8]) -> Result<ContentDigest, BlobError> {
        let digest = ContentDigest::of_bytes(bytes);
        let stalled = self.put_stalls.lock().unwrap().clone();
        if stalled.as_deref() == Some(digest.value.as_str()) {
            std::future::pending::<()>().await;
        }
        if self.put_fails.load(Ordering::SeqCst)
            || self.put_fails_for.lock().unwrap().as_deref() == Some(digest.value.as_str())
        {
            return Err(BlobError::Unavailable("the store is full".to_string()));
        }
        if self.put_swallows.load(Ordering::SeqCst) {
            return Ok(digest);
        }
        self.inner.put(bytes).await
    }
    async fn get(&self, digest: &ContentDigest) -> Result<Vec<u8>, BlobError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.check_up()?;
        if self.lies.load(Ordering::SeqCst) {
            return Ok(b"bytes that are not the content".to_vec());
        }
        self.inner.get(digest).await
    }
    async fn exists(&self, digest: &ContentDigest) -> Result<bool, BlobError> {
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
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.inner.get_with(digest, policy).await
    }
    async fn length(&self, digest: &ContentDigest) -> Result<u64, BlobError> {
        self.check_up()?;
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
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.check_up()?;
        self.inner.verify(digest).await
    }
    fn reclaimer(&self) -> Option<&dyn BlobReclaim> {
        self.inner.reclaimer()
    }
    fn conditional(&self) -> Option<&dyn ConditionalBlob> {
        self.inner.conditional()
    }
}

/// The ceilings the migrator is mounted with.
#[derive(Clone, Copy)]
struct Caps {
    blob: u64,
    request: u64,
}

const BIG: Caps = Caps {
    blob: 1 << 20,
    request: 1 << 24,
};

struct Vertical {
    app: Router,
    store: Arc<DgraphCatalogStore>,
    /// The content store the router and the migration read (the origin).
    cas: Arc<FileBlobStore>,
    origin: Arc<Wrapper>,
    /// The destination backend.
    dest: Arc<FileBlobStore>,
    destination: Arc<Wrapper>,
    /// The replica the repair reads from.
    replica: Arc<FileBlobStore>,
    gc: Arc<GcService>,
    audit: Arc<hodei_artifacts_api::audit::InMemoryAuditSink>,
    #[allow(dead_code)]
    tmp: TempDir,
}

/// `caps`: the migration ceilings; `None` mounts no destination.
async fn mount(account: &str, other: &str, caps: Option<Caps>) -> Option<Vertical> {
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
    let cas = Arc::new(FileBlobStore::new(tmp.path().join("cas")).expect("the CAS"));
    let dest = Arc::new(FileBlobStore::new(tmp.path().join("dest")).expect("the destination"));
    let replica = Arc::new(FileBlobStore::new(tmp.path().join("replica")).expect("the replica"));
    let origin = Wrapper::new(cas.clone());
    let destination = Wrapper::new(dest.clone());
    let ledger: Arc<dyn ContentLedger> = store.clone();
    let gc = Arc::new(GcService::new(ledger, cas.clone(), Duration::ZERO).expect("the GC service"));
    let audit = Arc::new(hodei_artifacts_api::audit::InMemoryAuditSink::new());
    let origin_view: Arc<dyn BlobStore> = origin.clone();
    let mut state = state
        .with_catalog(reader, writer)
        .with_content_store(origin_view.clone())
        .with_gc(gc.clone())
        .with_audit_sink(audit.clone())
        .with_authenticator(Arc::new(TokenAuthenticator))
        .with_authorization(cedar(&policy(account, other)))
        .with_repairer(Arc::new(Repairer::new(
            origin_view.clone(),
            replica.clone(),
            1 << 20,
        )));
    if let Some(caps) = caps {
        state = state.with_migrator(Arc::new(
            Migrator::new(origin_view, destination.clone(), caps.blob, caps.request)
                .with_io_timeout(Duration::from_secs(2)),
        ));
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

const A: &[u8] = b"release A: the first artifact to move between backends";
const B: &[u8] = b"release B: the second artifact, a bit longer than the first one";
const C: &[u8] = b"release C: the third";

fn digest_of(bytes: &[u8]) -> String {
    ContentDigest::of_bytes(bytes).to_string()
}

fn origin_path(v: &Vertical, bytes: &[u8]) -> std::path::PathBuf {
    v.cas.the_path_of(&ContentDigest::of_bytes(bytes))
}

fn dest_path(v: &Vertical, bytes: &[u8]) -> std::path::PathBuf {
    v.dest.the_path_of(&ContentDigest::of_bytes(bytes))
}

fn in_origin(v: &Vertical, bytes: &[u8]) -> Option<Vec<u8>> {
    std::fs::read(origin_path(v, bytes)).ok()
}

fn in_dest(v: &Vertical, bytes: &[u8]) -> Option<Vec<u8>> {
    std::fs::read(dest_path(v, bytes)).ok()
}

fn results(v: &Vertical) -> Vec<AuditResult> {
    v.audit
        .events()
        .iter()
        .filter(|event| event.action == ACTION)
        .map(|event| event.result)
        .collect()
}

const ACTION: &str = "Artifacts::Action::\"ManageStorageMigration\"";

/// A repository with A, B and C published.
async fn published(v: &Vertical, account: &str) {
    seed(v, account, "reg").await;
    let alice = format!("alice@{account}");
    upload(v, "reg", &alice, "a.bin", A).await;
    upload(v, "reg", &alice, "b.bin", B).await;
    upload(v, "reg", &alice, "c.bin", C).await;
}

async fn migrate(v: &Vertical, token: &str, repo: &str, blobs: &[&[u8]]) -> (StatusCode, Value) {
    let digests: Vec<String> = blobs.iter().map(|b| digest_of(b)).collect();
    migrate_digests(v, token, repo, &digests).await
}

async fn migrate_digests(
    v: &Vertical,
    token: &str,
    repo: &str,
    digests: &[String],
) -> (StatusCode, Value) {
    let (status, bytes) = send(
        &v.app,
        Method::POST,
        "/api/v1/storage/migrate",
        token,
        serde_json::to_vec(&json!({"repository": repo, "digests": digests})).unwrap(),
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

#[tokio::test]
async fn blobs_are_copied_verified_and_the_origin_is_left_intact() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(BIG)).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    let (before_a, before_b, before_c) = (in_origin(&v, A), in_origin(&v, B), in_origin(&v, C));
    assert_eq!(in_dest(&v, A), None, "nothing is in the destination yet");

    let (status, answer) = migrate(&v, &alice, "reg", &[A, B, C]).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["migrated"], 3, "{answer}");
    assert_eq!(answer["pending"], 0);
    assert_eq!(answer["source_retained"], true);
    assert_eq!(answer["bytes_copied"], (A.len() + B.len() + C.len()) as u64);
    for blob in [A, B, C] {
        assert_eq!(outcome_of(&answer, blob), "migrated");
        assert_eq!(
            in_dest(&v, blob).as_deref(),
            Some(blob),
            "the copy is exact"
        );
        // The copy hashes to its name: verification by re-hash, not by trust.
        v.dest
            .verify(&ContentDigest::of_bytes(blob))
            .await
            .expect("the destination copy verifies");
    }
    // The origin is byte-for-byte what it was, and the registry still serves.
    assert_eq!(
        (in_origin(&v, A), in_origin(&v, B), in_origin(&v, C)),
        (before_a, before_b, before_c)
    );
    let (status, served) = download(&v, &alice, "reg", "a.bin").await;
    assert_eq!((status, served.as_slice()), (StatusCode::OK, A));
    assert_eq!(results(&v), vec![AuditResult::Allowed]);

    // The GC loses nothing: referenced blobs stay in the origin and the copies
    // stay in the destination (the GC does not even know the destination).
    v.gc.sweep_platform("alice", &account)
        .await
        .expect("a sweep");
    assert_eq!(in_origin(&v, A).as_deref(), Some(A));
    assert_eq!(in_dest(&v, A).as_deref(), Some(A));
}

#[tokio::test]
async fn repeating_a_migration_is_the_same_as_running_it_once_and_reads_nothing() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(BIG)).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    let (status, _) = migrate(&v, &alice, "reg", &[A, B]).await;
    assert_eq!(status, StatusCode::OK);
    let before = in_dest(&v, A);
    let origin_reads = v.origin.reads.load(Ordering::SeqCst);

    // Also a duplicate in the request and a third blob that is new.
    let (status, answer) = migrate(&v, &alice, "reg", &[A, B, A, C]).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["already_present"], 2, "{answer}");
    assert_eq!(answer["migrated"], 1);
    assert_eq!(
        answer["entries"].as_array().unwrap().len(),
        3,
        "deduplicated"
    );
    assert_eq!(outcome_of(&answer, A), "already_present");
    assert_eq!(in_dest(&v, A), before, "a verified copy is not rewritten");
    assert_eq!(
        v.origin.reads.load(Ordering::SeqCst),
        origin_reads + 1,
        "only the new blob was read from the origin"
    );
}

#[tokio::test]
async fn bytes_that_do_not_hash_in_the_origin_are_never_written_and_the_repair_heals_it() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(BIG)).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    v.replica.put(B).await.expect("the replica is seeded");
    // Same length would pass a length check: only hashing can tell.
    let rotten: Vec<u8> = vec![b'x'; B.len()];
    std::fs::write(origin_path(&v, B), &rotten).unwrap();

    let (status, answer) = migrate(&v, &alice, "reg", &[A, B]).await;
    assert_eq!(status, StatusCode::MULTI_STATUS, "{answer}");
    assert_eq!(outcome_of(&answer, A), "migrated");
    assert_eq!(outcome_of(&answer, B), "source_corrupt", "{answer}");
    assert_eq!(answer["pending"], 1);
    assert_eq!(in_dest(&v, B), None, "what does not hash is not written");
    assert_eq!(
        in_origin(&v, B),
        Some(rotten),
        "the migration does not touch the origin"
    );
    assert_eq!(results(&v), vec![AuditResult::Failed]);

    // An origin that lies (hands over other bytes as if they were the content)
    // is caught by the hash made here, not by the store's own check.
    v.origin.lies.store(true, Ordering::SeqCst);
    let (status, answer) = migrate(&v, &alice, "reg", &[C]).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(outcome_of(&answer, C), "source_corrupt", "{answer}");
    assert_eq!(in_dest(&v, C), None, "a lie is not propagated");
    v.origin.lies.store(false, Ordering::SeqCst);

    // The remedy is the repair; afterwards the migration completes.
    let (status, bytes) = send(
        &v.app,
        Method::POST,
        "/api/v1/repair",
        &alice,
        serde_json::to_vec(&json!({"repository": "reg", "digest": digest_of(B)})).unwrap(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    let (status, answer) = migrate(&v, &alice, "reg", &[A, B]).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(outcome_of(&answer, B), "migrated");
    assert_eq!(in_dest(&v, B).as_deref(), Some(B));
}

#[tokio::test]
async fn a_destination_that_fails_half_way_leaves_the_origin_intact_and_the_retry_completes() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(BIG)).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    let origin: Vec<_> = [A, B, C].iter().map(|b| in_origin(&v, b)).collect();
    *v.destination.put_fails_for.lock().unwrap() = Some(ContentDigest::of_bytes(B).value);

    let (status, answer) = migrate(&v, &alice, "reg", &[A, B, C]).await;
    assert_eq!(status, StatusCode::MULTI_STATUS, "{answer}");
    assert_eq!(outcome_of(&answer, A), "migrated");
    assert_eq!(outcome_of(&answer, B), "destination_failed");
    assert_eq!(outcome_of(&answer, C), "migrated");
    assert_eq!(in_dest(&v, B), None);
    let after: Vec<_> = [A, B, C].iter().map(|b| in_origin(&v, b)).collect();
    assert_eq!(origin, after, "the origin is exactly as it was");

    // The destination recovers; the retry only finishes what was left.
    *v.destination.put_fails_for.lock().unwrap() = None;
    let (status, answer) = migrate(&v, &alice, "reg", &[A, B, C]).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(outcome_of(&answer, A), "already_present");
    assert_eq!(outcome_of(&answer, B), "migrated");
    assert_eq!(outcome_of(&answer, C), "already_present");
    assert_eq!(in_dest(&v, B).as_deref(), Some(B));
}

#[tokio::test]
async fn a_store_that_acknowledges_without_storing_is_never_reported_verified() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(BIG)).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    v.destination.put_swallows.store(true, Ordering::SeqCst);

    let (status, answer) = migrate(&v, &alice, "reg", &[A]).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{answer}");
    assert_eq!(outcome_of(&answer, A), "destination_failed", "{answer}");
    assert_eq!(in_dest(&v, A), None);
    assert_eq!(in_origin(&v, A).as_deref(), Some(A));
}

#[tokio::test]
async fn store_failures_fail_closed_and_touch_nothing() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(BIG)).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");

    // The destination is down altogether.
    v.destination.down.store(true, Ordering::SeqCst);
    let (status, answer) = migrate(&v, &alice, "reg", &[A, B]).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{answer}");
    assert_eq!(outcome_of(&answer, A), "destination_failed");
    assert_eq!(in_dest(&v, A), None);
    v.destination.down.store(false, Ordering::SeqCst);

    // The origin is down: nothing is invented.
    v.origin.down.store(true, Ordering::SeqCst);
    let (status, answer) = migrate(&v, &alice, "reg", &[A]).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{answer}");
    assert_eq!(outcome_of(&answer, A), "source_failed");
    assert_eq!(in_dest(&v, A), None);
    v.origin.down.store(false, Ordering::SeqCst);

    // An origin that lost the blob is a conflict, not an invented copy.
    std::fs::remove_file(origin_path(&v, C)).unwrap();
    let (status, answer) = migrate(&v, &alice, "reg", &[C]).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(outcome_of(&answer, C), "source_absent");
    assert_eq!(in_dest(&v, C), None);
    assert!(results(&v).iter().all(|r| *r == AuditResult::Failed));
}

#[tokio::test]
async fn a_damaged_copy_already_in_the_destination_is_left_as_it_was() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(BIG)).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    let path = dest_path(&v, A);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"rotten leftover").unwrap();

    let (status, answer) = migrate(&v, &alice, "reg", &[A, B]).await;
    assert_eq!(status, StatusCode::MULTI_STATUS, "{answer}");
    assert_eq!(outcome_of(&answer, A), "destination_damaged", "{answer}");
    assert_eq!(outcome_of(&answer, B), "migrated");
    assert_eq!(std::fs::read(&path).unwrap(), b"rotten leftover");
    assert_eq!(in_origin(&v, A).as_deref(), Some(A));
}

#[tokio::test]
async fn without_the_dedicated_cedar_action_there_is_no_migration() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(BIG)).await else {
        return;
    };
    published(&v, &account).await;
    let bob = format!("bob@{account}");

    let (status, _) = migrate(&v, &bob, "reg", &[A]).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "reading and publishing do not imply migration"
    );
    assert_eq!(in_dest(&v, A), None);
    assert_eq!(results(&v), vec![AuditResult::Denied]);
    let (status, _) = send(
        &v.app,
        Method::POST,
        "/api/v1/storage/migrate",
        "nobody",
        b"{}".to_vec(),
    )
    .await;
    assert_ne!(status, StatusCode::OK, "no identity, no migration");
}

#[tokio::test]
async fn another_account_or_an_unreferenced_digest_is_a_404_that_moves_nothing() {
    let account = unique_account();
    let other = format!("{account}x");
    let Some(v) = mount(&account, &other, Some(BIG)).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    let mallory = format!("mallory@{other}");
    seed(&v, &other, "mine").await;
    let theirs: &[u8] = b"a blob that belongs to the other account only";
    upload(&v, "mine", &mallory, "t.bin", theirs).await;

    // mallory, with the action in her own account, cannot reach alice's repository.
    let (status, _) = migrate(&v, &mallory, "reg", &[A]).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // alice cannot migrate (nor probe) mallory's digest through her own repository,
    // and a valid digest in the same request is not moved either.
    let (status, answer) = migrate(&v, &alice, "reg", &[A, theirs]).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    let (status_absent, answer_absent) =
        migrate(&v, &alice, "reg", &[A, b"a digest nobody ever published"]).await;
    assert_eq!(
        (status, &answer),
        (status_absent, &answer_absent),
        "a foreign digest looks exactly like an absent one"
    );
    assert_eq!(in_dest(&v, A), None, "nothing moved");
    assert_eq!(in_dest(&v, theirs), None);
    // A repository that does not exist is the same absence.
    let (status, _) = migrate(&v, &alice, "ghost", &[A]).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_cost_is_bounded_and_what_does_not_fit_is_resumable() {
    let account = unique_account();
    // Per blob: fits B (the longest); per request: fits A and C but not also B.
    let caps = Caps {
        blob: B.len() as u64,
        request: (A.len() + C.len()) as u64 + 1,
    };
    let Some(v) = mount(&account, "zz", Some(caps)).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");

    // Too many digests: refused before anything is read.
    let many: Vec<String> = (0..65).map(|i| format!("sha256:{i:064x}")).collect();
    let (status, _) = migrate_digests(&v, &alice, "reg", &many).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = migrate_digests(&v, &alice, "reg", &[]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = migrate_digests(&v, &alice, "reg", &["sha256:nothex".to_string()]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // The request budget: A and C fit, B is deferred, not lost.
    let (status, answer) = migrate(&v, &alice, "reg", &[A, C, B]).await;
    assert_eq!(status, StatusCode::MULTI_STATUS, "{answer}");
    assert_eq!(outcome_of(&answer, A), "migrated");
    assert_eq!(outcome_of(&answer, C), "migrated");
    assert_eq!(outcome_of(&answer, B), "deferred", "{answer}");
    assert_eq!(in_dest(&v, B), None);
    let (status, answer) = migrate(&v, &alice, "reg", &[A, C, B]).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(outcome_of(&answer, B), "migrated");
    assert_eq!(answer["bytes_copied"], B.len() as u64);
}

#[tokio::test]
async fn a_blob_above_the_per_blob_ceiling_is_refused_without_being_read() {
    let account = unique_account();
    let caps = Caps {
        blob: 8,
        request: 1 << 20,
    };
    let Some(v) = mount(&account, "zz", Some(caps)).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    let reads = v.origin.reads.load(Ordering::SeqCst);

    let (status, answer) = migrate(&v, &alice, "reg", &[A]).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(outcome_of(&answer, A), "too_large");
    assert_eq!(
        v.origin.reads.load(Ordering::SeqCst),
        reads,
        "a blob above the ceiling is not read"
    );
    assert_eq!(in_dest(&v, A), None);
}

#[tokio::test]
async fn only_so_many_migrations_run_at_once_and_a_hung_destination_times_out_closed() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(BIG)).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    *v.destination.put_stalls.lock().unwrap() = Some(ContentDigest::of_bytes(A).value);

    let spawn = |blob: &'static [u8]| {
        let (app, token) = (v.app.clone(), alice.clone());
        tokio::spawn(async move {
            let body =
                serde_json::to_vec(&json!({"repository": "reg", "digests": [digest_of(blob)]}))
                    .unwrap();
            send(&app, Method::POST, "/api/v1/storage/migrate", &token, body).await
        })
    };
    let first = spawn(A);
    let second = spawn(A);
    tokio::time::sleep(Duration::from_millis(500)).await;
    // Both permits are held by the hung destination: a third is turned away.
    let (status, answer) = migrate(&v, &alice, "reg", &[B]).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{answer}");
    assert_eq!(answer["error"], "E_MIGRATE_BUSY");

    for hung in [first, second] {
        let (status, bytes) = hung.await.expect("the request task");
        let answer = json_of(&bytes);
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{answer}");
        assert_eq!(outcome_of(&answer, A), "destination_failed");
    }
    assert_eq!(in_dest(&v, A), None);
    assert_eq!(in_origin(&v, A).as_deref(), Some(A));
    // The permits came back.
    *v.destination.put_stalls.lock().unwrap() = None;
    let (status, _) = migrate(&v, &alice, "reg", &[A]).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_deployment_without_a_destination_offers_no_migration() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", None).await else {
        return;
    };
    published(&v, &account).await;
    let alice = format!("alice@{account}");
    let (status, _) = migrate(&v, &alice, "reg", &[A]).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(results(&v), vec![AuditResult::Failed]);
}
