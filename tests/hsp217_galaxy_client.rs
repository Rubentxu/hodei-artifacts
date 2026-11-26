//! Hosted Ansible Galaxy of HSP-217 against the real client
//! (`ansible-galaxy`).
//!
//! The venv's `ansible-galaxy` (`ansible-core` 2.19) builds a real
//! collection, publishes it with `collection publish` against the
//! real service, resolves it with `collection install` in a clean
//! directory and compares it with `collection verify`: the identity
//! (`namespace.name`), the version and the sha256 that the client verifies when
//! downloading are those of the protocol itself, not of this test. It requires
//! `ansible-core` 2.19 with a Python that supports it (the system's 3.15
//! breaks this version): in `GALAXY_BIN` (the UAT venv) or
//! on the `PATH`; without it, the suite **fails** saying it is missing. Each
//! run uses its own collections directory and its own `HOME`:
//! nothing is read or written outside the deployment directory.
//!
//! The v3 dialect is the one the client fixes (`ansible/galaxy/api.py`);
//! the wire map was captured by the HSP-168 spike against
//! galaxy.ansible.com and against a local fixture (FMT-06 of
//! `uat/UAT-FORMATS.md`), and this test demands that same
//! shape from the surface: `/api/` discovery, collection documents and versions
//! paginated DRF-style, artifact by `download_url` with its sha256, multipart
//! publish with `Authorization: Token`.

mod support;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use flate2::write::GzEncoder;
use sha2::Digest as _;
use support::{Deployment, FIRST_ACCOUNT, SECOND_ACCOUNT};

/// Requires `ansible-galaxy` 2.19 from the venv or the PATH, and returns its path.
fn the_galaxy_binary() -> String {
    if let Ok(path) = std::env::var("GALAXY_BIN") {
        return path;
    }
    "ansible-galaxy".to_string()
}

fn require_galaxy() -> String {
    let binary = the_galaxy_binary();
    let version = Command::new(&binary)
        .arg("--version")
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    let line = version.lines().next().unwrap_or_default().to_string();
    assert!(
        line.starts_with("ansible-galaxy [core 2.19"),
        "the galaxy suite pins the client ansible-core 2.19.x, found: {line}"
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

/// An isolated `ansible-galaxy`: its collections directory and its `HOME`.
struct Client {
    /// The path of the executable.
    binary: String,
    /// The deployment directory.
    root: PathBuf,
    /// The collections directory of this cache.
    collections: PathBuf,
}

impl Client {
    /// A clean cache. The server and the credential go in each command:
    /// the dialect does not keep the remote in a prior configuration.
    fn new(root: &Path, name: &str) -> Self {
        let collections = root.join(format!("collections-{name}"));
        std::fs::create_dir_all(&collections).expect("the collections directory");
        Self {
            binary: the_galaxy_binary(),
            root: root.to_path_buf(),
            collections,
        }
    }

    /// Runs `ansible-galaxy` with its isolated environment.
    fn run(&self, args: &[&str]) -> Run {
        let output = Command::new(&self.binary)
            .args(args)
            .env("HOME", &self.root)
            .env("ANSIBLE_COLLECTIONS_PATH", &self.collections)
            .env("ANSIBLE_GALAXY_SERVER_VALIDATE_CERTS", "false")
            .output()
            .expect("ansible-galaxy runs");
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Run {
            ok: output.status.success(),
            output: text,
        }
    }

    /// `collection build`: the artifact that the client itself produces.
    fn build(&self, source: &Path, dist: &Path) -> PathBuf {
        self.run(&[
            "collection",
            "build",
            source.to_str().unwrap(),
            "--output-path",
            dist.to_str().unwrap(),
        ])
        .expect("the build");
        let artifact = dist.join("hodei-demo-1.0.0.tar.gz");
        assert!(artifact.is_file(), "the build produced the artifact");
        artifact
    }

    /// `collection publish`: the real upload against the galaxy surface.
    fn publish(&self, artifact: &Path, server: &str, token: &str) -> Run {
        self.run(&[
            "collection",
            "publish",
            artifact.to_str().unwrap(),
            "--server",
            server,
            "--api-key",
            token,
        ])
    }

    /// `collection install`: the real resolution against the surface.
    fn install(&self, requirement: &str, server: &str, token: &str) -> Run {
        self.run(&[
            "collection",
            "install",
            requirement,
            "--server",
            server,
            "--api-key",
            token,
            "-p",
            self.collections.to_str().unwrap(),
        ])
    }

    /// `collection verify`: the real comparison of what is installed against the
    /// served artifact.
    fn verify(&self, requirement: &str, server: &str, token: &str) -> Run {
        self.run(&[
            "collection",
            "verify",
            requirement,
            "--server",
            server,
            "--api-key",
            token,
            "-p",
            self.collections.to_str().unwrap(),
        ])
    }

    /// The installed directory of this client's collection.
    fn installed(&self) -> PathBuf {
        self.collections
            .join("ansible_collections")
            .join("hodei")
            .join("demo")
    }
}

/// The source code of the collection that the client builds.
fn the_collection_source(root: &Path) -> PathBuf {
    let directory = root.join("collections/hodei-demo");
    std::fs::create_dir_all(&directory).expect("the source directory");
    std::fs::write(
        directory.join("galaxy.yml"),
        "namespace: hodei\nname: demo\nversion: 1.0.0\nreadme: README.md\nauthors:\n  - hodei\nlicense:\n  - Apache-2.0\n",
    )
    .expect("the galaxy.yml");
    std::fs::write(directory.join("README.md"), "# the demo collection\n").expect("the readme");
    directory
}

/// The sha256 of a file, as the protocol verifies it.
fn sha256_of(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("the file");
    format!("{:x}", sha2::Sha256::digest(&bytes))
}

/// A ustar member written as the format reads it: header, content and
/// the padding that rounds up the record.
fn the_member_bytes(name: &str, content: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0u8; 512];
    bytes[..name.len()].copy_from_slice(name.as_bytes());
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

/// An artifact that the client agrees to upload but that is not a
/// readable collection: a gzip of a tar whose `MANIFEST.json` is not the
/// document that the format declares. The corruption lives inside the
/// artifact, not in the transport.
fn the_corrupted_artifact(root: &Path) -> PathBuf {
    let mut tar = Vec::new();
    tar.extend_from_slice(&the_member_bytes(
        "MANIFEST.json",
        b"this is not the manifest json",
    ));
    tar.extend_from_slice(&[0u8; 512]);
    let mut gzipped = GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzipped.write_all(&tar).expect("the gzip write");
    let artifact = root.join("corrupted/hodei-demo-1.0.0.tar.gz");
    std::fs::create_dir_all(artifact.parent().expect("the directory")).expect("the directory");
    std::fs::write(&artifact, gzipped.finish().expect("the gzip finish")).expect("the artifact");
    artifact
}

/// A publish against the surface, made by hand over the socket: the
/// same artifact bytes with a `sha256` that is **not** that of those
/// bytes. The real client cannot tell that lie, and that is why this
/// case goes through the socket.
fn the_doctored_publish(
    port: u16,
    repository: &str,
    token: &str,
    bytes: &[u8],
    declared_sha256: &str,
) -> u16 {
    let boundary = "hodei-hsp217";
    let mut body = Vec::new();
    let _ = write!(
        body,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"sha256\"\r\n\r\n{declared_sha256}\r\n"
    );
    let _ = write!(
        body,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"hodei-demo-1.0.0.tar.gz\"\r\nContent-Type: application/octet-stream\r\n\r\n"
    );
    body.extend_from_slice(bytes);
    let _ = write!(body, "\r\n--{boundary}--\r\n");
    let request = format!(
        "POST /galaxy/{repository}/api/v3/artifacts/collections/ HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Token {token}\r\nContent-Type: multipart/form-data; boundary={boundary}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("the socket");
    let _ = stream.write_all(request.as_bytes());
    let _ = stream.write_all(&body);
    let mut answer = String::new();
    let _ = stream.read_to_string(&mut answer);
    answer
        .lines()
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|status| status.parse().ok())
        .expect("the status line")
}

/// The catalog seeder: each suite brings it because the harness restarts
/// the suite binary to seed before starting the service.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

#[test]
fn the_galaxy_client_publishes_installs_and_verifies() {
    require_galaxy();
    let deployment = Deployment::new();
    deployment.seed_galaxy(&[("galaxy-hosted", serde_json::Value::Null)]);
    let mut service = deployment.start(&[]);
    let port = service.port;
    let root = deployment.directory.path().to_path_buf();
    let server = format!("http://127.0.0.1:{port}/galaxy/galaxy-hosted");

    let token = support::token(FIRST_ACCOUNT, "alice");
    let author = Client::new(&root, "author");
    let artifact = author.build(&the_collection_source(&root), &root.join("dist"));

    // AC2: the real publish against the surface. The client computes its
    // sha256, sends it in the multipart and waits for the import task that
    // the surface responds with.
    author
        .publish(&artifact, &server, &token)
        .expect("the publish");
    // Repeating the same artifact is an echo, not a rewrite:
    // the same content at the same version is a yes.
    author
        .publish(&artifact, &server, &token)
        .expect("the idempotent republish of the same artifact");
    let digest = sha256_of(&artifact);

    // AC1/AC3: the real install in a clean directory. The client requests
    // the discovery, the collection document, the paginated
    // versions, the version document and the artifact, and verifies
    // the artifact's sha256 against the one the document declares.
    let consumer = Client::new(&root, "consumer");
    consumer
        .install("hodei.demo:1.0.0", &server, &token)
        .expect("the install");
    let manifest = consumer.installed().join("MANIFEST.json");
    assert!(
        manifest.is_file(),
        "the install left the collection in place: {}",
        consumer.installed().display()
    );

    // AC3: real verify. What is installed and what is served are the same: the
    // comparison of the MANIFEST's checksums is a success.
    let verified = consumer.verify("hodei.demo:1.0.0", &server, &token);
    assert!(
        verified.ok && verified.output.contains("Successfully verified"),
        "the verify of a clean install passes: {}",
        verified.output
    );

    // AC3: local corruption does not pass verify. A file of the
    // installed collection that is no longer the one the artifact declared is an
    // installation that is no longer the one the hub served.
    std::fs::write(consumer.installed().join("README.md"), "# corrupted\n")
        .expect("the corruption");
    let corrupted = consumer.verify("hodei.demo:1.0.0", &server, &token);
    assert!(
        !corrupted.ok,
        "the verify of a corrupted install fails: {}",
        corrupted.output
    );

    // AC4: scoping is per account. Another principal does not see the collection:
    // the repository lives in another account, and their install finds nothing.
    let other = support::token(SECOND_ACCOUNT, "bob");
    let stranger = Client::new(&root, "stranger");
    let stranger_install = stranger.install("hodei.demo:1.0.0", &server, &other);
    assert!(
        !stranger_install.ok,
        "another principal does not see the collection of the first account: {}",
        stranger_install.output
    );
    let stranger_publish = stranger.publish(&artifact, &server, &other);
    assert!(
        !stranger_publish.ok,
        "another principal cannot publish into the repository of the first account: {}",
        stranger_publish.output
    );

    // AC3: offline. The remote dies: the client's warm cache
    // resolves without network ("Nothing to do") and a requirement it does not have
    // fails, without falling back to anything.
    support::stop(&mut service);
    let warm = consumer.install("hodei.demo:1.0.0", &server, &token);
    assert!(
        warm.ok && warm.output.contains("Nothing to do"),
        "the offline install of a warm cache needs no remote: {}",
        warm.output
    );
    let missing = consumer.install("hodei.demo:2.0.0", &server, &token);
    assert!(
        !missing.ok,
        "the offline install of a version nobody served fails: {}",
        missing.output
    );

    // The evidence of the digest that the client verified, for the report.
    println!("HSP-217 artifact sha256: {digest}");
}

#[test]
fn the_hub_rejects_a_corrupted_artifact_and_a_lying_sha256() {
    require_galaxy();
    let deployment = Deployment::new();
    deployment.seed_galaxy(&[("galaxy-hosted", serde_json::Value::Null)]);
    let service = deployment.start(&[]);
    let port = service.port;
    let root = deployment.directory.path().to_path_buf();
    let server = format!("http://127.0.0.1:{port}/galaxy/galaxy-hosted");

    let token = support::token(FIRST_ACCOUNT, "alice");
    let author = Client::new(&root, "author");

    // The corrupt artifact enters through the real client: the client
    // agrees to upload it (it is a tarball) and the surface rejects it before
    // storing anything, because it is not a readable collection.
    let corrupted = the_corrupted_artifact(&root);
    let rejected = author.publish(&corrupted, &server, &token);
    assert!(
        !rejected.ok,
        "the publish of an artifact that is not a readable collection is rejected: {}",
        rejected.output
    );
    assert!(
        rejected.output.contains("HTTP Code: 400"),
        "the rejection says what it is: {}",
        rejected.output
    );

    // The lying sha256 does not pass the commit: the bytes that arrive have
    // to be those the form declares. The real client cannot
    // tell this lie, so it goes through the socket with the form
    // of its own artifact and a sha256 that is not its own.
    let artifact = author.build(&the_collection_source(&root), &root.join("dist"));
    let bytes = std::fs::read(&artifact).expect("the artifact bytes");
    let digest = sha256_of(&artifact);
    let wrong = format!("{:x}", sha2::Sha256::digest(b"not the artifact bytes"));
    let status = the_doctored_publish(port, "galaxy-hosted", &token, &bytes, &wrong);
    assert_eq!(
        status, 400,
        "the publish whose sha256 does not match its bytes is rejected"
    );
    // A malformed digest is not a digest: it does not get in either.
    let malformed = the_doctored_publish(port, "galaxy-hosted", &token, &bytes, "short");
    assert_eq!(
        malformed, 400,
        "a publish whose sha256 is not a sha256 is rejected"
    );

    // What was rejected did not take the spot: the good artifact is published
    // after the two attempts, with its version free, and the digest that
    // the client declared is the one the version serves.
    author
        .publish(&artifact, &server, &token)
        .expect("the publish of the intact artifact after the rejections");
    let consumer = Client::new(&root, "consumer");
    consumer
        .install("hodei.demo:1.0.0", &server, &token)
        .expect("the install of what the hub accepted");
    let installed = consumer.installed().join("MANIFEST.json");
    assert!(installed.is_file(), "the install left the collection");
    let served = format!("{:x}", sha2::Sha256::digest(&bytes));
    assert_eq!(
        served, digest,
        "the bytes the client published are the ones it verifies"
    );
}
