//! Hugging Face hosted of HSP-218 against the real client (`hf`).
//!
//! The venv's `hf` (`huggingface_hub` 1.2.3, which speaks httpx) downloads
//! a model seeded on Hodei's hosted surface: it resolves the
//! revision (`/api/models/{repo}/revision/main`), reads the resolve
//! metadata (`302` with `X-Repo-Commit`/`X-Linked-Etag`/`X-Linked-Size` and
//! a relative `Location`) and downloads the blob streaming through the `Location`.
//! The identity is set by the dialect itself: the file's etag IS its
//! sha256, and that of what is installed is compared with the one the surface
//! declared. The venv's `hf` 1.x is required (the system's 3.15 breaks
//! old toolchains, lesson from HSP-168): in `HF_BIN` (the UAT's venv)
//! or in the `PATH`; without it, the suite **fails** saying it is missing.
//! Each run uses its own `HF_HOME`: nothing is read or written
//! outside the deployment directory.
//!
//! The dialect is the one the client fixes; the wire map was captured by the
//! HSP-169 spike against huggingface.co (classic HTTP route, no xet) and
//! against a local fixture (FMT-07 of `uat/UAT-FORMATS.md`). The server
//! does not emit `X-Xet-*`, so the client falls back to the classic route: the one
//! this release serves.
//!
//! The documented negative test reproduces the spike's finding (its
//! fixture in `CORRUPT=1` mode, here as an embedded mini-fixture with the
//! same shape): on the classic route the client ONLY verifies the size,
//! never the sha256 — a download served corrupt is ACCEPTED. Integrity
//! is guaranteed by the server's CAS (connection with HSP-105),
//! not by the client.

mod support;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::Digest as _;
use support::{Deployment, FIRST_ACCOUNT, SECOND_ACCOUNT};

/// Requires the venv's or the PATH's `hf` 1.x, and returns its path.
fn the_hf_binary() -> String {
    if let Ok(path) = std::env::var("HF_BIN") {
        return path;
    }
    "hf".to_string()
}

fn require_hf() -> String {
    let binary = the_hf_binary();
    let version = Command::new(&binary)
        .arg("version")
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    assert!(
        version.lines().any(|line| line.trim().starts_with("1.")),
        "the hf suite pins the client huggingface_hub 1.x of the UAT venv, found: {version}"
    );
    binary
}

/// The result of a client run.
struct Run {
    /// Whether the client finished successfully.
    ok: bool,
    /// What it said, on stdout and stderr.
    output: String,
}

impl Run {
    /// The text if it finished successfully, and a failure with everything else if not.
    fn expect(&self, what: &str) -> String {
        assert!(self.ok, "{what} failed:\n{}", self.output);
        self.output.clone()
    }
}

/// An isolated `hf`: its cache `HF_HOME` and its `HOME`.
struct Client {
    /// The path of the executable.
    binary: String,
    /// The deployment directory.
    root: PathBuf,
    /// This client's cache.
    home: PathBuf,
}

impl Client {
    /// A clean cache. The endpoint and the credential go through the environment in
    /// each command: the dialect does not store the remote in a previous
    /// configuration.
    fn new(root: &Path, name: &str) -> Self {
        let home = root.join(format!("hf-home-{name}"));
        std::fs::create_dir_all(&home).expect("the hf home");
        Self {
            binary: the_hf_binary(),
            root: root.to_path_buf(),
            home,
        }
    }

    /// Runs `hf` with its isolated environment.
    fn run(&self, args: &[&str], endpoint: &str, token: &str) -> Run {
        let output = Command::new(&self.binary)
            .args(args)
            .env("HOME", &self.root)
            .env("HF_HOME", &self.home)
            // The classic route is the served dialect: without native xet, the
            // spike fixture's decision that this release replicates.
            .env("HF_HUB_DISABLE_XET", "1")
            .env("HF_HUB_DISABLE_TELEMETRY", "1")
            .env("HF_HUB_DISABLE_PROGRESS_BARS", "1")
            .env("HF_ENDPOINT", endpoint)
            .env("HF_TOKEN", token)
            .output()
            .expect("hf runs");
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Run {
            ok: output.status.success(),
            output: text,
        }
    }

    /// `hf download`: the real resolution against the surface.
    fn download(&self, repo: &str, endpoint: &str, token: &str, extra: &[&str]) -> Run {
        let mut args = vec!["download", repo, "--quiet"];
        args.extend_from_slice(extra);
        self.run(&args, endpoint, token)
    }

    /// The installed snapshot of a model: the snapshots folder of its
    /// cache. The client names the model `models--{owner}--{name}`.
    fn the_snapshot_of(&self, repo: &str) -> PathBuf {
        let model_dir = self
            .home
            .join("hub")
            .join(format!("models--{}", repo.replace('/', "--")));
        let snapshots = model_dir.join("snapshots");
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&snapshots)
            .expect("the snapshots of the download")
            .flatten()
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        assert_eq!(entries.len(), 1, "one snapshot per download: {entries:?}");
        entries.remove(0)
    }
}

/// The small model the suite seeds: a `config.json` and a legitimate
/// `model.safetensors` (8-byte JSON header + tensors), the
/// dialect's artifact in its real form but with a megabyte of weights.
fn the_model_files(root: &Path) -> (PathBuf, PathBuf) {
    let directory = root.join("model/hodei-demo-model");
    std::fs::create_dir_all(&directory).expect("the model directory");
    let config = directory.join("config.json");
    std::fs::write(
        &config,
        "{\n  \"model_type\": \"bert\",\n  \"hidden_size\": 128,\n  \"num_hidden_layers\": 2,\n  \"vocab_size\": 30522\n}\n",
    )
    .expect("the config");
    let model = directory.join("model.safetensors");
    // A legitimate safetensors: u64 little endian with the length of the
    // header (multiple of 8), the JSON header and the weights.
    let header = serde_json::json!({
        "weights": { "dtype": "F32", "shape": [256, 1024], "data_offsets": [0, 1048576] }
    })
    .to_string();
    let padding = (8 - (header.len() % 8)) % 8;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&((header.len() + padding) as u64).to_le_bytes());
    bytes.extend_from_slice(header.as_bytes());
    bytes.extend(std::iter::repeat_n(b' ', padding));
    bytes.extend((0..1048576u32).map(|index| (index % 251) as u8));
    std::fs::write(&model, bytes).expect("the safetensors");
    (config, model)
}

/// The sha256 of a file, as the protocol verifies it.
fn sha256_of(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("the file");
    format!("{:x}", sha2::Sha256::digest(&bytes))
}

/// The raw response of the surface, by socket: status, lowercase
/// headers and body. The dialect's HEADs and GETs are checked
/// this way, as they arrive on the wire.
fn the_request(
    method: &str,
    port: u16,
    path: &str,
    token: &str,
) -> (u16, HashMap<String, String>, Vec<u8>) {
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    );
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("the socket");
    let _ = stream.write_all(request.as_bytes());
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw).into_owned();
    let split = text.find("\r\n\r\n");
    let (head, body) = match split {
        Some(index) => (text[..index].to_string(), raw[index + 4..].to_vec()),
        None => (text.to_string(), Vec::new()),
    };
    let mut lines = head.lines();
    let status_line = lines.next().unwrap_or_default().to_string();
    let status: u16 = status_line
        .split(' ')
        .nth(1)
        .and_then(|status| status.parse().ok())
        .unwrap_or_default();
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    (status, headers, body)
}

/// The catalog seeder: each suite brings it because the harness restarts
/// the suite's binary to seed before starting the service.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

#[test]
fn the_hf_client_downloads_the_model_and_the_sha256_is_the_etag() {
    require_hf();
    let deployment = Deployment::new();
    let (config, model) = the_model_files(deployment.directory.path());
    deployment.seed_hf(&[(
        "hf-hosted",
        vec![
            ("artifacts/hodei/demo-model/config.json", config.clone()),
            (
                "artifacts/hodei/demo-model/model.safetensors",
                model.clone(),
            ),
        ],
    )]);
    let mut service = deployment.start(&[]);
    let port = service.port;
    let root = deployment.directory.path().to_path_buf();
    let endpoint = format!("http://127.0.0.1:{port}/hf/hf-hosted");
    let token = support::token(FIRST_ACCOUNT, "alice");
    let client = Client::new(&root, "consumer");

    // The snapshot's commit is the digest of the listing that forms it: what
    // the surface declares has to be what the seeded files
    // produce.
    let config_digest = sha256_of(&config);
    let model_digest = sha256_of(&model);
    let expected_commit = the_snapshot_commit(&[
        ("config.json", &config_digest),
        ("model.safetensors", &model_digest),
    ]);

    // AC1: the revision document carries `id`, `sha` and `siblings`, and the
    // commit it declares is that of the seeded snapshot. The same revision
    // is served by `main` and by its commit.
    let (status, _, body) = the_request(
        "GET",
        port,
        "/hf/hf-hosted/api/models/hodei/demo-model/revision/main",
        &token,
    );
    assert_eq!(status, 200, "the revision document is served");
    let document: serde_json::Value = serde_json::from_slice(&body).expect("the revision document");
    assert_eq!(document["id"], "hodei/demo-model");
    assert_eq!(document["sha"], expected_commit);
    let siblings: Vec<&str> = document["siblings"]
        .as_array()
        .expect("the siblings")
        .iter()
        .filter_map(|sibling| sibling["rfilename"].as_str())
        .collect();
    assert!(
        siblings.contains(&"config.json"),
        "the siblings: {siblings:?}"
    );
    assert!(
        siblings.contains(&"model.safetensors"),
        "the siblings: {siblings:?}"
    );
    let (by_commit, _, body_by_commit) = the_request(
        "GET",
        port,
        &format!("/hf/hf-hosted/api/models/hodei/demo-model/revision/{expected_commit}"),
        &token,
    );
    assert_eq!(by_commit, 200, "the revision is served by its commit");
    assert_eq!(body_by_commit, body, "main and the commit are one snapshot");

    // AC1: the resolve by `HEAD` is a `302` with the real wire's headers
    // and a relative `Location` (a path, without scheme or host).
    let (status, headers, _) = the_request(
        "HEAD",
        port,
        "/hf/hf-hosted/hodei/demo-model/resolve/main/model.safetensors",
        &token,
    );
    assert_eq!(status, 302, "the resolve redirects");
    assert_eq!(
        headers.get("x-repo-commit").map(String::as_str),
        Some(expected_commit.as_str()),
        "the commit travels in the header the client reads"
    );
    let etag = headers
        .get("x-linked-etag")
        .expect("the linked etag of the wire")
        .trim_matches('"')
        .to_string();
    assert_eq!(etag, model_digest, "the etag of the blob is its sha256");
    assert_eq!(
        headers.get("x-linked-size").map(String::as_str),
        Some(
            model
                .metadata()
                .expect("the size")
                .len()
                .to_string()
                .as_str()
        ),
        "the size of the blob travels too"
    );
    let location = headers.get("location").expect("the location").to_string();
    assert!(
        location.starts_with('/') && !location.contains("://"),
        "the location is a relative path, the shape the client follows: {location}"
    );

    // AC2: the client's real download. The whole model, with the cache
    // that the client itself manages.
    client
        .download("hodei/demo-model", &endpoint, &token, &[])
        .expect("the download");

    // AC2: what is installed is what the surface declared: the sha256 of the
    // installed file is the resolve's etag, and the installed snapshot's
    // commit is the one the revision served.
    let snapshot = client.the_snapshot_of("hodei/demo-model");
    let installed = snapshot.join("model.safetensors");
    assert!(installed.is_file(), "the install left the model in place");
    assert_eq!(
        sha256_of(&installed),
        etag,
        "the installed bytes are the ones the etag named"
    );
    assert_eq!(
        snapshot.file_name().and_then(|name| name.to_str()),
        Some(expected_commit.as_str()),
        "the client named the snapshot by the commit the surface declared"
    );

    // AC1: the `Location` carries the digest and serves the CAS bytes: the
    // GET of the URL the surface built is the whole content.
    let (status, blob_headers, blob) = the_request("GET", port, &location, &token);
    assert_eq!(status, 200, "the blob is served");
    assert_eq!(
        blob_headers.get("etag").map(String::as_str),
        Some(format!("\"{model_digest}\"").as_str()),
        "the etag of the served bytes is their sha256"
    );
    assert_eq!(
        blob.len(),
        model.metadata().expect("the size").len() as usize,
        "the blob is whole"
    );
    assert_eq!(
        format!("{:x}", sha2::Sha256::digest(&blob)),
        model_digest,
        "the served bytes are the seeded content"
    );

    // The evidence of the digest the client verified, for the report.
    println!("HSP-218 model.safetensors sha256: {model_digest}");
    support::stop(&mut service);
}

#[test]
fn the_warm_cache_resolves_when_the_service_is_down() {
    require_hf();
    let deployment = Deployment::new();
    let (config, model) = the_model_files(deployment.directory.path());
    deployment.seed_hf(&[(
        "hf-hosted",
        vec![
            ("artifacts/hodei/demo-model/config.json", config.clone()),
            (
                "artifacts/hodei/demo-model/model.safetensors",
                model.clone(),
            ),
        ],
    )]);
    let mut service = deployment.start(&[]);
    let port = service.port;
    let root = deployment.directory.path().to_path_buf();
    let endpoint = format!("http://127.0.0.1:{port}/hf/hf-hosted");
    let token = support::token(FIRST_ACCOUNT, "alice");
    let client = Client::new(&root, "consumer");

    client
        .download("hodei/demo-model", &endpoint, &token, &[])
        .expect("the first download warms the cache");
    support::stop(&mut service);

    // The service is dead: any client request would fail.
    // That the warm-cache download finishes fine IS the proof that
    // the client does not request anything again.
    let warm = client.download("hodei/demo-model", &endpoint, &token, &[]);
    assert!(
        warm.ok,
        "the warm cache resolves without the remote: {}",
        warm.output
    );
    let snapshot = client.the_snapshot_of("hodei/demo-model");
    assert_eq!(
        sha256_of(&snapshot.join("model.safetensors")),
        sha256_of(&model),
        "the warm snapshot is the served content"
    );

    // And a model it does not have fails: without network, there is no resolution that
    // invents what the hub did not serve.
    let missing = client.download("hodei/other-model", &endpoint, &token, &[]);
    assert!(
        !missing.ok,
        "the offline download of a model nobody served fails: {}",
        missing.output
    );
}

#[test]
fn another_principal_does_not_see_the_model_of_the_first_account() {
    require_hf();
    let deployment = Deployment::new();
    let (config, model) = the_model_files(deployment.directory.path());
    deployment.seed_hf(&[(
        "hf-hosted",
        vec![
            ("artifacts/hodei/demo-model/config.json", config.clone()),
            (
                "artifacts/hodei/demo-model/model.safetensors",
                model.clone(),
            ),
        ],
    )]);
    let mut service = deployment.start(&[]);
    let port = service.port;
    let root = deployment.directory.path().to_path_buf();
    let endpoint = format!("http://127.0.0.1:{port}/hf/hf-hosted");

    // Scoping is per account: the repository lives in the first account,
    // and a principal of another account does not see it.
    let token = support::token(FIRST_ACCOUNT, "alice");
    let stranger_token = support::token(SECOND_ACCOUNT, "bob");
    let stranger = Client::new(&root, "stranger");
    let stranger_download = stranger.download("hodei/demo-model", &endpoint, &stranger_token, &[]);
    assert!(
        !stranger_download.ok,
        "another principal does not see the model of the first account: {}",
        stranger_download.output
    );
    let _ = token;
    support::stop(&mut service);
}

/// The snapshot's commit: the digest of the sorted `rfilename sha256`
/// lines, the derivation the surface declares.
fn the_snapshot_commit(entries: &[(&str, &str)]) -> String {
    let mut listing: Vec<String> = entries
        .iter()
        .map(|(filename, digest)| format!("{filename} {digest}"))
        .collect();
    listing.sort();
    let mut hasher = sha2::Sha256::new();
    for line in listing {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    format!("{:x}", hasher.finalize())
}

/// The documented negative test: the classic client ONLY verifies the
/// size.
///
/// A mini-fixture with the same shape as the HSP-169 spike's in its
/// `CORRUPT=1` mode (oracle of hub documents and headers): it serves a
/// repo `hodei/spike-model` with a `config.json` whose `X-Linked-Etag`
/// declares the sha256 of the ORIGINAL file, but whose served bytes
/// carry a flipped byte, with the correct `Content-Length`. The real
/// client ACCEPTS the download: it does not check the declared sha256, and only
/// an external `sha256sum` detects the corruption. That is why integrity in
/// hosted is guaranteed by the server's CAS (HSP-105), which does not deliver
/// bytes that are not its digest.
#[test]
fn the_classic_client_only_verifies_the_size_never_the_sha256() {
    require_hf();
    let deployment = Deployment::new();
    let root = deployment.directory.path().to_path_buf();

    // The original file: the one the fixture's etag declares.
    let original = root.join("spike/config.json");
    std::fs::create_dir_all(original.parent().expect("the directory")).expect("the directory");
    std::fs::write(&original, "{\n  \"model_type\": \"bert\"\n}\n").expect("the original");
    let declared = sha256_of(&original);

    // The oracle's fixture: the classic hub's dialect with one
    // served byte corrupt. Python stdlib, no dependencies: the same
    // gap as the spike's `CORRUPT=1`.
    let fixture = root.join("corrupt_fixture.py");
    std::fs::write(&fixture, CORRUPT_FIXTURE).expect("the fixture");
    let port = support::free_port();
    let mut server = Command::new("python3")
        .arg(&fixture)
        .env("FIXTURE_PORT", port.to_string())
        .env(
            "FIXTURE_ROOT",
            root.join("spike").to_string_lossy().into_owned(),
        )
        .env("FIXTURE_SHA256", &declared)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("the fixture runs");
    // The fixture says when it listens, and says it by writing the port to
    // its own output... by socket: wait for the port to open.
    let mut ready = false;
    for _ in 0..100 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            ready = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(ready, "the corrupt fixture opened its port");

    // The real client against the oracle: the corrupt download is
    // ACCEPTED. That is the documented finding.
    let client = Client::new(&root, "corrupted");
    let endpoint = format!("http://127.0.0.1:{port}");
    let accepted = client.download(
        "hodei/spike-model",
        &endpoint,
        "a-token-the-fixture-ignores",
        &["config.json"],
    );
    let killed = server.kill();
    let _ = server.wait();
    let _ = killed;
    assert!(
        accepted.ok,
        "the classic client accepts a download whose bytes are not the declared sha256: {}",
        accepted.output
    );

    // And the corruption is seen by whoever looks outside the client: what is installed
    // is no longer what the etag declared.
    let snapshot = client.the_snapshot_of("hodei/spike-model");
    let installed = sha256_of(&snapshot.join("config.json"));
    assert_ne!(
        installed, declared,
        "the accepted download is not the content the etag named"
    );
    println!("HSP-218 negative: accepted sha256={installed}, declared etag={declared}");
}

/// The negative test's fixture: the classic hub's dialect in its
/// `CORRUPT=1` mode, the same shape as the HSP-169 spike's fixture.
const CORRUPT_FIXTURE: &str = r#"import hashlib, os, sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = os.environ["FIXTURE_ROOT"]
DECLARED = os.environ["FIXTURE_SHA256"]
PORT = int(os.environ["FIXTURE_PORT"])
REPO = "hodei/spike-model"
FILE = "config.json"

original = open(os.path.join(ROOT, FILE), "rb").read()
size = len(original)
# El primer byte servido se voltea: el fichero en disco sigue siendo el
# que el etag declara, y el tamano declarado no cambia.
corrupted = bytes([original[0] ^ 0xFF]) + original[1:]


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_HEAD(self):
        if self.path == "/api/models/%s/revision/main" % REPO:
            body = ('{"id": "%s", "sha": "%s", "siblings": [{"rfilename": "%s"}]}'
                    % (REPO, "f1cee5eed1e02b6a5c7d2e1e17a0d8b13d5c3a91", FILE)).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if self.path.startswith("/%s/resolve/" % REPO):
            self.send_response(302)
            self.send_header("X-Repo-Commit", "f1cee5eed1e02b6a5c7d2e1e17a0d8b13d5c3a91")
            self.send_header("X-Linked-Etag", '"%s"' % DECLARED)
            self.send_header("X-Linked-Size", str(size))
            self.send_header("Location", "/cdn-lfs/%s/%s" % (DECLARED, FILE))
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        if self.path.startswith("/cdn-lfs/"):
            # La metadata final del redirect: de aqui la lee el cliente.
            self.send_response(200)
            self.send_header("X-Repo-Commit", "f1cee5eed1e02b6a5c7d2e1e17a0d8b13d5c3a91")
            self.send_header("X-Linked-Etag", '"%s"' % DECLARED)
            self.send_header("X-Linked-Size", str(size))
            self.send_header("Content-Length", str(size))
            self.end_headers()
            return
        self.send_response(404)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self):
        if self.path == "/api/models/%s/revision/main" % REPO:
            self.do_HEAD()
            return
        if self.path.startswith("/cdn-lfs/"):
            self.send_response(200)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("ETag", '"%s"' % DECLARED)
            self.send_header("Content-Length", str(size))
            self.end_headers()
            try:
                self.wfile.write(corrupted)
            except (BrokenPipeError, ConnectionResetError):
                pass
            return
        self.send_response(404)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def log_message(self, fmt, *args):
        pass


ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
"#;
