//! HSP-324 (from HSP-167/HST-9): key rotation of an apt proxy mirror with a window and a
//! default `apt.max_metadata_age_secs`, through the real router, the real proxy fetch against a
//! live upstream, real OpenPGP signatures, real Cedar and the real Dgraph ledger.
//!
//! - during the window both the pinned key and the next key are accepted (InRelease, Release +
//!   Release.gpg); from the cutoff (server clock) only the next one, and the retired key's
//!   signature is refused with its own code `E_METADATA_KEY_RETIRED` (audited), also for a
//!   Release the ledger had already accepted before the cutoff; a third key stays a plain
//!   invalid signature; none of the refusals moves the mark;
//! - a client-supplied `Date` header cannot move the cutoff;
//! - a pinned-key proxy without `max_metadata_age_secs` gets the 7 day default, an explicit
//!   value overrides it;
//! - unknown or unreadable apt settings are refused when the repository is declared, and a
//!   stored invalid declaration serves nothing (fail closed).
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` (with `HODEI_REQUIRE_IT=1` a missing harness fails instead
//! of skipping); serialize with `--test-threads=1`.

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
       action in [Artifacts::Action::"DownloadAsset", Artifacts::Action::"ManageMirrorMetadata", Artifacts::Action::"CreateRepository"],
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
    #[allow(dead_code)]
    fail_head: Arc<AtomicBool>,
    #[allow(dead_code)]
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
    extra: Value,
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
    if let Some(extra) = extra.as_object() {
        for (name, value) in extra {
            apt[name] = value.clone();
        }
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

const REPO: &str = "apt-proxy";

fn signer(uid: &str) -> MetadataSigner {
    MetadataSigner::generate(uid).expect("a signer")
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

async fn sequence(v: &Vertical, repo: &str, token: &str) -> Value {
    let (_, body) = send(
        &v.app,
        Method::GET,
        &format!("/api/v1/mirror-metadata?repository={repo}&distribution=bookworm"),
        token,
        Value::Null,
    )
    .await;
    serde_json::from_str::<Value>(&body).unwrap_or(Value::Null)["sequence"].clone()
}

fn rotation(next: &MetadataSigner, cutoff: DateTime<Utc>) -> Value {
    json!({"key_rotation": {"next_key_asc": next.key_asc().unwrap(), "cutoff": cutoff.to_rfc3339()}})
}

/// The operator edits the repository configuration (what a rotation is).
async fn set_cutoff(
    v: &Vertical,
    account: &str,
    old: &MetadataSigner,
    next: &MetadataSigner,
    cutoff: DateTime<Utc>,
) {
    let mut config = v
        .store
        .repository(account, REPO)
        .await
        .expect("the repository is read")
        .config;
    config.config_settings["apt"]["trusted_key_asc"] = json!(old.key_asc().unwrap());
    config.config_settings["apt"]["key_rotation"] = rotation(next, cutoff)["key_rotation"].clone();
    v.store
        .update_config(account, REPO, config)
        .await
        .expect("the configuration is updated");
}

fn retired_refusals(v: &Vertical) -> usize {
    v.audit
        .events()
        .iter()
        .filter(|e| e.action == "artifacts:VerifyMirrorMetadata" && e.result == AuditResult::Denied)
        .count()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn both_keys_in_the_window_and_only_the_new_one_after_the_cutoff() {
    let (account, other) = (unique_account("r"), unique_account("s"));
    let upstream = Upstream::start().await;
    let (old, next, third) = (signer("Old"), signer("Next"), signer("Third"));
    let Some(v) = mount(&account, &other, &upstream).await else {
        return;
    };
    seed_proxy(
        &v,
        &account,
        REPO,
        &upstream,
        Some(&old.key_asc().unwrap()),
        None,
        rotation(&next, Utc::now() + chrono::Duration::hours(1)),
    )
    .await;
    let alice = format!("alice@{account}");

    // Window open: the pinned key is accepted on every route.
    upstream.publish(&old, Some(days_ago(3)), "v1");
    for file in ["InRelease", "Release", "Release.gpg"] {
        let (status, body) = get_index(&v, REPO, &alice, file).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "old key in the window, {file}: {body}"
        );
    }
    assert_eq!(sequence(&v, REPO, &alice).await, 1);

    // Window open: the new key is accepted too and advances the mark.
    upstream.publish(&next, Some(days_ago(2)), "v2");
    for file in ["InRelease", "Release", "Release.gpg"] {
        let (status, body) = get_index(&v, REPO, &alice, file).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "new key in the window, {file}: {body}"
        );
    }
    assert_eq!(sequence(&v, REPO, &alice).await, 2);

    // A key that is neither stays an invalid signature (not the retired code).
    upstream.publish(&third, Some(days_ago(1)), "v3");
    let (status, body) = get_index(&v, REPO, &alice, "InRelease").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert!(body.contains("signature is invalid"), "{body}");
    assert!(!body.contains("E_METADATA_KEY_RETIRED"), "{body}");

    // A client cannot move the cutoff with a date of its own.
    upstream.publish(&old, Some(days_ago(1)), "v3-old");
    let request = Request::builder()
        .method(Method::GET)
        .uri(format!("/apt/{REPO}/dists/bookworm/InRelease"))
        .header(header::HOST, "127.0.0.1")
        .header(header::DATE, "Fri, 01 Jan 2100 00:00:00 GMT")
        .header(header::AUTHORIZATION, format!("Bearer {alice}"))
        .body(Body::empty())
        .unwrap();
    let status = v.app.clone().oneshot(request).await.unwrap().status();
    assert_eq!(
        status,
        StatusCode::OK,
        "a client Date header is not the clock"
    );

    // The cutoff passes (server clock): from now on only the new key.
    set_cutoff(
        &v,
        &account,
        &old,
        &next,
        Utc::now() - chrono::Duration::seconds(1),
    )
    .await;
    let mark_before = sequence(&v, REPO, &alice).await;
    let refused_before = retired_refusals(&v);

    // The old key, with a newer Release: retired, every route, own code.
    upstream.publish(&old, Some(days_ago(0)), "v4-old");
    for file in ["InRelease", "Release", "Release.gpg"] {
        let (status, body) = get_index(&v, REPO, &alice, file).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{file}: {body}");
        assert!(body.contains("E_METADATA_KEY_RETIRED"), "{file}: {body}");
    }
    // A Release the old key signed and the ledger accepted BEFORE the cutoff
    // (its Date is the baseline) is not served after it either.
    upstream.publish(&old, Some(days_ago(1)), "v3-old");
    let (status, body) = get_index(&v, REPO, &alice, "InRelease").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert!(body.contains("E_METADATA_KEY_RETIRED"), "{body}");
    assert_eq!(
        sequence(&v, REPO, &alice).await,
        mark_before,
        "a retired key never moves the mark"
    );
    assert!(
        retired_refusals(&v) >= refused_before + 4,
        "retired refusals are audited"
    );

    // The new key keeps working; re-signing the same content is idempotent.
    upstream.publish(&next, Some(days_ago(1)), "v3-old");
    let (status, body) = get_index(&v, REPO, &alice, "InRelease").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(sequence(&v, REPO, &alice).await, mark_before);
    upstream.publish(&next, Some(days_ago(0)), "v4");
    let (status, body) = get_index(&v, REPO, &alice, "InRelease").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        sequence(&v, REPO, &alice).await,
        mark_before.as_u64().unwrap() + 1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_default_max_metadata_age_applies_to_a_pinned_proxy_and_can_be_overridden() {
    let (account, other) = (unique_account("t"), unique_account("u"));
    let upstream = Upstream::start().await;
    let key = signer("Upstream fixture");
    let Some(v) = mount(&account, &other, &upstream).await else {
        return;
    };
    let pinned = key.key_asc().unwrap();
    seed_proxy(
        &v,
        &account,
        "default-age",
        &upstream,
        Some(&pinned),
        None,
        json!({}),
    )
    .await;
    seed_proxy(
        &v,
        &account,
        "long-age",
        &upstream,
        Some(&pinned),
        Some(30 * 86_400),
        json!({}),
    )
    .await;
    let alice = format!("alice@{account}");

    // 10 days old: older than the 7 day default, frozen.
    upstream.publish(&key, Some(days_ago(10)), "old");
    let (status, body) = get_index(&v, "default-age", &alice, "InRelease").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert!(body.contains("E_METADATA_FROZEN"), "{body}");
    // the explicit operator value wins.
    let (status, body) = get_index(&v, "long-age", &alice, "InRelease").await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // 6 days old: inside the default.
    upstream.publish(&key, Some(days_ago(6)), "recent");
    let (status, body) = get_index(&v, "default-age", &alice, "InRelease").await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unknown_or_unreadable_apt_settings_are_refused_and_a_stored_one_fails_closed() {
    let (account, other) = (unique_account("v"), unique_account("w"));
    let upstream = Upstream::start().await;
    let (old, next) = (signer("Old"), signer("Next"));
    let Some(v) = mount(&account, &other, &upstream).await else {
        return;
    };
    let alice = format!("alice@{account}");
    let proxy = |name: &str, apt: Value| {
        json!({
            "name": name, "format": "apt", "mode": "proxy",
            "settings": {
                "proxy": {"upstream": {"id": "u", "url": "http://127.0.0.1:1/", "scopes": ["dists/"]}},
                "apt": apt
            }
        })
    };
    let create = |body: Value| {
        let app = v.app.clone();
        let alice = alice.clone();
        async move {
            send(
                &app,
                Method::POST,
                "/api/v1/catalog/repositories",
                &alice,
                body,
            )
            .await
        }
    };
    let (old_asc, next_asc) = (old.key_asc().unwrap(), next.key_asc().unwrap());
    let cutoff = (Utc::now() + chrono::Duration::days(1)).to_rfc3339();
    let rot = |next: &str, cutoff: &str| json!({"next_key_asc": next, "cutoff": cutoff});

    let bad = [
        (
            "unknown-field",
            json!({"trusted_key_asc": old_asc, "key_rotaton": {}}),
        ),
        (
            "garbage-next",
            json!({"trusted_key_asc": old_asc, "key_rotation": rot("not a key", &cutoff)}),
        ),
        (
            "bad-cutoff",
            json!({"trusted_key_asc": old_asc, "key_rotation": rot(&next_asc, "tomorrow")}),
        ),
        (
            "no-pinned",
            json!({"key_rotation": rot(&next_asc, &cutoff)}),
        ),
        (
            "same-key",
            json!({"trusted_key_asc": old_asc, "key_rotation": rot(&old_asc, &cutoff)}),
        ),
        ("garbage-pinned", json!({"trusted_key_asc": "nope"})),
    ];
    for (name, apt) in bad {
        let (status, body) = create(proxy(name, apt)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name}: {body}");
    }
    let (status, body) = create(proxy(
        "good",
        json!({"trusted_key_asc": old_asc, "key_rotation": rot(&next_asc, &cutoff)}),
    ))
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // A declaration that got stored invalid anyway serves nothing.
    seed_proxy(
        &v,
        &account,
        "stored-bad",
        &upstream,
        Some(&old_asc),
        None,
        json!({"key_rotation": {"next_key_asc": next_asc, "cutoff": "tomorrow"}}),
    )
    .await;
    upstream.publish(&old, Some(days_ago(1)), "v1");
    let (status, body) = get_index(&v, "stored-bad", &alice, "InRelease").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
}
