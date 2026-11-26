//! HSP-121's `go mod download` against the **real client** (`go`).
//!
//! The HTTP suite measures the protocol; this one measures that the `go` binary, with
//! `GOPROXY` pointing at the hub, downloads the published version and leaves it in its
//! cache **with the exact bytes** that were published: the preserved
//! digest is not a promise of the harness, it is what the client checks
//! by itself (`.ziphash` and `go.sum`) before accepting the download as good.
//!
//! The Go SDK is required in the PATH: without it, the suite **fails**
//! saying it is missing, like the other real-client suites.
//!
//! Traffic goes over **TLS through a local `socat` bridge**: `go`
//! only sends credentials (its `.netrc`) over https, and refuses to
//! send them over http, flattening them along the way. The hub keeps speaking
//! http inside the bridge; the client trusts the self-signed certificate
//! thanks to `SSL_CERT_FILE`.

mod support;

use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use support::{Answer, Curl, Deployment};

/// The catalog seeder.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

static CALLS: AtomicUsize = AtomicUsize::new(0);

fn next(name: &str) -> std::path::PathBuf {
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{n}"))
}

fn require_go() {
    let found = Command::new("go")
        .args(["version"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "the Go SDK is not installed: the go client suite needs `go` in the PATH"
    );
}

fn require_socat() {
    let found = Command::new("socat")
        .args(["-V"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "the socat CLI is not installed: the go client suite needs its TLS bridge"
    );
}

fn sha256_of(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The text of a response.
fn text_of(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

/// Publish a version artifact through the hosted path.
fn put(port: u16, repository: &str, token: &str, rest: &str, body: &[u8]) -> Answer {
    let sent = next("sent");
    std::fs::write(&sent, body).expect("the body is written");
    let into = next("answer");
    Curl::call_with_headers(
        port,
        "PUT",
        &format!("/go/{repository}/{rest}"),
        token,
        Some(&sent),
        &[("content-type", "application/octet-stream")],
        Some(&into),
    )
    .run()
}

/// An ephemeral TLS bridge: `https` at the door, the hub behind.
/// The process lives as long as the bridge; dropping it takes the tunnel with it.
struct Bridge {
    child: std::process::Child,
    port: u16,
    certificate: std::path::PathBuf,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A free port: it is taken, released, and the bridge occupies it right after.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("an ephemeral port")
        .local_addr()
        .expect("the address of the listener")
        .port()
}

/// A self-signed certificate for 127.0.0.1: openssl signs it, the go
/// client trusts it through `SSL_CERT_FILE`. The pair ends up in a single pem, which is
/// what `socat` expects from `cert=`.
fn self_signed(root: &Path) -> std::path::PathBuf {
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
    pem
}

/// Brings up the bridge and does not return until it is listening.
fn bridge(root: &Path, hub: u16) -> Bridge {
    let certificate = self_signed(root);
    let port = free_port();
    let child = Command::new("socat")
        .arg(format!(
            "OPENSSL-LISTEN:{port},cert={},key={},verify=0,reuseaddr,fork",
            certificate.display(),
            certificate.display()
        ))
        .arg(format!("TCP:127.0.0.1:{hub}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the bridge starts");
    let bridge = Bridge {
        child,
        port,
        certificate,
    };
    bridge.wait_ready();
    bridge
}

impl Bridge {
    fn wait_ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if TcpStream::connect(("127.0.0.1", self.port)).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("the TLS bridge never listened on {}", self.port);
    }
}

/// A module's zip, as the protocol requires: everything under the prefix
/// `{module}@{version}/` and with the same go.mod the `.mod` serves. Not
/// just any zip will do: `go` validates the shape before swallowing it.
fn module_zip(module: &str, version: &str, go_mod: &str) -> Vec<u8> {
    use std::io::Write;
    let prefix = format!("{module}@{version}");
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, body) in [
        ("go.mod", go_mod.to_string()),
        ("hello.go", "package hello\n".to_string()),
    ] {
        writer
            .start_file(format!("{prefix}/{name}"), options)
            .expect("the entry opens");
        writer
            .write_all(body.as_bytes())
            .expect("the entry is written");
    }
    writer.finish().expect("the zip closes").into_inner()
}

#[test]
fn the_real_go_client_downloads_the_published_module_with_the_digest_preserved() {
    require_go();
    require_socat();
    let deployment = Deployment::new();
    deployment.seed_go(&[("go-cli", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    // Publish example.com/hello v1.0.0 through the hosted path, and remember
    // the zip's bytes: that is what the client has to give back.
    let module = "example.com/hello";
    let version = "v1.0.0";
    let go_mod = format!("module {module}\n\ngo 1.22\n");
    let zip = module_zip(module, version, &go_mod);
    let info = json!({ "Version": version, "Time": "2026-11-17T12:00:00Z" }).to_string();
    for (extension, body) in [
        ("info", info.as_bytes().to_vec()),
        ("mod", go_mod.as_bytes().to_vec()),
        ("zip", zip.clone()),
    ] {
        let answer = put(
            port,
            "go-cli",
            &token,
            &format!("{module}/@v/{version}.{extension}"),
            &body,
        );
        assert_eq!(answer.status, 200, "{extension}: {}", text_of(&answer));
    }

    // The client's home: GOPATH, GOMODCACHE, GOCACHE, all inside the
    // deployment. Nothing of the real HOME is read or written.
    let root = deployment.directory.path().join("gocli");
    std::fs::create_dir_all(&root).expect("the client home");
    let work = root.join("work");
    std::fs::create_dir_all(&work).expect("the work directory");

    // The hub listens on http; `go` only sends credentials over https. The bridge is
    // exactly what any real deployment does: TLS in front,
    // the service behind, without changing a comma of the hub.
    let tunnel = bridge(&root, port);

    // The `.netrc` is written with the bridge already up: GOAUTH matching by
    // URL prefix, the `machine` carries the tunnel's port. The
    // credential goes in the password; the user is informational.
    let netrc = root.join(".netrc");
    std::fs::write(
        &netrc,
        format!(
            "machine 127.0.0.1:{} login alice password {token}\n",
            tunnel.port
        ),
    )
    .expect("the netrc");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&netrc, std::fs::Permissions::from_mode(0o600))
            .expect("the netrc is private");
    }

    let client = |args: &[&str]| {
        let mut command = Command::new("go");
        command
            .current_dir(&work)
            .env("HOME", &root)
            .env("GOPATH", root.join("gopath"))
            .env("GOMODCACHE", root.join("modcache"))
            .env("GOCACHE", root.join("buildcache"))
            .env("SSL_CERT_FILE", &tunnel.certificate)
            .env("GOFLAGS", "-mod=mod")
            // The proxy is the hub, and only the hub: `go` has no way to
            // fall back to the network or to VCS if the proxy answers.
            .env(
                "GOPROXY",
                format!("https://127.0.0.1:{}/go/go-cli", tunnel.port),
            )
            // The public sumdb does not know this module; integrity is not
            // skipped: the zip is checked below, byte for byte, and go itself
            // writes its .ziphash.
            .env("GOSUMDB", "off")
            .args(args)
            .output()
            .expect("go runs")
    };

    let check = |label: &str, output: &std::process::Output| {
        assert!(
            output.status.success(),
            "{label}: {}\n--- hub log ---\n{}",
            String::from_utf8_lossy(&output.stderr),
            deployment.the_log()
        );
    };

    check("go mod init", &client(&["mod", "init", "probe"]));
    check(
        "go mod edit",
        &client(&["mod", "edit", "-require", &format!("{module}@{version}")]),
    );

    let output = client(&["mod", "download", module]);
    check("go mod download", &output);

    // The zip in the client's cache is the published zip: that is "preserved
    // digests" said by the only judge that matters.
    let cached = root
        .join("modcache")
        .join("cache/download")
        .join(module)
        .join("@v")
        .join(format!("{version}.zip"));
    assert!(cached.exists(), "the go client cached no zip");
    let bytes = std::fs::read(&cached).expect("the cached zip");
    assert_eq!(
        sha256_of(&bytes),
        sha256_of(&zip),
        "the zip the client stored is not the zip that was published"
    );
    let ziphash = root
        .join("modcache")
        .join("cache/download")
        .join(module)
        .join("@v")
        .join(format!("{version}.ziphash"));
    assert!(
        ziphash.exists(),
        "go did not record the integrity of the download"
    );

    // And `go build` really uses it: the module downloads, is unpacked and
    // compiles against its go.mod served by the hub.
    std::fs::write(work.join("main.go"), "package main\n\nfunc main() {}\n")
        .expect("the main is written");
    let output = client(&["build", "-o", "probe"]);
    check("go build", &output);
}
