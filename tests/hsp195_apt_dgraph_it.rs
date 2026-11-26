//! HSP-195: Debian APT sobre el motor Dgraph real, con el cliente `apt`
//! de verdad y las firmas verificadas.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` like the spine ITs (TLS + ACL, the
//! live h186 server). The apt vertical (`src/handlers/apt.rs` +
//! `crates/hodei-catalog/src/apt.rs`) feeds its indices and its signed
//! `Release`/`Release.gpg` metadata exclusively through the catalog ports
//! (HSP-187/190/191/193: keyed guards, atomic outbox, scoped listings),
//! and this slice demonstrates that against the ENGINE:
//!
//! - **AC1** the real apt client (`apt 2.6`, `dpkg 1.21`, `gpgv` from the
//!   pinned `debian:bookworm`) updates the index and installs a published
//!   package over the vertical backed by `DgraphCatalogStore`: the
//!   `Packages` index is assembled from immutable records listed through
//!   `assets_under`, the `Release` SHA256 lines pin the exact served bytes,
//!   `Release.gpg` opens against the served `key.asc` (host `gpgv` oracle
//!   and the container's own verification), and a Debian version carrying
//!   `+` and `~` (`1.0.0~rc1+dfsg`) publishes and resolves — the guard
//!   charset fix this slice adds.
//! - **AC2** a foreign repository never looks valid (scoped listings and
//!   404 downloads), a missing record blob takes the indices down (404,
//!   never stale-valid), a publish whose content is not in the CAS is
//!   refused by the probe with no catalog rows, a stranger keyring is
//!   refused by real `apt-get update`, and `InRelease`, a mismatched pool
//!   path and other bytes on a published path are 404/409 as the contract
//!   says. The outbox census is per tenant (`eq(hodei.catalog.outbox.tenant)`,
//!   the HSP-220 lesson) and the plan probe confirms the exact indexes.
//! - Contract reads: the published apt state seeded at store level answers
//!   what the grammar and the publication say (rows, digests and the
//!   assembled `Packages` bytes), and an immutable path conflicts.

mod support;

use std::collections::HashMap;
use std::io::Write as _;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use axum::Router;
use axum::body::Body;
use axum::extract::OriginalUri;
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::routing::get;
use base64::Engine as _;
use cedar_policy::Entities;
use chrono::Utc;
use flate2::Compression;
use flate2::write::GzEncoder;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::blob_probe::BlobStoreProbe;
use hodei_artifacts_api::proxy::dgraph_ledger::DgraphProvenanceLedger;
use hodei_artifacts_api::proxy::fetch::{FetchLimits, ProxyFetcher};
use hodei_artifacts_api::proxy::{ProxyPlane, egress::AllowedHost};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use hodei_catalog::apt;
use hodei_catalog::{
    AllowAllAuthorizer, AptCasReference, AptGroupSnapshot, AptSnapshotKey, AptSnapshotStore,
    CatalogReader, CatalogWriter, DigestScope, PackageCoordinates, Publication, RepositoryConfig,
    RepositoryDescriptor, RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_catalog_storage::DgraphCatalogStore;
use hodei_evidence::ContentDigest;
use hodei_graph_client::{GraphClient, GraphClientConfig};
use hodei_hub::blob::{BlobStore, FileBlobStore};
use sha2::Digest as _;
use support::{Answer, Curl};
use tempfile::TempDir;

/// The run: one per execution, so no test meets what another left behind.
fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

static GRAPH_BOOTSTRAP: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// One account per test and per run.
fn account(tag: &str) -> String {
    format!("it195{}{}", tag, &nonce()[8..])
}

fn config_from_env() -> Option<GraphClientConfig> {
    let endpoint = std::env::var("HODEI_GRAPH_IT_ENDPOINT").ok()?;
    let ca = std::env::var("HODEI_GRAPH_IT_CA").unwrap_or_default();
    let server_name = std::env::var("HODEI_GRAPH_IT_SERVER_NAME").unwrap_or_default();
    let user = std::env::var("HODEI_GRAPH_IT_USER").unwrap_or_default();
    let password = std::env::var("HODEI_GRAPH_IT_PASSWORD").unwrap_or_default();
    let mut text = format!(
        "endpoint = \"{endpoint}\"\n\n[tls]\nenabled = {enabled}\nca_cert_path = \"{ca}\"\n",
        enabled = !ca.is_empty()
    );
    if !server_name.is_empty() {
        text.push_str(&format!("server_name = \"{server_name}\"\n"));
    }
    if !user.is_empty() {
        text.push_str(&format!(
            "\n[credentials]\nuserid = \"{user}\"\npassword = \"{password}\"\n"
        ));
    }
    Some(GraphClientConfig::from_toml_str(&text).expect("the IT configuration validates"))
}

async fn fresh_client() -> Option<Arc<GraphClient>> {
    let Some(config) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return None;
    };
    let mut client = GraphClient::connect(config).await.expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    Some(Arc::new(client))
}

/// The evidence directory: `target/uat/hsp195/`.
fn the_evidence_dir() -> PathBuf {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let target = tmp.parent().unwrap_or(&tmp).to_path_buf();
    let dir = target.join("uat").join("hsp195");
    std::fs::create_dir_all(&dir).expect("the evidence directory");
    dir
}

/// One evidence line, Spanish like the repo documentation.
fn record_evidence(test: &str, line: &str) {
    let file = the_evidence_dir().join("evidencia.md");
    let entry = format!("- [{test}] {line}\n");
    if let Ok(mut out) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
    {
        let _ = writeln!(out, "{}", entry.trim_end());
    }
}

/// The Dgraph catalog store, with the bounded open: under load the guard
/// open is the pre-existing stack flake (HSP-190/HSP-219); the suite
/// absorbs it with three attempts and a breather.
async fn open_store(client: Arc<GraphClient>) -> DgraphCatalogStore {
    let mut last = String::new();
    for _attempt in 0..3 {
        match DgraphCatalogStore::open(client.clone(), Arc::new(AllowAllAuthorizer)).await {
            Ok(store) => return store,
            Err(error) => {
                last = error.to_string();
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        }
    }
    panic!("the dgraph catalog store does not open: {last}");
}

fn actor(the_account: &str) -> String {
    format!("hrn:hodei:iam:{the_account}:User/alice")
}

fn correlation_of(the_account: &str) -> String {
    format!("corr-hsp195-{the_account}")
}

/// A hosted apt repository descriptor.
fn an_apt_descriptor(the_account: &str, repo_key: &str) -> RepositoryDescriptor {
    let config = RepositoryConfig::new(DigestScope::Global);
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", the_account, repo_key),
        account: the_account.to_string(),
        repo_key: repo_key.to_string(),
        format: RepositoryFormat::Apt,
        mode: RepositoryMode::Hosted,
        config,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn an_apt_group_descriptor(
    the_account: &str,
    repo_key: &str,
    members: &[&str],
) -> RepositoryDescriptor {
    let mut config = RepositoryConfig::new(DigestScope::Global);
    config.config_settings = serde_json::json!({"group": {"members": members}});
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", the_account, repo_key),
        account: the_account.to_string(),
        repo_key: repo_key.to_string(),
        format: RepositoryFormat::Apt,
        mode: RepositoryMode::Group,
        config,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

// ---------------------------------------------------------------------------
// The Debian fixtures: the pool grammar of the contract, and the minimal
// `.deb` container hsp122 uses (ar with debian-binary, control.tar,
// data.tar), plus the real one built inside the pinned container.
// ---------------------------------------------------------------------------

const REPO: &str = "debian-hosted";
const POOL_PATH: &str = "pool/main/hello-hodei/hello-hodei_1.0.0_amd64.deb";
const PLUS_VERSION: &str = "1.0.0~rc1+dfsg";
const PLUS_POOL: &str = "pool/main/hello-plus/hello-plus_1.0.0~rc1+dfsg_amd64.deb";

/// The control of the real package: `dpkg-deb` demands it well formed.
const DEBIAN_CONTROL: &str = concat!(
    "Package: hello-hodei\n",
    "Version: 1.0.0\n",
    "Architecture: amd64\n",
    "Maintainer: Hodei <hodei@example.local>\n",
    "Priority: optional\n",
    "Section: utils\n",
    "Description: A tiny hello package\n",
    " Installed through the real apt client against Dgraph.\n",
);

fn ar_header(name: &str, size: usize) -> Vec<u8> {
    let mut header = format!("{name:<16}0           0     0     644      {size:<10}`\n");
    if header.len() != 60 {
        header.truncate(60);
    }
    header.into_bytes()
}

fn tar_block(name: &str, data: &[u8]) -> Vec<u8> {
    let mut block = vec![0u8; 512];
    block[0..name.len()].copy_from_slice(name.as_bytes());
    let size = format!("{:07o}\0", data.len());
    block[124..132].copy_from_slice(size.as_bytes());
    block[156] = b'0';
    block.extend_from_slice(data);
    let pad = (512 - data.len() % 512) % 512;
    block.extend(std::iter::repeat_n(0, pad));
    block
}

fn build_deb(control_text: &str) -> Vec<u8> {
    let mut control_tar = tar_block("./control", control_text.as_bytes());
    control_tar.extend(vec![0u8; 1024]);
    let data_tar = vec![0u8; 1024];

    let mut deb = Vec::new();
    deb.extend_from_slice(b"!<arch>\n");
    for (name, body) in [
        ("debian-binary", b"2.0\n".to_vec()),
        ("control.tar", control_tar),
        ("data.tar", data_tar),
    ] {
        deb.extend_from_slice(&ar_header(name, body.len()));
        deb.extend_from_slice(&body);
        if body.len() % 2 == 1 {
            deb.push(b'\n');
        }
    }
    deb
}

/// The synthetic package with the guard-hostile Debian version: `~` and
/// `+` are exactly what the guard subject used to reject on Dgraph.
fn plus_deb() -> Vec<u8> {
    build_deb(&format!(
        "Package: hello-plus\n\
         Version: {PLUS_VERSION}\n\
         Architecture: amd64\n\
         Description: A version with the debian charset\n"
    ))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Clone)]
struct ProxyFixture {
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    hits: Arc<AtomicUsize>,
}

async fn serve_proxy_fixture(
    axum::extract::State(fixture): axum::extract::State<ProxyFixture>,
    uri: OriginalUri,
) -> Response {
    fixture.hits.fetch_add(1, Ordering::Relaxed);
    match fixture
        .files
        .lock()
        .expect("fixture files lock")
        .get(uri.path().trim_start_matches('/'))
        .cloned()
    {
        Some(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_LENGTH, bytes.len())
            .body(Body::from(bytes))
            .expect("fixture response"),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .expect("fixture not found response"),
    }
}

struct ProxyUpstream {
    port: u16,
    hits: Arc<AtomicUsize>,
    thread: Option<std::thread::JoinHandle<()>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl ProxyUpstream {
    fn start(files: HashMap<String, Vec<u8>>) -> Self {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .fallback(get(serve_proxy_fixture))
            .with_state(ProxyFixture {
                files: Arc::new(Mutex::new(files)),
                hits: hits.clone(),
            });
        let listener = TcpListener::bind("127.0.0.1:0").expect("upstream listener");
        let port = listener.local_addr().expect("upstream address").port();
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let (shutdown, receiver) = tokio::sync::oneshot::channel();
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("upstream runtime");
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener).expect("tokio listener");
                axum::serve(listener, app)
                    .with_graceful_shutdown(async {
                        let _ = receiver.await;
                    })
                    .await
                    .expect("upstream serves");
            });
        });
        Self {
            port,
            hits,
            thread: Some(thread),
            shutdown: Some(shutdown),
        }
    }

    fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(thread) = self.thread.take() {
            thread.join().expect("upstream thread stops");
        }
    }
}

fn signed_proxy_fixture(
    package: &str,
    version: &str,
    pool_path: &str,
    deb: &[u8],
    signer: &hodei_artifacts_api::openpgp::MetadataSigner,
) -> HashMap<String, Vec<u8>> {
    let packages = format!(
        "Package: {package}\nVersion: {version}\nArchitecture: amd64\nFilename: {pool_path}\nSize: {}\nSHA256: {}\nDescription: proxy fixture\n\n",
        deb.len(), sha256_hex(deb)
    )
    .into_bytes();
    let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
    gzip.write_all(&packages).expect("Packages gzip input");
    let packages_gz = gzip.finish().expect("Packages gzip bytes");
    let release = format!(
        "Origin: Hodei fixture\nSuite: stable\nCodename: bookworm\nDate: Mon, 24 Nov 2026 10:00:00 UTC\nArchitectures: amd64\nComponents: main\nSHA256:\n {} {} main/binary-amd64/Packages\n {} {} main/binary-amd64/Packages.gz\n",
        sha256_hex(&packages), packages.len(), sha256_hex(&packages_gz), packages_gz.len()
    )
    .into_bytes();
    let signature = signer.sign_detached(&release).expect("upstream signature");
    HashMap::from([
        ("dists/bookworm/Release".into(), release),
        ("dists/bookworm/Release.gpg".into(), signature),
        ("dists/bookworm/main/binary-amd64/Packages".into(), packages),
        (
            "dists/bookworm/main/binary-amd64/Packages.gz".into(),
            packages_gz,
        ),
        (pool_path.into(), deb.to_vec()),
    ])
}

fn add_inrelease(
    files: &mut HashMap<String, Vec<u8>>,
    signer: &hodei_artifacts_api::openpgp::MetadataSigner,
) {
    let release = std::str::from_utf8(
        files
            .get("dists/bookworm/Release")
            .expect("fixture Release exists"),
    )
    .expect("fixture Release is UTF-8");
    let inrelease = signer
        .sign_cleartext(release)
        .expect("upstream cleartext signature");
    files.insert("dists/bookworm/InRelease".into(), inrelease);
}

/// The armored OpenPGP body as raw bytes (what `gpg --dearmor` yields).
fn dearmor(armored: &str) -> Vec<u8> {
    let body: String = armored
        .lines()
        .skip_while(|line| !line.starts_with("-----BEGIN"))
        .skip(1)
        .take_while(|line| !line.starts_with("-----END"))
        .filter(|line| !(line.trim().is_empty() || line.starts_with('=')))
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();
    base64::engine::general_purpose::STANDARD
        .decode(&body)
        .expect("the armored key is base64")
}

// ---------------------------------------------------------------------------
// The pinned real apt client: the same debian:bookworm hsp122 fixes.
// ---------------------------------------------------------------------------

const IMAGE: &str = "docker.io/library/debian:bookworm";

fn require_podman() {
    let found = Command::new("podman")
        .args(["--version"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "podman is not installed: the apt client suite needs a debian container"
    );
    let image = Command::new("podman")
        .args(["image", "exists", IMAGE])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    assert!(
        image,
        "the image {IMAGE} is not available: pull it before running the apt client suite"
    );
}

/// Runs a script inside the container with `work` mounted at `/work` and
/// host networking, returning the output (never panicking: the refusal
/// cases need to inspect the exit themselves).
fn container(work: &Path, script: &str) -> std::process::Output {
    Command::new("podman")
        .arg("run")
        .arg("--rm")
        .arg("--network=host")
        .arg("-v")
        .arg(format!("{}:/work:z", work.display()))
        .arg(IMAGE)
        .arg("sh")
        .arg("-c")
        .arg(script)
        .output()
        .expect("the container runs")
}

fn container_ok(work: &Path, script: &str) -> String {
    let output = container(work, script);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "the container failed ({script}):\n{stdout}\n{stderr}"
    );
    stdout
}

/// The host `gpgv` invocation: the same verifier apt itself calls.
fn gpgv_verdict(dir: &Path, signature: &str, payload: &str) -> bool {
    Command::new("gpgv")
        .arg("--keyring")
        .arg(dir.join("keyring.bin"))
        .arg(dir.join(signature))
        .arg(dir.join(payload))
        .env("LC_ALL", "C")
        .output()
        .expect("gpgv runs")
        .status
        .success()
}

// ---------------------------------------------------------------------------
// The HTTP vertical: the hub router over the Dgraph catalog.
// ---------------------------------------------------------------------------

struct ContractAuthenticator {
    the_account: String,
}

#[async_trait]
impl PrincipalAuthenticator for ContractAuthenticator {
    async fn authenticate(
        &self,
        credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        match credentials {
            Credentials::Bearer(token) if token == "alice" => Ok(PrincipalClaims::new(
                PrincipalId::new(format!("hrn:hodei:iam::{}:User/alice", self.the_account))
                    .unwrap(),
                AccountId::new(self.the_account.clone()).unwrap(),
            )),
            _ => Err(AuthenticationError::InvalidCredentials),
        }
    }
}

/// The Cedar bundle: only `alice` of this account uploads and downloads.
fn cedar_for(the_account: &str, repo_key: &str) -> Arc<dyn AuthorizerPort> {
    let policy = format!(
        r#"permit(
        principal == Iam::User::"hrn:hodei:iam::{the_account}:User/alice",
        action == Artifacts::Action::"UploadAsset",
        resource == Artifacts::Repository::"hrn:hodei:artifacts:eu:{the_account}:repository/{repo_key}"
    );
    permit(
        principal == Iam::User::"hrn:hodei:iam::{the_account}:User/alice",
        action == Artifacts::Action::"DownloadAsset",
        resource == Artifacts::Repository::"hrn:hodei:artifacts:eu:{the_account}:repository/{repo_key}"
    );"#
    );
    Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            &policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .expect("authorizer"),
    )))
}

fn cedar_for_repositories(the_account: &str, repositories: &[&str]) -> Arc<dyn AuthorizerPort> {
    let policy = repositories
        .iter()
        .flat_map(|repo_key| {
            ["UploadAsset", "DownloadAsset"].map(|action| {
                format!(
                    r#"permit(principal == Iam::User::"hrn:hodei:iam::{the_account}:User/alice", action == Artifacts::Action::"{action}", resource == Artifacts::Repository::"hrn:hodei:artifacts:eu:{the_account}:repository/{repo_key}");"#
                )
            })
        })
        .collect::<Vec<_>>()
        .join("\n");
    Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            &policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .expect("authorizer"),
    )))
}

struct FailAptSnapshotPublish {
    delegate: Arc<DgraphCatalogStore>,
}

#[async_trait]
impl AptSnapshotStore for FailAptSnapshotPublish {
    async fn current_apt_snapshot(
        &self,
        key: &AptSnapshotKey,
    ) -> Result<Option<AptGroupSnapshot>, hodei_catalog::CatalogError> {
        self.delegate.current_apt_snapshot(key).await
    }

    async fn apt_snapshot_by_revision(
        &self,
        key: &AptSnapshotKey,
        revision: u64,
    ) -> Result<Option<AptGroupSnapshot>, hodei_catalog::CatalogError> {
        self.delegate.apt_snapshot_by_revision(key, revision).await
    }

    async fn apt_snapshot_by_packages_hash(
        &self,
        key: &AptSnapshotKey,
        architecture: &str,
        digest: &ContentDigest,
    ) -> Result<Option<AptGroupSnapshot>, hodei_catalog::CatalogError> {
        self.delegate
            .apt_snapshot_by_packages_hash(key, architecture, digest)
            .await
    }

    async fn publish_apt_snapshot(
        &self,
        _key: &AptSnapshotKey,
        _expected_revision: Option<u64>,
        _snapshot: AptGroupSnapshot,
    ) -> Result<(), hodei_catalog::CatalogError> {
        Err(hodei_catalog::CatalogError::Unavailable {
            detail: "injected APT snapshot publish failure".into(),
        })
    }
}

struct FailMetadataSigning {
    delegate: hodei_artifacts_api::openpgp::MetadataSigner,
}

impl hodei_artifacts_api::openpgp::MetadataSigning for FailMetadataSigning {
    fn key_asc(&self) -> Result<String, hodei_artifacts_api::openpgp::OpenPgpError> {
        hodei_artifacts_api::openpgp::MetadataSigning::key_asc(&self.delegate)
    }

    fn fingerprint_hex(&self) -> String {
        hodei_artifacts_api::openpgp::MetadataSigning::fingerprint_hex(&self.delegate)
    }

    fn sign_detached(
        &self,
        _data: &[u8],
    ) -> Result<Vec<u8>, hodei_artifacts_api::openpgp::OpenPgpError> {
        Err(hodei_artifacts_api::openpgp::OpenPgpError::Signing(
            "injected metadata signing failure".into(),
        ))
    }

    fn sign_cleartext(
        &self,
        text: &str,
    ) -> Result<Vec<u8>, hodei_artifacts_api::openpgp::OpenPgpError> {
        hodei_artifacts_api::openpgp::MetadataSigning::sign_cleartext(&self.delegate, text)
    }
}

struct Vertical {
    app: Router,
    store: Arc<DgraphCatalogStore>,
    /// The CAS of the vertical: deleting a blob file is how the read-side
    /// missing-content case is produced.
    content: Arc<FileBlobStore>,
    #[allow(dead_code)]
    tmp: TempDir,
}

impl Vertical {
    async fn mount_apt(the_account: &str) -> Self {
        let client = fresh_client().await.expect("the dgraph client connects");
        let tmp = tempfile::tempdir().expect("temp dir");
        let content = Arc::new(FileBlobStore::new(tmp.path().join("cas")).expect("the CAS"));
        // The integrity probe is on: the contract's fail-closed publish
        // requires the CAS to actually hold what it declares.
        let store = Arc::new(
            open_store(client.clone())
                .await
                .with_probe(Arc::new(BlobStoreProbe::new(content.clone()))),
        );
        store
            .create_repository(the_account, an_apt_descriptor(the_account, REPO))
            .await
            .expect("the repository is created");

        let mut config = AppConfig::default();
        config.security.raw_api_enabled = true;
        // The graph profile is the only authority: the bootstrap connects to the
        // same alpha the test seeds and its catalog is replaced just below.
        config.graph = config_from_env();
        config.content.cas_path = tmp
            .path()
            .join("bootstrap-cas")
            .to_string_lossy()
            .into_owned();

        let _bootstrap_guard = GRAPH_BOOTSTRAP.lock().await;
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
        let _ = &reader;

        let state = state
            .with_catalog(reader, writer)
            .with_content_store(content.clone())
            .with_authenticator(Arc::new(ContractAuthenticator {
                the_account: the_account.to_string(),
            }))
            .with_authorization(cedar_for(the_account, REPO));

        Self {
            app: build_router(state, &config),
            store,
            content,
            tmp,
        }
    }

    async fn mount_apt_proxy_group(
        the_account: &str,
        upstream_port: u16,
        trusted_key_asc: &str,
    ) -> Self {
        let client = fresh_client().await.expect("the dgraph client connects");
        let tmp = tempfile::tempdir().expect("temp dir");
        let content = Arc::new(FileBlobStore::new(tmp.path().join("cas")).expect("the CAS"));
        let store = Arc::new(
            open_store(client.clone())
                .await
                .with_probe(Arc::new(BlobStoreProbe::new(content.clone()))),
        );
        let proxy_key = "apt-proxy";
        let mut proxy_config = RepositoryConfig::new(DigestScope::Global);
        proxy_config.config_settings = serde_json::json!({
            "proxy": {
                "upstream": {
                    "id": format!("apt-it-{upstream_port}"),
                    "url": format!("http://127.0.0.1:{upstream_port}/"),
                    "scopes": ["dists/", "pool/"]
                },
                "cache": {"ttl": {"metadata_secs": 300}}
            },
            "apt": {
                "distributions": ["bookworm"],
                "components": ["main"],
                "architectures": ["amd64"],
                "trusted_key_asc": trusted_key_asc
            }
        });
        store
            .create_repository(
                the_account,
                RepositoryDescriptor {
                    hrn: repository_hrn("hodei", "artifacts", "eu", the_account, proxy_key),
                    account: the_account.to_string(),
                    repo_key: proxy_key.to_string(),
                    format: RepositoryFormat::Apt,
                    mode: RepositoryMode::Proxy,
                    config: proxy_config,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                },
            )
            .await
            .expect("the APT proxy member is created");
        let hosted_key = "apt-hosted";
        store
            .create_repository(the_account, an_apt_descriptor(the_account, hosted_key))
            .await
            .expect("the hosted APT member is created");
        store
            .create_repository(
                the_account,
                an_apt_group_descriptor(the_account, "apt-group", &[proxy_key, hosted_key]),
            )
            .await
            .expect("the APT group is created");

        let mut config = AppConfig::default();
        config.security.raw_api_enabled = true;
        config.graph = config_from_env();
        config.content.cas_path = tmp
            .path()
            .join("bootstrap-cas")
            .to_string_lossy()
            .into_owned();
        let _bootstrap_guard = GRAPH_BOOTSTRAP.lock().await;
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
        let provenance = Arc::new(
            DgraphProvenanceLedger::open(client.clone())
                .await
                .expect("the Dgraph provenance ledger opens"),
        );
        let proxy_plane = Arc::new(ProxyPlane::new(
            ProxyFetcher::new(
                hodei_artifacts_api::proxy::egress::EgressPolicy::new(vec![AllowedHost {
                    host: "127.0.0.1".to_string(),
                    port: Some(upstream_port),
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
        let reader: Arc<dyn CatalogReader> = store.clone();
        let writer: Arc<dyn CatalogWriter> = store.clone();
        let snapshots: Arc<dyn AptSnapshotStore> = store.clone();
        let state = state
            .with_catalog(reader, writer)
            .with_apt_snapshot_store(snapshots)
            .with_content_store(content.clone())
            .with_proxy_plane(proxy_plane)
            .with_authenticator(Arc::new(ContractAuthenticator {
                the_account: the_account.to_string(),
            }))
            .with_authorization(cedar_for_repositories(
                the_account,
                &[proxy_key, hosted_key, "apt-group"],
            ));
        Self {
            app: build_router(state, &config),
            store,
            content,
            tmp,
        }
    }

    async fn mount_apt_group(the_account: &str) -> Self {
        Self::mount_apt_group_with(the_account, None, false).await
    }

    async fn mount_apt_group_with(
        the_account: &str,
        signer: Option<Arc<dyn hodei_artifacts_api::openpgp::MetadataSigning>>,
        fail_snapshot_publish: bool,
    ) -> Self {
        let client = fresh_client().await.expect("the dgraph client connects");
        let tmp = tempfile::tempdir().expect("temp dir");
        let content = Arc::new(FileBlobStore::new(tmp.path().join("cas")).expect("the CAS"));
        let store = Arc::new(
            open_store(client.clone())
                .await
                .with_probe(Arc::new(BlobStoreProbe::new(content.clone()))),
        );
        let member_a = "apt-member-a";
        let member_b = "apt-member-b";
        for member in [member_a, member_b] {
            store
                .create_repository(the_account, an_apt_descriptor(the_account, member))
                .await
                .expect("the APT member is created");
        }
        store
            .create_repository(
                the_account,
                an_apt_group_descriptor(the_account, "apt-group", &[member_a, member_b]),
            )
            .await
            .expect("the APT group is created");

        let mut config = AppConfig::default();
        config.security.raw_api_enabled = true;
        config.graph = config_from_env();
        config.content.cas_path = tmp
            .path()
            .join("bootstrap-cas")
            .to_string_lossy()
            .into_owned();
        let _bootstrap_guard = GRAPH_BOOTSTRAP.lock().await;
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
        let snapshots: Arc<dyn AptSnapshotStore> = if fail_snapshot_publish {
            Arc::new(FailAptSnapshotPublish {
                delegate: store.clone(),
            })
        } else {
            store.clone()
        };
        let mut state = state
            .with_catalog(reader, writer)
            .with_apt_snapshot_store(snapshots)
            .with_content_store(content.clone())
            .with_authenticator(Arc::new(ContractAuthenticator {
                the_account: the_account.to_string(),
            }))
            .with_authorization(cedar_for_repositories(
                the_account,
                &[member_a, member_b, "apt-group"],
            ));
        if let Some(signer) = signer {
            state = state.with_metadata_signer(signer);
        }
        Self {
            app: build_router(state, &config),
            store,
            content,
            tmp,
        }
    }
}

/// The same service on a real port: the apt client speaks TCP.
async fn serve_over_tcp(vertical: &Vertical) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("the service binds a local port");
    let address = listener
        .local_addr()
        .expect("the service knows its address");
    let app = vertical.app.clone();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    address.port()
}

// ---------------------------------------------------------------------------
// The curl transport, as hsp122: what the server says, byte for byte.
// ---------------------------------------------------------------------------

static CALLS: AtomicUsize = AtomicUsize::new(0);

fn next(name: &str) -> PathBuf {
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{n}"))
}

fn call(port: u16, method: &str, path: &str, body: Option<&[u8]>) -> Answer {
    call_as(port, method, path, "alice", body)
}

fn call_as(port: u16, method: &str, path: &str, token: &str, body: Option<&[u8]>) -> Answer {
    let sent = body.map(|bytes| {
        let target = next("sent");
        std::fs::write(&target, bytes).expect("the body is written");
        target
    });
    let into = next("answer");
    let headers: &[(&str, &str)] = if sent.is_some() {
        &[("content-type", "application/octet-stream")]
    } else {
        &[]
    };
    Curl::call_with_headers(
        port,
        method,
        path,
        token,
        sent.as_deref(),
        headers,
        Some(&into),
    )
    .run()
}

fn get_with(port: u16, path: &str, header: (&str, &str), into: &Path) -> Answer {
    Curl::with_header(port, path, "alice", header, into).run()
}

fn text_of(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

// ---------------------------------------------------------------------------
// Store-level seeding: the two immutable assets one `.deb` leaves behind
// (the binary and its `.record`), exactly as the handler publishes them.
// ---------------------------------------------------------------------------

async fn publish_deb_pair<Store>(
    store: &Store,
    the_account: &str,
    repository: &str,
    deb: &[u8],
) -> (String, ContentDigest, ContentDigest)
where
    Store: CatalogWriter + Send + Sync,
{
    let control = apt::read_deb_control(deb).expect("the deb control parses");
    let pool = apt::pool_path_for(&control.package, &control.version, &control.architecture)
        .expect("the pool path is the grammar's own");
    let deb_digest = ContentDigest::of_bytes(deb);
    let record = apt::packages_record(&control, &pool, deb.len() as u64, &deb_digest.value);
    let record_bytes = record.into_bytes();
    let record_digest = ContentDigest::of_bytes(&record_bytes);

    store
        .publish(
            the_account,
            Publication {
                repository: repository.to_string(),
                asset_path: pool.clone(),
                coordinates: PackageCoordinates {
                    package_name: control.package.clone(),
                    package_version: control.version.clone(),
                },
                digest: deb_digest.clone(),
                size_bytes: deb.len() as u64,
                tags: Vec::new(),
                actor: actor(the_account),
                correlation_id: correlation_of(the_account),
                overwrite: false,
                precondition: None,
                subject: None,
            },
        )
        .await
        .expect("the deb publication settles");
    store
        .publish(
            the_account,
            Publication {
                repository: repository.to_string(),
                asset_path: apt::record_path_for(&pool),
                coordinates: PackageCoordinates {
                    package_name: format!("{}#record", control.package),
                    package_version: control.version.clone(),
                },
                digest: record_digest.clone(),
                size_bytes: record_bytes.len() as u64,
                tags: Vec::new(),
                actor: actor(the_account),
                correlation_id: correlation_of(the_account),
                overwrite: false,
                precondition: None,
                subject: None,
            },
        )
        .await
        .expect("the record publication settles");
    (pool, deb_digest, record_digest)
}

fn a_package(name: &str, version: &str) -> Vec<u8> {
    build_deb(&format!(
        "Package: {name}\n\
         Version: {version}\n\
         Architecture: amd64\n\
         Description: {name} for the apt over dgraph suite\n"
    ))
}

// ---------------------------------------------------------------------------
// The per-tenant outbox census (the HSP-220 lesson: never the global
// `pending()`, which overflows the shared alpha's gRPC limit).
// ---------------------------------------------------------------------------

async fn the_events_of(client: &GraphClient, tenant: &str, resource_needle: &str) -> usize {
    let escaped = tenant.replace('\\', "\\\\").replace('"', "\\\"");
    let response = client
        .execute_dql(&format!(
            "{{ q(func: eq(hodei.catalog.outbox.tenant, \"{escaped}\")) \
             {{ hodei.catalog.outbox.resource }} }}"
        ))
        .await
        .expect("the outbox census answers");
    let nodes = response
        .json
        .get("q")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    nodes
        .iter()
        .filter(|node| {
            node.get("hodei.catalog.outbox.resource")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|resource| resource.contains(resource_needle))
        })
        .count()
}

// ---------------------------------------------------------------------------
// AC1: the real apt client updates, verifies the signature and installs
// over the Dgraph catalog, with the `+`/`~` guard fix exercised end to end.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_real_apt_client_updates_and_installs_over_dgraph() {
    let Some(_client) = fresh_client().await else {
        return;
    };
    require_podman();
    let the_account = account("apt");
    let vertical = Vertical::mount_apt(&the_account).await;
    let port = serve_over_tcp(&vertical).await;

    // The scratch tree the container shares: the package the pinned dpkg
    // will really build, plus the dearmored hub key.
    let work = next("stage");
    std::fs::create_dir_all(work.join("pkg/DEBIAN")).expect("the package tree");
    std::fs::write(work.join("pkg/DEBIAN/control"), DEBIAN_CONTROL).expect("the control");
    let payload = work.join("pkg/usr/share/hello-hodei");
    std::fs::create_dir_all(&payload).expect("the payload dir");
    std::fs::write(payload.join("greeting.txt"), "hola desde hodei\n").expect("the payload");

    // 1. A real `.deb`, built by the dpkg of the pinned distro.
    container_ok(
        &work,
        "cd /work && dpkg-deb --build --root-owner-group pkg hello.deb && chmod 644 hello.deb",
    );
    let deb = std::fs::read(work.join("hello.deb")).expect("the built deb");
    assert!(!deb.is_empty(), "dpkg-deb produced a package");

    // 2. Published through the apt surface over the Dgraph catalog.
    let published = call(port, "PUT", &format!("/apt/{REPO}/{POOL_PATH}"), Some(&deb));
    assert_eq!(published.status, 200, "{}", text_of(&published));

    // 3. The guard-hostile version: `~` and `+` in the pool path used to
    // be refused by the guard subject on Dgraph only. Here it publishes.
    let plus = plus_deb();
    let plus_published = call(
        port,
        "PUT",
        &format!("/apt/{REPO}/{PLUS_POOL}"),
        Some(&plus),
    );
    assert_eq!(
        plus_published.status,
        200,
        "a debian version with ~ and + must publish over Dgraph: {}",
        text_of(&plus_published)
    );
    record_evidence(
        "the_real_apt_client_updates_and_installs_over_dgraph",
        &format!(
            "publicación del .deb con versión {PLUS_VERSION} (guarda con ~ y + escapada) sobre el catálogo Dgraph: 200"
        ),
    );

    // 4. The hub key, dearmored the way Debian documents `signed-by`.
    let key = call(port, "GET", &format!("/apt/{REPO}/key.asc"), None);
    assert_eq!(key.status, 200, "{}", text_of(&key));
    std::fs::write(work.join("key.gpg"), dearmor(&text_of(&key))).expect("the keyring");

    // 5. The assembled index: both packages are in amd64, the record of
    // the plus package is exactly what the pure grammar says.
    let packages = call(
        port,
        "GET",
        &format!("/apt/{REPO}/dists/stable/binary-amd64/Packages"),
        None,
    );
    assert_eq!(packages.status, 200);
    let index = text_of(&packages);
    assert!(index.contains(&format!("Filename: {POOL_PATH}")));
    assert!(index.contains(&format!("Filename: {PLUS_POOL}")));
    assert!(index.contains(&format!("SHA256: {}", sha256_hex(&deb))));
    assert!(index.contains(&format!("SHA256: {}", sha256_hex(&plus))));
    let arm64 = call(
        port,
        "GET",
        &format!("/apt/{REPO}/dists/stable/binary-arm64/Packages"),
        None,
    );
    assert_eq!(arm64.status, 200);
    assert!(!text_of(&arm64).contains("Filename: pool/"));

    let records = vertical
        .store
        .assets_under(&the_account, REPO, "pool/main/hello-plus/")
        .await
        .expect("the pool window lists");
    let record_asset = records
        .iter()
        .find(|asset| asset.asset_path.ends_with(".record"))
        .expect("the record row is in the graph");
    let stored_record = vertical
        .content
        .get(&record_asset.digest)
        .await
        .expect("the record bytes are in the CAS");
    let control = apt::read_deb_control(&plus).expect("the control parses");
    let expected_record =
        apt::packages_record(&control, PLUS_POOL, plus.len() as u64, &sha256_hex(&plus));
    assert_eq!(
        String::from_utf8_lossy(&stored_record),
        expected_record,
        "the stored record is the grammar's own bytes"
    );

    // 6. The Release: deterministic across reads, pinning the exact index
    // bytes, and its detached signature opens against the served key.
    let release = call(
        port,
        "GET",
        &format!("/apt/{REPO}/dists/stable/Release"),
        None,
    );
    assert_eq!(release.status, 200);
    let again = call(
        port,
        "GET",
        &format!("/apt/{REPO}/dists/stable/Release"),
        None,
    );
    assert_eq!(
        release.body, again.body,
        "the Release must not drift between reads"
    );
    let release_text = text_of(&release);
    assert!(release_text.contains("Origin: Hodei"));
    assert!(release_text.contains("Suite: stable"));
    assert!(release_text.contains("Architectures: amd64 arm64"));
    assert!(release_text.contains("Components: main"));
    let index_sha = sha256_hex(&packages.body);
    assert!(
        release_text.contains(&format!(
            " {index_sha} {} main/binary-amd64/Packages",
            packages.size
        )),
        "Release must pin the exact bytes of the served index:\n{release_text}"
    );

    let signature = call(
        port,
        "GET",
        &format!("/apt/{REPO}/dists/stable/Release.gpg"),
        None,
    );
    assert_eq!(signature.status, 200);
    let dir = next("gpgv");
    std::fs::create_dir_all(&dir).expect("a scratch dir");
    std::fs::write(dir.join("Release"), &release.body).expect("write release");
    std::fs::write(dir.join("Release.gpg"), &signature.body).expect("write signature");
    std::fs::write(dir.join("keyring.bin"), dearmor(&text_of(&key))).expect("write keyring");
    let verified = Command::new("gpgv")
        .arg("--keyring")
        .arg(dir.join("keyring.bin"))
        .arg(dir.join("Release.gpg"))
        .arg(dir.join("Release"))
        .env("LC_ALL", "C")
        .output()
        .expect("gpgv runs");
    let stderr = String::from_utf8_lossy(&verified.stderr);
    assert!(
        verified.status.success() && stderr.contains("Good signature"),
        "gpgv said: {stderr}"
    );
    // The signature is of these bytes: a tampered Release does not open.
    std::fs::write(dir.join("tampered"), b"Origin: Nobody\n").expect("write tampered");
    assert!(
        !gpgv_verdict(&dir, "Release.gpg", "tampered"),
        "a forged Release must not open"
    );
    record_evidence(
        "the_real_apt_client_updates_and_installs_over_dgraph",
        "Release determinista (doble lectura byte a byte), SHA256 del índice servido fijado en el Release, Release.gpg abre contra la key.asc servida con el gpgv del host y un Release alterado no abre",
    );

    // Conditional read on the generated metadata: the same ETag answers 304.
    let etag = release.header("etag").expect("the Release carries an etag");
    let conditional_target = next("conditional");
    let not_modified = get_with(
        port,
        &format!("/apt/{REPO}/dists/stable/Release"),
        ("if-none-match", etag.as_str()),
        &conditional_target,
    );
    assert_eq!(not_modified.status, 304, "the ETag must answer 304");
    record_evidence(
        "the_real_apt_client_updates_and_installs_over_dgraph",
        &format!("lectura condicional del Release: If-None-Match ({etag}) responde 304"),
    );

    // 7. The atomic outbox, counted per tenant: every upload left exactly
    // the two publication facts, and the echo adds none.
    let census = fresh_client().await.expect("the census client");
    let before = the_events_of(&census, &the_account, &format!("{REPO}/pool/")).await;
    assert_eq!(
        before, 4,
        "two uploads of two immutable assets each leave four events"
    );
    let echoed = call(port, "PUT", &format!("/apt/{REPO}/{POOL_PATH}"), Some(&deb));
    assert_eq!(echoed.status, 200, "the same bytes echo");
    let after = the_events_of(&census, &the_account, &format!("{REPO}/pool/")).await;
    assert_eq!(before, after, "an echo says no new fact");
    record_evidence(
        "the_real_apt_client_updates_and_installs_over_dgraph",
        &format!(
            "censo por tenant ({the_account}): 4 hechos de publicación (dos subidas x .deb + .record) y re-entrega idémpotente sin hechos nuevos ({after})"
        ),
    );

    // 8. The real apt: update verifies the Release signature with its own
    // gpgv, install pulls the package from the pool and checks its SHA256,
    // and the `+`/`~` version resolves in the index.
    let script = format!(
        concat!(
            "set -e\n",
            "rm -f /etc/apt/sources.list.d/debian.sources\n",
            "printf 'deb [signed-by=/work/key.gpg] http://alice:alice@127.0.0.1:{port}/apt/{repo} stable main\\n' ",
            "> /etc/apt/sources.list.d/hodei.list\n",
            "apt-get update -qq\n",
            "apt-get install -y -qq hello-hodei\n",
            "dpkg-query -W -f='${{Status}} ${{Package}} ${{Version}}\\n' hello-hodei\n",
            "apt-cache policy hello-plus | grep Candidate\n",
            "cat /usr/share/hello-hodei/greeting.txt\n",
        ),
        port = port,
        repo = REPO,
    );
    let installed = container_ok(&work, &script);
    assert!(
        installed.contains("install ok installed hello-hodei 1.0.0"),
        "dpkg must report the package installed: {installed}"
    );
    assert!(
        installed.contains("hola desde hodei"),
        "the installed payload must be the published one: {installed}"
    );
    assert!(
        installed.contains(&format!("Candidate: {PLUS_VERSION}")),
        "apt must resolve the published ~/+ version: {installed}"
    );
    record_evidence(
        "the_real_apt_client_updates_and_installs_over_dgraph",
        "apt 2.6 real de debian:bookworm: update verifica la firma del Release, install baja el .deb del pool (SHA256 del índice) y apt-cache resuelve la versión 1.0.0~rc1+dfsg publicada sobre el grafo",
    );
}

// ---------------------------------------------------------------------------
// AC2: nothing foreign, broken or forged is exposed as valid.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn foreign_and_broken_apt_state_is_never_valid() {
    let Some(_client) = fresh_client().await else {
        return;
    };
    require_podman();
    let alice = account("owner");
    let mallory = account("foreign");
    let mine = Vertical::mount_apt(&alice).await;
    let theirs = Vertical::mount_apt(&mallory).await;
    let my_port = serve_over_tcp(&mine).await;
    let their_port = serve_over_tcp(&theirs).await;

    // Publish one real-shaped package in my repository.
    let broken = a_package("hello-broken", "1.0.0");
    let pool = "pool/main/hello-broken/hello-broken_1.0.0_amd64.deb";
    let uploaded = call(
        my_port,
        "PUT",
        &format!("/apt/{REPO}/{pool}"),
        Some(&broken),
    );
    assert_eq!(uploaded.status, 200, "{}", text_of(&uploaded));

    // The foreign account hosts the same repository key and reads it: what
    // it does not have is invisible, not stale, not borrowed.
    let foreign_deb = a_package("hello-ghost", "9.9.9");
    let foreign_pool = "pool/main/hello-ghost/hello-ghost_9.9.9_amd64.deb";
    let ghost = call(
        their_port,
        "PUT",
        &format!("/apt/{REPO}/{foreign_pool}"),
        Some(&foreign_deb),
    );
    assert_eq!(ghost.status, 200, "{}", text_of(&ghost));
    let stolen = call(my_port, "GET", &format!("/apt/{REPO}/{foreign_pool}"), None);
    assert_eq!(stolen.status, 404, "a foreign package is not served here");
    let my_index = call(
        my_port,
        "GET",
        &format!("/apt/{REPO}/dists/stable/binary-amd64/Packages"),
        None,
    );
    assert!(!text_of(&my_index).contains("hello-ghost"));
    let their_index = call(
        their_port,
        "GET",
        &format!("/apt/{REPO}/dists/stable/binary-amd64/Packages"),
        None,
    );
    assert!(!text_of(&their_index).contains("hello-broken"));
    assert!(their_index.status == 200 && text_of(&their_index).contains("hello-ghost"));
    record_evidence(
        "foreign_and_broken_apt_state_is_never_valid",
        "dos cuentas con la misma clave de repositorio: cada índice lista solo sus registros y descargar el .deb ajeno es 404",
    );

    // A broken control is refused before anything is committed.
    let garbage = call(
        my_port,
        "PUT",
        &format!("/apt/{REPO}/{pool}"),
        Some(b"not a debian archive"),
    );
    assert!(
        garbage.status == 400 || garbage.status == 409,
        "a body that is not a .deb is refused: {}",
        garbage.status
    );

    // The pool path is dictated by the control: other paths conflict.
    let wrong = call(
        my_port,
        "PUT",
        &format!("/apt/{REPO}/pool/main/elsewhere/hello-broken_1.0.0_amd64.deb"),
        Some(&broken),
    );
    assert_eq!(wrong.status, 409, "the path must match the control");

    // Other bytes on a published path: the version is for ever.
    let other = a_package_conflict();
    let conflict = call(my_port, "PUT", &format!("/apt/{REPO}/{pool}"), Some(&other));
    assert_eq!(
        conflict.status,
        409,
        "a published version is immutable: {}",
        text_of(&conflict)
    );
    record_evidence(
        "foreign_and_broken_apt_state_is_never_valid",
        "ruta del pool que no dicta el control 409, mismos bytes eco 200 y bytes distintos en una ruta publicada 409 (la versión es para siempre)",
    );

    // The legacy hosted surface does not publish InRelease.
    let inrelease = call(
        my_port,
        "GET",
        &format!("/apt/{REPO}/dists/stable/InRelease"),
        None,
    );
    assert_eq!(
        inrelease.status, 404,
        "the legacy apt surface has no InRelease"
    );

    // The read side fails closed: with the record blob gone, the indices
    // that would lie about it answer 404, not stale-valid.
    let records = mine
        .store
        .assets_under(&alice, REPO, "pool/main/hello-broken/")
        .await
        .expect("the window lists");
    let record_asset = records
        .iter()
        .find(|asset| asset.asset_path.ends_with(".record"))
        .expect("the record row exists");
    let record_digest = record_asset.digest.clone();
    let record_file = mine.content.the_path_of(&record_digest);
    let record_bytes = std::fs::read(&record_file).expect("the record blob file");
    std::fs::remove_file(&record_file).expect("the record blob is gone");
    let packages = call(
        my_port,
        "GET",
        &format!("/apt/{REPO}/dists/stable/binary-amd64/Packages"),
        None,
    );
    let release = call(
        my_port,
        "GET",
        &format!("/apt/{REPO}/dists/stable/Release"),
        None,
    );
    assert_eq!(
        packages.status, 404,
        "a vanished record must not produce a stale-valid index"
    );
    assert_eq!(release.status, 404, "and the Release cannot vouch for it");
    std::fs::write(&record_file, &record_bytes).expect("the record blob is restored");
    let repaired = call(
        my_port,
        "GET",
        &format!("/apt/{REPO}/dists/stable/binary-amd64/Packages"),
        None,
    );
    assert_eq!(
        repaired.status, 200,
        "the index opens again with its record"
    );
    record_evidence(
        "foreign_and_broken_apt_state_is_never_valid",
        "blob del registro borrado del CAS: Packages y Release responden 404 (nunca válidos rancios) y se recuperan al restaurar el blob",
    );

    // The write side fails closed: publishing bytes the CAS does not hold
    // is refused by the probe and leaves no catalog rows.
    let phantom = a_package("hello-phantom", "0.1.0");
    let phantom_digest = ContentDigest::of_bytes(&phantom);
    let control = apt::read_deb_control(&phantom).expect("the control parses");
    let phantom_pool =
        apt::pool_path_for(&control.package, &control.version, &control.architecture).unwrap();
    let refused = mine
        .store
        .publish(
            &alice,
            Publication {
                repository: REPO.to_string(),
                asset_path: phantom_pool.clone(),
                coordinates: PackageCoordinates {
                    package_name: control.package.clone(),
                    package_version: control.version.clone(),
                },
                digest: phantom_digest.clone(),
                size_bytes: phantom.len() as u64,
                tags: Vec::new(),
                actor: actor(&alice),
                correlation_id: correlation_of(&alice),
                overwrite: false,
                precondition: None,
                subject: None,
            },
        )
        .await
        .expect_err("a publish without its content is refused");
    assert!(
        refused
            .to_string()
            .contains("the content is not in the blob store"),
        "the refusal says why: {refused}"
    );
    let rows = mine
        .store
        .assets_under(&alice, REPO, "pool/main/hello-phantom/")
        .await
        .expect("the census window answers");
    assert!(rows.is_empty(), "the refusal left no catalog rows behind");
    let census = fresh_client().await.expect("the census client");
    let refusals =
        the_events_of(&census, &alice, &format!("{REPO}/pool/main/hello-phantom/")).await;
    assert_eq!(
        refusals, 1,
        "the refusal is an audited fact, not a silent no"
    );
    record_evidence(
        "foreign_and_broken_apt_state_is_never_valid",
        "publicación sin el blob en el CAS rechazada por la sonda (sin filas en el catálogo) y auditada como hecho fallido en el outbox por tenant",
    );

    // Real apt refuses the metadata signed by a stranger key.
    let stranger =
        hodei_artifacts_api::openpgp::MetadataSigner::generate("Stranger <x@example.local>")
            .expect("a second key");
    let work = next("stage-stranger");
    std::fs::create_dir_all(&work).expect("the scratch dir");
    std::fs::write(work.join("key.gpg"), stranger.keyring().expect("a keyring"))
        .expect("the stranger keyring");
    let script = format!(
        concat!(
            "rm -f /etc/apt/sources.list.d/debian.sources\n",
            "printf 'deb [signed-by=/work/key.gpg] http://alice:alice@127.0.0.1:{port}/apt/{repo} stable main\\n' ",
            "> /etc/apt/sources.list.d/hodei.list\n",
            "if apt-get update >/tmp/update.log 2>&1; then echo ACCEPTED; exit 1; fi\n",
            "cat /tmp/update.log; echo refused\n",
        ),
        port = my_port,
        repo = REPO,
    );
    let refused = container(&work, &script);
    let stdout = String::from_utf8_lossy(&refused.stdout);
    assert!(
        refused.status.success() && stdout.contains("refused") && !stdout.contains("ACCEPTED"),
        "apt must reject a Release whose signature does not open with the pinned key: {stdout}"
    );
    record_evidence(
        "foreign_and_broken_apt_state_is_never_valid",
        "apt real con el anillo de un tercero: update se niega (el Release no abre con la clave fijada)",
    );
}

/// A package with the identity of `hello-broken` and other bytes.
fn a_package_conflict() -> Vec<u8> {
    build_deb(concat!(
        "Package: hello-broken\n",
        "Version: 1.0.0\n",
        "Architecture: amd64\n",
        "Description: a different build of the same version\n",
    ))
}

// ---------------------------------------------------------------------------
// Scoped, keyed and indexed: the apt reads root on exact indexes, and the
// census never crosses tenants.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_apt_queries_are_scoped_keyed_and_indexed() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let mine = account("scope");
    let theirs = account("noise");
    let store = Arc::new(open_store(client.clone()).await);
    store
        .create_repository(&mine, an_apt_descriptor(&mine, REPO))
        .await
        .expect("the repository is created");
    store
        .create_repository(&theirs, an_apt_descriptor(&theirs, REPO))
        .await
        .expect("the foreign repository is created");

    // My pool: six packages. The distractor's: the same repository key,
    // the same package names, other bytes.
    let mut mine_digests = Vec::new();
    let mut theirs_seen = 0usize;
    for index in 1..=6u32 {
        let name = format!("pkg-{index:02}");
        let (pool, deb, record) =
            publish_deb_pair(store.as_ref(), &mine, REPO, &a_package(&name, "1.0")).await;
        mine_digests.push((pool, deb.value, record.value));
        publish_deb_pair(
            store.as_ref(),
            &theirs,
            REPO,
            &build_deb(&format!(
                "Package: {name}\n\
                 Version: 1.0\n\
                 Architecture: amd64\n\
                 Description: foreign bytes for the same name\n"
            )),
        )
        .await;
        theirs_seen += 1;
    }
    assert_eq!(theirs_seen, 6);

    // The pool window: only my rows, ordered by path, exactly my digests.
    let found = store
        .assets_under(&mine, REPO, "pool/")
        .await
        .expect("the window reads");
    let paths: Vec<&str> = found
        .iter()
        .map(|asset| asset.asset_path.as_str())
        .collect();
    assert_eq!(paths.len(), 12, "six binaries and six records");
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted, "the window comes back ordered by path");
    for (pool, deb, _record) in &mine_digests {
        let row = found
            .iter()
            .find(|asset| &asset.asset_path == pool)
            .expect("the binary row");
        assert_eq!(
            &row.digest.value, deb,
            "my own content, never the foreign one"
        );
    }
    let other = store
        .assets_under(&theirs, REPO, "pool/")
        .await
        .expect("the foreign window reads");
    assert_eq!(other.len(), 12);
    assert!(
        other
            .iter()
            .zip(found.iter())
            .all(|(x, y)| x.digest != y.digest),
        "the same paths in the same repository key hold different bytes per tenant"
    );
    record_evidence(
        "the_apt_queries_are_scoped_keyed_and_indexed",
        "assets_under del pool por cuenta: 12 filas propias ordenadas por ruta con los digests propios, y las mismas rutas de la cuenta distractora con otros bytes nunca se ven",
    );

    // The per-tenant census: my events never count into the foreign
    // tenant's tally, and vice versa.
    let mine_events = the_events_of(&client, &mine, &format!("{REPO}/pool/")).await;
    let theirs_events = the_events_of(&client, &theirs, &format!("{REPO}/pool/")).await;
    assert_eq!(mine_events, 12, "one fact per published asset");
    assert_eq!(theirs_events, 12);
    record_evidence(
        "the_apt_queries_are_scoped_keyed_and_indexed",
        "censo del outbox por tenant (eq(hodei.catalog.outbox.tenant)): 12+12 hechos separados por cuenta, nunca el censo global",
    );

    // The plan probe: every predicate the apt reads roots on carries the
    // exact index, and the keys carry the upsert guard.
    let probed = [
        ("hodei.catalog.asset.key", true),
        ("hodei.catalog.asset.repository", false),
        ("hodei.catalog.asset.account", false),
        ("hodei.catalog.asset.asset_path", false),
        ("hodei.catalog.asset.package_name", false),
        ("hodei.catalog.asset.package_version", false),
        ("hodei.catalog.outbox.key", true),
        ("hodei.catalog.outbox.tenant", false),
    ];
    for (predicate, upsert) in probed {
        let schema = client
            .execute_dql(&format!("schema(pred: [{predicate}]) {{}}"))
            .await
            .expect("the schema answers");
        let text = schema.json.to_string();
        assert!(
            text.contains("exact"),
            "the predicate {predicate} must carry the exact index: {text}"
        );
        if upsert {
            assert!(
                text.contains("upsert"),
                "the key predicate {predicate} must carry the upsert guard: {text}"
            );
        }
    }
    record_evidence(
        "the_apt_queries_are_scoped_keyed_and_indexed",
        "probe de plan: asset.key/outbox.key con @index(exact) @upsert y account/repository/asset_path/package_* con @index(exact); las raíces del vertical apt (eq de repository+ventana de asset_path, eq de tenant en el censo) van por índice, sin scan global",
    );
}

// ---------------------------------------------------------------------------
// Contract reads: the published apt state answers what the grammar and the
// publication say (HSP-314 F4: this used to compare against a second
// reference engine; every expected value now derives from the published
// bytes and the apt grammar).
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_apt_reads_match_the_contract_for_the_published_state() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let mine = account("parity");
    let graph = open_store(client.clone()).await;
    graph
        .create_repository(&mine, an_apt_descriptor(&mine, REPO))
        .await
        .expect("the dgraph repository is created");

    // Three packages (one with the guard-hostile version).
    let debs = [
        a_package("parity-one", "1.0"),
        a_package("parity-two", "2.0"),
        build_deb(concat!(
            "Package: parity-three\n",
            "Version: 3.0~beta+1\n",
            "Architecture: amd64\n",
            "Description: the tilde and the plus on the graph\n",
        )),
    ];
    // What the listing must carry: (path, digest) of every published asset.
    let mut expected_rows: Vec<(String, String)> = Vec::new();
    let mut expected_index = String::new();
    let mut records: Vec<(String, String)> = Vec::new();
    for deb in &debs {
        let control = apt::read_deb_control(deb).expect("the control parses");
        let expected =
            apt::pool_path_for(&control.package, &control.version, &control.architecture)
                .expect("the pool path");
        let (pool, graph_deb, graph_record) = publish_deb_pair(&graph, &mine, REPO, deb).await;
        assert_eq!(pool, expected, "the path is the grammar's own");
        // The published digests are the digests of the bytes and of the
        // `Packages` record the grammar assembles from them.
        let deb_digest = ContentDigest::of_bytes(deb);
        let record_bytes =
            apt::packages_record(&control, &pool, deb.len() as u64, &deb_digest.value).into_bytes();
        let record_digest = ContentDigest::of_bytes(&record_bytes);
        assert_eq!(graph_deb, deb_digest);
        assert_eq!(graph_record, record_digest);
        expected_rows.push((pool.clone(), deb_digest.value.clone()));
        expected_rows.push((apt::record_path_for(&pool), record_digest.value.clone()));
        records.push((apt::record_path_for(&pool), record_digest.value.clone()));

        // The version row carries the digest of the deb.
        let version = graph
            .version(
                &mine,
                REPO,
                &PackageCoordinates {
                    package_name: control.package.clone(),
                    package_version: control.version.clone(),
                },
            )
            .await
            .expect("the graph version");
        assert_eq!(version.digest, deb_digest);

        // Other bytes on the published path conflict: immutability holds.
        let conflict = Publication {
            repository: REPO.to_string(),
            asset_path: pool.clone(),
            coordinates: PackageCoordinates {
                package_name: control.package.clone(),
                package_version: control.version.clone(),
            },
            digest: ContentDigest::of_bytes(b"other bytes entirely"),
            size_bytes: 18,
            tags: Vec::new(),
            actor: actor(&mine),
            correlation_id: correlation_of(&mine),
            overwrite: false,
            precondition: None,
            subject: None,
        };
        let verdict = graph.publish(&mine, conflict).await;
        assert!(
            verdict.is_err(),
            "the immutable path conflicts: {:?}",
            verdict.map(|_| ()),
        );
    }
    records.sort();
    for (path, digest) in &records {
        expected_index.push_str(&format!("{path}\n{digest}\n"));
    }
    expected_rows.sort();

    // The listing: every published path, ascending, with its digest, and
    // the index assembled from its `.record` rows is the expected one.
    let mine_records = graph
        .assets_under(&mine, REPO, "pool/")
        .await
        .expect("the graph window");
    let listed: Vec<(String, String)> = mine_records
        .iter()
        .map(|row| (row.asset_path.clone(), row.digest.value.clone()))
        .collect();
    assert_eq!(listed, expected_rows, "the listing is what was published");
    assert_eq!(mine_records.len(), 6);
    let index_of = |rows: &[hodei_catalog::AssetRecord]| -> String {
        let mut out = String::new();
        for row in rows {
            if row.asset_path.ends_with(".record") {
                out.push_str(&format!("{}\n", row.asset_path));
                out.push_str(&row.digest.value);
                out.push('\n');
            }
        }
        out
    };
    assert_eq!(
        index_of(&mine_records),
        expected_index,
        "the listing the index assembles from is the published records, in order"
    );
    record_evidence(
        "the_apt_reads_match_the_contract_for_the_published_state",
        "lecturas apt segun el contrato: las publicaciones (con la version 3.0~beta+1) dejan las filas, los digests y el listado ordenado que dicen los bytes y la gramatica; otro contenido en la ruta publicada es conflicto y la inmutabilidad se respeta",
    );
}

fn apt_snapshot_fixture(account: &str, revision: u64, digest: &str) -> AptGroupSnapshot {
    let cas = |suffix: &str| AptCasReference {
        location: hodei_catalog::BlobLocation::new(
            account,
            DigestScope::Global,
            ContentDigest {
                algorithm: hodei_evidence::DigestAlgorithm::Sha256,
                value: format!("{digest}{suffix}"),
            },
        ),
        size_bytes: 10,
    };
    AptGroupSnapshot {
        revision,
        digest: ContentDigest {
            algorithm: hodei_evidence::DigestAlgorithm::Sha256,
            value: format!("snapshot{revision}"),
        },
        release: cas("a"),
        signature: cas("b"),
        packages_by_arch: [("amd64".to_string(), cas("c"))].into_iter().collect(),
        pool_by_path: Default::default(),
        members: vec![],
    }
}

#[tokio::test]
async fn hsp295_apt_snapshot_cas_concurrency_and_history() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let store = Arc::new(open_store(client).await);
    let key = AptSnapshotKey {
        account: account("hsp295"),
        group: "apt-group".into(),
        distribution: "bookworm".into(),
        principal_subject: "hrn:hodei:iam:test:User/apt".into(),
    };
    let left = apt_snapshot_fixture(&key.account, 1, "left");
    let right = apt_snapshot_fixture(&key.account, 1, "right");
    let (left_result, right_result) = tokio::join!(
        store.publish_apt_snapshot(&key, None, left.clone()),
        store.publish_apt_snapshot(&key, None, right.clone()),
    );
    assert_ne!(
        left_result.is_ok(),
        right_result.is_ok(),
        "exactly one publisher wins: {left_result:?} / {right_result:?}"
    );
    let winner = if left_result.is_ok() { left } else { right };
    let loser = if left_result.is_err() {
        left_result
    } else {
        right_result
    };
    assert!(
        matches!(
            loser,
            Err(hodei_catalog::CatalogError::PreconditionFailed { .. })
        ),
        "loser must observe CAS failure: {loser:?}"
    );
    assert_eq!(
        store.current_apt_snapshot(&key).await.unwrap(),
        Some(winner.clone())
    );
    assert_eq!(
        store.apt_snapshot_by_revision(&key, 1).await.unwrap(),
        Some(winner.clone())
    );
    let indexed = winner.packages_by_arch.get("amd64").unwrap();
    assert_eq!(
        store
            .apt_snapshot_by_packages_hash(&key, "amd64", &indexed.location.digest)
            .await
            .unwrap(),
        Some(winner.clone())
    );
    let next_left = apt_snapshot_fixture(&key.account, 2, "next-left");
    let next_right = apt_snapshot_fixture(&key.account, 2, "next-right");
    let (next_left_result, next_right_result) = tokio::join!(
        store.publish_apt_snapshot(&key, Some(1), next_left.clone()),
        store.publish_apt_snapshot(&key, Some(1), next_right.clone()),
    );
    assert_ne!(
        next_left_result.is_ok(),
        next_right_result.is_ok(),
        "exactly one revision-2 publisher wins: {next_left_result:?} / {next_right_result:?}"
    );
    let next_winner = if next_left_result.is_ok() {
        next_left
    } else {
        next_right
    };
    let next_loser = if next_left_result.is_err() {
        next_left_result
    } else {
        next_right_result
    };
    assert!(
        matches!(
            next_loser,
            Err(hodei_catalog::CatalogError::PreconditionFailed { .. })
        ),
        "revision-2 loser must observe CAS failure: {next_loser:?}"
    );
    assert_eq!(
        store.current_apt_snapshot(&key).await.unwrap(),
        Some(next_winner.clone())
    );
    assert_eq!(
        store.apt_snapshot_by_revision(&key, 2).await.unwrap(),
        Some(next_winner)
    );
    assert_eq!(
        store.apt_snapshot_by_revision(&key, 1).await.unwrap(),
        Some(winner.clone())
    );
    assert_eq!(
        store
            .apt_snapshot_by_packages_hash(&key, "amd64", &indexed.location.digest)
            .await
            .unwrap(),
        Some(winner)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hsp295_dgraph_apt_group_snapshot_stays_coherent_during_publication() {
    let Some(_client) = fresh_client().await else {
        return;
    };
    let the_account = account("hsp295-group");
    let vertical = Vertical::mount_apt_group(&the_account).await;
    let port = serve_over_tcp(&vertical).await;
    let key = call(port, "GET", "/apt/apt-group/key.asc", None);
    assert_eq!(key.status, 200, "{}", text_of(&key));
    let verification_dir = next("apt-group-release-signature");
    std::fs::create_dir_all(&verification_dir).expect("the signature verification directory");
    std::fs::write(
        verification_dir.join("keyring.bin"),
        dearmor(&text_of(&key)),
    )
    .expect("the group keyring is written");

    let packages: Vec<(String, Vec<u8>)> = (0..8)
        .map(|index| {
            let bytes = a_package(&format!("race-pkg-{index}"), "1.0.0");
            let control = apt::read_deb_control(&bytes).expect("the package control parses");
            let pool =
                apt::pool_path_for(&control.package, &control.version, &control.architecture)
                    .expect("the canonical pool path");
            (pool, bytes)
        })
        .collect();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(packages.len() + 1));
    std::thread::scope(|scope| {
        for (pool, bytes) in &packages {
            let barrier = barrier.clone();
            scope.spawn(move || {
                barrier.wait();
                let answer = call(
                    port,
                    "PUT",
                    &format!("/apt/apt-member-a/{pool}"),
                    Some(bytes),
                );
                assert_eq!(
                    answer.status,
                    200,
                    "concurrent APT package publication failed: {}",
                    text_of(&answer)
                );
            });
        }
        barrier.wait();

        for iteration in 0..12 {
            let inrelease = call(port, "GET", "/apt/apt-group/dists/bookworm/InRelease", None);
            assert_eq!(inrelease.status, 200, "InRelease iteration {iteration}");
            let signed_path = verification_dir.join(format!("inrelease-{iteration}"));
            let extracted_path = verification_dir.join(format!("release-{iteration}"));
            std::fs::write(&signed_path, &inrelease.body).expect("InRelease is written");
            let verdict = Command::new("gpgv")
                .arg("--keyring")
                .arg(verification_dir.join("keyring.bin"))
                .arg("--output")
                .arg(&extracted_path)
                .arg(&signed_path)
                .env("LC_ALL", "C")
                .output()
                .expect("gpgv runs");
            assert!(
                verdict.status.success(),
                "InRelease iteration {iteration}: {}",
                String::from_utf8_lossy(&verdict.stderr)
            );
            let release = std::fs::read(&extracted_path).expect("gpgv extracts the signed Release");
            let release =
                apt::parse_release(std::str::from_utf8(&release).expect("Release is UTF-8"))
                    .expect("the signed Release parses");
            let (packages_digest, packages_size) = release
                .sha256_entries
                .get("main/binary-amd64/Packages")
                .expect("InRelease pins the amd64 Packages index");

            let by_hash = call(
                port,
                "GET",
                &format!(
                    "/apt/apt-group/dists/bookworm/main/binary-amd64/by-hash/SHA256/{packages_digest}"
                ),
                None,
            );
            assert_eq!(
                by_hash.status,
                200,
                "by-hash iteration {iteration}: {}",
                text_of(&by_hash)
            );
            assert_eq!(by_hash.body.len() as u64, *packages_size);
            assert_eq!(sha256_hex(&by_hash.body), *packages_digest);

            let stanzas = apt::parse_packages_stanzas(&text_of(&by_hash))
                .expect("the pinned Packages index parses");
            for stanza in stanzas {
                let field = |name: &str| {
                    stanza
                        .raw
                        .lines()
                        .find_map(|line| line.strip_prefix(&format!("{name}: ")))
                        .unwrap_or_else(|| panic!("{} stanza lacks {name}", stanza.package))
                        .to_owned()
                };
                let expected_size: usize = field("Size").parse().expect("the package size parses");
                let expected_digest = field("SHA256");
                let pool = call(
                    port,
                    "GET",
                    &format!("/apt/apt-group/{}", stanza.filename),
                    None,
                );
                assert_eq!(
                    pool.status, 200,
                    "pool path {} remains available",
                    stanza.filename
                );
                assert_eq!(
                    pool.body.len(),
                    expected_size,
                    "pool size for {}",
                    stanza.package
                );
                assert_eq!(
                    sha256_hex(&pool.body),
                    expected_digest,
                    "pool digest for {}",
                    stanza.package
                );
            }
        }
    });

    let release = call(port, "GET", "/apt/apt-group/dists/bookworm/Release", None);
    assert_eq!(release.status, 200, "{}", text_of(&release));
    let signature = call(
        port,
        "GET",
        "/apt/apt-group/dists/bookworm/Release.gpg",
        None,
    );
    assert_eq!(signature.status, 200, "{}", text_of(&signature));
    let release_file = verification_dir.join("final-release");
    let signature_file = verification_dir.join("final-signature");
    std::fs::write(&release_file, &release.body).expect("the final Release is written");
    std::fs::write(&signature_file, &signature.body).expect("the final signature is written");
    assert!(
        gpgv_verdict(&verification_dir, "final-signature", "final-release"),
        "the stable final Release verifies against the group key"
    );
    let release = apt::parse_release(&text_of(&release)).expect("the final Release parses");
    let (packages_digest, packages_size) = release
        .sha256_entries
        .get("main/binary-amd64/Packages")
        .expect("Release pins the final Packages index");
    let packages = call(
        port,
        "GET",
        "/apt/apt-group/dists/bookworm/main/binary-amd64/Packages",
        None,
    );
    assert_eq!(packages.status, 200);
    assert_eq!(packages.body.len() as u64, *packages_size);
    assert_eq!(sha256_hex(&packages.body), *packages_digest);

    let mut reconfigured = vertical
        .store
        .repository(&the_account, "apt-group")
        .await
        .expect("the group is readable")
        .config;
    reconfigured.config_settings = serde_json::json!({"group": {"members": ["apt-member-b"]}});
    let updated = vertical
        .store
        .update_config(&the_account, "apt-group", reconfigured)
        .await
        .expect("the authorized group configuration is updated");
    assert_eq!(updated.config.version, 2);

    let revoked_release = call(port, "GET", "/apt/apt-group/dists/bookworm/Release", None);
    assert_eq!(revoked_release.status, 200, "{}", text_of(&revoked_release));
    let revoked_packages = call(
        port,
        "GET",
        "/apt/apt-group/dists/bookworm/main/binary-amd64/Packages",
        None,
    );
    assert_eq!(revoked_packages.status, 200);
    assert!(!text_of(&revoked_packages).contains("race-pkg-0"));
    let revoked_by_hash = call(
        port,
        "GET",
        &format!(
            "/apt/apt-group/dists/bookworm/main/binary-amd64/by-hash/SHA256/{packages_digest}"
        ),
        None,
    );
    assert_eq!(revoked_by_hash.status, 404);
    let revoked_pool = call(
        port,
        "GET",
        "/apt/apt-group/pool/main/r/race-pkg-0/race-pkg-0_1.0.0_amd64.deb",
        None,
    );
    assert_eq!(revoked_pool.status, 404);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hsp295_dgraph_apt_group_by_hash_is_scoped_to_its_group() {
    let Some(_client) = fresh_client().await else {
        return;
    };
    let the_account = account("hsp295-revocation");
    let vertical = Vertical::mount_apt_group(&the_account).await;
    let port = serve_over_tcp(&vertical).await;

    vertical
        .store
        .create_repository(
            &the_account,
            an_apt_group_descriptor(&the_account, "outside-group", &["apt-member-a"]),
        )
        .await
        .expect("the other group is created");
    let foreign_index = b"this digest belongs only to another APT group\n";
    let foreign_digest = vertical
        .content
        .put(foreign_index)
        .await
        .expect("the foreign index is stored in the shared CAS");
    let foreign_reference = AptCasReference {
        location: hodei_catalog::BlobLocation::new(
            &the_account,
            DigestScope::Global,
            foreign_digest.clone(),
        ),
        size_bytes: foreign_index.len() as u64,
    };
    let outside_key = AptSnapshotKey {
        account: the_account.clone(),
        group: "outside-group".into(),
        distribution: "bookworm".into(),
        principal_subject: format!("hrn:hodei:iam::{the_account}:User/alice"),
    };
    vertical
        .store
        .publish_apt_snapshot(
            &outside_key,
            None,
            AptGroupSnapshot {
                revision: 1,
                digest: foreign_digest.clone(),
                release: foreign_reference.clone(),
                signature: foreign_reference.clone(),
                packages_by_arch: [("amd64".into(), foreign_reference.clone())]
                    .into_iter()
                    .collect(),
                pool_by_path: Default::default(),
                members: vec![hodei_catalog::AptSnapshotMember {
                    repository: "apt-member-a".into(),
                    package: "foreign-index".into(),
                    version: "1.0".into(),
                    content: foreign_reference,
                }],
            },
        )
        .await
        .expect("the outside group registers its own Packages hash");

    let foreign_hash = call(
        port,
        "GET",
        &format!(
            "/apt/apt-group/dists/bookworm/main/binary-amd64/by-hash/SHA256/{}",
            foreign_digest.value
        ),
        None,
    );
    assert_eq!(
        foreign_hash.status, 404,
        "a digest registered only by another group is not served"
    );
}

async fn assert_failed_group_snapshot_is_not_served(
    vertical: &Vertical,
    the_account: &str,
    expected_status: StatusCode,
) {
    let port = serve_over_tcp(vertical).await;
    let package = a_package("failure-probe", "1.0.0");
    let pool_path = "pool/main/failure-probe/failure-probe_1.0.0_amd64.deb";
    let uploaded = call(
        port,
        "PUT",
        &format!("/apt/apt-member-a/{pool_path}"),
        Some(&package),
    );
    assert_eq!(uploaded.status, 200, "{}", text_of(&uploaded));

    let failed = call(port, "GET", "/apt/apt-group/dists/bookworm/Release", None);
    assert_eq!(failed.status, expected_status, "{}", text_of(&failed));
    assert!(!text_of(&failed).contains("Package: failure-probe"));

    let key = AptSnapshotKey {
        account: the_account.to_string(),
        group: "apt-group".into(),
        distribution: "bookworm".into(),
        principal_subject: format!("hrn:hodei:iam::{the_account}:User/alice"),
    };
    assert_eq!(
        vertical.store.current_apt_snapshot(&key).await.unwrap(),
        None,
        "the failed attempt must not move the Dgraph current pointer"
    );
    let pool_key = AptSnapshotKey {
        distribution: "__pool__".into(),
        ..key.clone()
    };
    assert_eq!(
        vertical
            .store
            .current_apt_snapshot(&pool_key)
            .await
            .unwrap(),
        None,
        "the failed attempt must not publish a pool snapshot"
    );

    for path in [
        "/apt/apt-group/dists/bookworm/Release",
        "/apt/apt-group/dists/bookworm/main/binary-amd64/Packages",
    ] {
        let answer = call(port, "GET", path, None);
        assert_eq!(
            answer.status,
            expected_status,
            "{path}: {}",
            text_of(&answer)
        );
        assert!(!text_of(&answer).contains("failure-probe"));
    }
    let by_hash = call(
        port,
        "GET",
        &format!(
            "/apt/apt-group/dists/bookworm/main/binary-amd64/by-hash/SHA256/{}",
            "0".repeat(64)
        ),
        None,
    );
    assert_ne!(by_hash.status, StatusCode::OK, "{}", text_of(&by_hash));
    assert!(!text_of(&by_hash).contains("failure-probe"));
    let pool = call(port, "GET", &format!("/apt/apt-group/{pool_path}"), None);
    assert_ne!(pool.status, StatusCode::OK, "{}", text_of(&pool));
    assert!(!text_of(&pool).contains("failure-probe"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hsp295_dgraph_apt_group_signing_failure_is_not_served() {
    let Some(_client) = fresh_client().await else {
        return;
    };
    let the_account = account("hsp295-sign-failure");
    let signer = hodei_artifacts_api::openpgp::MetadataSigner::generate("failure fixture")
        .expect("fixture signer");
    let vertical = Vertical::mount_apt_group_with(
        &the_account,
        Some(Arc::new(FailMetadataSigning { delegate: signer })),
        false,
    )
    .await;
    assert_failed_group_snapshot_is_not_served(
        &vertical,
        &the_account,
        StatusCode::INTERNAL_SERVER_ERROR,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hsp295_dgraph_apt_group_snapshot_publish_failure_is_not_served() {
    let Some(_client) = fresh_client().await else {
        return;
    };
    let the_account = account("hsp295-store-failure");
    let vertical = Vertical::mount_apt_group_with(&the_account, None, true).await;
    assert_failed_group_snapshot_is_not_served(
        &vertical,
        &the_account,
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hsp295_dgraph_apt_proxy_precedes_hosted_with_persisted_provenance() {
    let Some(_client) = fresh_client().await else {
        return;
    };
    let the_account = account("hsp295-proxy-hosted");
    let package = "precedence-pkg";
    let version = "1.0.0";
    let pool_path = "pool/main/precedence-pkg/precedence-pkg_1.0.0_amd64.deb";
    let proxy_bytes = a_package(package, version);
    let hosted_bytes = build_deb(concat!(
        "Package: precedence-pkg\n",
        "Version: 1.0.0\n",
        "Architecture: amd64\n",
        "Description: hosted fallback bytes\n",
    ));
    assert_ne!(proxy_bytes, hosted_bytes);
    let signer = hodei_artifacts_api::openpgp::MetadataSigner::generate("Upstream fixture")
        .expect("fixture signer");
    let trusted_key_asc = signer.key_asc().expect("upstream public key");
    let mut proxy_files = signed_proxy_fixture(package, version, pool_path, &proxy_bytes, &signer);
    add_inrelease(&mut proxy_files, &signer);
    let upstream = ProxyUpstream::start(proxy_files);
    let vertical =
        Vertical::mount_apt_proxy_group(&the_account, upstream.port, &trusted_key_asc).await;
    let port = serve_over_tcp(&vertical).await;
    let hosted = call(
        port,
        "PUT",
        &format!("/apt/apt-hosted/{pool_path}"),
        Some(&hosted_bytes),
    );
    assert_eq!(hosted.status, 200, "{}", text_of(&hosted));

    let release = call(port, "GET", "/apt/apt-group/dists/bookworm/Release", None);
    assert_eq!(release.status, 200, "{}", text_of(&release));
    let packages = call(
        port,
        "GET",
        "/apt/apt-group/dists/bookworm/main/binary-amd64/Packages",
        None,
    );
    assert_eq!(packages.status, 200, "{}", text_of(&packages));
    assert!(text_of(&packages).contains(&format!("SHA256: {}", sha256_hex(&proxy_bytes))));
    assert!(!text_of(&packages).contains(&sha256_hex(&hosted_bytes)));
    let pool = call(port, "GET", &format!("/apt/apt-group/{pool_path}"), None);
    assert_eq!(pool.status, 200, "{}", text_of(&pool));
    assert_eq!(pool.body, proxy_bytes);
    let release_data = apt::parse_release(&text_of(&release)).expect("Release parses");
    let expected_packages_digest = sha256_hex(&packages.body);
    assert_eq!(
        release_data.sha256_entries["main/binary-amd64/Packages"].0,
        expected_packages_digest
    );

    let key = AptSnapshotKey {
        account: the_account.clone(),
        group: "apt-group".into(),
        distribution: "bookworm".into(),
        principal_subject: format!("hrn:hodei:iam::{the_account}:User/alice"),
    };
    let snapshot = vertical
        .store
        .current_apt_snapshot(&key)
        .await
        .unwrap()
        .expect("the Dgraph group snapshot persists");
    let member = snapshot
        .members
        .iter()
        .find(|member| member.package == package)
        .expect("snapshot records the winning package");
    assert_eq!(member.repository, "apt-proxy");

    let ledger = DgraphProvenanceLedger::open(fresh_client().await.expect("ledger client"))
        .await
        .expect("Dgraph provenance ledger opens");
    let receipts =
        hodei_catalog::ProvenanceLedger::receipts_of(&ledger, &the_account, "apt-proxy", pool_path)
            .await
            .expect("persisted Dgraph receipts are readable");
    let receipt = receipts
        .iter()
        .find(|receipt| receipt.digest.value == sha256_hex(&proxy_bytes))
        .expect("proxy receipt with winning bytes persists in Dgraph");
    assert_eq!(receipt.upstream_id, format!("apt-it-{}", upstream.port));
    assert_eq!(receipt.digest.value, sha256_hex(&proxy_bytes));
    assert_ne!(receipt.digest.value, sha256_hex(&hosted_bytes));
    upstream.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hsp295_dgraph_apt_proxy_group_rejects_untrusted_release_signature() {
    let Some(_client) = fresh_client().await else {
        return;
    };
    let account = account("hsp295-untrusted-proxy");
    let package = "untrusted-proxy-pkg";
    let version = "1.0.0";
    let pool_path = "pool/main/u/untrusted-proxy-pkg/untrusted-proxy-pkg_1.0.0_amd64.deb";
    let deb = a_package(package, version);
    let upstream_signer =
        hodei_artifacts_api::openpgp::MetadataSigner::generate("Unexpected upstream signer")
            .expect("upstream signer");
    let wrong_trust_anchor =
        hodei_artifacts_api::openpgp::MetadataSigner::generate("Operator-pinned signer")
            .expect("pinned signer");
    let trusted_key_asc = wrong_trust_anchor
        .key_asc()
        .expect("operator-pinned public key");
    let mut upstream_files =
        signed_proxy_fixture(package, version, pool_path, &deb, &upstream_signer);
    add_inrelease(&mut upstream_files, &upstream_signer);
    let upstream = ProxyUpstream::start(upstream_files);
    let vertical = Vertical::mount_apt_proxy_group(&account, upstream.port, &trusted_key_asc).await;
    let port = serve_over_tcp(&vertical).await;

    let response = call(port, "GET", "/apt/apt-group/dists/bookworm/Release", None);
    assert_eq!(
        response.status,
        StatusCode::BAD_GATEWAY,
        "{}",
        text_of(&response)
    );

    let key = AptSnapshotKey {
        account: account.clone(),
        group: "apt-group".into(),
        distribution: "bookworm".into(),
        principal_subject: format!("hrn:hodei:iam::{account}:User/alice"),
    };
    assert!(
        vertical
            .store
            .current_apt_snapshot(&key)
            .await
            .expect("snapshot lookup")
            .is_none(),
        "untrusted metadata must not publish a durable group snapshot"
    );
    upstream.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hsp295_dgraph_apt_proxy_group_revocation_blocks_cached_package() {
    let Some(_client) = fresh_client().await else {
        return;
    };
    let account = account("hsp295-proxy-revocation");
    let package = "cached-proxy-pkg";
    let version = "1.0.0";
    let pool_path = "pool/main/c/cached-proxy-pkg/cached-proxy-pkg_1.0.0_amd64.deb";
    let deb = a_package(package, version);
    let signer = hodei_artifacts_api::openpgp::MetadataSigner::generate(
        "Upstream fixture <upstream@example.local>",
    )
    .expect("fixture signer");
    let trusted_key_asc = signer.key_asc().expect("upstream public key");
    let upstream = ProxyUpstream::start(signed_proxy_fixture(
        package, version, pool_path, &deb, &signer,
    ));
    let vertical = Vertical::mount_apt_proxy_group(&account, upstream.port, &trusted_key_asc).await;
    let port = serve_over_tcp(&vertical).await;

    let release = call(port, "GET", "/apt/apt-group/dists/bookworm/Release", None);
    assert_eq!(release.status, 200, "{}", text_of(&release));
    let old_packages_digest = apt::parse_release(&text_of(&release))
        .expect("the proxy group Release parses")
        .sha256_entries
        .get("main/binary-amd64/Packages")
        .expect("Release pins Packages")
        .0
        .clone();
    let packages = call(
        port,
        "GET",
        "/apt/apt-group/dists/bookworm/main/binary-amd64/Packages",
        None,
    );
    assert_eq!(packages.status, 200, "{}", text_of(&packages));
    assert!(text_of(&packages).contains("Package: cached-proxy-pkg"));

    let cached_package = call(port, "GET", &format!("/apt/apt-group/{pool_path}"), None);
    assert_eq!(cached_package.status, 200, "{}", text_of(&cached_package));
    assert_eq!(cached_package.body, deb);
    assert_eq!(
        cached_package.header("x-hodei-cache").as_deref(),
        Some("miss")
    );
    let upstream_hits = upstream.hits.load(Ordering::Relaxed);
    let cached_again = call(port, "GET", &format!("/apt/apt-group/{pool_path}"), None);
    assert_eq!(cached_again.status, 200);
    assert_eq!(cached_again.body, cached_package.body);
    assert_eq!(cached_again.header("x-hodei-cache").as_deref(), Some("hit"));
    assert_eq!(upstream.hits.load(Ordering::Relaxed), upstream_hits);

    let mut config = vertical
        .store
        .repository(&account, "apt-group")
        .await
        .expect("group descriptor")
        .config;
    vertical
        .store
        .create_repository(&account, an_apt_descriptor(&account, "unrelated-apt"))
        .await
        .expect("unrelated hosted member");
    config.config_settings = serde_json::json!({"group": {"members": ["unrelated-apt"]}});
    let updated = vertical
        .store
        .update_config(&account, "apt-group", config)
        .await
        .expect("the authorized Dgraph group config is updated");
    assert_eq!(updated.config.version, 2);

    let revoked_release = call(port, "GET", "/apt/apt-group/dists/bookworm/Release", None);
    assert_eq!(revoked_release.status, 200, "{}", text_of(&revoked_release));
    let new_release = apt::parse_release(&text_of(&revoked_release))
        .expect("the reconfigured group Release parses");
    let new_packages_digest = new_release
        .sha256_entries
        .get("main/binary-amd64/Packages")
        .expect("the new Release pins Packages");
    assert_ne!(new_packages_digest.0, old_packages_digest);

    let revoked_packages = call(
        port,
        "GET",
        "/apt/apt-group/dists/bookworm/main/binary-amd64/Packages",
        None,
    );
    assert_eq!(
        revoked_packages.status,
        200,
        "{}",
        text_of(&revoked_packages)
    );
    assert!(!text_of(&revoked_packages).contains("Package: cached-proxy-pkg"));

    let revoked_by_hash = call(
        port,
        "GET",
        &format!(
            "/apt/apt-group/dists/bookworm/main/binary-amd64/by-hash/SHA256/{old_packages_digest}"
        ),
        None,
    );
    assert_eq!(revoked_by_hash.status, 404, "{}", text_of(&revoked_by_hash));

    let revoked = call(port, "GET", &format!("/apt/apt-group/{pool_path}"), None);
    assert_eq!(revoked.status, 404, "{}", text_of(&revoked));
    assert!(revoked.header("x-hodei-cache").is_none());
    assert_eq!(upstream.hits.load(Ordering::Relaxed), upstream_hits);
    upstream.stop();
}
