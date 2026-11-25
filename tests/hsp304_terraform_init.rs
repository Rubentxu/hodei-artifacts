//! HSP-304 (AC1): a REAL `terraform init` against the provider network mirror
//! of Hodei, over verified https.
//!
//! `hsp157_terraform_mirror` speaks the mirror protocol with `curl`;
//! `hsp158_terraform_modules_client` drives the module registry over plain
//! http with a `services` override. This suite closes the gap the goldfish
//! review left (G4): the real client resolves a provider through the mirror
//! exactly as an operator configures it,
//!
//! ```text
//! provider_installation { network_mirror { url = "https://127.0.0.1:PORT/terraform/MIRROR/" } }
//! credentials "127.0.0.1:PORT" { token = "..." }
//! ```
//!
//! Terraform refuses a network mirror that is not https, so the hub sits
//! behind an ephemeral TLS bridge (`socat` + a throwaway `openssl`
//! certificate, the pattern of `hsp300_go_sumdb`). The client trusts
//! `SSL_CERT_FILE` = system CA bundle + the bridge certificate, so the TLS
//! verification is real: nothing is `insecure`.
//!
//! What is proven with the real client:
//!
//! - with the credential, the real client reads the index and the version
//!   document over verified https, and installs the provider: the version
//!   document of the mirror names the archive with a short-lived signed URL,
//!   because terraform sends no credential when it downloads the archive. The
//!   `zh:` hash of the installed archive is verified by the client itself
//!   (the lock file records it);
//! - a signed URL is no way around the credential: without it the client does
//!   not get past the index, and an archive whose bytes do not match the
//!   declared hash is refused by the client;
//! - without the credential, `init` fails and costs the upstream nothing;
//! - the module registry discovery cannot be driven by the real client over
//!   https without a `services` override, because the hub's
//!   `/.well-known/terraform.json` needs a Bearer credential (the account is
//!   named by it) and terraform's discovery sends none. That is asserted, not
//!   hidden: terraform asks for the document, gets the 401 and reports that the host
//!   does not provide a modules service.
//!
//! Without `terraform`, `socat`, `openssl` or a system CA bundle the suite is
//! NOT_RUN: it prints the reason and FAILS unless `HODEI_ALLOW_NOT_RUN=1`.
//! It is never a silent PASS.

mod support;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;
use sha2::Digest as _;
use support::{Curl, Deployment, FIRST_ACCOUNT};

const PROVIDER: &str = "registry.terraform.io/hashicorp/null";
const ZIP_NAME: &str = "terraform-provider-null_3.2.1_linux_amd64.zip";

#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// NOT_RUN: visible reason, and a failure unless explicitly accepted.
fn not_run(reason: &str) {
    eprintln!("NOT_RUN: hsp304_terraform_init: {reason}");
    assert!(
        std::env::var("HODEI_ALLOW_NOT_RUN").as_deref() == Ok("1"),
        "hsp304_terraform_init is NOT_RUN ({reason}); set HODEI_ALLOW_NOT_RUN=1 to accept"
    );
}

/// The real terraform binary: the asdf shim resolves its version from the
/// working directory, which a temporary deployment does not have, so the
/// real path is resolved once from here.
fn the_terraform_binary() -> Option<String> {
    let binary = std::env::var("TF_CLI").unwrap_or_else(|_| "terraform".to_string());
    let works = |candidate: &str| {
        Command::new(candidate)
            .arg("version")
            .output()
            .is_ok_and(|output| output.status.success())
    };
    let resolved = Command::new("asdf")
        .args(["which", "terraform"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|path| !path.is_empty() && Path::new(path).is_file());
    resolved
        .filter(|path| works(path))
        .or_else(|| works(&binary).then_some(binary))
}

/// The platform terraform runs on, from `terraform version` (`on linux_amd64`).
fn the_platform(binary: &str) -> (String, String) {
    let output = Command::new(binary)
        .arg("version")
        .output()
        .expect("terraform version runs");
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let version = text.lines().next().unwrap_or_default().to_string();
    let platform = text
        .lines()
        .find_map(|line| line.strip_prefix("on "))
        .unwrap_or("linux_amd64")
        .trim()
        .to_string();
    (version, platform)
}

fn tool_exists(tool: &str) -> bool {
    ["-version", "version"].iter().any(|flag| {
        Command::new(tool)
            .arg(flag)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
    })
}

fn system_ca_bundle() -> Option<PathBuf> {
    [
        "/etc/ssl/certs/ca-certificates.crt",
        "/etc/pki/tls/certs/ca-bundle.crt",
        "/etc/ssl/cert.pem",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|path| path.exists())
}

fn why_not_run() -> Option<String> {
    if the_terraform_binary().is_none() {
        return Some("the terraform binary is not installed".into());
    }
    for tool in ["socat", "openssl"] {
        if !tool_exists(tool) {
            return Some(format!("{tool} is not installed"));
        }
    }
    if system_ca_bundle().is_none() {
        return Some("no system CA bundle found".into());
    }
    None
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("an ephemeral port")
        .local_addr()
        .expect("the address of the listener")
        .port()
}

/// An ephemeral TLS bridge: `https` in front, the hub behind.
struct Bridge {
    child: std::process::Child,
    port: u16,
    /// System CAs plus the bridge certificate.
    bundle: PathBuf,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn bridge(root: &Path, hub: u16) -> Bridge {
    std::fs::create_dir_all(root).expect("the bridge directory");
    let pem = root.join("bridge.pem");
    let output = Command::new("openssl")
        .args(["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout"])
        .arg(&pem)
        .arg("-out")
        .arg(&pem)
        .args(["-days", "1", "-subj", "/CN=127.0.0.1"])
        .args(["-addext", "subjectAltName=IP:127.0.0.1"])
        .stderr(Stdio::piped())
        .output()
        .expect("openssl runs");
    assert!(
        output.status.success(),
        "openssl req: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut bundle_bytes =
        std::fs::read(system_ca_bundle().expect("checked by the guard")).expect("system CAs");
    bundle_bytes.push(b'\n');
    bundle_bytes.extend(std::fs::read(&pem).expect("the bridge pem"));
    let bundle = root.join("ca-bundle.pem");
    std::fs::write(&bundle, bundle_bytes).expect("the CA bundle");

    let port = free_port();
    let child = Command::new("socat")
        .arg(format!(
            "OPENSSL-LISTEN:{port},cert={},key={},verify=0,reuseaddr,fork",
            pem.display(),
            pem.display()
        ))
        .arg(format!("TCP:127.0.0.1:{hub}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the bridge starts");
    let deadline = Instant::now() + Duration::from_secs(10);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "the TLS bridge never listened");
        std::thread::sleep(Duration::from_millis(50));
    }
    Bridge {
        child,
        port,
        bundle,
    }
}

/// A fake upstream: serves what it is given by path and counts each ask.
struct Upstream {
    port: u16,
    asked: Arc<Mutex<HashMap<String, usize>>>,
}

impl Upstream {
    fn asked_for(&self, path: &str) -> usize {
        self.asked
            .lock()
            .expect("the counter")
            .get(path)
            .copied()
            .unwrap_or(0)
    }
}

fn start_upstream(pages: HashMap<String, Vec<u8>>) -> Upstream {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("the upstream binds");
    let port = listener.local_addr().expect("the address").port();
    let asked: Arc<Mutex<HashMap<String, usize>>> = Arc::new(Mutex::new(HashMap::new()));
    let pages = Arc::new(pages);
    let counter = Arc::clone(&asked);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let counter = Arc::clone(&counter);
            let pages = Arc::clone(&pages);
            std::thread::spawn(move || {
                let mut buffer = [0u8; 8192];
                let mut read = String::new();
                loop {
                    match stream.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => {
                            read.push_str(String::from_utf8_lossy(&buffer[..n]).as_ref());
                            if read.contains("\r\n\r\n") {
                                break;
                            }
                        }
                        Err(_) => return,
                    }
                }
                let Some(path) = read
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .map(str::to_string)
                else {
                    return;
                };
                *counter
                    .lock()
                    .expect("the counter")
                    .entry(path.clone())
                    .or_default() += 1;
                let answer = match pages.get(&path) {
                    Some(body) => {
                        let mut bytes = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .into_bytes();
                        bytes.extend_from_slice(body);
                        bytes
                    }
                    None => {
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_vec()
                    }
                };
                let _ = stream.write_all(&answer);
                let _ = stream.flush();
            });
        }
    });
    Upstream { port, asked }
}

/// A provider zip with one executable, as the registry ships it.
fn a_provider_zip(marker: &str) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o755);
    writer
        .start_file("terraform-provider-null_v3.2.1_x5", options)
        .expect("the executable starts");
    writer
        .write_all(format!("#!/bin/sh\n# {marker}\n").as_bytes())
        .expect("the executable is written");
    writer.finish().expect("the zip is closed").into_inner()
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", sha2::Sha256::digest(bytes))
}

/// The pages of a fake registry mirror: the index, the version document
/// (declaring `declared_hash` for the archive) and the archive served.
fn the_pages(platform: &str, declared_hash: &str, served_zip: &[u8]) -> HashMap<String, Vec<u8>> {
    let version = json!({
        "archives": { platform: { "url": ZIP_NAME, "hashes": [format!("zh:{declared_hash}")] } }
    });
    let mut pages = HashMap::new();
    pages.insert(
        format!("/{PROVIDER}/index.json"),
        br#"{"versions":{"3.2.1":{}}}"#.to_vec(),
    );
    pages.insert(
        format!("/{PROVIDER}/3.2.1.json"),
        version.to_string().into_bytes(),
    );
    pages.insert(format!("/{PROVIDER}/{ZIP_NAME}"), served_zip.to_vec());
    pages
}

static CALLS: AtomicUsize = AtomicUsize::new(0);

/// One real `terraform` with its own HOME, CLI config and working directory.
struct Client {
    binary: String,
    root: PathBuf,
    work: PathBuf,
    bundle: PathBuf,
    config: PathBuf,
}

impl Client {
    /// `network_mirror` for `mirror`, with the credential of the host when
    /// `token` is given.
    fn new(
        deployment: &Deployment,
        tunnel: &Bridge,
        binary: &str,
        mirror: &str,
        token: Option<&str>,
    ) -> Self {
        let n = CALLS.fetch_add(1, Ordering::Relaxed);
        let root = deployment.directory.path().join(format!("tf-client-{n}"));
        let work = root.join("work");
        std::fs::create_dir_all(&work).expect("the work directory");
        let host = format!("127.0.0.1:{}", tunnel.port);
        let mut config = format!(
            "provider_installation {{\n  network_mirror {{\n    url = \"https://{host}/terraform/{mirror}/\"\n  }}\n}}\n"
        );
        if let Some(token) = token {
            config.push_str(&format!(
                "\ncredentials \"{host}\" {{\n  token = \"{token}\"\n}}\n"
            ));
        }
        let config_path = root.join("terraform.rc");
        std::fs::write(&config_path, config).expect("the CLI config");
        Self {
            binary: binary.to_string(),
            root,
            work,
            bundle: tunnel.bundle.clone(),
            config: config_path,
        }
    }

    fn write_main(&self, text: &str) {
        std::fs::write(self.work.join("main.tf"), text).expect("main.tf");
    }

    /// `init` with the isolated environment; stdout and stderr together.
    fn init(&self) -> (bool, String) {
        let output = Command::new(&self.binary)
            .args(["init", "-input=false", "-no-color"])
            .current_dir(&self.work)
            .env("HOME", &self.root)
            .env("TF_CLI_CONFIG_FILE", &self.config)
            .env("SSL_CERT_FILE", &self.bundle)
            .env("TF_INPUT", "0")
            .env("TF_IN_AUTOMATION", "1")
            .env("CHECKPOINT_DISABLE", "1")
            .output()
            .expect("terraform runs");
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        (output.status.success(), text)
    }

    fn installed(&self, platform: &str) -> PathBuf {
        self.work
            .join(".terraform/providers")
            .join(PROVIDER)
            .join("3.2.1")
            .join(platform)
            .join("terraform-provider-null_v3.2.1_x5")
    }
}

const MAIN_PROVIDER: &str = "terraform {\n  required_providers {\n    null = {\n      source  = \"hashicorp/null\"\n      version = \"3.2.1\"\n    }\n  }\n}\n";

#[test]
fn the_real_terraform_inits_a_provider_through_the_https_mirror() {
    if let Some(reason) = why_not_run() {
        not_run(&reason);
        return;
    }
    let binary = the_terraform_binary().expect("checked by the guard");
    let (version, platform) = the_platform(&binary);
    eprintln!("HSP-304 terraform client: {version} on {platform}");

    let zip = a_provider_zip("hodei hsp-304 provider");
    let zip_hash = sha256_hex(&zip);
    let good = start_upstream(the_pages(&platform, &zip_hash, &zip));
    let deployment = Deployment::new();
    let proxy = |upstream: &Upstream| {
        json!({
            "upstream": {
                "id": "lab",
                "url": format!("http://127.0.0.1:{}/", upstream.port),
                "scopes": ["/"],
                "max_bytes": 1_048_576,
            },
        })
    };
    deployment.seed_terraform_proxies(&[("tf-mirror", proxy(&good))]);
    let allowed = format!("127.0.0.1:{};private;http", good.port);
    let mut service = deployment.start(&[
        ("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str()),
        ("HODEI_SIGNING__KEYS__0__ID", "k1"),
        (
            "HODEI_SIGNING__KEYS__0__SECRET_ENV",
            "HODEI_TEST_SIGNING_SECRET_K1",
        ),
        ("HODEI_TEST_SIGNING_SECRET_K1", support::SIGNING_SECRET),
    ]);
    let tunnel = bridge(&deployment.directory.path().join("bridge"), service.port);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // The https is verified: curl with the same CA bundle reads the mirror.
    let into = deployment.directory.path().join("curl-index.json");
    let curl = Command::new("curl")
        .args(["-s", "-m", "10", "--cacert"])
        .arg(&tunnel.bundle)
        .args(["-o"])
        .arg(&into)
        .args(["-w", "%{http_code}", "-H"])
        .arg(format!("Authorization: Bearer {token}"))
        .arg(format!(
            "https://127.0.0.1:{}/terraform/tf-mirror/{PROVIDER}/index.json",
            tunnel.port
        ))
        .output()
        .expect("curl runs");
    assert_eq!(
        String::from_utf8_lossy(&curl.stdout),
        "200",
        "the mirror answers over verified https"
    );

    // 1) With the credential, the real client reads the index and the
    // version document over verified https, and the version document names
    // the archive with a signed URL that needs no credential: terraform
    // downloads archives with a plain HTTP client that sends none.
    let client = Client::new(&deployment, &tunnel, &binary, "tf-mirror", Some(&token));
    client.write_main(MAIN_PROVIDER);
    let (ok, text) = client.init();
    assert!(
        ok,
        "terraform init installs the provider from the mirror: {text}"
    );
    assert_eq!(
        good.asked_for(&format!("/{PROVIDER}/index.json")),
        1,
        "the credential got the real client past the index: {text}"
    );
    assert_eq!(
        good.asked_for(&format!("/{PROVIDER}/3.2.1.json")),
        1,
        "and past the version document: {text}"
    );
    assert_eq!(
        good.asked_for(&format!("/{PROVIDER}/{ZIP_NAME}")),
        1,
        "the archive was fetched once, through the signed URL"
    );

    // 2) The installed provider is the content of the upstream archive.
    let installed = client.installed(&platform);
    assert!(installed.exists(), "the provider is installed: {text}");
    assert_eq!(
        std::fs::read(&installed).expect("the installed provider"),
        b"#!/bin/sh\n# hodei hsp-304 provider\n",
        "the installed executable is the one inside the upstream zip"
    );
    // The lock file records the h1: hash of what the client unpacked. That the
    // client compared the archive with the declared `zh:` hash before
    // unpacking is proved from the other side by the tampered-archive test.
    let lock = std::fs::read_to_string(client.work.join(".terraform.lock.hcl"))
        .expect("terraform writes the lock file");
    assert!(
        lock.contains("h1:"),
        "the lock file pins the provider: {lock}"
    );
    // The hub serves the archive to an authenticated reader byte for byte
    // too: what terraform verified is what the upstream shipped.
    let again = Curl::call_with_headers(
        service.port,
        "GET",
        &format!("/terraform/tf-mirror/{PROVIDER}/{ZIP_NAME}"),
        &token,
        None,
        &[],
        Some(&deployment.directory.path().join("zip-again")),
    )
    .run();
    assert_eq!(again.status, 200);
    assert_eq!(again.body, zip, "the mirror serves the zip byte for byte");
    assert_eq!(sha256_hex(&again.body), zip_hash);

    // 3) Without the credential the real client does not get past the index:
    // nothing is asked upstream on its behalf.
    let before = good.asked_for(&format!("/{PROVIDER}/index.json"));
    let anonymous = Client::new(&deployment, &tunnel, &binary, "tf-mirror", None);
    anonymous.write_main(MAIN_PROVIDER);
    let (ok, text) = anonymous.init();
    assert!(!ok, "init without a credential must fail: {text}");
    assert!(
        text.contains("rejected the given authentication"),
        "the refusal is the authentication one: {text}"
    );
    assert_eq!(
        good.asked_for(&format!("/{PROVIDER}/index.json")),
        before,
        "the anonymous client cost the upstream nothing"
    );
    assert!(!anonymous.installed(&platform).exists());

    // 4) Discovery of the module registry, real client, https: it needs a
    // credential the discovery protocol never sends.
    let discovery = Client::new(&deployment, &tunnel, &binary, "tf-mirror", Some(&token));
    discovery.write_main(&format!(
        "module \"demo\" {{\n  source = \"127.0.0.1:{}/hodei/demo/local\"\n}}\n",
        tunnel.port
    ));
    let (ok, text) = discovery.init();
    assert!(
        !ok && text.contains("does not provide a modules service"),
        "real discovery over https finds no modules service: {text}"
    );
    assert!(
        deployment.the_log().contains("/.well-known/terraform.json"),
        "the client did ask the hub for the discovery document, which answered 401 without a credential"
    );

    support::stop(&mut service);
}

/// The integrity is the client's, not the hub's: a mirror whose upstream
/// serves bytes that do not match the declared `zh:` hash does not install,
/// even though the signed URL is valid and the transport is fine.
#[test]
fn the_real_terraform_refuses_an_archive_that_does_not_match_its_zh_hash() {
    if let Some(reason) = why_not_run() {
        not_run(&reason);
        return;
    }
    let binary = the_terraform_binary().expect("checked by the guard");
    let (_, platform) = the_platform(&binary);
    let declared = sha256_hex(&a_provider_zip("hodei hsp-304 provider"));
    let tampered = a_provider_zip("hodei hsp-304 TAMPERED provider");
    assert_ne!(sha256_hex(&tampered), declared);
    let corrupt = start_upstream(the_pages(&platform, &declared, &tampered));
    let deployment = Deployment::new();
    deployment.seed_terraform_proxies(&[(
        "tf-corrupt",
        json!({
            "upstream": {
                "id": "lab",
                "url": format!("http://127.0.0.1:{}/", corrupt.port),
                "scopes": ["/"],
                "max_bytes": 1_048_576,
            },
        }),
    )]);
    let allowed = format!("127.0.0.1:{};private;http", corrupt.port);
    let mut service = deployment.start(&[
        ("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str()),
        ("HODEI_SIGNING__KEYS__0__ID", "k1"),
        (
            "HODEI_SIGNING__KEYS__0__SECRET_ENV",
            "HODEI_TEST_SIGNING_SECRET_K1",
        ),
        ("HODEI_TEST_SIGNING_SECRET_K1", support::SIGNING_SECRET),
    ]);
    let tunnel = bridge(&deployment.directory.path().join("bridge"), service.port);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let client = Client::new(&deployment, &tunnel, &binary, "tf-corrupt", Some(&token));
    client.write_main(MAIN_PROVIDER);
    let (ok, text) = client.init();
    assert!(
        !ok,
        "terraform must refuse an archive that does not match its zh: hash: {text}"
    );
    assert_eq!(
        corrupt.asked_for(&format!("/{PROVIDER}/{ZIP_NAME}")),
        1,
        "the client did download the archive through the signed URL: {text}"
    );
    assert!(
        text.contains("checksum") || text.contains("hash"),
        "the refusal is the integrity one: {text}"
    );
    assert!(!client.installed(&platform).exists());
    support::stop(&mut service);
}
