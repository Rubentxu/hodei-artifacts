//! HSP-322 tramo f (from HSP-164): self-healing of one blob of the content store,
//! through the real router over the real Dgraph catalog, a real file CAS, the
//! real Cedar engine and the real garbage-collection service.
//!
//! What this suite proves (each case is a behaviour of `POST /api/v1/repair`):
//!
//! - a corrupt (or missing) blob is repaired from the configured source and the
//!   registry serves the right bytes again; the damaged bytes are kept apart;
//! - a source whose bytes do not hash to the digest (it lies, or is itself
//!   damaged) is rejected and the store is left byte-for-byte as it was;
//! - a sound blob is never overwritten, and the source is not even read;
//! - without the dedicated Cedar action there is no repair (403, audited);
//! - another account (or another repository) cannot repair, nor probe, a digest
//!   it does not reference: the answer is the 404 of absence;
//! - repairing twice is the same as repairing once (and costs no source read);
//! - every failure of the source or of the store fails closed and says so, and a
//!   ceiling on the bytes of one repair is enforced;
//! - a deployment without a repair source offers no repair (501).
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
use hodei_artifacts_api::recovery::Repairer;
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

/// alice: reads, publishes and holds `ManageStorageRepair`. bob can read and
/// publish but does NOT hold the repair action. mallory belongs to another
/// account and holds every repository action there.
fn policy(account: &str, other: &str) -> String {
    format!(
        r#"
permit(principal == Iam::User::"hrn:hodei:iam::{account}:User/alice",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset",
                  Artifacts::Action::"CreateRepository",
                  Artifacts::Action::"ManageStorageRepair"],
       resource);
permit(principal == Iam::User::"hrn:hodei:iam::{account}:User/bob",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset"],
       resource);
permit(principal == Iam::User::"hrn:hodei:iam::{other}:User/mallory",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset",
                  Artifacts::Action::"ManageStorageRepair"],
       resource);
"#
    )
}

/// The content store the router sees: the real file CAS, except that `put` can
/// be made to fail, as a store that fills up or goes away would.
struct FlakyStore {
    inner: Arc<FileBlobStore>,
    put_fails: Arc<AtomicBool>,
}

#[async_trait]
impl BlobStore for FlakyStore {
    async fn put(&self, bytes: &[u8]) -> Result<ContentDigest, BlobError> {
        if self.put_fails.load(Ordering::SeqCst) {
            return Err(BlobError::Unavailable("the store is full".to_string()));
        }
        self.inner.put(bytes).await
    }
    async fn get(&self, digest: &ContentDigest) -> Result<Vec<u8>, BlobError> {
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
    fn reclaimer(&self) -> Option<&dyn BlobReclaim> {
        self.inner.reclaimer()
    }
    fn conditional(&self) -> Option<&dyn ConditionalBlob> {
        self.inner.conditional()
    }
}

/// How the repair source misbehaves.
#[derive(Clone, Copy, PartialEq)]
enum SourceMode {
    Honest,
    /// Answers every read with bytes that do not hash to the digest, without
    /// saying so: a source that lies about its own integrity.
    Lying,
    /// Every call fails, as a network that went down would.
    Down,
}

/// The repair source: a real file directory, plus a way to lie or to go down,
/// and a count of the reads the repair asked of it.
struct Source {
    inner: Arc<FileBlobStore>,
    mode: Arc<Mutex<SourceMode>>,
    reads: Arc<AtomicU64>,
}

#[async_trait]
impl BlobStore for Source {
    async fn put(&self, bytes: &[u8]) -> Result<ContentDigest, BlobError> {
        self.inner.put(bytes).await
    }
    async fn get(&self, digest: &ContentDigest) -> Result<Vec<u8>, BlobError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let mode = *self.mode.lock().unwrap();
        match mode {
            SourceMode::Honest => self.inner.get(digest).await,
            SourceMode::Lying => Ok(b"bytes that are not the content".to_vec()),
            SourceMode::Down => Err(BlobError::Unavailable("network down".to_string())),
        }
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
        self.inner.get_with(digest, policy).await
    }
    async fn length(&self, digest: &ContentDigest) -> Result<u64, BlobError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if *self.mode.lock().unwrap() == SourceMode::Down {
            return Err(BlobError::Unavailable("network down".to_string()));
        }
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

struct Vertical {
    app: Router,
    store: Arc<DgraphCatalogStore>,
    cas: Arc<FileBlobStore>,
    source: Arc<FileBlobStore>,
    mode: Arc<Mutex<SourceMode>>,
    reads: Arc<AtomicU64>,
    put_fails: Arc<AtomicBool>,
    audit: Arc<hodei_artifacts_api::audit::InMemoryAuditSink>,
    #[allow(dead_code)]
    tmp: TempDir,
}

/// `repair_max`: the ceiling of one repair; `None` mounts no repair source.
async fn mount(account: &str, other: &str, repair_max: Option<u64>) -> Option<Vertical> {
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
    let source = Arc::new(FileBlobStore::new(tmp.path().join("replica")).expect("the replica"));
    let put_fails = Arc::new(AtomicBool::new(false));
    let flaky: Arc<dyn BlobStore> = Arc::new(FlakyStore {
        inner: cas.clone(),
        put_fails: put_fails.clone(),
    });
    let mode = Arc::new(Mutex::new(SourceMode::Honest));
    let reads = Arc::new(AtomicU64::new(0));
    let ledger: Arc<dyn ContentLedger> = store.clone();
    let gc = Arc::new(GcService::new(ledger, cas.clone(), Duration::ZERO).expect("the GC service"));
    let audit = Arc::new(hodei_artifacts_api::audit::InMemoryAuditSink::new());
    let mut state = state
        .with_catalog(reader, writer)
        .with_content_store(flaky.clone())
        .with_gc(gc)
        .with_audit_sink(audit.clone())
        .with_authenticator(Arc::new(TokenAuthenticator))
        .with_authorization(cedar(&policy(account, other)));
    if let Some(max) = repair_max {
        let source_view: Arc<dyn BlobStore> = Arc::new(Source {
            inner: source.clone(),
            mode: mode.clone(),
            reads: reads.clone(),
        });
        state = state.with_repairer(Arc::new(Repairer::new(flaky, source_view, max)));
    }
    Some(Vertical {
        app: build_router(state, &config),
        store,
        cas,
        source,
        mode,
        reads,
        put_fails,
        audit,
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

const CONTENT: &[u8] = b"the bytes of a release that must survive a damaged disk";

fn digest_of(bytes: &[u8]) -> String {
    ContentDigest::of_bytes(bytes).to_string()
}

fn path_of(v: &Vertical, bytes: &[u8]) -> std::path::PathBuf {
    v.cas.the_path_of(&ContentDigest::of_bytes(bytes))
}

/// The bytes on the disk right now, exactly as they are (no verification).
fn on_disk(v: &Vertical, bytes: &[u8]) -> Option<Vec<u8>> {
    std::fs::read(path_of(v, bytes)).ok()
}

fn damage(v: &Vertical, bytes: &[u8]) -> Vec<u8> {
    let damaged = b"bit rot".to_vec();
    std::fs::write(path_of(v, bytes), &damaged).expect("the blob is damaged");
    damaged
}

async fn repair(v: &Vertical, token: &str, repo: &str, digest: &str) -> (StatusCode, Value) {
    let (status, bytes) = send(
        &v.app,
        Method::POST,
        "/api/v1/repair",
        token,
        serde_json::to_vec(&json!({"repository": repo, "digest": digest})).unwrap(),
    )
    .await;
    (status, json_of(&bytes))
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

fn results(v: &Vertical, action: &str) -> Vec<AuditResult> {
    v.audit
        .events()
        .iter()
        .filter(|event| event.action == action)
        .map(|event| event.result)
        .collect()
}

const ACTION: &str = "Artifacts::Action::\"ManageStorageRepair\"";

/// A repository with `CONTENT` published and replicated in the source.
async fn published(v: &Vertical, account: &str) -> String {
    seed(v, account, "reg", RepositoryFormat::Raw, json!({})).await;
    let alice = format!("alice@{account}");
    upload(v, "reg", &alice, "release.bin", CONTENT).await;
    v.source.put(CONTENT).await.expect("the replica is seeded");
    digest_of(CONTENT)
}

#[tokio::test]
async fn a_corrupt_blob_is_repaired_from_the_source_and_the_registry_serves_the_right_bytes() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(1 << 20)).await else {
        return;
    };
    let digest = published(&v, &account).await;
    let alice = format!("alice@{account}");
    let damaged = damage(&v, CONTENT);
    let (status, _) = download(&v, &alice, "reg", "release.bin").await;
    assert_ne!(status, StatusCode::OK, "a damaged blob is never served");

    let (status, answer) = repair(&v, &alice, "reg", &digest).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["outcome"], "repaired", "{answer}");
    assert_eq!(answer["was"], "corrupt");
    assert_eq!(answer["digest"], digest);
    assert_eq!(answer["quarantined"], true, "the damaged bytes are kept");
    assert_eq!(answer["bytes_written"], CONTENT.len());

    assert_eq!(on_disk(&v, CONTENT).as_deref(), Some(CONTENT));
    let (status, served) = download(&v, &alice, "reg", "release.bin").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        served, CONTENT,
        "the registry serves the right digest again"
    );
    // The damaged bytes were moved aside, not destroyed.
    let kept = std::fs::read_dir(
        v.cas
            .the_path_of(&ContentDigest::of_bytes(CONTENT))
            .ancestors()
            .nth(3)
            .unwrap()
            .join("quarantine"),
    )
    .map(|runs| {
        runs.flatten()
            .flat_map(|run| {
                std::fs::read_dir(run.path())
                    .into_iter()
                    .flatten()
                    .flatten()
            })
            .any(|entry| std::fs::read(entry.path()).ok().as_deref() == Some(&damaged[..]))
    })
    .unwrap_or(false);
    assert!(kept, "the damaged bytes survive under a quarantine name");
    assert_eq!(results(&v, ACTION), vec![AuditResult::Allowed]);
}

#[tokio::test]
async fn a_missing_blob_is_repaired_too() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(1 << 20)).await else {
        return;
    };
    let digest = published(&v, &account).await;
    let alice = format!("alice@{account}");
    std::fs::remove_file(path_of(&v, CONTENT)).expect("the blob vanishes");

    let (status, answer) = repair(&v, &alice, "reg", &digest).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["was"], "absent");
    assert_eq!(answer["quarantined"], false);
    let (status, served) = download(&v, &alice, "reg", "release.bin").await;
    assert_eq!((status, served.as_slice()), (StatusCode::OK, CONTENT));
}

#[tokio::test]
async fn a_source_whose_bytes_do_not_hash_to_the_digest_is_rejected_and_nothing_is_written() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(1 << 20)).await else {
        return;
    };
    let digest = published(&v, &account).await;
    let alice = format!("alice@{account}");

    // (a) a source that lies: it hands over other bytes as if they were the content.
    *v.mode.lock().unwrap() = SourceMode::Lying;
    let damaged = damage(&v, CONTENT);
    let (status, answer) = repair(&v, &alice, "reg", &digest).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert_eq!(answer["error"], "E_REPAIR_SOURCE_MISMATCH");
    assert_eq!(
        on_disk(&v, CONTENT),
        Some(damaged.clone()),
        "the store is untouched"
    );

    // (b) a source that is itself damaged on disk.
    *v.mode.lock().unwrap() = SourceMode::Honest;
    std::fs::write(
        v.source.the_path_of(&ContentDigest::of_bytes(CONTENT)),
        b"rotten replica",
    )
    .unwrap();
    let (status, answer) = repair(&v, &alice, "reg", &digest).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert_eq!(
        on_disk(&v, CONTENT),
        Some(damaged),
        "the store is untouched"
    );

    // (c) the same for a blob that is missing: nothing appears.
    std::fs::remove_file(path_of(&v, CONTENT)).unwrap();
    let (status, _) = repair(&v, &alice, "reg", &digest).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(on_disk(&v, CONTENT), None, "no bytes were invented");
    assert!(
        results(&v, ACTION)
            .iter()
            .all(|r| *r == AuditResult::Failed)
    );
}

#[tokio::test]
async fn a_sound_blob_is_never_overwritten_and_the_source_is_not_even_read() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(1 << 20)).await else {
        return;
    };
    let digest = published(&v, &account).await;
    let alice = format!("alice@{account}");
    // The source is hostile: if it were consulted it would answer garbage.
    *v.mode.lock().unwrap() = SourceMode::Lying;
    let before = on_disk(&v, CONTENT);
    let reads = v.reads.load(Ordering::SeqCst);

    let (status, answer) = repair(&v, &alice, "reg", &digest).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["outcome"], "already_sound");
    assert_eq!(
        on_disk(&v, CONTENT),
        before,
        "a healthy blob is not rewritten"
    );
    assert_eq!(
        v.reads.load(Ordering::SeqCst),
        reads,
        "the source was not read"
    );
}

#[tokio::test]
async fn without_the_dedicated_cedar_action_there_is_no_repair() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(1 << 20)).await else {
        return;
    };
    let digest = published(&v, &account).await;
    let bob = format!("bob@{account}");
    let damaged = damage(&v, CONTENT);

    let (status, _) = repair(&v, &bob, "reg", &digest).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "reading and publishing do not imply repair"
    );
    assert_eq!(on_disk(&v, CONTENT), Some(damaged));
    assert_eq!(results(&v, ACTION), vec![AuditResult::Denied]);
    let (status, _) = send(
        &v.app,
        Method::POST,
        "/api/v1/repair",
        "nobody",
        b"{}".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn another_account_or_repository_cannot_repair_or_probe_a_digest_it_does_not_reference() {
    let account = unique_account();
    let other = format!("{account}x");
    let Some(v) = mount(&account, &other, Some(1 << 20)).await else {
        return;
    };
    let digest = published(&v, &account).await;
    let alice = format!("alice@{account}");
    let mallory = format!("mallory@{other}");
    seed(&v, &other, "mine", RepositoryFormat::Raw, json!({})).await;
    seed(&v, &account, "empty", RepositoryFormat::Raw, json!({})).await;
    let damaged = damage(&v, CONTENT);
    let reads = v.reads.load(Ordering::SeqCst);

    // The repository of another account does not exist for mallory ...
    let (status_other, body_other) = repair(&v, &mallory, "reg", &digest).await;
    let (status_absent, body_absent) = repair(&v, &mallory, "no-such-repo", &digest).await;
    assert_eq!(status_other, StatusCode::NOT_FOUND);
    assert_eq!((status_other, &body_other), (status_absent, &body_absent));
    // ... her own repository does not reference the digest ...
    let (status, _) = repair(&v, &mallory, "mine", &digest).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // ... and neither does another repository of the owner.
    let (status, _) = repair(&v, &alice, "empty", &digest).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    assert_eq!(on_disk(&v, CONTENT), Some(damaged), "nothing was written");
    assert_eq!(
        v.reads.load(Ordering::SeqCst),
        reads,
        "the source was never consulted"
    );
}

#[tokio::test]
async fn repairing_twice_is_the_same_as_repairing_once() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(1 << 20)).await else {
        return;
    };
    let digest = published(&v, &account).await;
    let alice = format!("alice@{account}");
    damage(&v, CONTENT);

    let (_, first) = repair(&v, &alice, "reg", &digest).await;
    assert_eq!(first["outcome"], "repaired", "{first}");
    let reads = v.reads.load(Ordering::SeqCst);
    let after_first = on_disk(&v, CONTENT);
    let (status, second) = repair(&v, &alice, "reg", &digest).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second["outcome"], "already_sound", "{second}");
    assert_eq!(on_disk(&v, CONTENT), after_first);
    assert_eq!(
        v.reads.load(Ordering::SeqCst),
        reads,
        "the second repair costs no source read"
    );
    assert_eq!(
        results(&v, ACTION),
        vec![AuditResult::Allowed, AuditResult::Allowed]
    );
}

#[tokio::test]
async fn every_failure_of_the_source_or_the_store_fails_closed() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(1 << 20)).await else {
        return;
    };
    let digest = published(&v, &account).await;
    let alice = format!("alice@{account}");
    let damaged = damage(&v, CONTENT);

    // The network to the source is down.
    *v.mode.lock().unwrap() = SourceMode::Down;
    let (status, answer) = repair(&v, &alice, "reg", &digest).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{answer}");
    assert_eq!(on_disk(&v, CONTENT), Some(damaged.clone()));
    *v.mode.lock().unwrap() = SourceMode::Honest;

    // The source holds no copy: unavailable, never a pretended repair.
    let other = b"content nobody replicated";
    seed(&v, &account, "lonely", RepositoryFormat::Raw, json!({})).await;
    upload(&v, "lonely", &alice, "x.bin", other).await;
    std::fs::write(path_of(&v, other), b"damaged").unwrap();
    let (status, answer) = repair(&v, &alice, "lonely", &digest_of(other)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(answer["error"], "E_REPAIR_UNAVAILABLE");

    // The store cannot write: the repair is not claimed, and a retry heals.
    v.put_fails.store(true, Ordering::SeqCst);
    let (status, answer) = repair(&v, &alice, "reg", &digest).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{answer}");
    let (status, _) = download(&v, &alice, "reg", "release.bin").await;
    assert_ne!(
        status,
        StatusCode::OK,
        "a failed repair never reads as healed"
    );
    v.put_fails.store(false, Ordering::SeqCst);
    let (status, answer) = repair(&v, &alice, "reg", &digest).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(on_disk(&v, CONTENT).as_deref(), Some(CONTENT));
}

#[tokio::test]
async fn the_ceiling_on_the_bytes_of_one_repair_is_enforced_before_any_byte_moves() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", Some(8)).await else {
        return;
    };
    let digest = published(&v, &account).await;
    let alice = format!("alice@{account}");
    let damaged = damage(&v, CONTENT);
    let (status, answer) = repair(&v, &alice, "reg", &digest).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{answer}");
    assert_eq!(on_disk(&v, CONTENT), Some(damaged));
}

#[tokio::test]
async fn a_deployment_without_a_repair_source_offers_no_repair() {
    let account = unique_account();
    let Some(v) = mount(&account, "zz", None).await else {
        return;
    };
    let digest = published(&v, &account).await;
    let alice = format!("alice@{account}");
    let damaged = damage(&v, CONTENT);
    let (status, _) = repair(&v, &alice, "reg", &digest).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(on_disk(&v, CONTENT), Some(damaged));
}
