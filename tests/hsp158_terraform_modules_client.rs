//! The private module registry of HSP-158 against the real client
//! (`terraform`).
//!
//! The environment's `terraform` 1.10 resolves a real private module:
//! it writes a configuration whose source is
//! `127.0.0.1:PORT/hodei/demo/local`, declares the registry in its CLI
//! config (`host` + `services`, the on-disk `ForceHostServices`: the client
//! always requests discovery over https and a hub without TLS declares the
//! service this way), carries its credential as the host's `credentials`, and the
//! real `init` goes through discovery → versions → download → go-getter, which
//! downloads the tarball with the `netrc` credential and verifies the URL's
//! `?checksum=sha256:` against the bytes it receives. It requires
//! `terraform` 1.10.x (in `TF_CLI` or on the `PATH`): without it, the suite
//! **fails** saying it is missing. Each run uses its own `HOME`, its
//! CLI config and its `NETRC`: nothing is read or written outside the
//! deployment directory.
//!
//! The dialect is the one the client fixes (`internal/registry/client.go`,
//! `terraform-svchost/disco` and `go-getter` v1.7 of terraform 1.10.1); the
//! wire map is recorded in FMT-04 of `uat/UAT-FORMATS.md` and in
//! `target/uat/hsp158/evidencia.md`.

mod support;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use flate2::write::GzEncoder;
use sha2::Digest as _;
use support::{Deployment, FIRST_ACCOUNT, SECOND_ACCOUNT};

/// Requires the environment's `terraform` 1.10.x and returns the **real** path of the
/// executable. The asdf shim resolves the version from the working
/// directory, and the deployment does not have it: it is resolved once, from the
/// repo folder, and the client runs with the absolute binary.
fn require_terraform() -> String {
    let binary = std::env::var("TF_CLI").unwrap_or_else(|_| "terraform".to_string());
    let version = Command::new(&binary)
        .arg("version")
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    let line = version.lines().next().unwrap_or_default().to_string();
    assert!(
        line.starts_with("Terraform v1.10."),
        "the terraform suite pins the client terraform 1.10.x, found: {line}"
    );
    let resolved = Command::new("asdf")
        .args(["which", "terraform"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|path| !path.is_empty() && Path::new(path).is_file());
    resolved.unwrap_or(binary)
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

/// An isolated `terraform`: its configuration directory, its CLI
/// config and its `NETRC`.
struct Client {
    /// The path of the executable.
    binary: String,
    /// The deployment directory.
    root: PathBuf,
    /// The working directory of this configuration.
    work: PathBuf,
    /// The host and port of the hub, as the source names it.
    host: String,
}

impl Client {
    /// A clean client for an account. The CLI config declares the
    /// host's `modules.v1` service (discovery over https is not
    /// reachable on an http hub) and the host's credential; the `NETRC`
    /// carries the credential that go-getter sends when downloading the artifact.
    fn new(root: &Path, name: &str, port: u16, token: &str, the_galaxy_terraform: String) -> Self {
        let work = root.join(format!("tf-{name}"));
        std::fs::create_dir_all(&work).expect("the work directory");
        let host = format!("127.0.0.1:{port}");
        let config = format!(
            "host \"{host}\" {{\n  services = {{\n    \"modules.v1\" = \"http://{host}/terraform/modules-hosted/v1/modules/\"\n  }}\n}}\n\ncredentials \"{host}\" {{\n  token = \"{token}\"\n}}\n"
        );
        std::fs::write(work.join("terraform.rc"), &config).expect("the CLI config");
        // go-getter looks up the netrc entry by , which carries the
        // port: the entry names the host with its port.
        let netrc = format!("machine 127.0.0.1:{port} login hodei password {token}\n");
        std::fs::write(work.join("netrc"), &netrc).expect("the netrc");
        Self {
            binary: the_galaxy_terraform,
            root: root.to_path_buf(),
            work,
            host,
        }
    }

    /// Writes the configuration of the private module that this client
    /// resolves, with its version constraint.
    fn write_config(&self, version: Option<&str>) -> PathBuf {
        let constraint = version
            .map(|version| format!("  version = \"{version}\"\n"))
            .unwrap_or_default();
        let config = format!(
            "module \"demo\" {{\n  source  = \"{}/hodei/demo/local\"\n{}}}\n",
            self.host, constraint
        );
        std::fs::write(self.work.join("main.tf"), &config).expect("the module config");
        self.work.clone()
    }

    /// Runs `terraform` with its isolated environment.
    fn run(&self, args: &[&str]) -> Run {
        let output = Command::new(&self.binary)
            .args(args)
            .current_dir(&self.work)
            .env("HOME", &self.root)
            .env("TF_CLI_CONFIG_FILE", self.work.join("terraform.rc"))
            .env("NETRC", self.work.join("netrc"))
            .env("TF_INPUT", "0")
            .env("TF_IN_AUTOMATION", "1")
            .output()
            .expect("terraform runs");
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Run {
            ok: output.status.success(),
            output: text,
        }
    }

    /// `init`: the real resolution against the surface.
    fn init(&self) -> Run {
        self.run(&["init", "-input=false", "-no-color"])
    }

    /// The module installed by this client: the directory that the
    /// installer names with the key of the block .
    fn installed(&self) -> PathBuf {
        self.work.join(".terraform").join("modules").join("demo")
    }

    /// The manifest that the installer writes.
    fn manifest(&self) -> String {
        std::fs::read_to_string(
            self.work
                .join(".terraform")
                .join("modules")
                .join("modules.json"),
        )
        .expect("the modules manifest")
    }
}

/// The sha256 of some bytes, as the protocol verifies it.
fn sha256_of(bytes: &[u8]) -> String {
    format!("{:x}", sha2::Sha256::digest(bytes))
}

/// A ustar member written as the format reads it: header, content and
/// the padding that rounds up the record.
fn the_member_bytes(name: &str, content: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0u8; 512];
    bytes[..name.len()].copy_from_slice(name.as_bytes());
    // The mode goes in the header, and go-getter respects it on extraction: a
    // mode 0000 is a file the client cannot read.
    bytes[100..108].copy_from_slice(b"0000644\0");
    let size = format!("{:011o}", content.len());
    bytes[124..124 + size.len()].copy_from_slice(size.as_bytes());
    bytes[148..156].copy_from_slice(b"        ");
    bytes[156] = b'0';
    bytes[257..262].copy_from_slice(b"ustar");
    let checksum: u32 = bytes.iter().map(|byte| *byte as u32).sum();
    let checksum = format!("{:06o}\0 ", checksum);
    bytes[148..156].copy_from_slice(checksum.as_bytes());
    let mut member = bytes;
    member.extend_from_slice(content);
    let padding = (512 - (content.len() % 512)) % 512;
    member.extend(std::iter::repeat_n(0u8, padding));
    member
}

/// The tarball of a module: the files at the archive root, as the
/// registry stores them and go-getter decompresses them.
fn the_module_tarball(root: &Path, name: &str, content: &str) -> Vec<u8> {
    let mut tar = Vec::new();
    tar.extend_from_slice(&the_member_bytes(
        "main.tf",
        format!("# {name}\nvariable \"region\" {{\n  type    = string\n  default = \"eu\"\n}}\n{content}\n")
            .as_bytes(),
    ));
    tar.extend_from_slice(&the_member_bytes(
        "outputs.tf",
        b"output \"region\" {\n  value = var.region\n}\n",
    ));
    tar.extend_from_slice(&[0u8; 512]);
    let mut gzipped = GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzipped.write_all(&tar).expect("the gzip write");
    let bytes = gzipped.finish().expect("the gzip finish");
    let path = root.join(format!("{name}.tar.gz"));
    std::fs::write(&path, &bytes).expect("the tarball");
    bytes
}

/// A publication made by hand over the socket: the publication
/// surface is defined by the hub (the CLI cannot publish modules).
/// Returns the status and the body of the response.
#[allow(clippy::too_many_arguments)]
fn publish_module(
    port: u16,
    repository: &str,
    token: &str,
    version: &str,
    bytes: &[u8],
    declared_sha256: Option<&str>,
) -> (u16, String) {
    let mut request = format!(
        "POST /terraform/{repository}/publish/hodei/demo/local/{version} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/gzip\r\n"
    );
    if let Some(declared) = declared_sha256 {
        request.push_str(&format!("X-Hodei-Sha256: {declared}\r\n"));
    }
    request.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        bytes.len()
    ));
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("the socket");
    let _ = stream.write_all(request.as_bytes());
    let _ = stream.write_all(bytes);
    let mut answer = String::new();
    let _ = stream.read_to_string(&mut answer);
    let status = answer
        .lines()
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|status| status.parse().ok())
        .expect("the status line");
    let body = answer
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or_default()
        .to_string();
    (status, body)
}

/// A GET request made by hand over the socket, with its headers and the
/// body **in bytes**: the one that records the protocol wire for the
/// evidence, and the one that compares the served tarball with the published one.
fn socket_get(port: u16, path: &str, authorization: Option<&str>) -> (u16, String, Vec<u8>) {
    let mut request =
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n");
    if let Some(token) = authorization {
        request.push_str(&format!("Authorization: {token}\r\n"));
    }
    request.push_str("\r\n");
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("the socket");
    let _ = stream.write_all(request.as_bytes());
    let mut answer = Vec::new();
    let _ = stream.read_to_end(&mut answer);
    let split = answer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("the head and the body separate");
    let head = String::from_utf8_lossy(&answer[..split]).into_owned();
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|status| status.parse().ok())
        .expect("the status line");
    let headers = head.lines().skip(1).collect::<Vec<_>>().join("\n");
    (status, headers, answer[split + 4..].to_vec())
}

/// The catalog seeder: each suite brings it because the harness restarts the
/// suite binary to seed before starting the service.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// AC1 + AC2: the complete flow of the first private module, with the real
/// client: versioned and immutable publication, authenticated `init` that
/// resolves by version and verifies the tarball checksum, incompatible
/// mutation rejected and resolution from another account denied.
#[test]
fn the_terraform_client_inits_a_private_module() {
    require_terraform();
    let deployment = Deployment::new();
    deployment.seed_terraform(&[("modules-hosted", serde_json::Value::Null)]);
    let mut service = deployment.start(&[]);
    let port = service.port;
    let root = deployment.directory.path().to_path_buf();

    let the_binary = require_terraform();
    let token = support::token(FIRST_ACCOUNT, "alice");
    let v1 = the_module_tarball(&root, "module-v1", "# version one");
    let v1_digest = sha256_of(&v1);

    // AC2: the publication is a receipt with the sha256 that the hub computed
    // at commit; the same one that the download later declares.
    let (status, receipt) = publish_module(port, "modules-hosted", &token, "1.0.0", &v1, None);
    assert_eq!(
        status, 201,
        "the publish of a new version is a receipt: {receipt}"
    );
    let receipt: serde_json::Value = serde_json::from_str(&receipt).expect("the receipt");
    assert_eq!(
        receipt["sha256"], v1_digest,
        "the receipt names the sha256 of the bytes"
    );
    assert_eq!(
        receipt["size_bytes"],
        v1.len() as u64,
        "the receipt names the size"
    );

    // AC1: the real `init`. The client requests versions with its credential,
    // resolves the constraint, follows the `X-Terraform-Get` and go-getter downloads
    // the tarball with the netrc credential verifying `?checksum=`.
    let author = Client::new(&root, "author", port, &token, the_binary.clone());
    author.write_config(Some("1.0.0"));
    author.init().expect("the init of the private module");
    assert!(
        author.installed().join("main.tf").is_file(),
        "the init left the module in place: {}",
        author.installed().display()
    );
    let manifest = author.manifest();
    assert!(
        manifest.contains("\"Version\":\"1.0.0\""),
        "the manifest records the resolved version: {manifest}"
    );

    // AC1: the version is IMMUTABLE. Repeating the publication -- with the same
    // bytes -- is a rejection, not an echo.
    let (status, again) = publish_module(port, "modules-hosted", &token, "1.0.0", &v1, None);
    assert_eq!(
        status, 409,
        "the republish of the same version is rejected: {again}"
    );
    // AC2: the incompatible mutation -- same version, other content -- is
    // another rejection.
    let tampered = the_module_tarball(&root, "module-v1-tampered", "# version one, changed");
    let (status, mutated) =
        publish_module(port, "modules-hosted", &token, "1.0.0", &tampered, None);
    assert_eq!(
        status, 409,
        "the same version with other content is rejected: {mutated}"
    );

    // AC2: what is published is the receipt. The served tarball is byte for byte the
    // one that went in, and the download declares the receipt's checksum.
    let (_, headers, _body) = socket_get(
        port,
        "/terraform/modules-hosted/v1/modules/hodei/demo/local/1.0.0/download",
        Some(&format!("Bearer {token}")),
    );
    assert!(
        headers.to_lowercase().contains("x-terraform-get"),
        "the download answers with the go-getter source: {headers}"
    );
    let location = headers
        .lines()
        .find(|line| line.to_lowercase().starts_with("x-terraform-get"))
        .and_then(|line| line.split_once(':'))
        .map(|(_, value)| value.trim().to_string())
        .expect("the location");
    assert!(
        location.ends_with(&format!(
            "artifacts/hodei/demo/local/1.0.0.tar.gz?checksum=sha256:{v1_digest}"
        )),
        "the download points at the artifact with the receipt checksum: {location}"
    );
    let (status, served_headers, artifact) =
        socket_get(port, &location, Some(&format!("Bearer {token}")));
    assert_eq!(status, 200, "the artifact is served: {served_headers}");
    assert!(
        served_headers.contains("content-type: application/gzip"),
        "the artifact is served as a tarball: {served_headers}"
    );
    assert_eq!(
        sha256_of(&artifact),
        v1_digest,
        "the bytes served are the bytes published"
    );

    // AC1: scoping is per account. Another principal does not resolve the module:
    // the repository lives in another account, and its init finds nothing.
    let other = support::token(SECOND_ACCOUNT, "bob");
    let stranger = Client::new(&root, "stranger", port, &other, the_binary.clone());
    stranger.write_config(Some("1.0.0"));
    let denied = stranger.init();
    assert!(
        !denied.ok,
        "another account does not resolve the module of the first account: {}",
        denied.output
    );
    let (status, _, _) = socket_get(
        port,
        "/terraform/modules-hosted/v1/modules/hodei/demo/local/versions",
        Some(&format!("Bearer {other}")),
    );
    assert_eq!(
        status, 404,
        "the versions of another account's repository do not exist for it"
    );
    let (status, _) = publish_module(port, "modules-hosted", &other, "2.0.0", &tampered, None);
    assert_eq!(
        status, 404,
        "the repository of another account does not exist for the stranger"
    );

    // AC2: the version index. The second version gets in and the
    // version resolution reaches it.
    let v2 = the_module_tarball(&root, "module-v2", "# version two");
    let (status, receipt2) = publish_module(port, "modules-hosted", &token, "2.0.0", &v2, None);
    assert_eq!(status, 201, "the second version publishes: {receipt2}");
    let (_, _, index) = socket_get(
        port,
        "/terraform/modules-hosted/v1/modules/hodei/demo/local/versions",
        Some(&format!("Bearer {token}")),
    );
    let index: serde_json::Value = serde_json::from_slice(&index).expect("the index");
    let versions: Vec<&str> = index["modules"][0]["versions"]
        .as_array()
        .expect("the versions")
        .iter()
        .filter_map(|version| version["version"].as_str())
        .collect();
    assert_eq!(
        versions,
        ["1.0.0", "2.0.0"],
        "the index lists the versions in order"
    );

    let pinned = Client::new(&root, "pinned", port, &token, the_binary.clone());
    pinned.write_config(Some("2.0.0"));
    pinned.init().expect("the init of the second version");
    assert!(pinned.installed().join("main.tf").is_file());

    // AC4: the host's discovery names the account's only registry, and
    // only modules.v1: AC3 not implemented, nothing is announced. It is the
    // document the client requests over https in a deployment with TLS and
    // through the CLI config in this one.
    let (status, _, document) = socket_get(
        port,
        "/.well-known/terraform.json",
        Some(&format!("Bearer {token}")),
    );
    assert_eq!(
        status, 200,
        "the discovery of a single-registry account answers"
    );
    let document: serde_json::Value =
        serde_json::from_slice(&document).expect("the discovery document");
    assert_eq!(
        document["modules.v1"], "/terraform/modules-hosted/v1/modules/",
        "the discovery names the repository of the account"
    );
    assert!(
        document.get("providers.v1").is_none(),
        "AC3: the provider registry is not announced because it is not implemented"
    );

    // AC4: offline mode. With the hub stopped, the init of a warm
    // cache needs no network (the installer reuses what is installed) and
    // a resolution that has no cache fails, without falling back to anything.
    support::stop(&mut service);
    let warm = author.init();
    assert!(
        warm.ok,
        "the offline init of a warm cache needs no remote: {}",
        warm.output
    );
    let cold = Client::new(&root, "cold", port, &token, the_binary.clone());
    cold.write_config(Some("1.0.0"));
    let missing = cold.init();
    assert!(
        !missing.ok,
        "the offline init of a cold cache fails: {}",
        missing.output
    );

    // The evidence of the digest that the client verified, for the report.
    println!("HSP-158 module v1 sha256: {v1_digest}");
}

/// AC4: the hub's service discovery surface and the rest of the wire
/// that the real client does not reach over http (discovery always goes over
/// https): the document that names the account's registry, and the boundary
/// of another account.
#[test]
fn the_discovery_names_the_module_registry_of_the_account() {
    let deployment = Deployment::new();
    deployment.seed_terraform(&[
        ("modules-hosted", serde_json::Value::Null),
        ("modules-second", serde_json::Value::Null),
    ]);
    let service = deployment.start(&[]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");

    // Two registries in the same account: discovery cannot choose
    // on its own, and says so.
    let (status, _, body) = socket_get(
        port,
        "/.well-known/terraform.json",
        Some(&format!("Bearer {token}")),
    );
    assert_eq!(
        status,
        409,
        "an account with several registries is told to name one: {}",
        String::from_utf8_lossy(&body)
    );

    // An account without registries: there is nothing to announce.
    let other = support::token(SECOND_ACCOUNT, "bob");
    let (status, _, body) = socket_get(
        port,
        "/.well-known/terraform.json",
        Some(&format!("Bearer {other}")),
    );
    assert_eq!(
        status,
        404,
        "an account without registries announces nothing: {}",
        String::from_utf8_lossy(&body)
    );

    // Without a credential there is no discovery: the document names repositories of
    // an account, and an account is verified, not declared. The discovery
    // credential is the registry's (Bearer); the netrc Basic is
    // the artifact's, and does not work here.
    let (status, _, _) = socket_get(port, "/.well-known/terraform.json", None);
    assert_eq!(status, 401, "the discovery needs a credential");
    let (status, _, _) = socket_get(
        port,
        "/.well-known/terraform.json",
        Some(&format!("Basic {}", base64_of(&format!("hodei:{other}")))),
    );
    assert_eq!(
        status, 401,
        "the discovery wants the registry credential, not a netrc one"
    );
}

/// The base64 of some bytes, as written by the client that sends Basic.
fn base64_of(value: &str) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(value)
}
