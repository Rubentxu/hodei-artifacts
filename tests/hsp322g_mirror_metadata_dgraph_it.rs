//! HSP-322 tramo g (from HSP-167): rollback and freeze protection of the signed
//! index of an apt proxy mirror, through the real router, the real proxy fetch
//! against a live (mutable) upstream, real OpenPGP signatures, the real Cedar
//! engine and the real Dgraph ledger.
//!
//! What this suite proves (each case is a behaviour of the public surface):
//!
//! - a validly signed, newer `Release` is served and advances the persisted
//!   mark; an older one (still validly signed: a replay) is refused with
//!   `E_METADATA_ROLLBACK` and the mark does not move; the same `Date` with other
//!   content is refused as a fork; the same `Release` again is idempotent;
//! - a frozen upstream is refused: past `Valid-Until`, older than the operator's
//!   `max_metadata_age_secs`, dated in the future, or without a `Date`; none of
//!   them touches the mark;
//! - a `Release` signed by a key that is not the pinned one never advances the
//!   mark (an attacker cannot poison it with a far-future date), and the key that
//!   signs is only ever the operator-pinned one;
//! - the ledger fails closed: a read error or a write error is a 503 and no index
//!   is served;
//! - the mark survives a restart (a second router over the same Dgraph);
//! - lineages are isolated by account and by repository;
//! - the operator surface (`ManageMirrorMetadata`): state reads and the explicit
//!   re-baseline are gated by the dedicated action (403 audited, 404 across
//!   accounts) and the re-baseline is the only way back;
//! - control: without a pinned key the proxy is unauthenticated and the very same
//!   rollback IS served, so the guard is what the assertions above measure.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` (with `HODEI_REQUIRE_IT=1` a missing
//! harness fails instead of skipping); serialize with `--test-threads=1`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::OriginalUri,
    http::{Method, Request, StatusCode, header},
    response::Response,
    routing::get,
};
use cedar_policy::Entities;
use chrono::{DateTime, Utc};
use hodei_artifacts_api::audit::{AuditResult, InMemoryAuditSink};
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::openpgp::MetadataSigner;
use hodei_artifacts_api::proxy::dgraph_ledger::DgraphProvenanceLedger;
use hodei_artifacts_api::proxy::dgraph_metadata_ledger::DgraphMirrorMetadataLedger;
use hodei_artifacts_api::proxy::fetch::{FetchLimits, ProxyFetcher};
use hodei_artifacts_api::proxy::metadata_guard::{
    Head, LedgerError, MetadataKey, MirrorMetadataLedger, Step,
};
use hodei_artifacts_api::proxy::{ProxyPlane, egress::AllowedHost};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use hodei_catalog::{
    AllowAllAuthorizer, CatalogReader, CatalogWriter, DigestScope, RepositoryConfig,
    RepositoryDescriptor, RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_catalog_storage::DgraphCatalogStore;
use hodei_graph_client::{GraphClient, GraphClientConfig};
use hodei_hub::blob::FileBlobStore;
use serde_json::{Value, json};
use sha2::Digest as _;
use tempfile::TempDir;
use tower::ServiceExt;

fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

fn unique_account(tag: &str) -> String {
    format!("g{tag}{}a", &nonce()[8..])
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

fn skip_or_fail() {
    assert!(
        std::env::var("HODEI_REQUIRE_IT").as_deref() != Ok("1"),
        "HODEI_REQUIRE_IT=1 but HODEI_GRAPH_IT_ENDPOINT is not set: a skipped IT is a false green"
    );
    eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
}

/// Tokens are `<user>@<account>`.
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

/// alice: downloads and holds `ManageMirrorMetadata`. bob only downloads.
/// mallory belongs to `other` and holds everything there.
fn cedar(account: &str, other: &str) -> Arc<dyn AuthorizerPort> {
    let policy = format!(
        r#"
permit(principal == Iam::User::"hrn:hodei:iam::{account}:User/alice",
       action in [Artifacts::Action::"DownloadAsset", Artifacts::Action::"ManageMirrorMetadata"],
       resource);
permit(principal == Iam::User::"hrn:hodei:iam::{account}:User/bob",
       action == Artifacts::Action::"DownloadAsset", resource);
permit(principal == Iam::User::"hrn:hodei:iam::{other}:User/mallory",
       action in [Artifacts::Action::"DownloadAsset", Artifacts::Action::"ManageMirrorMetadata"],
       resource);
"#
    );
    Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            &policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .expect("the policy parses"),
    )))
}

// ---------------------------------------------------------------------------
// The upstream: a live HTTP server whose files the test swaps between requests.
// ---------------------------------------------------------------------------

type Files = Arc<Mutex<HashMap<String, Vec<u8>>>>;

struct Upstream {
    port: u16,
    files: Files,
}

async fn serve_file(
    axum::extract::State(files): axum::extract::State<Files>,
    uri: OriginalUri,
) -> Response {
    let path = uri.path().trim_start_matches('/').to_string();
    let found = {
        let mut files = files.lock().unwrap();
        // A `once:` entry is served a single time, then the normal file again:
        // a replay that upstream (or a cache in front of it) serves only once.
        files
            .remove(&format!("once:{path}"))
            .or_else(|| files.get(&path).cloned())
    };
    match found {
        Some(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_LENGTH, bytes.len())
            .body(Body::from(bytes))
            .unwrap(),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap(),
    }
}

impl Upstream {
    async fn start() -> Self {
        let files: Files = Arc::new(Mutex::new(HashMap::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = Router::new()
            .fallback(get(serve_file))
            .with_state(files.clone());
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self { port, files }
    }

    /// Publishes a `Release` (and its detached signature and clearsigned form)
    /// signed by `signer`.
    /// Publishes a signed index for `dist`, listing `deb` (pool path and bytes).
    fn publish_dist(
        &self,
        dist: &str,
        signer: &MetadataSigner,
        date: DateTime<Utc>,
        deb: Option<(&str, &[u8])>,
    ) {
        let mut packages = String::from("Package: other\nVersion: 1.0\nArchitecture: amd64\n\n");
        if let Some((pool, bytes)) = deb {
            packages.push_str(&format!(
                "Package: hello\nVersion: 1.0\nArchitecture: amd64\nFilename: {pool}\nSize: {}\nSHA256: {}\n\n",
                bytes.len(),
                sha256_hex(bytes)
            ));
            self.files
                .lock()
                .unwrap()
                .insert(pool.to_string(), bytes.to_vec());
        }
        let release = format!(
            "Origin: Hodei fixture\nSuite: {dist}\nCodename: {dist}\nDate: {}\nArchitectures: amd64\nComponents: main\nSHA256:\n {} {} main/binary-amd64/Packages\n",
            apt_time(date),
            sha256_hex(packages.as_bytes()),
            packages.len()
        );
        let mut files = self.files.lock().unwrap();
        files.insert(
            format!("dists/{dist}/Release.gpg"),
            signer.sign_detached(release.as_bytes()).unwrap(),
        );
        files.insert(
            format!("dists/{dist}/InRelease"),
            signer.sign_cleartext(&release).unwrap(),
        );
        files.insert(format!("dists/{dist}/Release"), release.into_bytes());
        files.insert(
            format!("dists/{dist}/main/binary-amd64/Packages"),
            packages.into_bytes(),
        );
    }

    /// An `InRelease` of `new` next to a `Release` + `Release.gpg` of `old`:
    /// the inconsistent upstream (or stale mirror in front of it) of HSP-167.
    fn publish_split(&self, signer: &MetadataSigner, new: DateTime<Utc>, old: DateTime<Utc>) {
        self.publish(signer, Some(old), "old");
        let stale: Vec<(String, Vec<u8>)> = {
            let files = self.files.lock().unwrap();
            ["Release", "Release.gpg"]
                .iter()
                .map(|f| {
                    (
                        format!("dists/bookworm/{f}"),
                        files[&format!("dists/bookworm/{f}")].clone(),
                    )
                })
                .collect()
        };
        self.publish(signer, Some(new), "new");
        let mut files = self.files.lock().unwrap();
        for (path, bytes) in stale {
            files.insert(path, bytes);
        }
    }

    fn publish(&self, signer: &MetadataSigner, date: Option<DateTime<Utc>>, extra: &str) {
        self.publish_with(signer, date, None, extra);
    }

    fn publish_with(
        &self,
        signer: &MetadataSigner,
        date: Option<DateTime<Utc>>,
        valid_until: Option<DateTime<Utc>>,
        extra: &str,
    ) {
        let packages = format!("Package: hello\nVersion: 1.0\nArchitecture: amd64\n# {extra}\n\n");
        let mut release =
            String::from("Origin: Hodei fixture\nSuite: stable\nCodename: bookworm\n");
        if let Some(date) = date {
            release.push_str(&format!("Date: {}\n", apt_time(date)));
        }
        if let Some(until) = valid_until {
            release.push_str(&format!("Valid-Until: {}\n", apt_time(until)));
        }
        release.push_str(&format!(
            "Architectures: amd64\nComponents: main\nSHA256:\n {} {} main/binary-amd64/Packages\n",
            sha256_hex(packages.as_bytes()),
            packages.len()
        ));
        let mut files = self.files.lock().unwrap();
        files.insert(
            "dists/bookworm/Release.gpg".into(),
            signer.sign_detached(release.as_bytes()).unwrap(),
        );
        files.insert(
            "dists/bookworm/InRelease".into(),
            signer.sign_cleartext(&release).unwrap(),
        );
        files.insert("dists/bookworm/Release".into(), release.into_bytes());
        files.insert(
            "dists/bookworm/main/binary-amd64/Packages".into(),
            packages.into_bytes(),
        );
    }
}

fn apt_time(when: DateTime<Utc>) -> String {
    when.format("%a, %d %b %Y %H:%M:%S UTC").to_string()
}

fn sha256_hex(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn days_ago(days: i64) -> DateTime<Utc> {
    // One reference instant per run, in whole seconds (the ledger stores
    // seconds): the same `days_ago(n)` is the same instant in every call.
    static NOW: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    let now = *NOW.get_or_init(|| Utc::now().timestamp());
    DateTime::from_timestamp(now - days * 86_400, 0).unwrap()
}

// ---------------------------------------------------------------------------
// A ledger that can be made to fail, as a store that goes away would.
// ---------------------------------------------------------------------------

struct FlakyLedger {
    inner: Arc<dyn MirrorMetadataLedger>,
    fail_head: Arc<AtomicBool>,
    fail_append: Arc<AtomicBool>,
}

#[async_trait]
impl MirrorMetadataLedger for FlakyLedger {
    async fn head(&self, key: &MetadataKey) -> Result<Head, LedgerError> {
        if self.fail_head.load(Ordering::SeqCst) {
            return Err(LedgerError("injected read failure".into()));
        }
        self.inner.head(key).await
    }
    async fn append(
        &self,
        key: &MetadataKey,
        seq: u64,
        step: &Step,
        at: DateTime<Utc>,
    ) -> Result<bool, LedgerError> {
        if self.fail_append.load(Ordering::SeqCst) {
            return Err(LedgerError("injected write failure".into()));
        }
        self.inner.append(key, seq, step, at).await
    }
}

// ---------------------------------------------------------------------------
// The vertical.
// ---------------------------------------------------------------------------

struct Vertical {
    app: Router,
    store: Arc<DgraphCatalogStore>,
    audit: Arc<InMemoryAuditSink>,
    fail_head: Arc<AtomicBool>,
    fail_append: Arc<AtomicBool>,
    #[allow(dead_code)]
    tmp: TempDir,
}

async fn client() -> Option<(Arc<GraphClient>, GraphClientConfig)> {
    let Some(config) = config_from_env() else {
        skip_or_fail();
        return None;
    };
    let mut client = GraphClient::connect(config.clone())
        .await
        .expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    Some((Arc::new(client), config))
}

/// Mounts the service: real router, Dgraph catalog and ledger, real proxy fetch.
async fn mount(account: &str, other: &str, upstream: &Upstream) -> Option<Vertical> {
    let (client, graph) = client().await?;
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
    let provenance = Arc::new(
        DgraphProvenanceLedger::open(client.clone())
            .await
            .expect("the provenance ledger opens"),
    );
    let plane = Arc::new(ProxyPlane::new(
        ProxyFetcher::new(
            hodei_artifacts_api::proxy::egress::EgressPolicy::new(vec![AllowedHost {
                host: "127.0.0.1".to_string(),
                port: Some(upstream.port),
                allow_private: true,
                allow_http: true,
            }])
            .expect("the fixture egress is allowed"),
            FetchLimits {
                connect_timeout: Duration::from_secs(2),
                total_timeout: Duration::from_secs(30),
                max_bytes: 1 << 20,
            },
            4,
        ),
        provenance,
    ));
    let audit = Arc::new(InMemoryAuditSink::new());
    let fail_head = Arc::new(AtomicBool::new(false));
    let fail_append = Arc::new(AtomicBool::new(false));
    let state = state
        .with_catalog(reader, writer)
        .with_content_store(cas)
        .with_proxy_plane(plane)
        .with_audit_sink(audit.clone())
        .with_authenticator(Arc::new(TokenAuthenticator))
        .with_authorization(cedar(account, other));
    // The graph profile wires the real ledger on its own; the test replaces it
    // with the same Dgraph ledger behind a switch that can make it fail.
    let real: Arc<dyn MirrorMetadataLedger> = Arc::new(
        DgraphMirrorMetadataLedger::open(client.clone())
            .await
            .expect("the metadata ledger opens"),
    );
    let state = state.with_mirror_metadata(Arc::new(FlakyLedger {
        inner: real,
        fail_head: fail_head.clone(),
        fail_append: fail_append.clone(),
    }));
    Some(Vertical {
        app: build_router(state, &config),
        store,
        audit,
        fail_head,
        fail_append,
        tmp,
    })
}

/// Seeds an apt proxy repository pinned to `trusted` (or unpinned).
async fn seed_proxy(
    v: &Vertical,
    account: &str,
    key: &str,
    upstream: &Upstream,
    trusted: Option<&str>,
    max_age: Option<u64>,
) {
    let mut config = RepositoryConfig::new(DigestScope::Global);
    let mut apt =
        json!({"distributions": ["bookworm"], "components": ["main"], "architectures": ["amd64"]});
    if let Some(trusted) = trusted {
        apt["trusted_key_asc"] = json!(trusted);
    }
    if let Some(max_age) = max_age {
        apt["max_metadata_age_secs"] = json!(max_age);
    }
    config.config_settings = json!({
        "proxy": {
            "upstream": {
                "id": format!("apt-it-{}", upstream.port),
                "url": format!("http://127.0.0.1:{}/", upstream.port),
                "scopes": ["dists/", "pool/"]
            },
            "cache": {"ttl": {"metadata_secs": 0}}
        },
        "apt": apt
    });
    v.store
        .create_repository(
            account,
            RepositoryDescriptor {
                hrn: repository_hrn("hodei", "artifacts", "eu", account, key),
                account: account.to_string(),
                repo_key: key.to_string(),
                format: RepositoryFormat::Apt,
                mode: RepositoryMode::Proxy,
                config,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        )
        .await
        .expect("the proxy repository is seeded");
}

async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    token: &str,
    body: Value,
) -> (StatusCode, String) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "127.0.0.1")
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    let payload = if body.is_null() {
        Body::empty()
    } else {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        Body::from(body.to_string())
    };
    let response = app
        .clone()
        .oneshot(builder.body(payload).unwrap())
        .await
        .expect("a response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

async fn get_index(v: &Vertical, repo: &str, token: &str, file: &str) -> (StatusCode, String) {
    send(
        &v.app,
        Method::GET,
        &format!("/apt/{repo}/dists/bookworm/{file}"),
        token,
        Value::Null,
    )
    .await
}

async fn mark(v: &Vertical, repo: &str, token: &str) -> (StatusCode, Value) {
    let (status, body) = send(
        &v.app,
        Method::GET,
        &format!("/api/v1/mirror-metadata?repository={repo}&distribution=bookworm"),
        token,
        Value::Null,
    )
    .await;
    (status, serde_json::from_str(&body).unwrap_or(Value::Null))
}

fn signer(uid: &str) -> MetadataSigner {
    MetadataSigner::generate(uid).expect("a signer")
}

const REPO: &str = "apt-proxy";

// ---------------------------------------------------------------------------
// The tests.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_newer_release_advances_and_a_replayed_older_one_is_a_rollback() {
    let (account, other) = (unique_account("a"), unique_account("b"));
    let upstream = Upstream::start().await;
    let key = signer("Upstream fixture");
    let Some(v) = mount(&account, &other, &upstream).await else {
        return;
    };
    seed_proxy(
        &v,
        &account,
        REPO,
        &upstream,
        Some(&key.key_asc().unwrap()),
        None,
    )
    .await;
    let alice = format!("alice@{account}");

    // v1: accepted; the mark is its date.
    let (d1, d2) = (days_ago(3), days_ago(1));
    upstream.publish(&key, Some(d1), "v1");
    let (status, body) = get_index(&v, REPO, &alice, "InRelease").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, state) = mark(&v, REPO, &alice).await;
    assert_eq!(state["sequence"], 1);
    assert_eq!(state["baseline"]["date"], d1.to_rfc3339());

    // the same Release again is idempotent: the mark does not advance.
    let (status, _) = get_index(&v, REPO, &alice, "InRelease").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(mark(&v, REPO, &alice).await.1["sequence"], 1);

    // v2, newer: accepted, the mark advances.
    upstream.publish(&key, Some(d2), "v2");
    let (status, body) = get_index(&v, REPO, &alice, "InRelease").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, state) = mark(&v, REPO, &alice).await;
    assert_eq!(state["sequence"], 2);
    assert_eq!(state["baseline"]["date"], d2.to_rfc3339());

    // v1 replayed: validly signed, but older. Refused on every route that
    // consumes the index, and the mark does not move.
    upstream.publish(&key, Some(d1), "v1");
    for file in ["InRelease", "Release", "main/binary-amd64/Packages"] {
        let (status, body) = get_index(&v, REPO, &alice, file).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{file}: {body}");
        assert!(body.contains("E_METADATA_ROLLBACK"), "{file}: {body}");
    }
    let (_, state) = mark(&v, REPO, &alice).await;
    assert_eq!(state["sequence"], 2);
    assert_eq!(state["baseline"]["date"], d2.to_rfc3339());

    // the same Date with other content is a fork, refused.
    upstream.publish(&key, Some(d2), "v2-forked");
    let (status, body) = get_index(&v, REPO, &alice, "InRelease").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert!(body.contains("E_METADATA_FORK"), "{body}");

    // upstream recovers with a genuinely newer Release: served again.
    upstream.publish(&key, Some(days_ago(0)), "v3");
    let (status, body) = get_index(&v, REPO, &alice, "InRelease").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(mark(&v, REPO, &alice).await.1["sequence"], 3);

    // every refusal is on the audit trail.
    let denied = v
        .audit
        .events()
        .iter()
        .filter(|e| e.action == "artifacts:VerifyMirrorMetadata" && e.result == AuditResult::Denied)
        .count();
    assert!(denied >= 4, "rollback refusals are audited, got {denied}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_frozen_or_undatable_release_is_refused_and_never_moves_the_mark() {
    let (account, other) = (unique_account("c"), unique_account("d"));
    let upstream = Upstream::start().await;
    let key = signer("Upstream fixture");
    let Some(v) = mount(&account, &other, &upstream).await else {
        return;
    };
    // One day of tolerated age.
    seed_proxy(
        &v,
        &account,
        REPO,
        &upstream,
        Some(&key.key_asc().unwrap()),
        Some(86_400),
    )
    .await;
    let alice = format!("alice@{account}");

    // Stale: signed and not expired, but upstream stopped publishing days ago.
    upstream.publish(&key, Some(days_ago(10)), "stale");
    let (status, body) = get_index(&v, REPO, &alice, "InRelease").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert!(body.contains("E_METADATA_FROZEN"), "{body}");

    // Expired by its own Valid-Until.
    upstream.publish_with(&key, Some(days_ago(0)), Some(days_ago(1)), "expired");
    let (status, body) = get_index(&v, REPO, &alice, "InRelease").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert!(body.to_lowercase().contains("expired"), "{body}");

    // From the future: it would lock out every genuine successor.
    upstream.publish(
        &key,
        Some(Utc::now() + chrono::Duration::days(30)),
        "future",
    );
    let (status, body) = get_index(&v, REPO, &alice, "InRelease").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert!(body.contains("E_METADATA_FUTURE"), "{body}");

    // No Date: freshness cannot be established.
    upstream.publish(&key, None, "undated");
    let (status, body) = get_index(&v, REPO, &alice, "InRelease").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert!(body.contains("E_METADATA_NO_DATE"), "{body}");

    // None of the refusals moved the mark.
    let (_, state) = mark(&v, REPO, &alice).await;
    assert_eq!(state["sequence"], 0);
    assert!(state["baseline"].is_null());

    // A fresh Release is served again.
    upstream.publish(&key, Some(days_ago(0)), "fresh");
    let (status, body) = get_index(&v, REPO, &alice, "InRelease").await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_release_signed_by_another_key_is_refused_and_cannot_poison_the_mark() {
    let (account, other) = (unique_account("e"), unique_account("f"));
    let upstream = Upstream::start().await;
    let pinned = signer("Pinned");
    let attacker = signer("Attacker");
    let Some(v) = mount(&account, &other, &upstream).await else {
        return;
    };
    seed_proxy(
        &v,
        &account,
        REPO,
        &upstream,
        Some(&pinned.key_asc().unwrap()),
        None,
    )
    .await;
    let alice = format!("alice@{account}");

    upstream.publish(&pinned, Some(days_ago(2)), "genuine");
    assert_eq!(
        get_index(&v, REPO, &alice, "InRelease").await.0,
        StatusCode::OK
    );

    // Dated the newest it can be, signed by a key the operator never pinned.
    upstream.publish(&attacker, Some(days_ago(0)), "attacker");
    for file in ["InRelease", "Release"] {
        let (status, body) = get_index(&v, REPO, &alice, file).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{file}: {body}");
        assert!(body.contains("signature is invalid"), "{file}: {body}");
    }
    let (_, state) = mark(&v, REPO, &alice).await;
    assert_eq!(
        state["sequence"], 1,
        "the attacker's date never reached the ledger"
    );
    assert_eq!(state["baseline"]["date"], days_ago(2).to_rfc3339());

    // The genuine upstream, with a Date older than the attacker's, still works.
    upstream.publish(&pinned, Some(days_ago(1)), "genuine-2");
    assert_eq!(
        get_index(&v, REPO, &alice, "InRelease").await.0,
        StatusCode::OK
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_ledger_that_cannot_answer_fails_closed() {
    let (account, other) = (unique_account("g"), unique_account("h"));
    let upstream = Upstream::start().await;
    let key = signer("Upstream fixture");
    let Some(v) = mount(&account, &other, &upstream).await else {
        return;
    };
    seed_proxy(
        &v,
        &account,
        REPO,
        &upstream,
        Some(&key.key_asc().unwrap()),
        None,
    )
    .await;
    let alice = format!("alice@{account}");
    upstream.publish(&key, Some(days_ago(1)), "v1");
    assert_eq!(
        get_index(&v, REPO, &alice, "InRelease").await.0,
        StatusCode::OK
    );

    // The mark cannot be read: nothing is served.
    v.fail_head.store(true, Ordering::SeqCst);
    let (status, body) = get_index(&v, REPO, &alice, "InRelease").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(body.contains("E_METADATA_LEDGER"), "{body}");
    v.fail_head.store(false, Ordering::SeqCst);

    // A newer Release whose mark cannot be written is not served either.
    upstream.publish(&key, Some(days_ago(0)), "v2");
    v.fail_append.store(true, Ordering::SeqCst);
    let (status, body) = get_index(&v, REPO, &alice, "InRelease").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    v.fail_append.store(false, Ordering::SeqCst);
    assert_eq!(mark(&v, REPO, &alice).await.1["sequence"], 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_mark_survives_a_restart() {
    let (account, other) = (unique_account("i"), unique_account("j"));
    let upstream = Upstream::start().await;
    let key = signer("Upstream fixture");
    let Some(first) = mount(&account, &other, &upstream).await else {
        return;
    };
    seed_proxy(
        &first,
        &account,
        REPO,
        &upstream,
        Some(&key.key_asc().unwrap()),
        None,
    )
    .await;
    let alice = format!("alice@{account}");
    upstream.publish(&key, Some(days_ago(1)), "v2");
    assert_eq!(
        get_index(&first, REPO, &alice, "InRelease").await.0,
        StatusCode::OK
    );
    drop(first);

    // A brand new process image over the same Dgraph: new router, new state,
    // new ledger instance, empty caches.
    let Some(second) = mount(&account, &other, &upstream).await else {
        return;
    };
    upstream.publish(&key, Some(days_ago(5)), "v1-replayed");
    let (status, body) = get_index(&second, REPO, &alice, "InRelease").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert!(body.contains("E_METADATA_ROLLBACK"), "{body}");
    assert_eq!(mark(&second, REPO, &alice).await.1["sequence"], 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lineages_are_isolated_by_account_and_repository() {
    let (account, other) = (unique_account("k"), unique_account("l"));
    let upstream = Upstream::start().await;
    let key = signer("Upstream fixture");
    let Some(v) = mount(&account, &other, &upstream).await else {
        return;
    };
    let asc = key.key_asc().unwrap();
    seed_proxy(&v, &account, REPO, &upstream, Some(&asc), None).await;
    seed_proxy(&v, &account, "apt-proxy-2", &upstream, Some(&asc), None).await;
    seed_proxy(&v, &other, REPO, &upstream, Some(&asc), None).await;
    let (alice, mallory) = (format!("alice@{account}"), format!("mallory@{other}"));

    // Account A's repository reaches a recent mark.
    upstream.publish(&key, Some(days_ago(0)), "recent");
    assert_eq!(
        get_index(&v, REPO, &alice, "InRelease").await.0,
        StatusCode::OK
    );

    // The same older Release is fine for another repository of the same
    // account and for the same repository name of another account: each
    // lineage has its own mark.
    upstream.publish(&key, Some(days_ago(4)), "older");
    assert_eq!(
        get_index(&v, "apt-proxy-2", &alice, "InRelease").await.0,
        StatusCode::OK
    );
    assert_eq!(
        get_index(&v, REPO, &mallory, "InRelease").await.0,
        StatusCode::OK
    );
    // ... while A's own repository still refuses it.
    assert_eq!(
        get_index(&v, REPO, &alice, "InRelease").await.0,
        StatusCode::BAD_GATEWAY
    );

    // Another account cannot read the state of A's lineage: it only sees its own.
    let (status, state) = mark(&v, REPO, &mallory).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(state["baseline"]["date"], days_ago(4).to_rfc3339());
    let (status, state) = mark(&v, REPO, &alice).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(state["baseline"]["date"], days_ago(0).to_rfc3339());
    // A repository that only exists in the other account is a 404 for A.
    let (status, _) = mark(&v, "ghost", &alice).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_operator_surface_is_gated_and_reset_is_the_only_way_back() {
    let (account, other) = (unique_account("m"), unique_account("n"));
    let upstream = Upstream::start().await;
    let key = signer("Upstream fixture");
    let Some(v) = mount(&account, &other, &upstream).await else {
        return;
    };
    seed_proxy(
        &v,
        &account,
        REPO,
        &upstream,
        Some(&key.key_asc().unwrap()),
        None,
    )
    .await;
    let (alice, bob) = (format!("alice@{account}"), format!("bob@{account}"));

    upstream.publish(&key, Some(days_ago(1)), "current");
    assert_eq!(
        get_index(&v, REPO, &bob, "InRelease").await.0,
        StatusCode::OK
    );

    // bob downloads but does not hold the dedicated action: 403, audited.
    assert_eq!(mark(&v, REPO, &bob).await.0, StatusCode::FORBIDDEN);
    let reset = json!({"repository": REPO, "distribution": "bookworm"});
    let (status, _) = send(
        &v.app,
        Method::POST,
        "/api/v1/mirror-metadata/reset",
        &bob,
        reset.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        mark(&v, REPO, &alice).await.1["sequence"],
        1,
        "bob's reset did nothing"
    );

    // upstream legitimately went back (a restored mirror): refused until the
    // operator re-baselines, explicitly.
    upstream.publish(&key, Some(days_ago(6)), "restored");
    assert_eq!(
        get_index(&v, REPO, &alice, "InRelease").await.0,
        StatusCode::BAD_GATEWAY
    );
    let (status, body) = send(
        &v.app,
        Method::POST,
        "/api/v1/mirror-metadata/reset",
        &alice,
        reset,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, state) = mark(&v, REPO, &alice).await;
    assert_eq!(state["sequence"], 2);
    assert!(state["baseline"].is_null());
    // The reset accepts nothing by itself: the next verified Release becomes the baseline.
    let (status, body) = get_index(&v, REPO, &alice, "InRelease").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, state) = mark(&v, REPO, &alice).await;
    assert_eq!(state["baseline"]["date"], days_ago(6).to_rfc3339());
    // ... and protection resumes from the new baseline.
    upstream.publish(&key, Some(days_ago(8)), "older-again");
    assert_eq!(
        get_index(&v, REPO, &alice, "InRelease").await.0,
        StatusCode::BAD_GATEWAY
    );

    let manage = "Artifacts::Action::\"ManageMirrorMetadata\"";
    let results: Vec<AuditResult> = v
        .audit
        .events()
        .iter()
        .filter(|e| e.action == manage)
        .map(|e| e.result)
        .collect();
    assert!(
        results.contains(&AuditResult::Denied),
        "bob's denials are audited: {results:?}"
    );
    assert!(
        results.contains(&AuditResult::Allowed),
        "the reset is audited: {results:?}"
    );

    // A hosted or missing repository has no lineage to manage.
    let (status, _) = mark(&v, "nope", &alice).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Control: the same rollback, with no key pinned, IS served. This is what makes
/// the assertions above measure the guard and not the fixture.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn control_without_a_pinned_key_the_rollback_is_served() {
    let (account, other) = (unique_account("o"), unique_account("p"));
    let upstream = Upstream::start().await;
    let key = signer("Upstream fixture");
    let Some(v) = mount(&account, &other, &upstream).await else {
        return;
    };
    seed_proxy(&v, &account, REPO, &upstream, None, None).await;
    let alice = format!("alice@{account}");
    upstream.publish(&key, Some(days_ago(0)), "new");
    assert_eq!(
        get_index(&v, REPO, &alice, "InRelease").await.0,
        StatusCode::OK
    );
    upstream.publish(&key, Some(days_ago(9)), "old");
    let (status, body) = get_index(&v, REPO, &alice, "InRelease").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "an unauthenticated mirror has no protection: {body}"
    );
}

/// H1: the Release and Release.gpg routes are judged like InRelease, and what is
/// served is what was judged: an upstream with a new InRelease next to an old
/// Release never gets the old one served.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn release_and_signature_routes_are_judged_and_never_serve_an_unjudged_copy() {
    let (account, other) = (unique_account("q"), unique_account("r"));
    let upstream = Upstream::start().await;
    let key = signer("Upstream fixture");
    let Some(v) = mount(&account, &other, &upstream).await else {
        return;
    };
    seed_proxy(
        &v,
        &account,
        REPO,
        &upstream,
        Some(&key.key_asc().unwrap()),
        None,
    )
    .await;
    let alice = format!("alice@{account}");

    // A consistent index: all three forms are served and none is a fork of
    // another (the digest is that of the verified body, not of the armor).
    upstream.publish(&key, Some(days_ago(2)), "v1");
    for file in [
        "InRelease",
        "Release",
        "Release.gpg",
        "InRelease",
        "Release",
    ] {
        let (status, body) = get_index(&v, REPO, &alice, file).await;
        assert_eq!(status, StatusCode::OK, "{file}: {body}");
    }
    assert_eq!(mark(&v, REPO, &alice).await.1["sequence"], 1);

    // New InRelease, old Release: the clearsigned form advances the mark ...
    upstream.publish_split(&key, days_ago(0), days_ago(2));
    assert_eq!(
        get_index(&v, REPO, &alice, "InRelease").await.0,
        StatusCode::OK
    );
    assert_eq!(mark(&v, REPO, &alice).await.1["sequence"], 2);
    // ... and the old Release (validly signed, older) is a rollback on both of
    // its routes: it is not served unjudged.
    for file in ["Release", "Release.gpg"] {
        let (status, body) = get_index(&v, REPO, &alice, file).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{file}: {body}");
        assert!(body.contains("E_METADATA_ROLLBACK"), "{file}: {body}");
    }
    assert_eq!(mark(&v, REPO, &alice).await.1["sequence"], 2);
}

/// H2 and the race: a replay served once (a stale cache or a mirror that lags
/// for one request) is not served, is not final, and does not poison the next
/// answer: the refused metadata is refetched once and the verdict is about what
/// upstream serves now.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_one_shot_replay_is_never_served_and_does_not_poison_the_answer() {
    let (account, other) = (unique_account("s"), unique_account("t"));
    let upstream = Upstream::start().await;
    let key = signer("Upstream fixture");
    let Some(v) = mount(&account, &other, &upstream).await else {
        return;
    };
    seed_proxy(
        &v,
        &account,
        REPO,
        &upstream,
        Some(&key.key_asc().unwrap()),
        None,
    )
    .await;
    let alice = format!("alice@{account}");
    upstream.publish(&key, Some(days_ago(1)), "v2");
    let current = {
        let files = upstream.files.lock().unwrap();
        files["dists/bookworm/InRelease"].clone()
    };
    assert_eq!(
        get_index(&v, REPO, &alice, "InRelease").await.0,
        StatusCode::OK
    );

    // The next fetch of InRelease returns an old, validly signed copy, once.
    upstream.publish(&key, Some(days_ago(5)), "v1");
    let replay = upstream.files.lock().unwrap()["dists/bookworm/InRelease"].clone();
    {
        let mut files = upstream.files.lock().unwrap();
        files.insert("once:dists/bookworm/InRelease".into(), replay.clone());
        files.insert("dists/bookworm/InRelease".into(), current.clone());
    }
    let (status, body) = get_index(&v, REPO, &alice, "InRelease").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the forced refetch heals it: {body}"
    );
    assert_ne!(
        body.as_bytes(),
        replay.as_slice(),
        "the replayed copy is not what is served"
    );
    assert_eq!(body.as_bytes(), current.as_slice());
    assert_eq!(mark(&v, REPO, &alice).await.1["sequence"], 1);
}

/// H3: a distribution whose index cannot be trusted does not abort the request
/// for a package of another distribution, and nothing is ever served from it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_rolled_back_distribution_does_not_block_the_package_of_another() {
    let (account, other) = (unique_account("u"), unique_account("v"));
    let upstream = Upstream::start().await;
    let key = signer("Upstream fixture");
    let Some(v) = mount(&account, &other, &upstream).await else {
        return;
    };
    // "alpha" sorts before "beta": it is visited first.
    let mut config = RepositoryConfig::new(DigestScope::Global);
    config.config_settings = json!({
        "proxy": {"upstream": {"id": "apt-it", "url": format!("http://127.0.0.1:{}/", upstream.port),
                  "scopes": ["dists/", "pool/"]}, "cache": {"ttl": {"metadata_secs": 0}}},
        "apt": {"distributions": ["alpha", "beta"], "components": ["main"],
                "architectures": ["amd64"], "trusted_key_asc": key.key_asc().unwrap()}
    });
    v.store
        .create_repository(
            &account,
            RepositoryDescriptor {
                hrn: repository_hrn("hodei", "artifacts", "eu", &account, REPO),
                account: account.clone(),
                repo_key: REPO.to_string(),
                format: RepositoryFormat::Apt,
                mode: RepositoryMode::Proxy,
                config,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        )
        .await
        .unwrap();
    let alice = format!("alice@{account}");
    let (good_pool, rolled_pool) = (
        "pool/main/h/hello/hello_1.0_amd64.deb",
        "pool/main/o/old/old_1.0_amd64.deb",
    );
    let (good_deb, rolled_deb) = (
        b"the good deb".to_vec(),
        b"the deb of the rolled back dist".to_vec(),
    );
    upstream.publish_dist("alpha", &key, days_ago(1), Some((rolled_pool, &rolled_deb)));
    upstream.publish_dist("beta", &key, days_ago(1), Some((good_pool, &good_deb)));
    for dist in ["alpha", "beta"] {
        let (status, body) = send(
            &v.app,
            Method::GET,
            &format!("/apt/{REPO}/dists/{dist}/main/binary-amd64/Packages"),
            &alice,
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{dist}: {body}");
    }
    let pool = |path: &str| {
        let app = v.app.clone();
        let alice = alice.clone();
        let uri = format!("/apt/{REPO}/{path}");
        async move { send(&app, Method::GET, &uri, &alice, Value::Null).await }
    };
    assert_eq!(
        pool(good_pool).await.0,
        StatusCode::OK,
        "control: both dists are trusted"
    );
    assert_eq!(pool(rolled_pool).await.0, StatusCode::OK, "control");

    // alpha goes back in time: replayed with an older date.
    upstream.publish_dist("alpha", &key, days_ago(6), Some((rolled_pool, &rolled_deb)));
    let (status, body) = pool(good_pool).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "beta's package is still served: {body}"
    );
    assert_eq!(body.as_bytes(), good_deb.as_slice());
    // Nothing is served on the authority of the rolled back distribution.
    let (status, _) = pool(rolled_pool).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// H7-adjacent and the operator surface: the reset validates the distribution,
/// and Cedar decides before the mode of the repository is revealed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reset_validates_the_distribution_and_cedar_decides_before_the_mode() {
    let (account, other) = (unique_account("w"), unique_account("x"));
    let upstream = Upstream::start().await;
    let key = signer("Upstream fixture");
    let Some(v) = mount(&account, &other, &upstream).await else {
        return;
    };
    seed_proxy(
        &v,
        &account,
        REPO,
        &upstream,
        Some(&key.key_asc().unwrap()),
        None,
    )
    .await;
    v.store
        .create_repository(
            &account,
            RepositoryDescriptor {
                hrn: repository_hrn("hodei", "artifacts", "eu", &account, "apt-hosted"),
                account: account.clone(),
                repo_key: "apt-hosted".to_string(),
                format: RepositoryFormat::Apt,
                mode: RepositoryMode::Hosted,
                config: RepositoryConfig::new(DigestScope::Global),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        )
        .await
        .unwrap();
    let (alice, bob) = (format!("alice@{account}"), format!("bob@{account}"));
    let reset = |repo: &str, dist: &str, token: &str| {
        let app = v.app.clone();
        let (token, body) = (
            token.to_string(),
            json!({"repository": repo, "distribution": dist}),
        );
        async move {
            send(
                &app,
                Method::POST,
                "/api/v1/mirror-metadata/reset",
                &token,
                body,
            )
            .await
            .0
        }
    };
    // A distribution the repository does not serve cannot be reset.
    assert_eq!(reset(REPO, "trixie", &alice).await, StatusCode::BAD_REQUEST);
    assert_eq!(reset(REPO, "../x", &alice).await, StatusCode::BAD_REQUEST);
    // bob does not hold the action: 403 for a hosted repository too, so the
    // mode is not revealed to him; alice, who holds it, learns it is not a proxy.
    assert_eq!(
        reset("apt-hosted", "bookworm", &bob).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        reset("apt-hosted", "bookworm", &alice).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(reset(REPO, "bookworm", &alice).await, StatusCode::OK);
}
