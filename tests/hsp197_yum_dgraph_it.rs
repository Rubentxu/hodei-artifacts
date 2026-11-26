//! HSP-197: RPM/yum over the real Dgraph engine, with the real `dnf` clients
//! and the `repomd.xml` signature verified.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` like the spine ITs (TLS + ACL, the
//! live h186 server). The yum vertical (`src/handlers/yum.rs` +
//! `crates/hodei-catalog/src/yum.rs`) feeds its `primary` index, its
//! `repomd.xml` and its detached `repomd.xml.asc` signature exclusively
//! through the catalog ports (HSP-187/190/191/193/195: keyed guards,
//! atomic outbox, scoped listings), and this slice demonstrates that
//! against the ENGINE:
//!
//! - **AC1** the host's own `microdnf` (dnf5) refreshes the metadata of
//!   the Dgraph-backed vertical and verifies its detached signature
//!   (`repo_gpgcheck=1`), the served `primary.xml.gz` is content-addressed
//!   by its own SHA256 and pinned in `repomd.xml`, `gpgv` on the host
//!   opens the signature against the served `key.asc` and refuses a
//!   tampered `repomd`, a brand-new process over the same graph and the
//!   same CAS serves byte-identical metadata after a restart, and the
//!   real `dnf` of the pinned `quay.io/fedora/fedora` container — the
//!   distro the contract fixes — installs the published `.rpm`. The
//!   published binary is built by the container's own `rpmbuild` with an
//!   epoch and a version carrying `~` and `+` (`1.0.0~rc1+dfsg`, epoch
//!   `1`), so the pool path
//!   `pool/hello-tilde-1:1.0.0~rc1+dfsg-1.x86_64.rpm` exercises exactly
//!   the charset the `guard_part` escape (HSP-195) resolves on Dgraph.
//! - **AC2** a foreign repository never looks valid (404s on pool,
//!   repodata and indices, which list only their own rows), a vanished
//!   record blob takes `repomd.xml` and `primary` down (404, never
//!   stale-valid), a publish whose content is not in the CAS is refused
//!   by the probe with no catalog rows, a stranger keyring is refused by
//!   the real dnf, unmapped routes are 404, a path the header does not
//!   dictate is 409 and other bytes on a published version are 409 while
//!   the same bytes echo 200. The outbox census is per tenant
//!   (`eq(hodei.catalog.outbox.tenant)`, the HSP-220 lesson) and the plan
//!   probe confirms the exact indexes.
//! - Contract reads: the published yum state seeded at store level answers
//!   what the grammar and the publication say (rows, digests and the
//!   ordered listing the `primary` assembles from), and an immutable path conflicts.

mod support;

use std::io::Read as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use axum::Router;
use base64::Engine as _;
use cedar_policy::Entities;
use chrono::Utc;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::blob_probe::BlobStoreProbe;
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use hodei_catalog::yum;
use hodei_catalog::{
    AllowAllAuthorizer, CatalogReader, CatalogWriter, DigestScope, PackageCoordinates, Publication,
    RepositoryConfig, RepositoryDescriptor, RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_catalog_storage::DgraphCatalogStore;
use hodei_evidence::ContentDigest;
use hodei_graph_client::{GraphClient, GraphClientConfig};
use hodei_hub::blob::FileBlobStore;
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

/// One account per test and per run.
fn account(tag: &str) -> String {
    format!("it197{}{}", tag, &nonce()[8..])
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

/// The evidence directory: `target/uat/hsp197/`.
fn the_evidence_dir() -> PathBuf {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let target = tmp.parent().unwrap_or(&tmp).to_path_buf();
    let dir = target.join("uat").join("hsp197");
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
    format!("corr-hsp197-{the_account}")
}

/// A hosted rpm repository descriptor.
fn an_rpm_descriptor(the_account: &str, repo_key: &str) -> RepositoryDescriptor {
    let config = RepositoryConfig::new(DigestScope::Global);
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", the_account, repo_key),
        account: the_account.to_string(),
        repo_key: repo_key.to_string(),
        format: RepositoryFormat::Rpm,
        mode: RepositoryMode::Hosted,
        config,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

// ---------------------------------------------------------------------------
// The RPM fixtures: the pool grammar of the contract with the hostile
// charset (`:` epoch, `~`, `+`), the synthetic container the hsp122 suite
// uses for the store-level cases, and the real one built inside the
// pinned container.
// ---------------------------------------------------------------------------

const REPO: &str = "fedora-hosted";

/// The version the RPM world considers hostile for guard scopes: epoch,
/// tilde and plus together. rpmbuild lands it verbatim in the header.
const TILDE_VERSION: &str = "1.0.0~rc1+dfsg";
const TILDE_RELEASE: &str = "1";
const TILDE_EPOCH: u32 = 1;
const TILDE_POOL: &str = "pool/hello-tilde-1:1.0.0~rc1+dfsg-1.x86_64.rpm";
const TILDE_RECORD: &str = "pool/hello-tilde-1:1.0.0~rc1+dfsg-1.x86_64.rpm.record";

/// The spec of the real package: `rpmbuild` demands it well formed, and
/// the header must carry epoch, tilde and plus for the guard charset.
const RPM_SPEC: &str = concat!(
    "Name: hello-tilde\n",
    "Version: 1.0.0~rc1+dfsg\n",
    "Release: 1\n",
    "Epoch: 1\n",
    "Summary: A tiny hello package\n",
    "License: MIT\n",
    "BuildRoot: %{_tmppath}/hello-tilde-root\n",
    "%description\n",
    "Installed through the real dnf clients against Hodei.\n",
    "%install\n",
    "mkdir -p %{buildroot}/usr/share/hello-tilde\n",
    "cp /work/greeting.txt %{buildroot}/usr/share/hello-tilde/greeting.txt\n",
    "%files\n",
    "/usr/share/hello-tilde/greeting.txt\n",
);

// The tag numbers of `rpmtag.h`, as the grammar in `yum.rs` anchors them.
const TAG_NAME: i32 = 1000;
const TAG_VERSION: i32 = 1001;
const TAG_RELEASE: i32 = 1002;
const TAG_EPOCH: i32 = 1003;
const TAG_SUMMARY: i32 = 1004;
const TAG_DESCRIPTION: i32 = 1005;
const TAG_BUILDTIME: i32 = 1006;
const TAG_SIZE: i32 = 1009;
const TAG_LICENSE: i32 = 1014;
const TAG_ARCH: i32 = 1022;

#[derive(Clone)]
enum HeaderValue {
    Str(&'static str),
    U32(u32),
}

#[derive(Clone)]
struct TestHeaderField {
    tag: i32,
    value: HeaderValue,
}

/// Builds a `.rpm`: lead, empty signature header and main header with the
/// fields asked for. Not installable (the payload is empty), but the
/// grammar's own parser sees it as a real one. Ported from the unit
/// fixtures of `yum.rs` (they are `cfg(test)` there).
fn build_test_rpm(fields: &[TestHeaderField]) -> Vec<u8> {
    let mut lead = vec![0u8; 96];
    lead[0..4].copy_from_slice(b"\xed\xab\xee\xdb");
    lead[4] = 3;
    lead[6..8].copy_from_slice(&1u16.to_be_bytes());

    let mut signature = vec![0x8e, 0xad, 0xe8, 1];
    signature.extend_from_slice(&[0u8; 4]);
    signature.extend_from_slice(&0u32.to_be_bytes());
    signature.extend_from_slice(&0u32.to_be_bytes());

    let mut store: Vec<u8> = Vec::new();
    let mut index: Vec<u8> = Vec::new();
    for field in fields {
        let (offset, entry_size) = match &field.value {
            HeaderValue::Str(text) => {
                let offset = store.len() as u32;
                store.extend_from_slice(text.as_bytes());
                store.push(0);
                (offset, 0u32)
            }
            HeaderValue::U32(number) => {
                let pad = (4 - store.len() % 4) % 4;
                store.extend(std::iter::repeat_n(0, pad));
                let offset = store.len() as u32;
                store.extend_from_slice(&number.to_be_bytes());
                (offset, 1u32)
            }
        };
        let kind: u32 = match &field.value {
            HeaderValue::Str(_) => 6,
            HeaderValue::U32(_) => 4,
        };
        index.extend_from_slice(&field.tag.to_be_bytes());
        index.extend_from_slice(&kind.to_be_bytes());
        index.extend_from_slice(&offset.to_be_bytes());
        index.extend_from_slice(&entry_size.to_be_bytes());
    }
    let mut main = vec![0x8e, 0xad, 0xe8, 1];
    main.extend_from_slice(&[0u8; 4]);
    main.extend_from_slice(&(fields.len() as u32).to_be_bytes());
    main.extend_from_slice(&(store.len() as u32).to_be_bytes());
    main.extend_from_slice(&index);
    main.extend_from_slice(&store);

    let mut rpm = lead;
    rpm.extend(signature);
    let pad = (8 - rpm.len() % 8) % 8;
    rpm.extend(std::iter::repeat_n(0, pad));
    rpm.extend(main);
    rpm
}

fn rpm_fields(
    name: &'static str,
    version: &'static str,
    release: &'static str,
) -> Vec<TestHeaderField> {
    let field = |tag, value| TestHeaderField { tag, value };
    vec![
        field(TAG_NAME, HeaderValue::Str(name)),
        field(TAG_VERSION, HeaderValue::Str(version)),
        field(TAG_RELEASE, HeaderValue::Str(release)),
        field(TAG_SUMMARY, HeaderValue::Str("A tiny hello package")),
        field(TAG_DESCRIPTION, HeaderValue::Str("Installed through dnf.")),
        field(TAG_BUILDTIME, HeaderValue::U32(1_700_000_000)),
        field(TAG_SIZE, HeaderValue::U32(4096)),
        field(TAG_ARCH, HeaderValue::Str("x86_64")),
        field(TAG_LICENSE, HeaderValue::Str("MIT")),
    ]
}

/// A synthetic but header-valid `.rpm` with no epoch.
fn a_rpm(name: &'static str, version: &'static str, release: &'static str) -> Vec<u8> {
    build_test_rpm(&rpm_fields(name, version, release))
}

/// The same identity with the tilde/plus/epoch charset: the path the
/// guard used to refuse on Dgraph.
fn a_tilde_rpm() -> Vec<u8> {
    let mut fields = rpm_fields("hello-tilde", TILDE_VERSION, TILDE_RELEASE);
    fields.insert(
        3,
        TestHeaderField {
            tag: TAG_EPOCH,
            value: HeaderValue::U32(TILDE_EPOCH),
        },
    );
    build_test_rpm(&fields)
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

fn try_gunzip(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut decoder = flate2::read::GzDecoder::new(bytes);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).ok().map(|_| out)
}

fn gunzip(bytes: &[u8]) -> Vec<u8> {
    try_gunzip(bytes).expect("the gz decodes")
}

/// The primary location the repomd itself declares; it already carries
/// the `repodata/` prefix.
fn href_of(repomd_text: &str) -> String {
    repomd_text
        .split("<location href=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("the repomd declares the primary location")
        .to_string()
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

/// A whole directory, copied: a restart of the service keeps its CAS.
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The pinned real clients: host `microdnf` (dnf5) and the dnf of the
// quay.io/fedora/fedora image hsp122 fixes.
// ---------------------------------------------------------------------------

const IMAGE: &str = "quay.io/fedora/fedora:latest";

fn require_podman() {
    let found = Command::new("podman")
        .args(["--version"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "podman is not installed: the yum client suite needs a fedora container"
    );
    let image = Command::new("podman")
        .args(["image", "exists", IMAGE])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    assert!(
        image,
        "the image {IMAGE} is not available: pull it before running the yum client suite"
    );
}

/// The host's own dnf5: microdnf preferred (it is the client that
/// enforces repomd signatures), dnf as the alternative. None means the
/// host-side half of AC is skipped with a recorded reason.
fn host_dnf() -> Option<&'static str> {
    for binary in ["microdnf", "dnf"] {
        let found = Command::new(binary)
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);
        if found {
            return Some(binary);
        }
    }
    None
}

/// The repo file for the real host clients: the userinfo the contract's
/// Basic auth expects, repo_gpgcheck on, the key the hub serves.
fn host_repo_file(port: u16, key_url: &str) -> String {
    format!(
        concat!(
            "[hodei]\n",
            "name=Hodei over dgraph\n",
            "baseurl=http://alice:alice@127.0.0.1:{port}/yum/{repo}/\n",
            "enabled=1\n",
            "gpgcheck=0\n",
            "repo_gpgcheck=1\n",
            "gpgkey={key_url}\n",
        ),
        port = port,
        repo = REPO,
        key_url = key_url,
    )
}

/// Runs the host dnf5 isolated: its own installroot, cachedir and an
/// empty home, so runs never inherit each other's keyrings.
fn host_dnf_run(binary: &str, work: &Path, command: &[&str]) -> std::process::Output {
    let installroot = work.join("iroot");
    let cache = work.join("cache");
    let home = work.join("home");
    std::fs::create_dir_all(&installroot).expect("installroot");
    std::fs::create_dir_all(&cache).expect("cache dir");
    std::fs::create_dir_all(&home).expect("home dir");
    Command::new(binary)
        .arg("-y")
        .arg(format!("--installroot={}", installroot.display()))
        .arg("--releasever=44")
        .arg(format!("--setopt=cachedir={}", cache.display()))
        .arg("--disablerepo=*")
        .arg("--enablerepo=hodei")
        .args(command)
        .env("HOME", &home)
        .env("LC_ALL", "C")
        .output()
        .expect("the host dnf runs")
}

fn container(work: &Path, script: &str) -> std::process::Output {
    Command::new("podman")
        .arg("run")
        .arg("--rm")
        .arg("--network=host")
        .arg("-v")
        .arg(format!("{}:/work:z", work.display()))
        .arg(IMAGE)
        .arg("bash")
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

/// The host `gpgv` invocation: the same verifier dnf itself calls.
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

/// Every file under `dir` whose name contains `needle`.
fn find_under(dir: &Path, needle: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(entry) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&entry) else {
            continue;
        };
        for item in read.flatten() {
            let path = item.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains(needle))
            {
                out.push(path);
            }
        }
    }
    out
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

struct Vertical {
    app: Router,
    store: Arc<DgraphCatalogStore>,
    /// The CAS of the vertical: deleting a blob file is how the read-side
    /// missing-content case is produced.
    content: Arc<FileBlobStore>,
    content_root: PathBuf,
    #[allow(dead_code)]
    tmp: TempDir,
}

impl Vertical {
    async fn mount_yum(the_account: &str) -> Self {
        Self::mount(the_account, REPO, None).await
    }

    /// A brand-new process over the same graph with the CAS bytes of
    /// `previous` copied in: the restart the AC1 asks for.
    async fn restart_over(previous: &Vertical, the_account: &str) -> Self {
        Self::mount(the_account, REPO, Some(&previous.content_root)).await
    }

    async fn mount(the_account: &str, repo_key: &str, cas_source: Option<&Path>) -> Self {
        let client = fresh_client().await.expect("the dgraph client connects");
        let tmp = tempfile::tempdir().expect("temp dir");
        let content_root = tmp.path().join("cas");
        if let Some(source) = cas_source {
            copy_dir(source, &content_root).expect("the CAS survives the restart");
        }
        let content = Arc::new(FileBlobStore::new(&content_root).expect("the CAS"));
        // The integrity probe is on: the contract's fail-closed publish
        // requires the CAS to actually hold what it declares.
        let store = Arc::new(
            open_store(client.clone())
                .await
                .with_probe(Arc::new(BlobStoreProbe::new(content.clone()))),
        );
        store
            .create_repository(the_account, an_rpm_descriptor(the_account, repo_key))
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
            .with_authorization(cedar_for(the_account, repo_key));

        Self {
            app: build_router(state, &config),
            store,
            content,
            content_root,
            tmp,
        }
    }
}

/// The same service on a real port: the dnf clients speak TCP.
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
// The curl transport, as hsp122/hsp195: what the server says, byte for byte.
// ---------------------------------------------------------------------------

static CALLS: AtomicUsize = AtomicUsize::new(0);

fn next(name: &str) -> PathBuf {
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{n}"))
}

fn call(port: u16, method: &str, path: &str, body: Option<&[u8]>) -> Answer {
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
        "alice",
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
// Store-level seeding: the two immutable assets one `.rpm` leaves behind
// (the binary and its `.record`), exactly as the handler publishes them.
// ---------------------------------------------------------------------------

async fn publish_rpm_pair<Store>(
    store: &Store,
    the_account: &str,
    repository: &str,
    rpm: &[u8],
) -> (String, ContentDigest, ContentDigest)
where
    Store: CatalogWriter + Send + Sync,
{
    let metadata = yum::parse_rpm(rpm).expect("the rpm header parses");
    let pool = yum::pool_path_for(&metadata).expect("the pool path is the grammar's own");
    let rpm_digest = ContentDigest::of_bytes(rpm);
    let record = yum::package_record(&metadata, &pool, rpm.len() as u64, &rpm_digest.value);
    let record_bytes = record.into_bytes();
    let record_digest = ContentDigest::of_bytes(&record_bytes);
    let version = format!("{}-{}", metadata.version, metadata.release);

    store
        .publish(
            the_account,
            Publication {
                repository: repository.to_string(),
                asset_path: pool.clone(),
                coordinates: PackageCoordinates {
                    package_name: metadata.name.clone(),
                    package_version: version.clone(),
                },
                digest: rpm_digest.clone(),
                size_bytes: rpm.len() as u64,
                tags: Vec::new(),
                actor: actor(the_account),
                correlation_id: correlation_of(the_account),
                overwrite: false,
                precondition: None,
                subject: None,
            },
        )
        .await
        .expect("the rpm publication settles");
    store
        .publish(
            the_account,
            Publication {
                repository: repository.to_string(),
                asset_path: yum::record_path_for(&pool),
                coordinates: PackageCoordinates {
                    package_name: format!("{}#record", metadata.name),
                    package_version: version,
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
    (pool, rpm_digest, record_digest)
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
// AC1: the real dnf clients resolve the metadata, verify its signature and
// install a published RPM over the Dgraph catalog — including the
// `:`/`~`/`+` pool path the guard escape resolves.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_real_dnf_clients_verify_the_signed_repodata_over_dgraph() {
    let Some(_client) = fresh_client().await else {
        return;
    };
    require_podman();
    let the_account = account("yum");
    let vertical = Vertical::mount_yum(&the_account).await;
    let port = serve_over_tcp(&vertical).await;

    // The scratch tree the container shares: the package the pinned
    // rpmbuild will really build, plus the hub key and the dnf config.
    let work = next("stage");
    std::fs::create_dir_all(&work).expect("the scratch dir");
    std::fs::write(work.join("hello.spec"), RPM_SPEC).expect("the spec");
    std::fs::write(work.join("greeting.txt"), "hola desde hodei\n").expect("the payload");

    // 1. A real `.rpm`, built by the rpm of the pinned distro, with the
    //    epoch/tilde/plus version the contract's charset demands.
    container_ok(
        &work,
        concat!(
            "set -eu\n",
            "dnf -y -q install rpm-build >/dev/null\n",
            "mkdir -p /work/rpmbuild/BUILD /work/rpmbuild/RPMS /work/rpmbuild/SOURCES \\\n",
            "         /work/rpmbuild/SPECS /work/rpmbuild/SRPMS\n",
            "rpmbuild --define '_topdir /work/rpmbuild' -bb /work/hello.spec >/dev/null\n",
            "chmod 644 /work/rpmbuild/RPMS/*/*.rpm\n",
        ),
    );
    let built = find_under(&work.join("rpmbuild/RPMS"), ".rpm")
        .into_iter()
        .next()
        .expect("rpmbuild produced a package");
    let rpm = std::fs::read(&built).expect("the built rpm");
    assert!(!rpm.is_empty(), "rpmbuild produced a package");

    // 2. Published through the yum surface over the Dgraph catalog. The
    //    path is dictated by the header: epoch, tilde and plus verbatim.
    let published = call(
        port,
        "PUT",
        &format!("/yum/{REPO}/{TILDE_POOL}"),
        Some(&rpm),
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));
    record_evidence(
        "the_real_dnf_clients_verify_the_signed_repodata_over_dgraph",
        "rpmbuild real del contenedor fedora fijado con epoch 1 y version 1.0.0~rc1+dfsg; PUT al pool dictado por el header (ruta con :, ~ y +) publica 200 sobre Dgraph (el escape guard_part de HSP-195)",
    );

    // 3. The repomd: deterministic across reads, pinning the exact gz.
    let repomd = call(
        port,
        "GET",
        &format!("/yum/{REPO}/repodata/repomd.xml"),
        None,
    );
    assert_eq!(repomd.status, 200);
    let again = call(
        port,
        "GET",
        &format!("/yum/{REPO}/repodata/repomd.xml"),
        None,
    );
    assert_eq!(
        repomd.body, again.body,
        "the repomd must not drift between reads"
    );
    let repomd_text = text_of(&repomd);
    assert!(repomd_text.contains("<data type=\"primary\">"));
    let primary_href = href_of(&repomd_text);
    let primary = call(port, "GET", &format!("/yum/{REPO}/{primary_href}"), None);
    assert_eq!(primary.status, 200);
    let gz_sha = sha256_hex(&primary.body);
    assert!(
        repomd_text.contains(&gz_sha),
        "repomd must pin the SHA256 of the served gz: {repomd_text}"
    );
    let document = gunzip(&primary.body);
    let document_text = String::from_utf8_lossy(&document).into_owned();
    assert!(
        repomd_text.contains(&sha256_hex(&document)),
        "repomd must pin the open-checksum of the index"
    );
    assert!(
        document_text.contains("<name>hello-tilde</name>")
            && document_text.contains(&format!("ver=\"{TILDE_VERSION}\""))
            && document_text.contains(&format!("epoch=\"{TILDE_EPOCH}\""))
            && document_text.contains(&format!("href=\"{TILDE_POOL}\"")),
        "the index must describe the published rpm: {document_text}"
    );
    // A route whose checksum is not the served bytes: 404, never content.
    let wrong = call(
        port,
        "GET",
        &format!("/yum/{REPO}/repodata/{}-primary.xml.gz", "cd".repeat(32)),
        None,
    );
    assert_eq!(wrong.status, 404, "a checksummed route names its own bytes");
    record_evidence(
        "the_real_dnf_clients_verify_the_signed_repodata_over_dgraph",
        "repomd determinista (doble lectura byte a byte), primary.xml.gz direccionado por su SHA256 y fijado en el repomd junto al open-checksum; ruta con checksum ajeno 404",
    );

    // 4. The detached signature: gpgv on the host opens it against the
    //    served key, and a tampered repomd does not open.
    let signature = call(
        port,
        "GET",
        &format!("/yum/{REPO}/repodata/repomd.xml.asc"),
        None,
    );
    assert_eq!(signature.status, 200);
    let key = call(port, "GET", &format!("/yum/{REPO}/key.asc"), None);
    assert_eq!(key.status, 200);
    std::fs::write(work.join("key.asc"), &key.body).expect("the key");
    let dir = next("gpgv");
    std::fs::create_dir_all(&dir).expect("a scratch dir");
    std::fs::write(dir.join("repomd.xml"), &repomd.body).expect("write repomd");
    std::fs::write(dir.join("repomd.xml.asc"), &signature.body).expect("write signature");
    std::fs::write(dir.join("keyring.bin"), dearmor(&text_of(&key))).expect("write keyring");
    let verified = Command::new("gpgv")
        .arg("--keyring")
        .arg(dir.join("keyring.bin"))
        .arg(dir.join("repomd.xml.asc"))
        .arg(dir.join("repomd.xml"))
        .env("LC_ALL", "C")
        .output()
        .expect("gpgv runs");
    let stderr = String::from_utf8_lossy(&verified.stderr);
    assert!(
        verified.status.success() && stderr.contains("Good signature"),
        "gpgv said: {stderr}"
    );
    std::fs::write(dir.join("tampered"), b"<repomd/>\n").expect("write tampered");
    assert!(
        !gpgv_verdict(&dir, "repomd.xml.asc", "tampered"),
        "a forged repomd must not open"
    );
    record_evidence(
        "the_real_dnf_clients_verify_the_signed_repodata_over_dgraph",
        "repomd.xml.asc abre con la key.asc servida (gpgv del host, Good signature) y un repomd alterado no abre",
    );

    // Conditional read on the generated metadata: the same ETag answers 304.
    let etag = repomd.header("etag").expect("the repomd carries an etag");
    let conditional_target = next("conditional");
    let not_modified = get_with(
        port,
        &format!("/yum/{REPO}/repodata/repomd.xml"),
        ("if-none-match", etag.as_str()),
        &conditional_target,
    );
    assert_eq!(not_modified.status, 304, "the ETag must answer 304");

    // 5. The host's own dnf5 refreshes and verifies: repo_gpgcheck=1,
    //    the served key, and the cached metadata is the served one.
    match host_dnf() {
        Some(binary) => {
            let repodir = work.join("iroot/etc/yum.repos.d");
            std::fs::create_dir_all(&repodir).expect("the repodir");
            std::fs::write(
                repodir.join("hodei.repo"),
                host_repo_file(port, &format!("file://{}", work.join("key.asc").display())),
            )
            .expect("the repo file");
            let refreshed = host_dnf_run(binary, &work, &["makecache", "--refresh"]);
            let stdout = String::from_utf8_lossy(&refreshed.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&refreshed.stderr).into_owned();
            assert!(
                refreshed.status.success(),
                "{binary} must refresh the signed metadata over Dgraph:\n{stdout}\n{stderr}"
            );
            let cache = work.join("cache");
            let cached_repomd = find_under(&cache, "repomd.xml")
                .into_iter()
                .find(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.ends_with("repomd.xml"))
                })
                .expect("dnf cached the repomd");
            assert_eq!(
                std::fs::read(&cached_repomd).expect("the cached repomd"),
                repomd.body,
                "the cached repomd must be the served bytes"
            );
            let cached_primary = find_under(&cache, "-primary.xml.gz")
                .into_iter()
                .find(|path| {
                    try_gunzip(&std::fs::read(path).unwrap_or_default())
                        .is_some_and(|plain| plain == document)
                })
                .expect("dnf cached the published primary");
            assert!(
                !std::fs::read(&cached_primary)
                    .expect("the cached gz")
                    .is_empty(),
                "the cached index is a real file"
            );
            record_evidence(
                "the_real_dnf_clients_verify_the_signed_repodata_over_dgraph",
                &format!(
                    "{binary} del host (dnf5, sin contenedor) hace makecache --refresh contra el hub sobre Dgraph con repo_gpgcheck=1: la metadata cacheada es exactamente la servida"
                ),
            );
        }
        None => record_evidence(
            "the_real_dnf_clients_verify_the_signed_repodata_over_dgraph",
            "microdnf/dnf del host no disponible en esta maquina: la verificacion de firma del cliente real se delega al contenedor fedora fijado (paso 6)",
        ),
    }

    // 6. The real dnf of the pinned distro: it downloads the repomd,
    //    verifies its detached signature against the served key, pulls
    //    the primary, checks the SHA256 of the package against the index
    //    and installs it.
    let script = format!(
        concat!(
            "set -eu\n",
            "cat > /etc/yum.repos.d/hodei.repo <<'EOF'\n",
            "[hodei]\n",
            "name=Hodei over dgraph\n",
            "baseurl=http://alice:alice@127.0.0.1:{port}/yum/{repo}/\n",
            "enabled=1\n",
            "gpgcheck=0\n",
            "repo_gpgcheck=1\n",
            "gpgkey=file:///work/key.asc\n",
            "EOF\n",
            "dnf -y --disablerepo='*' --enablerepo=hodei --refresh install hello-tilde\n",
            // The default NEVRA `rpm -q` does not print the epoch: it is requested
            // explicitly with the query format, which is what proves the real
            // immersion of the charset in the rpm transaction.
            "rpm -q --qf '%{{NAME}}-%{{EPOCH}}:%{{VERSION}}-%{{RELEASE}}.%{{ARCH}}\\n' hello-tilde\n",
            "cat /usr/share/hello-tilde/greeting.txt\n",
        ),
        port = port,
        repo = REPO,
    );
    let installed = container_ok(&work, &script);
    assert!(
        installed.contains("hello-tilde-1:1.0.0~rc1+dfsg-1.x86_64"),
        "rpm must report the package installed with epoch, tilde and plus: {installed}"
    );
    assert!(
        installed.contains("hola desde hodei"),
        "the installed payload must be the published one: {installed}"
    );
    record_evidence(
        "the_real_dnf_clients_verify_the_signed_repodata_over_dgraph",
        "dnf real del contenedor fedora fijado: install verifica el repomd firmado, baja el .rpm del pool (SHA256 del indice) e instala la version 1:1.0.0~rc1+dfsg publicada sobre el grafo",
    );

    // 7. After a restart: a brand-new process over the same graph and
    //    the same stored bytes serves byte-identical metadata; the
    //    revision is the state, not the clock.
    let second = Vertical::restart_over(&vertical, &the_account).await;
    let second_port = serve_over_tcp(&second).await;
    let republished = call(
        second_port,
        "PUT",
        &format!("/yum/{REPO}/{TILDE_POOL}"),
        Some(&rpm),
    );
    assert_eq!(
        republished.status,
        200,
        "the same bytes echo on the restarted vertical: {}",
        text_of(&republished)
    );
    let fresh_repomd = call(
        second_port,
        "GET",
        &format!("/yum/{REPO}/repodata/repomd.xml"),
        None,
    );
    assert_eq!(
        fresh_repomd.body, repomd.body,
        "the same state must produce the same repomd bytes across processes"
    );
    let fresh_primary = call(
        second_port,
        "GET",
        &format!("/yum/{REPO}/{primary_href}"),
        None,
    );
    assert_eq!(
        fresh_primary.body, primary.body,
        "the index is content-addressed: it survives the restart byte for byte"
    );
    let fresh_pool = call(
        second_port,
        "GET",
        &format!("/yum/{REPO}/{TILDE_POOL}"),
        None,
    );
    assert_eq!(
        fresh_pool.status, 200,
        "the binary is served from the restarted CAS at the path with :, ~ and +"
    );
    // The new process signs with its own ephemeral key: the client must
    // read the served key again, and the served signature opens with it.
    let fresh_key = call(second_port, "GET", &format!("/yum/{REPO}/key.asc"), None);
    let fresh_sig = call(
        second_port,
        "GET",
        &format!("/yum/{REPO}/repodata/repomd.xml.asc"),
        None,
    );
    let dir2 = next("gpgv2");
    std::fs::create_dir_all(&dir2).expect("a scratch dir");
    std::fs::write(dir2.join("repomd.xml"), &fresh_repomd.body).expect("write repomd");
    std::fs::write(dir2.join("repomd.xml.asc"), &fresh_sig.body).expect("write signature");
    std::fs::write(dir2.join("keyring.bin"), dearmor(&text_of(&fresh_key))).expect("write keyring");
    let reopened = Command::new("gpgv")
        .arg("--keyring")
        .arg(dir2.join("keyring.bin"))
        .arg(dir2.join("repomd.xml.asc"))
        .arg(dir2.join("repomd.xml"))
        .env("LC_ALL", "C")
        .output()
        .expect("gpgv runs");
    let reopened_err = String::from_utf8_lossy(&reopened.stderr);
    assert!(
        reopened.status.success() && reopened_err.contains("Good signature"),
        "the restarted hub signs its identical metadata with its own fresh key: {reopened_err}"
    );
    record_evidence(
        "the_real_dnf_clients_verify_the_signed_repodata_over_dgraph",
        "tras un proceso nuevo sobre el mismo grafo y el mismo CAS: repomd y primary byte a byte identicos (revision del estado, no del reloj), la subida eco 200 y la firma fresca abre con la clave que el propio proceso sirve",
    );
}

// ---------------------------------------------------------------------------
// AC2: nothing foreign, broken or forged is exposed as valid.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn foreign_and_broken_yum_state_is_never_valid() {
    let Some(_client) = fresh_client().await else {
        return;
    };
    let alice = account("owner");
    let mallory = account("foreign");
    let mine = Vertical::mount_yum(&alice).await;
    let theirs = Vertical::mount_yum(&mallory).await;
    let my_port = serve_over_tcp(&mine).await;
    let their_port = serve_over_tcp(&theirs).await;

    // Publish one synthetic-shaped package in my repository, and its
    // namesake (other bytes) in the foreign one.
    let broken = a_rpm("hello-broken", "1.0.0", "1");
    let broken_pool = "pool/hello-broken-1.0.0-1.x86_64.rpm";
    let uploaded = call(
        my_port,
        "PUT",
        &format!("/yum/{REPO}/{broken_pool}"),
        Some(&broken),
    );
    assert_eq!(uploaded.status, 200, "{}", text_of(&uploaded));

    let ghost = a_rpm("hello-ghost", "9.9.9", "1");
    let ghost_pool = "pool/hello-ghost-9.9.9-1.x86_64.rpm";
    let published_ghost = call(
        their_port,
        "PUT",
        &format!("/yum/{REPO}/{ghost_pool}"),
        Some(&ghost),
    );
    assert_eq!(published_ghost.status, 200, "{}", text_of(&published_ghost));

    // The neighbor hosts the same repository key: what it published is
    // invisible from my vertical — 404, never stale, never borrowed —
    // and the indices of each list only their own rows.
    let stolen = call(my_port, "GET", &format!("/yum/{REPO}/{ghost_pool}"), None);
    assert_eq!(stolen.status, 404, "a foreign package is not served here");
    let their_repomd = call(
        their_port,
        "GET",
        &format!("/yum/{REPO}/repodata/repomd.xml"),
        None,
    );
    assert_eq!(their_repomd.status, 200);
    let their_href = href_of(&text_of(&their_repomd));
    let their_primary = call(
        their_port,
        "GET",
        &format!("/yum/{REPO}/{their_href}"),
        None,
    );
    let their_index = String::from_utf8_lossy(&gunzip(&their_primary.body)).into_owned();
    assert!(
        their_index.contains("<name>hello-ghost</name>") && !their_index.contains("hello-broken"),
        "the foreign index lists only its own rows: {their_index}"
    );
    let my_repomd = call(
        my_port,
        "GET",
        &format!("/yum/{REPO}/repodata/repomd.xml"),
        None,
    );
    assert_eq!(my_repomd.status, 200);
    let my_href = href_of(&text_of(&my_repomd));
    let my_primary = call(my_port, "GET", &format!("/yum/{REPO}/{my_href}"), None);
    let my_index = String::from_utf8_lossy(&gunzip(&my_primary.body)).into_owned();
    assert!(
        my_index.contains("<name>hello-broken</name>") && !my_index.contains("hello-ghost"),
        "and so does mine: {my_index}"
    );
    // Each hub signs with its own identity: the served keys differ.
    let my_key = text_of(&call(my_port, "GET", &format!("/yum/{REPO}/key.asc"), None));
    let their_key = text_of(&call(
        their_port,
        "GET",
        &format!("/yum/{REPO}/key.asc"),
        None,
    ));
    assert_ne!(
        my_key, their_key,
        "two accounts, two ephemeral hub identities"
    );
    record_evidence(
        "foreign_and_broken_yum_state_is_never_valid",
        "cuenta ajena: el pool vecino no se sirve aqui (404), cada indice lista solo sus filas y cada clave es la de su hub; el aislamiento hosted es del catalogo, no del nombre",
    );

    // A garbage upload is not an rpm: 400, and it stays out of the graph.
    let garbage = call(
        my_port,
        "PUT",
        &format!("/yum/{REPO}/pool/garbage-1.0-1.x86_64.rpm"),
        Some(b"not an rpm at all"),
    );
    assert_eq!(
        garbage.status,
        400,
        "the header must parse: {}",
        text_of(&garbage)
    );

    // The pool path is dictated by the header: other paths conflict.
    let wrong = call(
        my_port,
        "PUT",
        &format!("/yum/{REPO}/pool/elsewhere-1.0.0-1.x86_64.rpm"),
        Some(&broken),
    );
    assert_eq!(wrong.status, 409, "the path must match the header");

    // Other bytes on a published path: the version is for ever. Same
    // bytes: the echo.
    let conflict_rpm = {
        let mut fields = rpm_fields("hello-broken", "1.0.0", "1");
        fields[3] = TestHeaderField {
            tag: TAG_SUMMARY,
            value: HeaderValue::Str("a different build of the same version"),
        };
        build_test_rpm(&fields)
    };
    let conflict = call(
        my_port,
        "PUT",
        &format!("/yum/{REPO}/{broken_pool}"),
        Some(&conflict_rpm),
    );
    assert_eq!(
        conflict.status,
        409,
        "a published version is immutable: {}",
        text_of(&conflict)
    );
    let echo = call(
        my_port,
        "PUT",
        &format!("/yum/{REPO}/{broken_pool}"),
        Some(&broken),
    );
    assert_eq!(echo.status, 200, "the same bytes echo: {}", text_of(&echo));
    record_evidence(
        "foreign_and_broken_yum_state_is_never_valid",
        "subida sin header rpm 400, ruta del pool que no dicta el header 409, bytes distintos en una ruta publicada 409 y mismos bytes eco 200 (la version es para siempre)",
    );

    // Routes the grammar does not speak: 404, never a silent yes.
    let nonsense = call(
        my_port,
        "GET",
        &format!("/yum/{REPO}/repodata/nope.xml"),
        None,
    );
    assert_eq!(nonsense.status, 404);
    let unhashed = call(my_port, "GET", &format!("/yum/{REPO}/index.html"), None);
    assert_eq!(unhashed.status, 404);
    let notgzip = call(
        my_port,
        "GET",
        &format!("/yum/{REPO}/repodata/{}-primary.xml.zst", "ab".repeat(32)),
        None,
    );
    assert_eq!(notgzip.status, 404, "only the gz grammar exists here");

    // The read side fails closed: with the record blob gone, the
    // metadata that would lie about it answers 404, not stale-valid.
    let before = call(
        my_port,
        "GET",
        &format!("/yum/{REPO}/repodata/repomd.xml"),
        None,
    );
    assert_eq!(before.status, 200);
    let href = href_of(&text_of(&before));
    let records = mine
        .store
        .assets_under(&alice, REPO, "pool/")
        .await
        .expect("the window lists");
    let record_asset = records
        .iter()
        .find(|asset| asset.asset_path.ends_with(".record"))
        .expect("the record row exists");
    let record_file = mine.content.the_path_of(&record_asset.digest);
    let record_bytes = std::fs::read(&record_file).expect("the record blob file");
    std::fs::remove_file(&record_file).expect("the record blob is gone");
    let repomd = call(
        my_port,
        "GET",
        &format!("/yum/{REPO}/repodata/repomd.xml"),
        None,
    );
    assert_eq!(
        repomd.status, 404,
        "a vanished record must not produce a stale-valid repomd"
    );
    let primary = call(my_port, "GET", &format!("/yum/{REPO}/{href}"), None);
    assert_eq!(primary.status, 404, "and the index cannot vouch for it");
    // The binary itself is still there: the package is invisible to dnf,
    // never corrupt.
    let pool_after = call(my_port, "GET", &format!("/yum/{REPO}/{broken_pool}"), None);
    assert_eq!(pool_after.status, 200, "the rpm bytes are untouched");
    std::fs::write(&record_file, &record_bytes).expect("the record blob is restored");
    let repaired = call(
        my_port,
        "GET",
        &format!("/yum/{REPO}/repodata/repomd.xml"),
        None,
    );
    assert_eq!(
        repaired.status, 200,
        "the metadata opens again with its record"
    );
    record_evidence(
        "foreign_and_broken_yum_state_is_never_valid",
        "blob del registro borrado del CAS: repomd.xml y primary responden 404 (nunca validos rancios) mientras el .rpm del pool sigue intacto, y la metadata se recupera al restaurar el blob",
    );

    // The write side fails closed: publishing bytes the CAS does not hold
    // is refused by the probe and leaves no catalog rows.
    let phantom = a_rpm("hello-phantom", "0.1.0", "1");
    let metadata = yum::parse_rpm(&phantom).expect("the header parses");
    let phantom_pool = yum::pool_path_for(&metadata).unwrap();
    let phantom_digest = ContentDigest::of_bytes(&phantom);
    let refused = mine
        .store
        .publish(
            &alice,
            Publication {
                repository: REPO.to_string(),
                asset_path: phantom_pool.clone(),
                coordinates: PackageCoordinates {
                    package_name: metadata.name.clone(),
                    package_version: format!("{}-{}", metadata.version, metadata.release),
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
        .assets_under(&alice, REPO, "pool/hello-phantom")
        .await
        .expect("the census window answers");
    assert!(rows.is_empty(), "the refusal left no catalog rows behind");
    let census = fresh_client().await.expect("the census client");
    let refusals = the_events_of(&census, &alice, &format!("{REPO}/pool/hello-phantom")).await;
    assert_eq!(
        refusals, 1,
        "the refusal is an audited fact, not a silent no"
    );
    record_evidence(
        "foreign_and_broken_yum_state_is_never_valid",
        "publicacion sin el blob en el CAS rechazada por la sonda (sin filas en el catalogo) y auditada como hecho fallido en el outbox por tenant",
    );

    // The real dnf refuses metadata whose signature does not open with
    // the key it pins: host first, container as the fallback oracle.
    let stranger =
        hodei_artifacts_api::openpgp::MetadataSigner::generate("Stranger <x@example.local>")
            .expect("a second key");
    let work = next("stage-stranger");
    std::fs::create_dir_all(&work).expect("the scratch dir");
    std::fs::write(
        work.join("stranger.asc"),
        stranger.key_asc().expect("armor"),
    )
    .expect("the stranger key");
    match host_dnf() {
        Some(binary) => {
            let repodir = work.join("iroot/etc/yum.repos.d");
            std::fs::create_dir_all(&repodir).expect("the repodir");
            std::fs::write(
                repodir.join("hodei.repo"),
                host_repo_file(
                    my_port,
                    &format!("file://{}", work.join("stranger.asc").display()),
                ),
            )
            .expect("the repo file");
            let refused = host_dnf_run(binary, &work, &["makecache", "--refresh"]);
            let stdout = String::from_utf8_lossy(&refused.stdout);
            let stderr = String::from_utf8_lossy(&refused.stderr);
            assert!(
                !refused.status.success()
                    && (stdout.contains("signature") || stderr.contains("signature")),
                "{binary} must refuse repodata signed by another key: {stdout}\n{stderr}"
            );
            record_evidence(
                "foreign_and_broken_yum_state_is_never_valid",
                &format!(
                    "{binary} real del host con repo_gpgcheck=1 y la clave de un tercero: makecache se niega (la firma del repomd no abre con la clave fijada)"
                ),
            );
        }
        None => {
            let script = format!(
                concat!(
                    "set -eu\n",
                    "cat > /etc/yum.repos.d/hodei.repo <<'EOF'\n",
                    "[hodei]\n",
                    "name=Hodei over dgraph\n",
                    "baseurl=http://alice:alice@127.0.0.1:{port}/yum/{repo}/\n",
                    "enabled=1\n",
                    "gpgcheck=0\n",
                    "repo_gpgcheck=1\n",
                    "gpgkey=file:///work/stranger.asc\n",
                    "EOF\n",
                    "if dnf -y --disablerepo='*' --enablerepo=hodei --refresh makecache \\",
                    " >/tmp/makecache.log 2>&1; then echo ACCEPTED; exit 1; fi\n",
                    "cat /tmp/makecache.log; echo refused\n",
                ),
                port = my_port,
                repo = REPO,
            );
            let refused = container(&work, &script);
            let stdout = String::from_utf8_lossy(&refused.stdout);
            assert!(
                refused.status.success()
                    && stdout.contains("refused")
                    && !stdout.contains("ACCEPTED"),
                "dnf must reject repodata signed by the stranger key: {stdout}"
            );
            record_evidence(
                "foreign_and_broken_yum_state_is_never_valid",
                "sin dnf del host: el contenedor fedora fijado con repo_gpgcheck=1 y la clave de un tercero niega el makecache (el contrato hsp122 aprieta aqui)",
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Scoped, keyed and indexed: the yum reads root on exact indexes, and the
// census never crosses tenants.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_yum_queries_are_scoped_keyed_and_indexed() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let mine = account("scope");
    let theirs = account("noise");
    let store = Arc::new(open_store(client.clone()).await);
    store
        .create_repository(&mine, an_rpm_descriptor(&mine, REPO))
        .await
        .expect("the repository is created");
    store
        .create_repository(&theirs, an_rpm_descriptor(&theirs, REPO))
        .await
        .expect("the foreign repository is created");

    // My pool: six packages, the tilde/plus/epoch one included. The
    // distractor's: the same repository key, the same names, other bytes.
    let mut mine_digests = Vec::new();
    for index in 1..=5u32 {
        let name = format!("pkg-{index:02}");
        let leaked: &'static str = Box::leak(name.clone().into_boxed_str());
        let (pool, rpm, record) =
            publish_rpm_pair(store.as_ref(), &mine, REPO, &a_rpm(leaked, "1.0", "1")).await;
        mine_digests.push((pool, rpm.value, record.value));
        let mut fields = rpm_fields("distractor", "2.0", "2");
        fields[0] = TestHeaderField {
            tag: TAG_NAME,
            value: HeaderValue::Str(leaked),
        };
        publish_rpm_pair(store.as_ref(), &theirs, REPO, &build_test_rpm(&fields)).await;
    }
    let (tilde_pool, tilde_rpm, tilde_record) =
        publish_rpm_pair(store.as_ref(), &mine, REPO, &a_tilde_rpm()).await;
    assert_eq!(
        tilde_pool, TILDE_POOL,
        "the grammar's path carries the epoch, tilde and plus"
    );
    assert!(
        tilde_record.value.len() == 64 && tilde_rpm.value.len() == 64,
        "both digests are sha256 hex"
    );

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
    for (pool, rpm, record) in &mine_digests {
        let row = found
            .iter()
            .find(|asset| &asset.asset_path == pool)
            .expect("my binary is in the graph");
        assert_eq!(&row.digest.value, rpm);
        let record_row = found
            .iter()
            .find(|asset| asset.asset_path == yum::record_path_for(pool))
            .expect("my record is in the graph");
        assert_eq!(&record_row.digest.value, record);
    }
    let tilde_row = found
        .iter()
        .find(|asset| asset.asset_path == TILDE_POOL)
        .expect("the guard-hostile path is in the graph");
    assert_eq!(tilde_row.digest.value, tilde_rpm.value);
    let foreign = store
        .assets_under(&theirs, REPO, "pool/")
        .await
        .expect("the foreign window reads");
    assert_eq!(
        foreign.len(),
        10,
        "five foreign binaries and five foreign records, never mine"
    );
    assert!(
        foreign
            .iter()
            .all(|asset| asset.digest.value != tilde_rpm.value),
        "the foreign window cannot borrow my bytes"
    );

    // The outbox census per tenant: my events mention my repository, the
    // neighbor's mention their own, and neither count is a global scan.
    let mine_events = the_events_of(&client, &mine, REPO).await;
    let theirs_events = the_events_of(&client, &theirs, REPO).await;
    assert!(
        mine_events >= 12,
        "twelve publish facts for this tenant: {mine_events}"
    );
    assert!(
        theirs_events >= 10,
        "ten publish facts for the neighbor: {theirs_events}"
    );
    let only_mine = the_events_of(&client, &mine, "hello-ghost").await;
    assert_eq!(only_mine, 0, "a foreign needle never lands on my tenant");
    record_evidence(
        "the_yum_queries_are_scoped_keyed_and_indexed",
        "censo del outbox por tenant (eq(hodei.catalog.outbox.tenant)): hechos separados por cuenta (>=12/10) y aguja ajena 0, nunca el censo global",
    );

    // The plan probe: every predicate the yum reads roots on carries the
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
        "the_yum_queries_are_scoped_keyed_and_indexed",
        "probe de plan: asset.key/outbox.key con @index(exact) @upsert y account/repository/asset_path/package_* con @index(exact); las raices del vertical yum (eq de repository+ventana de asset_path, eq de tenant en el censo) van por indice, sin scan global",
    );
}

// ---------------------------------------------------------------------------
// Contract reads: the published yum state answers what the grammar and the
// publication say (HSP-314 F4: this used to compare against a second
// reference engine; every expected value now derives from the published
// bytes and the yum grammar).
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_yum_reads_match_the_contract_for_the_published_state() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let mine = account("parity");
    let graph = open_store(client.clone()).await;
    graph
        .create_repository(&mine, an_rpm_descriptor(&mine, REPO))
        .await
        .expect("the dgraph repository is created");

    // Three packages (one with the guard-hostile path).
    let rpms = [
        a_rpm("parity-one", "1.0", "1"),
        a_rpm("parity-two", "2.0", "1"),
        a_tilde_rpm(),
    ];
    let mut expected_rows: Vec<(String, String)> = Vec::new();
    let mut records: Vec<(String, String)> = Vec::new();
    for rpm in &rpms {
        let metadata = yum::parse_rpm(rpm).expect("the header parses");
        let expected = yum::pool_path_for(&metadata).expect("the pool path");
        let (graph_pool, graph_rpm, graph_record) =
            publish_rpm_pair(&graph, &mine, REPO, rpm).await;
        assert_eq!(graph_pool, expected);
        // The published digests are those of the bytes and of the record
        // the grammar assembles from them.
        let rpm_digest = ContentDigest::of_bytes(rpm);
        let record_digest = ContentDigest::of_bytes(
            yum::package_record(&metadata, &expected, rpm.len() as u64, &rpm_digest.value)
                .as_bytes(),
        );
        assert_eq!(graph_rpm, rpm_digest);
        assert_eq!(graph_record, record_digest);
        expected_rows.push((expected.clone(), rpm_digest.value.clone()));
        expected_rows.push((yum::record_path_for(&expected), record_digest.value.clone()));
        records.push((yum::record_path_for(&expected), record_digest.value.clone()));
    }
    expected_rows.sort();
    records.sort();

    // The immutable path: the same identity, other bytes, conflicts.
    {
        let mut fields = rpm_fields("parity-one", "1.0", "1");
        fields[3] = TestHeaderField {
            tag: TAG_SUMMARY,
            value: HeaderValue::Str("other bytes for a published version"),
        };
        let conflicting = build_test_rpm(&fields);
        let metadata = yum::parse_rpm(&conflicting).expect("the header parses");
        let digest = ContentDigest::of_bytes(&conflicting);
        let conflict = Publication {
            repository: REPO.to_string(),
            asset_path: yum::pool_path_for(&metadata).unwrap(),
            coordinates: PackageCoordinates {
                package_name: metadata.name.clone(),
                package_version: format!("{}-{}", metadata.version, metadata.release),
            },
            digest,
            size_bytes: conflicting.len() as u64,
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

    // The listing: every published path, ascending, with its digest, and
    // the index assembled from its `.record` rows is the published records.
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
    let mut expected_index = String::new();
    for (path, digest) in &records {
        expected_index.push_str(&format!("{path}\n{digest}\n"));
    }
    assert_eq!(
        index_of(&mine_records),
        expected_index,
        "the listing the index assembles from is the published records, in order"
    );
    assert!(
        index_of(&mine_records).contains(TILDE_RECORD),
        "the epoch/tilde/plus record is part of the published state"
    );
    record_evidence(
        "the_yum_reads_match_the_contract_for_the_published_state",
        "lecturas yum segun el contrato: las publicaciones (con la ruta pool/hello-tilde-1:1.0.0~rc1+dfsg-1.x86_64.rpm) dejan las filas, los digests y el listado ordenado que dicen los bytes y la gramatica; otro contenido en la ruta publicada es conflicto y la inmutabilidad se respeta",
    );
}
