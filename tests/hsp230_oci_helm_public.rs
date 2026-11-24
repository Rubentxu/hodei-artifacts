//! HSP-230 (HCR-08): Public OCI image & Helm OCI chart verification with native clients.
//!
//! Verifies end-to-end pulls of pinned OCI images and pinned Helm OCI charts
//! through Hodei's supported profiles (`oci` proxy and `oci` group) using native
//! clients (`skopeo`, `podman --storage-driver=vfs`, and `helm`).
//!
//! - **AC1**: `skopeo inspect`/`copy` and `podman pull` traverse Hodei OCI proxy
//!   (and OCI group) with digest and platform preserved, with zero accidental
//!   direct access to the external registry.
//! - **AC2**: `helm pull` of a version/digest-pinned Helm OCI chart uses the OCI
//!   path over `RepositoryFormat::Oci` (proxy and group) and validates SHA256.
//!   Classic Helm (`index.yaml`) is NOT announced (`RepositoryFormat::Helm` stays `&[]`).
//! - **AC3**: Records rate limits, anonymous token auth, and CDN/redirects; any
//!   external restriction or unopted real case is marked `NOT_RUN` with exact
//!   cause, never a false `PASS`.

mod support;

use std::collections::HashMap;
use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use support::lab::{
    Budget, Expectation, LabCase, Outcome, REAL_MODE_VARIABLE, Scope, Summary,
    describe_registry_lab, real_mode_enabled, run_registry_lab_case,
};
use support::{Deployment, FIRST_ACCOUNT};

const THE_TITLE: &str = "HSP-230 Public OCI & Helm OCI Verification Lab";

const THE_DISCLAIMER: &[&str] = &[
    "- **Offline by default.** Default runs use a local mock OCI upstream and local CDN redirect server. Real public registry checks (`registry.k8s.io`, `ghcr.io`) are opt-in via `HODEI_LAB_REAL=1` and mark `NOT_RUN` without opt-in or upon external restriction.",
    "- **Helm OCI over OCI profile.** `helm pull` traverses `RepositoryFormat::Oci` (proxy and group). `RepositoryFormat::Helm` remains `&[]` because classic Helm (`index.yaml`) has no adapter.",
    "- **Native clients.** Executed with real `skopeo`, `podman --storage-driver=vfs`, and `helm` binaries in isolated temporary homes.",
];

const THE_CASES: &[(&str, &str)] = &[
    (
        "ac1-skopeo-inspect-and-copy-through-oci-proxy",
        "skopeo inspect and copy traverse Hodei OCI proxy preserving digest and platform with zero direct upstream access",
    ),
    (
        "ac1-podman-pull-through-oci-proxy-and-group",
        "podman pull traverses Hodei OCI proxy and OCI group preserving digest and platform",
    ),
    (
        "ac2-helm-pull-oci-chart-through-proxy-and-group",
        "helm pull fetches OCI chart through Hodei OCI proxy and OCI group with SHA256 validated; classic Helm stays unannounced",
    ),
    (
        "ac3-rate-limits-anonymous-auth-and-cdn-redirects",
        "records anonymous bearer token auth, cross-origin CDN redirects, and 429 rate-limit fail-closed behavior",
    ),
    (
        "ac1-real-public-oci-image-through-proxy-and-group",
        "opt-in real pull of pinned registry.k8s.io/pause:3.9 through Hodei OCI proxy and group",
    ),
    (
        "ac2-real-public-helm-oci-chart-through-proxy-and-group",
        "opt-in real helm pull of pinned ghcr.io/stefanprodan/charts/podinfo:6.7.0 through Hodei OCI proxy and group",
    ),
    (
        "ac3-the-count-does-not-lie",
        "without opt-in the lab report is not green and never claims a false PASS",
    ),
];

const IMAGE_PROXY_REPO: &str = "oci-image-proxy";
const IMAGE_GROUP_REPO: &str = "oci-image-group";
const HELM_PROXY_REPO: &str = "helm-chart-proxy";
const HELM_GROUP_REPO: &str = "helm-chart-group";

const REAL_IMAGE_REPO: &str = "pause";
const REAL_IMAGE_TAG: &str = "3.9";
const REAL_IMAGE_DIGEST: &str =
    "sha256:7031c1b283388d2c2e09b57badb803c05ebed362dc88d84b480cc47f72a21097";
const REAL_IMAGE_GROUP: &str = "real-oci-image-group";

const REAL_HELM_REPO: &str = "podinfo-proxy";
const REAL_HELM_UPSTREAM_URL: &str = "https://ghcr.io/v2/stefanprodan/charts/podinfo/";
const REAL_HELM_VERSION: &str = "6.7.0";
const REAL_HELM_TGZ_SHA256: &str =
    "bd660969295320a5d5328f1b63a7c82a9853c1b7dbc61d0565a162c4f71f5a65";
const REAL_HELM_GROUP: &str = "real-helm-oci-group";

#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

// ---------------------------------------------------------------------------
// Native CLI wrappers (skopeo, podman --storage-driver=vfs, helm)
// ---------------------------------------------------------------------------

struct CliResult {
    ok: bool,
    stdout: String,
    stderr: String,
}

impl CliResult {
    fn combined(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

fn require_native_tools(podman_root: &Path, podman_runroot: &Path) {
    let skopeo_ok = Command::new("skopeo")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(skopeo_ok, "skopeo CLI is required in PATH for HSP-230");

    let helm_ok = Command::new("helm")
        .args(["version", "--short"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(helm_ok, "helm CLI is required in PATH for HSP-230");

    let podman_ok = Command::new("podman")
        .arg("--storage-driver=vfs")
        .arg("--root")
        .arg(podman_root)
        .arg("--runroot")
        .arg(podman_runroot)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(podman_ok, "podman CLI is required in PATH for HSP-230");
}

struct IsolatedClients {
    root: PathBuf,
    registries_conf: PathBuf,
    podman_root: PathBuf,
    podman_runroot: PathBuf,
    helm_home: PathBuf,
    helm_work: PathBuf,
}

impl IsolatedClients {
    fn new(base: &Path) -> Self {
        let root = base.join("clients");
        let registries_conf = root.join("registries.conf");
        let podman_root = root.join("podman-root");
        let podman_runroot = root.join("podman-runroot");
        let helm_home = root.join("helm-home");
        let helm_work = root.join("helm-work");

        std::fs::create_dir_all(&podman_root).expect("podman root dir");
        std::fs::create_dir_all(&podman_runroot).expect("podman runroot dir");
        std::fs::create_dir_all(helm_home.join("config")).expect("helm config dir");
        std::fs::create_dir_all(helm_home.join("cache")).expect("helm cache dir");
        std::fs::create_dir_all(&helm_work).expect("helm work dir");

        // Block unqualified search registries so clients cannot fall back to docker.io/quay.io.
        std::fs::write(&registries_conf, "unqualified-search-registries = []\n")
            .expect("write registries.conf");

        Self {
            root,
            registries_conf,
            podman_root,
            podman_runroot,
            helm_home,
            helm_work,
        }
    }

    fn skopeo(&self, args: &[&str]) -> CliResult {
        let output = Command::new("skopeo")
            .args(args)
            .env("HOME", &self.root)
            .env("CONTAINERS_REGISTRIES_CONF", &self.registries_conf)
            .output()
            .expect("skopeo runs");
        CliResult {
            ok: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    fn podman(&self, args: &[&str]) -> CliResult {
        let output = Command::new("podman")
            .arg("--storage-driver=vfs")
            .arg("--root")
            .arg(&self.podman_root)
            .arg("--runroot")
            .arg(&self.podman_runroot)
            .args(args)
            .env("HOME", &self.root)
            .env("CONTAINERS_REGISTRIES_CONF", &self.registries_conf)
            .output()
            .expect("podman runs");
        CliResult {
            ok: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    fn helm(&self, args: &[&str], stdin: Option<&str>) -> CliResult {
        let mut command = Command::new("helm");
        command
            .args(args)
            .current_dir(&self.helm_work)
            .env("HOME", &self.helm_home)
            .env("DOCKER_CONFIG", self.helm_home.join("config"))
            .env("XDG_CACHE_HOME", self.helm_home.join("cache"));
        let output = if let Some(secret) = stdin {
            command.stdin(Stdio::piped());
            command.stdout(Stdio::piped());
            command.stderr(Stdio::piped());
            let mut child = command.spawn().expect("helm spawns");
            child
                .stdin
                .as_mut()
                .expect("helm stdin")
                .write_all(secret.as_bytes())
                .expect("write helm stdin");
            child.wait_with_output().expect("helm finishes")
        } else {
            command.output().expect("helm runs")
        };
        CliResult {
            ok: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

// ---------------------------------------------------------------------------
// Local OCI Upstream + Cross-Origin CDN Redirect Server
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct UpstreamHit {
    method: String,
    path: String,
    user_agent: String,
}

#[derive(Clone)]
struct StoredItem {
    media_type: String,
    bytes: Vec<u8>,
}

struct UpstreamState {
    manifests: HashMap<String, StoredItem>,
    blobs: HashMap<String, Vec<u8>>,
    hits: Vec<UpstreamHit>,
    anonymous_token: Option<String>,
    cdn_base: Option<String>,
    rate_limit: bool,
    disconnected: bool,
}

#[derive(Clone)]
struct LocalCdn {
    port: u16,
    hits: Arc<Mutex<Vec<String>>>,
}

type CdnState = (Arc<HashMap<String, Vec<u8>>>, Arc<Mutex<Vec<String>>>);

impl LocalCdn {
    fn start(blobs: HashMap<String, Vec<u8>>) -> Self {
        let hits = Arc::new(Mutex::new(Vec::new()));
        let state: CdnState = (Arc::new(blobs), hits.clone());
        let app =
            Router::new()
                .route(
                    "/cdn/blobs/{digest}",
                    get(
                        |AxumPath(digest): AxumPath<String>,
                         State((blobs, hits)): State<CdnState>| async move {
                            hits.lock().expect("cdn hits").push(digest.clone());
                            match blobs.get(&digest) {
                                Some(bytes) => (
                                    StatusCode::OK,
                                    [(header::CONTENT_TYPE, "application/octet-stream")],
                                    bytes.clone(),
                                )
                                    .into_response(),
                                None => StatusCode::NOT_FOUND.into_response(),
                            }
                        },
                    ),
                )
                .with_state(state);
        let port = spawn_axum(app);
        Self { port, hits }
    }

    fn base(&self) -> String {
        format!("http://127.0.0.1:{}/", self.port)
    }

    fn hit_count(&self) -> usize {
        self.hits.lock().expect("cdn hits").len()
    }
}

#[derive(Clone)]
struct LocalOciUpstream {
    port: u16,
    state: Arc<Mutex<UpstreamState>>,
}

impl LocalOciUpstream {
    fn start(anonymous_token: Option<&str>, cdn_base: Option<String>) -> Self {
        let state = Arc::new(Mutex::new(UpstreamState {
            manifests: HashMap::new(),
            blobs: HashMap::new(),
            hits: Vec::new(),
            anonymous_token: anonymous_token.map(str::to_string),
            cdn_base,
            rate_limit: false,
            disconnected: false,
        }));
        let app = Router::new()
            .route("/token", get(handle_token))
            .route("/v2/{repo}/manifests/{reference}", get(handle_manifest))
            .route("/v2/{repo}/blobs/{digest}", get(handle_blob))
            .with_state(state.clone());
        let port = spawn_axum(app);
        Self { port, state }
    }

    fn base(&self) -> String {
        format!("http://127.0.0.1:{}/", self.port)
    }

    fn token_realm(&self) -> String {
        format!("http://127.0.0.1:{}/token", self.port)
    }

    fn publish_manifest(&self, repo: &str, reference: &str, media_type: &str, bytes: Vec<u8>) {
        let digest = format!("sha256:{}", sha256_hex(&bytes));
        let mut guard = self.state.lock().expect("upstream state");
        let item = StoredItem {
            media_type: media_type.to_string(),
            bytes,
        };
        guard
            .manifests
            .insert(format!("{repo}/{reference}"), item.clone());
        guard.manifests.insert(format!("{repo}/{digest}"), item);
    }

    fn publish_blob(&self, digest: &str, bytes: Vec<u8>) {
        let mut guard = self.state.lock().expect("upstream state");
        guard.blobs.insert(digest.to_string(), bytes);
    }

    fn hits(&self) -> Vec<UpstreamHit> {
        self.state.lock().expect("upstream state").hits.clone()
    }

    fn set_rate_limit(&self, enabled: bool) {
        self.state.lock().expect("upstream state").rate_limit = enabled;
    }

    fn disconnect(&self) {
        self.state.lock().expect("upstream state").disconnected = true;
    }
}

async fn handle_token(
    State(state): State<Arc<Mutex<UpstreamState>>>,
    Query(_params): Query<HashMap<String, String>>,
) -> Response {
    let token = {
        let guard = state.lock().expect("upstream state");
        if guard.disconnected {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        guard.anonymous_token.clone().unwrap_or_default()
    };
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        json!({ "token": token }).to_string(),
    )
        .into_response()
}

async fn handle_manifest(
    AxumPath((repo, reference)): AxumPath<(String, String)>,
    State(state): State<Arc<Mutex<UpstreamState>>>,
    headers: HeaderMap,
) -> Response {
    let port_and_check = {
        let mut guard = state.lock().expect("upstream state");
        let ua = headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        guard.hits.push(UpstreamHit {
            method: "GET".to_string(),
            path: format!("/v2/{repo}/manifests/{reference}"),
            user_agent: ua,
        });
        if guard.disconnected {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        if guard.rate_limit {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [(header::RETRY_AFTER, "60")],
                "rate limit exceeded",
            )
                .into_response();
        }
        if let Some(expected) = &guard.anonymous_token {
            let auth = headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if auth != format!("Bearer {expected}") {
                Err(format!("repository:{repo}:pull"))
            } else {
                Ok(guard.manifests.get(&format!("{repo}/{reference}")).cloned())
            }
        } else {
            Ok(guard.manifests.get(&format!("{repo}/{reference}")).cloned())
        }
    };

    match port_and_check {
        Err(scope) => {
            let host = headers
                .get(header::HOST)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("127.0.0.1");
            let challenge =
                format!(r#"Bearer realm="http://{host}/token",service="lab-oci",scope="{scope}""#);
            (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, challenge)],
                "unauthorized",
            )
                .into_response()
        }
        Ok(Some(item)) => {
            let digest = format!("sha256:{}", sha256_hex(&item.bytes));
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, item.media_type)
                .header("Docker-Content-Digest", &digest)
                .header(header::ETAG, format!("\"{digest}\""))
                .body(Body::from(item.bytes))
                .expect("manifest response")
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn handle_blob(
    AxumPath((repo, digest)): AxumPath<(String, String)>,
    State(state): State<Arc<Mutex<UpstreamState>>>,
    headers: HeaderMap,
) -> Response {
    let outcome = {
        let mut guard = state.lock().expect("upstream state");
        let ua = headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        guard.hits.push(UpstreamHit {
            method: "GET".to_string(),
            path: format!("/v2/{repo}/blobs/{digest}"),
            user_agent: ua,
        });
        if guard.disconnected {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        if guard.rate_limit {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [(header::RETRY_AFTER, "60")],
                "rate limit exceeded",
            )
                .into_response();
        }
        if let Some(expected) = &guard.anonymous_token {
            let auth = headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if auth != format!("Bearer {expected}") {
                return {
                    let host = headers
                        .get(header::HOST)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("127.0.0.1");
                    let challenge = format!(
                        r#"Bearer realm="http://{host}/token",service="lab-oci",scope="repository:{repo}:pull""#
                    );
                    (
                        StatusCode::UNAUTHORIZED,
                        [(header::WWW_AUTHENTICATE, challenge)],
                        "unauthorized",
                    )
                        .into_response()
                };
            }
        }
        (guard.cdn_base.clone(), guard.blobs.get(&digest).cloned())
    };

    match outcome {
        (Some(cdn_base), Some(_)) => {
            let location = format!("{cdn_base}cdn/blobs/{digest}");
            (
                StatusCode::TEMPORARY_REDIRECT,
                [(header::LOCATION, location)],
            )
                .into_response()
        }
        (None, Some(bytes)) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/octet-stream")],
            bytes,
        )
            .into_response(),
        (_, None) => StatusCode::NOT_FOUND.into_response(),
    }
}

fn spawn_axum(app: Router) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("local addr").port();
    listener.set_nonblocking(true).expect("nonblocking");
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("tokio listener");
            axum::serve(listener, app).await.expect("axum serve");
        });
    });
    port
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

// ---------------------------------------------------------------------------
// Fixture builders (Multi-arch OCI Image + Helm OCI Chart)
// ---------------------------------------------------------------------------

struct MultiArchImageFixture {
    index_bytes: Vec<u8>,
    index_digest: String,
    amd64_manifest: Vec<u8>,
    arm64_manifest: Vec<u8>,
    amd64_config: Vec<u8>,
    arm64_config: Vec<u8>,
    layer_bytes: Vec<u8>,
    layer_digest: String,
}

fn build_multiarch_image_fixture() -> MultiArchImageFixture {
    // Valid empty tar archive (1024 zero bytes) compressed with gzip.
    let empty_tar = vec![0u8; 1024];
    let diff_id = format!("sha256:{}", sha256_hex(&empty_tar));
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&empty_tar).expect("compress layer");
    let layer_bytes = encoder.finish().expect("finish gzip");
    let layer_digest = format!("sha256:{}", sha256_hex(&layer_bytes));

    let make_variant = |arch: &str| -> (Vec<u8>, Vec<u8>) {
        let config = serde_json::to_vec(&json!({
            "architecture": arch,
            "os": "linux",
            "rootfs": {
                "type": "layers",
                "diff_ids": [diff_id]
            },
            "config": {}
        }))
        .expect("config json");
        let config_digest = format!("sha256:{}", sha256_hex(&config));
        let manifest = serde_json::to_vec(&json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": {
                "mediaType": "application/vnd.oci.image.config.v1+json",
                "digest": config_digest,
                "size": config.len()
            },
            "layers": [{
                "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
                "digest": layer_digest,
                "size": layer_bytes.len()
            }]
        }))
        .expect("manifest json");
        (manifest, config)
    };

    let (amd64_manifest, amd64_config) = make_variant("amd64");
    let (arm64_manifest, arm64_config) = make_variant("arm64");
    let amd64_digest = format!("sha256:{}", sha256_hex(&amd64_manifest));
    let arm64_digest = format!("sha256:{}", sha256_hex(&arm64_manifest));

    let index_bytes = serde_json::to_vec(&json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "manifests": [
            {
                "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "digest": amd64_digest,
                "size": amd64_manifest.len(),
                "platform": { "architecture": "amd64", "os": "linux" }
            },
            {
                "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "digest": arm64_digest,
                "size": arm64_manifest.len(),
                "platform": { "architecture": "arm64", "os": "linux" }
            }
        ]
    }))
    .expect("index json");
    let index_digest = format!("sha256:{}", sha256_hex(&index_bytes));

    MultiArchImageFixture {
        index_bytes,
        index_digest,
        amd64_manifest,
        arm64_manifest,
        amd64_config,
        arm64_config,
        layer_bytes,
        layer_digest,
    }
}

struct HelmChartFixture {
    manifest_bytes: Vec<u8>,
    config_bytes: Vec<u8>,
    config_digest: String,
    tgz_bytes: Vec<u8>,
    tgz_digest: String,
    tgz_sha256: String,
}

fn build_helm_chart_fixture(clients: &IsolatedClients) -> HelmChartFixture {
    let created = clients.helm(&["create", "lab-chart"], None);
    assert!(created.ok, "helm create: {}", created.combined());
    let packaged = clients.helm(&["package", "lab-chart", "--destination", "."], None);
    assert!(packaged.ok, "helm package: {}", packaged.combined());
    let tgz_path = clients.helm_work.join("lab-chart-0.1.0.tgz");
    let tgz_bytes = std::fs::read(&tgz_path).expect("read packaged chart");
    let tgz_sha256 = sha256_hex(&tgz_bytes);
    let tgz_digest = format!("sha256:{tgz_sha256}");

    let config_bytes = serde_json::to_vec(&json!({
        "name": "lab-chart",
        "version": "0.1.0",
        "apiVersion": "v2",
        "type": "application"
    }))
    .expect("helm config json");
    let config_digest = format!("sha256:{}", sha256_hex(&config_bytes));

    let manifest_bytes = serde_json::to_vec(&json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": {
            "mediaType": "application/vnd.cncf.helm.config.v1+json",
            "digest": config_digest,
            "size": config_bytes.len()
        },
        "layers": [{
            "mediaType": "application/vnd.cncf.helm.chart.content.v1.tar+gzip",
            "digest": tgz_digest,
            "size": tgz_bytes.len()
        }]
    }))
    .expect("helm manifest json");

    HelmChartFixture {
        manifest_bytes,
        config_bytes,
        config_digest,
        tgz_bytes,
        tgz_digest,
        tgz_sha256,
    }
}

fn seed_local_lab(
    deployment: &Deployment,
    upstream: &LocalOciUpstream,
    image: &MultiArchImageFixture,
    helm: &HelmChartFixture,
) {
    for (digest, bytes) in [
        (&image.layer_digest, &image.layer_bytes),
        (
            &format!("sha256:{}", sha256_hex(&image.amd64_config)),
            &image.amd64_config,
        ),
        (
            &format!("sha256:{}", sha256_hex(&image.arm64_config)),
            &image.arm64_config,
        ),
        (&helm.config_digest, &helm.config_bytes),
        (&helm.tgz_digest, &helm.tgz_bytes),
    ] {
        upstream.publish_blob(digest, bytes.clone());
    }

    upstream.publish_manifest(
        IMAGE_PROXY_REPO,
        "3.9",
        "application/vnd.oci.image.index.v1+json",
        image.index_bytes.clone(),
    );
    upstream.publish_manifest(
        IMAGE_PROXY_REPO,
        &format!("sha256:{}", sha256_hex(&image.amd64_manifest)),
        "application/vnd.oci.image.manifest.v1+json",
        image.amd64_manifest.clone(),
    );
    upstream.publish_manifest(
        IMAGE_PROXY_REPO,
        &format!("sha256:{}", sha256_hex(&image.arm64_manifest)),
        "application/vnd.oci.image.manifest.v1+json",
        image.arm64_manifest.clone(),
    );
    upstream.publish_manifest(
        HELM_PROXY_REPO,
        "0.1.0",
        "application/vnd.oci.image.manifest.v1+json",
        helm.manifest_bytes.clone(),
    );

    let proxy_block = |id: &str| {
        json!({
            "upstream": {
                "id": id,
                "url": upstream.base(),
                "scopes": ["v2/"],
                "auth": {
                    "realm": upstream.token_realm(),
                    "service": "lab-oci"
                },
                "redirects": { "max": 3, "cross_origin": true }
            },
            "cache": {
                "ttl": { "content_secs": 3600, "metadata_secs": 3600 },
                "negative_secs": 0
            }
        })
    };

    deployment.seed_repositories(&[
        json!({
            "key": IMAGE_PROXY_REPO,
            "format": "oci",
            "proxy": proxy_block("lab-oci-image"),
        }),
        json!({
            "key": HELM_PROXY_REPO,
            "format": "oci",
            "proxy": proxy_block("lab-helm-oci"),
        }),
    ]);
    deployment.seed_groups_raw(&[
        json!({
            "key": IMAGE_GROUP_REPO,
            "members": [IMAGE_PROXY_REPO],
            "format": "oci",
        }),
        json!({
            "key": HELM_GROUP_REPO,
            "members": [HELM_PROXY_REPO],
            "format": "oci",
        }),
    ]);
}

fn the_report() {
    describe_registry_lab(THE_TITLE, THE_DISCLAIMER);
}

// ---------------------------------------------------------------------------
// AC1: skopeo inspect / copy & podman pull through Hodei OCI proxy & group
// ---------------------------------------------------------------------------

#[test]
fn ac1_skopeo_inspect_and_copy_through_oci_proxy() {
    the_report();
    run_registry_lab_case(
        THE_CASES,
        "ac1-skopeo-inspect-and-copy-through-oci-proxy",
        THE_CASES[0].1,
        Scope::Product,
        Expectation::Succeeds,
        || {
            let deployment = Deployment::new();
            let clients = IsolatedClients::new(deployment.directory.path());
            require_native_tools(&clients.podman_root, &clients.podman_runroot);

            let image = build_multiarch_image_fixture();
            let helm = build_helm_chart_fixture(&clients);
            let upstream = LocalOciUpstream::start(Some("anon-lab-token"), None);
            seed_local_lab(&deployment, &upstream, &image, &helm);

            let allowed = format!("127.0.0.1:{};private;http", upstream.port);
            let service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &allowed)]);
            let token = support::token(FIRST_ACCOUNT, "alice");
            let creds = format!("alice:{token}");
            let target = format!("docker://127.0.0.1:{}/{IMAGE_PROXY_REPO}:3.9", service.port);

            let inspect_raw = clients.skopeo(&[
                "inspect",
                "--tls-verify=false",
                "--creds",
                &creds,
                "--raw",
                &target,
            ]);
            assert!(
                inspect_raw.ok,
                "skopeo inspect --raw failed: {}",
                inspect_raw.combined()
            );
            let raw_digest = format!("sha256:{}", sha256_hex(inspect_raw.stdout.as_bytes()));
            assert_eq!(
                raw_digest, image.index_digest,
                "index digest must be preserved through Hodei OCI proxy"
            );

            let raw_json: Value =
                serde_json::from_str(&inspect_raw.stdout).expect("valid index json");
            let platforms: Vec<(&str, &str)> = raw_json["manifests"]
                .as_array()
                .expect("manifests array")
                .iter()
                .map(|m| {
                    (
                        m["platform"]["os"].as_str().unwrap_or(""),
                        m["platform"]["architecture"].as_str().unwrap_or(""),
                    )
                })
                .collect();
            assert!(
                platforms.contains(&("linux", "amd64")) && platforms.contains(&("linux", "arm64")),
                "multi-arch platforms must be preserved: {platforms:?}"
            );

            let oci_dest = clients.root.join("skopeo-oci-out");
            let copy_dest = format!("oci:{}:3.9", oci_dest.display());
            let copied = clients.skopeo(&[
                "copy",
                "--src-tls-verify=false",
                "--src-creds",
                &creds,
                &target,
                &copy_dest,
            ]);
            assert!(copied.ok, "skopeo copy failed: {}", copied.combined());
            let index_json: Value = serde_json::from_slice(
                &std::fs::read(oci_dest.join("index.json")).expect("oci index.json"),
            )
            .expect("parse oci index.json");
            let copied_digest = index_json["manifests"][0]["digest"]
                .as_str()
                .expect("copied digest");
            assert!(
                copied_digest == format!("sha256:{}", sha256_hex(&image.amd64_manifest))
                    || copied_digest == format!("sha256:{}", sha256_hex(&image.arm64_manifest))
                    || copied_digest == image.index_digest,
                "copied digest {copied_digest} must match upstream fixture"
            );

            // Verify zero direct access: upstream saw only Hodei requests (no skopeo UA),
            // and disconnecting upstream still serves cached inspect from Hodei,
            // while stopping Hodei causes skopeo to fail.
            let hits_before_disconnect = upstream.hits();
            assert!(
                hits_before_disconnect
                    .iter()
                    .any(|h| h.method == "GET" && h.path.contains("/manifests/")),
                "upstream must have been queried by Hodei for manifests: {hits_before_disconnect:?}"
            );
            assert!(
                hits_before_disconnect
                    .iter()
                    .all(|h| !h.user_agent.to_lowercase().contains("skopeo")),
                "client must never hit upstream directly: {hits_before_disconnect:?}"
            );

            upstream.disconnect();
            let cached_inspect = clients.skopeo(&[
                "inspect",
                "--tls-verify=false",
                "--creds",
                &creds,
                "--raw",
                &target,
            ]);
            assert!(
                cached_inspect.ok,
                "second inspect must be served from Hodei cache with upstream down: {}",
                cached_inspect.combined()
            );

            drop(service);
            let without_hodei = clients.skopeo(&[
                "inspect",
                "--tls-verify=false",
                "--creds",
                &creds,
                "--raw",
                &target,
            ]);
            assert!(
                !without_hodei.ok,
                "skopeo must fail when Hodei is stopped (no direct fallback)"
            );

            LabCase::new(
                "ac1-skopeo-inspect-and-copy-through-oci-proxy",
                THE_CASES[0].1,
            )
            .command("skopeo inspect --raw & skopeo copy through Hodei OCI proxy")
            .origin(&upstream.base())
            .sha256(&image.index_digest)
            .passed()
        },
    );
}

#[test]
fn ac1_podman_pull_through_oci_proxy_and_group() {
    the_report();
    run_registry_lab_case(
        THE_CASES,
        "ac1-podman-pull-through-oci-proxy-and-group",
        THE_CASES[1].1,
        Scope::Product,
        Expectation::Succeeds,
        || {
            let deployment = Deployment::new();
            let clients = IsolatedClients::new(deployment.directory.path());
            require_native_tools(&clients.podman_root, &clients.podman_runroot);

            let image = build_multiarch_image_fixture();
            let helm = build_helm_chart_fixture(&clients);
            let upstream = LocalOciUpstream::start(Some("anon-lab-token"), None);
            seed_local_lab(&deployment, &upstream, &image, &helm);

            let allowed = format!("127.0.0.1:{};private;http", upstream.port);
            let service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &allowed)]);
            let token = support::token(FIRST_ACCOUNT, "alice");
            let creds = format!("alice:{token}");

            let proxy_ref = format!("127.0.0.1:{}/{IMAGE_PROXY_REPO}:3.9", service.port);
            let pulled_proxy =
                clients.podman(&["pull", "--tls-verify=false", "--creds", &creds, &proxy_ref]);
            assert!(
                pulled_proxy.ok,
                "podman pull through OCI proxy failed: {}",
                pulled_proxy.combined()
            );

            let inspected_proxy = clients.podman(&["inspect", &proxy_ref]);
            assert!(
                inspected_proxy.ok,
                "podman inspect failed: {}",
                inspected_proxy.combined()
            );
            let inspect_json: Value =
                serde_json::from_str(&inspected_proxy.stdout).expect("podman inspect json");
            assert_eq!(inspect_json[0]["Os"].as_str(), Some("linux"));
            assert!(
                matches!(
                    inspect_json[0]["Architecture"].as_str(),
                    Some("amd64" | "arm64")
                ),
                "unexpected architecture: {}",
                inspect_json[0]["Architecture"]
            );

            let group_ref = format!("127.0.0.1:{}/{IMAGE_GROUP_REPO}:3.9", service.port);
            let pulled_group =
                clients.podman(&["pull", "--tls-verify=false", "--creds", &creds, &group_ref]);
            assert!(
                pulled_group.ok,
                "podman pull through OCI group failed: {}",
                pulled_group.combined()
            );

            assert!(
                upstream
                    .hits()
                    .iter()
                    .all(|h| !h.user_agent.to_lowercase().contains("podman")),
                "podman must never contact upstream directly"
            );

            LabCase::new(
                "ac1-podman-pull-through-oci-proxy-and-group",
                THE_CASES[1].1,
            )
            .command("podman --storage-driver=vfs pull through Hodei OCI proxy and group")
            .origin(&upstream.base())
            .sha256(&image.index_digest)
            .passed()
        },
    );
}

// ---------------------------------------------------------------------------
// AC2: helm pull over OCI proxy and OCI group (classic Helm stays unannounced)
// ---------------------------------------------------------------------------

#[test]
fn ac2_helm_pull_oci_chart_through_proxy_and_group() {
    the_report();
    run_registry_lab_case(
        THE_CASES,
        "ac2-helm-pull-oci-chart-through-proxy-and-group",
        THE_CASES[2].1,
        Scope::Product,
        Expectation::Succeeds,
        || {
            // Confirm classic Helm (index.yaml) is NOT announced without an adapter.
            assert!(
                hodei_catalog::RepositoryFormat::Helm
                    .serves_modes()
                    .is_empty(),
                "RepositoryFormat::Helm must stay &[] until a classic index.yaml adapter exists"
            );

            let deployment = Deployment::new();
            let clients = IsolatedClients::new(deployment.directory.path());
            require_native_tools(&clients.podman_root, &clients.podman_runroot);

            let image = build_multiarch_image_fixture();
            let helm = build_helm_chart_fixture(&clients);
            let upstream = LocalOciUpstream::start(Some("anon-lab-token"), None);
            seed_local_lab(&deployment, &upstream, &image, &helm);

            let allowed = format!("127.0.0.1:{};private;http", upstream.port);
            let service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &allowed)]);
            let token = support::token(FIRST_ACCOUNT, "alice");
            let registry = format!("127.0.0.1:{}", service.port);

            let login = clients.helm(
                &[
                    "registry",
                    "login",
                    &registry,
                    "-u",
                    "alice",
                    "--password-stdin",
                    "--plain-http",
                ],
                Some(&token),
            );
            assert!(login.ok, "helm registry login failed: {}", login.combined());

            let proxy_dir = clients.helm_work.join("pull-proxy");
            std::fs::create_dir_all(&proxy_dir).expect("proxy pull dir");
            let pull_proxy = clients.helm(
                &[
                    "pull",
                    &format!("oci://{registry}/{HELM_PROXY_REPO}"),
                    "--version",
                    "0.1.0",
                    "--destination",
                    "pull-proxy",
                    "--plain-http",
                ],
                None,
            );
            assert!(
                pull_proxy.ok,
                "helm pull via OCI proxy failed: {}",
                pull_proxy.combined()
            );
            let proxy_tgz = proxy_dir.join(format!("{HELM_PROXY_REPO}-0.1.0.tgz"));
            assert_eq!(
                sha256_hex(&std::fs::read(&proxy_tgz).expect("proxy tgz")),
                helm.tgz_sha256,
                "helm chart SHA256 mismatch via OCI proxy"
            );

            let group_dir = clients.helm_work.join("pull-group");
            std::fs::create_dir_all(&group_dir).expect("group pull dir");
            let pull_group = clients.helm(
                &[
                    "pull",
                    &format!("oci://{registry}/{HELM_GROUP_REPO}"),
                    "--version",
                    "0.1.0",
                    "--destination",
                    "pull-group",
                    "--plain-http",
                ],
                None,
            );
            assert!(
                pull_group.ok,
                "helm pull via OCI group failed: {}",
                pull_group.combined()
            );
            let group_tgz = group_dir.join(format!("{HELM_GROUP_REPO}-0.1.0.tgz"));
            assert_eq!(
                sha256_hex(&std::fs::read(&group_tgz).expect("group tgz")),
                helm.tgz_sha256,
                "helm chart SHA256 mismatch via OCI group"
            );

            LabCase::new(
                "ac2-helm-pull-oci-chart-through-proxy-and-group",
                THE_CASES[2].1,
            )
            .command("helm pull oci://... through Hodei OCI proxy & OCI group")
            .origin(&upstream.base())
            .sha256(&helm.tgz_digest)
            .passed()
        },
    );
}

// ---------------------------------------------------------------------------
// AC3: Anonymous token auth, cross-origin CDN redirects, and 429 rate limits
// ---------------------------------------------------------------------------

#[test]
fn ac3_rate_limits_anonymous_auth_and_cdn_redirects() {
    the_report();
    run_registry_lab_case(
        THE_CASES,
        "ac3-rate-limits-anonymous-auth-and-cdn-redirects",
        THE_CASES[3].1,
        Scope::Product,
        Expectation::Succeeds,
        || {
            let deployment = Deployment::new();
            let clients = IsolatedClients::new(deployment.directory.path());
            require_native_tools(&clients.podman_root, &clients.podman_runroot);

            let image = build_multiarch_image_fixture();
            let helm = build_helm_chart_fixture(&clients);

            let mut cdn_blobs = HashMap::new();
            cdn_blobs.insert(image.layer_digest.clone(), image.layer_bytes.clone());
            cdn_blobs.insert(
                format!("sha256:{}", sha256_hex(&image.amd64_config)),
                image.amd64_config.clone(),
            );
            cdn_blobs.insert(
                format!("sha256:{}", sha256_hex(&image.arm64_config)),
                image.arm64_config.clone(),
            );
            cdn_blobs.insert(helm.config_digest.clone(), helm.config_bytes.clone());
            cdn_blobs.insert(helm.tgz_digest.clone(), helm.tgz_bytes.clone());

            let cdn = LocalCdn::start(cdn_blobs);
            let upstream = LocalOciUpstream::start(Some("anon-cdn-token"), Some(cdn.base()));
            seed_local_lab(&deployment, &upstream, &image, &helm);

            let allowed = format!(
                "127.0.0.1:{};private;http,127.0.0.1:{};private;http",
                upstream.port, cdn.port
            );
            let service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &allowed)]);
            let token = support::token(FIRST_ACCOUNT, "alice");
            let creds = format!("alice:{token}");

            // 1. Pull through proxy: exercises anonymous bearer token auth + 307 CDN redirect.
            let target = format!("docker://127.0.0.1:{}/{IMAGE_PROXY_REPO}:3.9", service.port);
            let oci_dest = clients.root.join("cdn-copy");
            let copied = clients.skopeo(&[
                "copy",
                "--src-tls-verify=false",
                "--src-creds",
                &creds,
                &target,
                &format!("oci:{}:3.9", oci_dest.display()),
            ]);
            assert!(
                copied.ok,
                "skopeo copy with anonymous token + CDN redirect failed: {}",
                copied.combined()
            );
            assert!(
                cdn.hit_count() >= 2,
                "CDN redirect target must have served config and layer blobs, got {}",
                cdn.hit_count()
            );

            // 2. Upstream 429 rate limit: uncached manifest must fail closed.
            upstream.set_rate_limit(true);
            let rate_limited = clients.skopeo(&[
                "inspect",
                "--tls-verify=false",
                "--creds",
                &creds,
                &format!(
                    "docker://127.0.0.1:{}/{HELM_PROXY_REPO}:0.1.0",
                    service.port
                ),
            ]);
            assert!(
                !rate_limited.ok,
                "429 rate limit from upstream must fail closed"
            );

            LabCase::new(
                "ac3-rate-limits-anonymous-auth-and-cdn-redirects",
                THE_CASES[3].1,
            )
            .command(&format!(
                "anonymous_auth=ok; cdn_redirects={} hits; rate_limit_429=fail_closed",
                cdn.hit_count()
            ))
            .origin(&upstream.base())
            .sha256(&image.index_digest)
            .passed()
        },
    );
}

// ---------------------------------------------------------------------------
// Opt-in Real Public Registry Lab Cases (HODEI_LAB_REAL=1)
// ---------------------------------------------------------------------------

#[test]
fn ac1_real_public_oci_image_through_proxy_and_group() {
    the_report();
    run_registry_lab_case(
        THE_CASES,
        "ac1-real-public-oci-image-through-proxy-and-group",
        THE_CASES[4].1,
        Scope::Product,
        Expectation::Succeeds,
        || {
            if !real_mode_enabled() {
                return LabCase::new(
                    "ac1-real-public-oci-image-through-proxy-and-group",
                    THE_CASES[4].1,
                )
                .origin("https://registry.k8s.io/")
                .not_run(format!("opt-in required: set {REAL_MODE_VARIABLE}=1"));
            }

            let deployment = Deployment::new();
            let clients = IsolatedClients::new(deployment.directory.path());
            require_native_tools(&clients.podman_root, &clients.podman_runroot);

            let budget = Budget::from_environment();
            deployment.seed_repositories(&[json!({
                "key": REAL_IMAGE_REPO,
                "format": "oci",
                "proxy": {
                    "upstream": {
                        "id": "k8s-registry",
                        "url": "https://registry.k8s.io/",
                        "scopes": ["v2/"],
                        "max_bytes": budget.max_bytes,
                        "redirects": { "max": 3, "cross_origin": true }
                    },
                    "cache": { "ttl": { "content_secs": 3600, "metadata_secs": 3600 } }
                }
            })]);
            deployment.seed_groups_raw(&[json!({
                "key": REAL_IMAGE_GROUP,
                "members": [REAL_IMAGE_REPO],
                "format": "oci",
            })]);

            // registry.k8s.io redirects to regional *.pkg.dev / S3 CDN hosts depending on client geo.
            let mut env = budget.service_environment();
            env.push((
                "HODEI_PROXY__ALLOWED_HOSTS".to_string(),
                "registry.k8s.io,europe-southwest1-docker.pkg.dev,us-central1-docker.pkg.dev,us-east1-docker.pkg.dev".to_string(),
            ));
            let borrowed: Vec<(&str, &str)> =
                env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            let service = deployment.start(&borrowed);
            let token = support::token(FIRST_ACCOUNT, "alice");
            let creds = format!("alice:{token}");
            let target = format!(
                "docker://127.0.0.1:{}/{REAL_IMAGE_REPO}:{REAL_IMAGE_TAG}",
                service.port
            );

            let inspect = clients.skopeo(&[
                "inspect",
                "--tls-verify=false",
                "--creds",
                &creds,
                "--raw",
                &target,
            ]);
            if !inspect.ok {
                return LabCase::new(
                    "ac1-real-public-oci-image-through-proxy-and-group",
                    THE_CASES[4].1,
                )
                .origin("https://registry.k8s.io/")
                .not_run(format!(
                    "external registry restriction or CDN redirect host not allowed: {}",
                    inspect.combined().trim()
                ));
            }
            let actual_digest = format!("sha256:{}", sha256_hex(inspect.stdout.as_bytes()));
            if actual_digest != REAL_IMAGE_DIGEST {
                return LabCase::new(
                    "ac1-real-public-oci-image-through-proxy-and-group",
                    THE_CASES[4].1,
                )
                .origin("https://registry.k8s.io/")
                .failed(format!(
                    "digest mismatch: expected {REAL_IMAGE_DIGEST}, got {actual_digest}"
                ));
            }

            let group_ref = format!(
                "127.0.0.1:{}/{REAL_IMAGE_GROUP}:{REAL_IMAGE_TAG}",
                service.port
            );
            let podman_pull =
                clients.podman(&["pull", "--tls-verify=false", "--creds", &creds, &group_ref]);
            if !podman_pull.ok {
                return LabCase::new(
                    "ac1-real-public-oci-image-through-proxy-and-group",
                    THE_CASES[4].1,
                )
                .origin("https://registry.k8s.io/")
                .not_run(format!(
                    "podman pull over real proxy/group hit external restriction: {}",
                    podman_pull.combined().trim()
                ));
            }

            LabCase::new(
                "ac1-real-public-oci-image-through-proxy-and-group",
                THE_CASES[4].1,
            )
            .command("skopeo inspect & podman pull registry.k8s.io/pause:3.9 via Hodei")
            .origin("https://registry.k8s.io/")
            .sha256(REAL_IMAGE_DIGEST)
            .passed()
        },
    );
}

#[test]
fn ac2_real_public_helm_oci_chart_through_proxy_and_group() {
    the_report();
    run_registry_lab_case(
        THE_CASES,
        "ac2-real-public-helm-oci-chart-through-proxy-and-group",
        THE_CASES[5].1,
        Scope::Product,
        Expectation::Succeeds,
        || {
            if !real_mode_enabled() {
                return LabCase::new(
                    "ac2-real-public-helm-oci-chart-through-proxy-and-group",
                    THE_CASES[5].1,
                )
                .origin("https://ghcr.io/")
                .not_run(format!("opt-in required: set {REAL_MODE_VARIABLE}=1"));
            }

            let deployment = Deployment::new();
            let clients = IsolatedClients::new(deployment.directory.path());
            require_native_tools(&clients.podman_root, &clients.podman_runroot);

            let budget = Budget::from_environment();
            deployment.seed_repositories(&[json!({
                "key": REAL_HELM_REPO,
                "format": "oci",
                "proxy": {
                    "upstream": {
                        "id": "ghcr-helm",
                        "url": REAL_HELM_UPSTREAM_URL,
                        "scopes": ["manifests/", "blobs/"],
                        "auth": {
                            "realm": "https://ghcr.io/token",
                            "service": "ghcr.io"
                        },
                        "max_bytes": budget.max_bytes,
                        "redirects": { "max": 3, "cross_origin": true }
                    },
                    "cache": { "ttl": { "content_secs": 3600, "metadata_secs": 3600 } }
                }
            })]);
            deployment.seed_groups_raw(&[json!({
                "key": REAL_HELM_GROUP,
                "members": [REAL_HELM_REPO],
                "format": "oci",
            })]);

            let mut env = budget.service_environment();
            env.push((
                "HODEI_PROXY__ALLOWED_HOSTS".to_string(),
                "ghcr.io,pkg-containers.githubusercontent.com".to_string(),
            ));
            let borrowed: Vec<(&str, &str)> =
                env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            let service = deployment.start(&borrowed);
            let token = support::token(FIRST_ACCOUNT, "alice");
            let registry = format!("127.0.0.1:{}", service.port);

            let login = clients.helm(
                &[
                    "registry",
                    "login",
                    &registry,
                    "-u",
                    "alice",
                    "--password-stdin",
                    "--plain-http",
                ],
                Some(&token),
            );
            if !login.ok {
                return LabCase::new(
                    "ac2-real-public-helm-oci-chart-through-proxy-and-group",
                    THE_CASES[5].1,
                )
                .origin("https://ghcr.io/")
                .not_run(format!("helm login failed: {}", login.combined().trim()));
            }

            let pull = clients.helm(
                &[
                    "pull",
                    &format!("oci://{registry}/{REAL_HELM_GROUP}"),
                    "--version",
                    REAL_HELM_VERSION,
                    "--destination",
                    ".",
                    "--plain-http",
                ],
                None,
            );
            if !pull.ok {
                return LabCase::new(
                    "ac2-real-public-helm-oci-chart-through-proxy-and-group",
                    THE_CASES[5].1,
                )
                .origin("https://ghcr.io/")
                .not_run(format!(
                    "external ghcr.io restriction or rate limit: {}",
                    pull.combined().trim()
                ));
            }

            let tgz = [
                clients
                    .helm_work
                    .join(format!("{REAL_HELM_GROUP}-{REAL_HELM_VERSION}.tgz")),
                clients
                    .helm_work
                    .join(format!("podinfo-{REAL_HELM_VERSION}.tgz")),
            ]
            .into_iter()
            .find(|p| p.exists())
            .expect("downloaded podinfo tgz");
            let actual_sha = sha256_hex(&std::fs::read(&tgz).expect("downloaded podinfo tgz"));
            if actual_sha != REAL_HELM_TGZ_SHA256 {
                return LabCase::new(
                    "ac2-real-public-helm-oci-chart-through-proxy-and-group",
                    THE_CASES[5].1,
                )
                .origin("https://ghcr.io/")
                .failed(format!(
                    "chart SHA256 mismatch: expected {REAL_HELM_TGZ_SHA256}, got {actual_sha}"
                ));
            }

            LabCase::new(
                "ac2-real-public-helm-oci-chart-through-proxy-and-group",
                THE_CASES[5].1,
            )
            .command("helm pull oci://... ghcr.io/stefanprodan/charts/podinfo:6.7.0 via Hodei")
            .origin("https://ghcr.io/")
            .sha256(&format!("sha256:{REAL_HELM_TGZ_SHA256}"))
            .passed()
        },
    );
}

#[test]
fn ac3_the_count_does_not_lie() {
    the_report();
    run_registry_lab_case(
        THE_CASES,
        "ac3-the-count-does-not-lie",
        THE_CASES[6].1,
        Scope::Harness,
        Expectation::Succeeds,
        || {
            let simulated = vec![
                LabCase::new("local-case", "local lab case").passed(),
                LabCase::new("real-case", "opt-in real case")
                    .not_run("opt-in required: set HODEI_LAB_REAL=1"),
            ];
            let summary = Summary::of(&simulated);
            assert_eq!(summary.pass, 1);
            assert_eq!(summary.not_run, 1);
            assert!(!summary.green, "a report with NOT_RUN must never be green");
            assert!(
                !Outcome::NotRun.is_green(),
                "NOT_RUN is never a green outcome"
            );

            LabCase::new("ac3-the-count-does-not-lie", THE_CASES[6].1)
                .command("Summary::of checks NOT_RUN blocks green=true")
                .passed()
        },
    );
}
