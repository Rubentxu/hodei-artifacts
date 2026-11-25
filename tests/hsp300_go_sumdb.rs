//! HSP-300: the Go group resolves a REAL public module and its integrity is
//! checked against a trusted reference, not against itself.
//!
//! `hsp232_go_group` pins `GOSUMDB=off` and `GONOSUMDB=*` against a local lab
//! upstream, so the public checksum database never takes part. This suite
//! closes that gap with the real `go` client and the real public network:
//!
//! - the group `go-all` has one proxy member whose upstream is the real
//!   `https://proxy.golang.org/`;
//! - `github.com/google/uuid@v1.6.0` (small, stable, pinned) is resolved
//!   through the group;
//! - the sumdb policy is explicit: `GOSUMDB=sum.golang.org` (the trusted
//!   reference; `GONOSUMDB`/`GONOSUMCHECK`/`GOPRIVATE`/`GOINSECURE` are empty,
//!   so nothing is exempted). The group does not implement `/sumdb/`: the
//!   client probes `GOPROXY/sumdb/sum.golang.org/supported`, gets a 4xx and
//!   falls back to asking `sum.golang.org` directly. This is asserted below
//!   so a future change of that behaviour is noticed;
//! - the `go.sum` the client writes must equal the `h1:` hashes pinned in
//!   this file (second, independent reference);
//! - a MUTATION proves the check has teeth: a second group serves the same
//!   module path and version, but a zip with one source file tampered. The
//!   client must reject it, both with the real sumdb (no pin) and with a
//!   pinned `go.sum` and `GOSUMDB=off`.
//!
//! Without network, `go`, `socat`, `openssl` or a system CA bundle the test is
//! NOT_RUN: it prints the reason and FAILS unless `HODEI_ALLOW_NOT_RUN=1`.
//! It is never a silent PASS.

mod support;

use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use support::{Curl, Deployment};

const MODULE: &str = "github.com/google/uuid";
const VERSION: &str = "v1.6.0";
/// Pinned `h1:` of the module zip (independent reference, from sum.golang.org).
const PINNED_ZIP_H1: &str = "h1:NIvaJDMOsjHA8n1jAhLSgzrAzy1Hgr+hNrb57e+94F0=";
/// Pinned `h1:` of the module `go.mod`.
const PINNED_MOD_H1: &str = "h1:TIyPZe4MgqvfeYDBFedMoGGpEw/LqOeaOT+nhxU+yHo=";

const GROUP_REAL: &str = "go-all";
const MEMBER_REAL: &str = "go-public";
const GROUP_MUTATED: &str = "go-tampered-all";
const MEMBER_MUTATED: &str = "go-tampered";

/// The catalog seeder: `Deployment` re-runs this test binary to seed.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// NOT_RUN: visible reason, and a failure unless explicitly accepted.
fn not_run(reason: &str) {
    eprintln!("NOT_RUN: hsp300_go_sumdb: {reason}");
    assert!(
        std::env::var("HODEI_ALLOW_NOT_RUN").as_deref() == Ok("1"),
        "hsp300_go_sumdb is NOT_RUN ({reason}); set HODEI_ALLOW_NOT_RUN=1 to accept"
    );
}

fn the_go_binary() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("HODEI_GO") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Some(path);
        }
    }
    for candidate in ["go", "/home/linuxbrew/.linuxbrew/bin/go"] {
        if Command::new(candidate)
            .arg("version")
            .output()
            .is_ok_and(|output| output.status.success())
        {
            return Some(PathBuf::from(candidate));
        }
    }
    None
}

fn tool_exists(tool: &str) -> bool {
    Command::new(tool)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
        || Command::new(tool)
            .arg("version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
}

fn reachable(host: &str) -> bool {
    (host, 443)
        .to_socket_addrs()
        .ok()
        .and_then(|mut addresses| addresses.next())
        .is_some_and(|address| TcpStream::connect_timeout(&address, Duration::from_secs(5)).is_ok())
}

/// A system CA bundle: needed because the client must trust both the local
/// TLS bridge and the real `sum.golang.org`.
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

/// The reason this environment cannot run the suite, if any.
fn why_not_run() -> Option<String> {
    if the_go_binary().is_none() {
        return Some("the Go SDK is not installed".into());
    }
    for tool in ["socat", "openssl"] {
        if !tool_exists(tool) {
            return Some(format!("{tool} is not installed"));
        }
    }
    if system_ca_bundle().is_none() {
        return Some("no system CA bundle found".into());
    }
    for host in ["proxy.golang.org", "sum.golang.org"] {
        if !reachable(host) {
            return Some(format!("{host}:443 is not reachable (no network)"));
        }
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
    // The real sum.golang.org must stay verifiable, so SSL_CERT_FILE is the
    // system bundle plus the bridge certificate, not the bridge alone.
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

/// The sumdb policy of one client run.
#[derive(Clone, Copy)]
enum Sumdb {
    /// The trusted public reference, queried for real.
    Public,
    /// Off: only a pinned `go.sum` verifies.
    OffPinned,
}

/// A `go` client with its own HOME, module cache and GOPATH, all temporary.
struct Client {
    root: PathBuf,
    work: PathBuf,
    binary: PathBuf,
    port: u16,
    bundle: PathBuf,
    group: &'static str,
    sumdb: Sumdb,
    log: PathBuf,
}

impl Client {
    fn new(
        deployment: &Deployment,
        tunnel: &Bridge,
        token: &str,
        name: &str,
        group: &'static str,
        sumdb: Sumdb,
    ) -> Self {
        let root = deployment.directory.path().join(name);
        let work = root.join("work");
        std::fs::create_dir_all(&work).expect("the work directory");
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
        Self {
            root,
            work,
            binary: the_go_binary().expect("checked by the guard"),
            port: tunnel.port,
            bundle: tunnel.bundle.clone(),
            group,
            sumdb,
            log: deployment.log_path(),
        }
    }

    /// Explicit policy, every knob justified:
    ///
    /// - `GOPROXY` is only the hub (no `,direct`): the module bytes can only
    ///   come through the group.
    /// - `GOSUMDB` is `sum.golang.org` (trusted reference) or `off` with a
    ///   pinned go.sum; `GONOSUMDB`, `GONOSUMCHECK`, `GOINSECURE` and
    ///   `GOPRIVATE` are empty so no module is exempted from verification.
    /// - `GOTOOLCHAIN=local` stops the client from downloading a toolchain.
    /// - `GOFLAGS=-mod=mod` lets the probe module write its go.sum.
    fn run(&self, args: &[&str]) -> std::process::Output {
        let sumdb = match self.sumdb {
            Sumdb::Public => "sum.golang.org",
            Sumdb::OffPinned => "off",
        };
        Command::new(&self.binary)
            .current_dir(&self.work)
            .env("HOME", &self.root)
            .env("GOPATH", self.root.join("gopath"))
            .env("GOMODCACHE", self.root.join("modcache"))
            .env("GOCACHE", self.root.join("buildcache"))
            .env("SSL_CERT_FILE", &self.bundle)
            .env("GOFLAGS", "-mod=mod")
            .env("GOTOOLCHAIN", "local")
            .env("GOSUMDB", sumdb)
            .env("GONOSUMDB", "")
            .env("GONOSUMCHECK", "")
            .env("GOINSECURE", "")
            .env("GOPRIVATE", "")
            .env("GONOPROXY", "")
            .env(
                "GOPROXY",
                format!("https://127.0.0.1:{}/go/{}", self.port, self.group),
            )
            .args(args)
            .output()
            .expect("go runs")
    }

    fn ok(&self, label: &str, args: &[&str]) -> std::process::Output {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "{label}: {}\n--- hub log ---\n{}",
            String::from_utf8_lossy(&output.stderr),
            std::fs::read_to_string(&self.log).unwrap_or_default()
        );
        output
    }

    /// `go mod init` + `-require MODULE@VERSION`.
    fn prepare(&self) {
        self.ok("go mod init", &["mod", "init", "probe"]);
        self.ok(
            "go mod edit",
            &["mod", "edit", "-require", &format!("{MODULE}@{VERSION}")],
        );
    }

    fn pin_sum(&self) {
        std::fs::write(
            self.work.join("go.sum"),
            format!(
                "{MODULE} {VERSION} {PINNED_ZIP_H1}\n{MODULE} {VERSION}/go.mod {PINNED_MOD_H1}\n"
            ),
        )
        .expect("the pinned go.sum");
    }

    fn the_sum(&self) -> String {
        std::fs::read_to_string(self.work.join("go.sum")).unwrap_or_default()
    }

    fn cached(&self, extension: &str) -> PathBuf {
        self.root
            .join("modcache/cache/download")
            .join(MODULE)
            .join("@v")
            .join(format!("{VERSION}.{extension}"))
    }
}

/// Everything under `dir`, as (relative path with `/`, bytes), sorted.
fn tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(base: &Path, dir: &Path, into: &mut Vec<(String, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).expect("read dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                walk(base, &path, into);
            } else {
                let relative = path
                    .strip_prefix(base)
                    .expect("inside the base")
                    .to_string_lossy()
                    .replace('\\', "/");
                into.push((relative, std::fs::read(&path).expect("read file")));
            }
        }
    }
    let mut files = Vec::new();
    walk(dir, dir, &mut files);
    files.sort();
    files
}

/// The module zip rebuilt from the extracted tree, with ONE source file
/// tampered. Same module path and version, different bytes.
fn tampered_zip(extracted: &Path) -> Vec<u8> {
    use std::io::Write;
    let prefix = format!("{MODULE}@{VERSION}");
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut tampered = false;
    for (relative, mut bytes) in tree(extracted) {
        if relative == "uuid.go" {
            bytes.extend_from_slice(b"\n// tampered by the HSP-300 mutation\n");
            tampered = true;
        }
        writer
            .start_file(format!("{prefix}/{relative}"), options)
            .expect("zip entry");
        writer.write_all(&bytes).expect("zip bytes");
    }
    assert!(tampered, "uuid.go must exist in the extracted module");
    writer.finish().expect("the zip closes").into_inner()
}

fn real_proxy() -> Value {
    json!({
        "upstream": {
            "id": "public-go-proxy",
            "url": "https://proxy.golang.org/",
            "scopes": ["/"],
            "max_bytes": 16_777_216,
        },
        "cache": { "ttl": { "metadata_secs": 3600 } },
    })
}

fn lab_proxy(upstream: &support::lab::Upstream) -> Value {
    json!({
        "upstream": {
            "id": "lab-tampered",
            "url": format!("http://127.0.0.1:{}/", upstream.port()),
            "scopes": ["/"],
            "max_bytes": 16_777_216,
        },
        "cache": { "ttl": { "metadata_secs": 3600 } },
    })
}

fn get(port: u16, path: &str, token: &str, into: &Path) -> support::Answer {
    Curl::call_with_headers(port, "GET", path, token, None, &[], Some(into)).run()
}

#[test]
fn a_real_public_module_through_the_group_is_verified_against_the_public_sumdb() {
    if let Some(reason) = why_not_run() {
        not_run(&reason);
        return;
    }

    // The mutated member is a lab upstream that serves the real module path
    // and version with different zip bytes. It is filled after the positive
    // run (the real bytes are needed), but the repositories are seeded first.
    let upstream = support::lab::Upstream::start();
    let deployment = Deployment::new();
    deployment.seed_go_proxies(&[
        (MEMBER_REAL, real_proxy()),
        (MEMBER_MUTATED, lab_proxy(&upstream)),
    ]);
    deployment.seed_go_group(GROUP_REAL, &[MEMBER_REAL]);
    deployment.seed_go_group(GROUP_MUTATED, &[MEMBER_MUTATED]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let allowed = format!(
        "127.0.0.1:{};private;http,proxy.golang.org",
        upstream.port()
    );
    let service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);
    let tunnel = bridge(&deployment.directory.path().join("tls"), service.port);

    // ---- AC1 + AC2: real module, real sumdb, through the group.
    let good = Client::new(
        &deployment,
        &tunnel,
        &token,
        "good",
        GROUP_REAL,
        Sumdb::Public,
    );
    good.prepare();
    good.ok("go mod download (real sumdb)", &["mod", "download", MODULE]);
    let sum = good.the_sum();
    assert!(
        sum.contains(&format!("{MODULE} {VERSION} {PINNED_ZIP_H1}"))
            && sum.contains(&format!("{MODULE} {VERSION}/go.mod {PINNED_MOD_H1}")),
        "the go.sum written from the group must equal the pinned reference: {sum:?}"
    );
    // The bytes really came through Hodei, not around it.
    let scratch = deployment.directory.path().join("probe-answers");
    std::fs::create_dir_all(&scratch).expect("scratch");
    let served = get(
        service.port,
        &format!("/go/{GROUP_REAL}/{MODULE}/@v/{VERSION}.zip"),
        &token,
        &scratch.join("zip"),
    );
    assert_eq!(served.status, 200, "the group serves the zip");
    assert_eq!(
        served.body,
        std::fs::read(good.cached("zip")).expect("the cached zip"),
        "the client zip is the one the group serves"
    );

    // What the group does with /sumdb/ today: not served (404/410 makes the
    // client fall back to asking sum.golang.org directly).
    let supported = get(
        service.port,
        &format!("/go/{GROUP_REAL}/sumdb/sum.golang.org/supported"),
        &token,
        &scratch.join("supported"),
    );
    eprintln!("sumdb probe on the group: status {}", supported.status);
    assert!(
        matches!(supported.status, 404 | 410),
        "the group is expected to leave /sumdb/ to the client (404/410), got {}",
        supported.status
    );

    // ---- Mutation: same path and version, one source file tampered.
    let extracted = good
        .root
        .join("modcache")
        .join(format!("{MODULE}@{VERSION}"));
    let zip = tampered_zip(&extracted);
    let real_zip = std::fs::read(good.cached("zip")).expect("real zip");
    assert_ne!(zip, real_zip, "the mutation must change the bytes");
    let base = format!("{MODULE}/@v/{VERSION}");
    upstream.put(
        &format!("{base}.info"),
        &std::fs::read(good.cached("info")).expect("info"),
        None,
    );
    upstream.put(
        &format!("{base}.mod"),
        &std::fs::read(good.cached("mod")).expect("mod"),
        None,
    );
    upstream.put(&format!("{base}.zip"), &zip, None);

    // Sensitivity control: with every check off, the tampered module IS
    // served and accepted. Without this the rejections below prove nothing.
    let control = Client::new(
        &deployment,
        &tunnel,
        &token,
        "control",
        GROUP_MUTATED,
        Sumdb::OffPinned,
    );
    control.prepare();
    let accepted = control.run(&["mod", "download", MODULE]);
    assert!(
        accepted.status.success(),
        "control (no pin, no sumdb) must accept the tampered module: {}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    assert!(
        !control.the_sum().contains(PINNED_ZIP_H1),
        "the control go.sum must not carry the real zip hash"
    );

    // Mutation 1: real sumdb, empty go.sum -> sum.golang.org rejects.
    let by_sumdb = Client::new(
        &deployment,
        &tunnel,
        &token,
        "by-sumdb",
        GROUP_MUTATED,
        Sumdb::Public,
    );
    by_sumdb.prepare();
    let rejected = by_sumdb.run(&["mod", "download", MODULE]);
    let stderr = String::from_utf8_lossy(&rejected.stderr).into_owned();
    assert!(
        !rejected.status.success(),
        "the tampered module must be rejected by sum.golang.org"
    );
    assert!(
        stderr.contains("SECURITY ERROR") || stderr.contains("checksum mismatch"),
        "expected a checksum rejection from the sumdb, got: {stderr}"
    );
    assert!(
        !by_sumdb
            .the_sum()
            .contains(&format!("{MODULE} {VERSION} h1:")),
        "a rejected module must not be recorded in go.sum"
    );

    // Mutation 2: sumdb off, pinned go.sum -> the pin rejects.
    let by_pin = Client::new(
        &deployment,
        &tunnel,
        &token,
        "by-pin",
        GROUP_MUTATED,
        Sumdb::OffPinned,
    );
    by_pin.prepare();
    by_pin.pin_sum();
    let rejected = by_pin.run(&["mod", "download", MODULE]);
    let stderr = String::from_utf8_lossy(&rejected.stderr).into_owned();
    assert!(
        !rejected.status.success(),
        "the tampered module must be rejected by the pinned go.sum"
    );
    assert!(
        stderr.contains("checksum mismatch") || stderr.contains("SECURITY ERROR"),
        "expected a checksum mismatch from the pin, got: {stderr}"
    );
}
