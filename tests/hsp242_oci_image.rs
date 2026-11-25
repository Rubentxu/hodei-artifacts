//! OCI reproducible image, declarative profiles, release packaging, and Podman
//! deployment smoke tests (HSP-242 / HCR-20).
//!
//! Verifies:
//! - AC1: `podman run` with volumes, non-root UID, `--read-only` rootfs, and
//!   declarative config starts cleanly, persists data across stop/restart,
//!   exposes the HTTP port, respects permissions, passes `/health/live` and
//!   `/health/ready`, and shuts down gracefully on `SIGTERM`. An artifact
//!   written through the Raw API before the restart must be read back with
//!   the SAME digest after it (HSP-287); the test is red when the volume does
//!   not persist the data.
//! - AC2: External Dgraph/S3 profile does not touch RocksDB paths and fails
//!   closed when the backend is unreachable; runtime image contains no build
//!   tools and no baked-in tokens/secrets.
//! - AC3: Local release packaging produces correlated tarball + OCI archive +
//!   digest + checksums for the same commit/version/arch, and clean-directory
//!   checksum verification passes and rejects tampered assets.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const REPO_ROOT: &str = env!("CARGO_MANIFEST_DIR");
const IMAGE_TAG: &str = "localhost/hodei-artifacts-api:latest";
const CONTAINER_UID: u32 = 10001;
const CONTAINER_GID: u32 = 10001;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(45);

fn podman_args() -> Vec<String> {
    let driver = std::env::var("PODMAN_STORAGE_DRIVER").unwrap_or_else(|_| "vfs".to_string());
    let root = std::env::var("PODMAN_ROOT").unwrap_or_else(|_| "/var/tmp/podman-root".to_string());
    let runroot =
        std::env::var("PODMAN_RUNROOT").unwrap_or_else(|_| "/var/tmp/podman-runroot".to_string());
    vec![
        format!("--storage-driver={driver}"),
        "--root".to_string(),
        root,
        "--runroot".to_string(),
        runroot,
    ]
}

fn podman(args: &[&str]) -> Output {
    let base = podman_args();
    Command::new("podman")
        .args(&base)
        .args(args)
        .output()
        .expect("podman command must execute")
}

fn podman_checked(args: &[&str]) -> String {
    let out = podman(args);
    if !out.status.success() {
        panic!(
            "podman {:?} failed (status {:?}):\nstdout:\n{}\nstderr:\n{}",
            args,
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    String::from_utf8_lossy(&out.stdout).into_owned()
}

struct ContainerGuard {
    name: String,
}

impl ContainerGuard {
    fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let _ = podman(&["rm", "-f", &name]);
        Self { name }
    }
}

impl Drop for ContainerGuard {
    fn drop(&mut self) {
        let _ = podman(&["rm", "-f", &self.name]);
    }
}

/// Package the release bundle + OCI image once per test binary invocation so
/// all tests inspect the exact same correlated output.
fn packaged_dist() -> &'static Path {
    static DIST: OnceLock<(TempDir, PathBuf)> = OnceLock::new();
    let (_, path) = DIST.get_or_init(|| {
        let dir = TempDir::new().expect("create temp dist directory");
        let dist_path = dir.path().to_path_buf();
        let script = Path::new(REPO_ROOT).join("scripts/package-oci.sh");
        let out = Command::new(&script)
            .arg(&dist_path)
            .current_dir(REPO_ROOT)
            .output()
            .expect("run scripts/package-oci.sh");
        assert!(
            out.status.success(),
            "scripts/package-oci.sh failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (dir, dist_path)
    });
    path.as_path()
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    listener.local_addr().expect("local addr").port()
}

fn http_get(port: u16, path: &str) -> std::io::Result<(String, String)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes())?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut status_line = String::new();
    reader.read_line(&mut status_line)?;
    let mut body = String::new();
    reader.read_to_string(&mut body)?;
    Ok((status_line, body))
}

fn wait_until_healthy(port: u16, container_name: &str) {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    while Instant::now() < deadline {
        if let Ok((status, _)) = http_get(port, "/health/live")
            && status.contains("200")
        {
            let (ready_status, _) = http_get(port, "/health/ready").expect("/health/ready request");
            assert!(
                ready_status.contains("200"),
                "expected 200 on /health/ready, got: {ready_status}"
            );
            return;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let logs = podman(&["logs", container_name]);
    panic!(
        "container {container_name} did not become healthy within {:?}.\nstdout:\n{}\nstderr:\n{}",
        STARTUP_TIMEOUT,
        String::from_utf8_lossy(&logs.stdout),
        String::from_utf8_lossy(&logs.stderr)
    );
}

fn prepare_container_data_dir(dir: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o755))
            .expect("set data dir permissions");
    }
    let uid_gid = format!("{CONTAINER_UID}:{CONTAINER_GID}");
    let dir_str = dir.to_str().expect("utf8 data dir");
    podman_checked(&["unshare", "chown", "-R", &uid_gid, dir_str]);
}

const LAB_ACCOUNT: &str = "lab";
const LAB_USER: &str = "pioneer";
const LAB_ISSUER: &str = "https://issuer.lab.hodei.local";
const LAB_AUDIENCE: &str = "hodei-artifacts-lab";
const LAB_SECRET: &str = "hsp287-ephemeral-test-secret-0123456789abcdef";
const LAB_REPOSITORY: &str = "lab-raw";

/// Mint a real HS256 token with the claims the service verifies (same shape as
/// `scripts/community-quickstart.sh`).
fn mint_token() -> String {
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs()
        + 3600;
    let claims = serde_json::json!({
        "sub": format!("hrn:hodei:iam::{LAB_ACCOUNT}:User/{LAB_USER}"),
        "account": LAB_ACCOUNT,
        "iss": LAB_ISSUER,
        "aud": LAB_AUDIENCE,
        "exp": exp,
    });
    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(LAB_SECRET.as_bytes()),
    )
    .expect("mint JWT")
}

/// Write the lab policy bundle (community template bound to the lab principal)
/// into a world-readable temp directory that is mounted read-only.
fn prepare_policy_dir() -> TempDir {
    let dir = TempDir::new().expect("create policy dir");
    let template = fs::read_to_string(Path::new(REPO_ROOT).join("config/community/policies.cedar"))
        .expect("read policy template")
        .replace("__ACCOUNT__", LAB_ACCOUNT)
        .replace("__USER__", LAB_USER);
    fs::write(dir.path().join("policies.cedar"), template).expect("write policy");
    fs::copy(
        Path::new(REPO_ROOT).join("config/community/schema.json"),
        dir.path().join("schema.json"),
    )
    .expect("copy schema");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).expect("chmod dir");
        for f in ["policies.cedar", "schema.json"] {
            fs::set_permissions(dir.path().join(f), fs::Permissions::from_mode(0o644))
                .expect("chmod file");
        }
    }
    dir
}

/// One HTTP/1.1 request; returns the status code and the raw body.
fn http_request(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    body: &[u8],
) -> (u16, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .expect("timeout");
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    stream.write_all(head.as_bytes()).expect("write head");
    stream.write_all(body).expect("write body");
    stream.flush().expect("flush");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read response");
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("response head terminator");
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("status code");
    let mut body = raw[split + 4..].to_vec();
    if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        body = dechunk(&body);
    }
    (status, body)
}

fn dechunk(mut data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let Some(eol) = data.windows(2).position(|w| w == b"\r\n") else {
            return out;
        };
        let size =
            usize::from_str_radix(String::from_utf8_lossy(&data[..eol]).trim(), 16).unwrap_or(0);
        if size == 0 {
            return out;
        }
        out.extend_from_slice(&data[eol + 2..eol + 2 + size]);
        data = &data[eol + 2 + size + 2..];
    }
}

fn sha256_digest(bytes: &[u8]) -> String {
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("sha256:{hex}")
}

/// Start the standalone image with the Raw API enabled and a real
/// authenticator + policy bundle, on the given data volume.
fn run_authenticated_container(name: &str, data_mount: &str, policy_mount: &str) -> u16 {
    let port = free_port();
    let port_map = format!("127.0.0.1:{port}:3000");
    let envs = [
        ("RUN_MODE", "standalone"),
        ("HODEI_SECURITY__RAW_API_ENABLED", "true"),
        ("HODEI_SECURITY__AUTH_JWT_SECRET", LAB_SECRET),
        ("HODEI_SECURITY__AUTH_ISSUER", LAB_ISSUER),
        ("HODEI_SECURITY__AUTH_AUDIENCE", LAB_AUDIENCE),
        (
            "HODEI_SECURITY__AUTHORIZATION_POLICY_PATH",
            "/policy/policies.cedar",
        ),
        (
            "HODEI_SECURITY__AUTHORIZATION_SCHEMA_PATH",
            "/policy/schema.json",
        ),
    ];
    let env_args: Vec<String> = envs.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let mut args: Vec<&str> = vec![
        "run",
        "-d",
        "--name",
        name,
        "--read-only",
        "--tmpfs",
        "/tmp:rw,noexec,nosuid,size=64m",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges:true",
        "-v",
        data_mount,
        "-v",
        policy_mount,
        "-p",
        &port_map,
    ];
    for e in &env_args {
        args.push("-e");
        args.push(e);
    }
    args.push(IMAGE_TAG);
    podman_checked(&args);
    wait_until_healthy(port, name);
    port
}

#[test]
fn artifact_written_before_restart_is_read_back_with_same_digest() {
    let _ = packaged_dist();

    let data_dir = TempDir::new().expect("create data temp dir");
    prepare_container_data_dir(data_dir.path());
    let data_mount = format!("{}:/app/data:rw,Z", data_dir.path().display());
    let policy_dir = prepare_policy_dir();
    let policy_mount = format!("{}:/policy:ro,Z", policy_dir.path().display());
    let token = mint_token();
    let auth = ("Authorization", format!("Bearer {token}"));

    // Deterministic, non-trivial payload so a wrong blob cannot collide.
    let payload: Vec<u8> = (0..65_536u32)
        .map(|i| (i.wrapping_mul(31) % 251) as u8)
        .collect();
    let expected = sha256_digest(&payload);
    let blob_path = format!("/repository/{LAB_REPOSITORY}/restart/payload.bin");

    let c1 = format!("hodei-hsp287-c1-{}", std::process::id());
    let _g1 = ContainerGuard::new(&c1);
    let port1 = run_authenticated_container(&c1, &data_mount, &policy_mount);

    let repo_body = format!(r#"{{"name":"{LAB_REPOSITORY}","format":"raw","mode":"hosted"}}"#);
    let (status, answer) = http_request(
        port1,
        "POST",
        "/api/v1/catalog/repositories",
        &[
            auth.clone(),
            ("Content-Type", "application/json".to_string()),
        ],
        repo_body.as_bytes(),
    );
    assert_eq!(
        status,
        201,
        "repository creation: {}",
        String::from_utf8_lossy(&answer)
    );
    let (status, answer) = http_request(
        port1,
        "PUT",
        &blob_path,
        &[
            auth.clone(),
            (
                "x-checksum-sha256",
                expected.trim_start_matches("sha256:").to_string(),
            ),
        ],
        &payload,
    );
    assert_eq!(
        status,
        201,
        "artifact publish: {}",
        String::from_utf8_lossy(&answer)
    );
    let answer: Value = serde_json::from_slice(&answer).expect("publish answer is JSON");
    assert_eq!(
        answer["digest"].as_str(),
        Some(expected.as_str()),
        "the service must report the digest of the written bytes"
    );
    let (status, before) = http_request(port1, "GET", &blob_path, std::slice::from_ref(&auth), b"");
    assert_eq!(status, 200, "read-back before the restart");
    assert_eq!(sha256_digest(&before), expected, "digest before restart");

    podman_checked(&["stop", "-t", "15", &c1]);
    podman_checked(&["rm", "-f", &c1]);

    // Same volume, brand-new container: the artifact must come back intact.
    let c2 = format!("hodei-hsp287-c2-{}", std::process::id());
    let _g2 = ContainerGuard::new(&c2);
    let port2 = run_authenticated_container(&c2, &data_mount, &policy_mount);
    let (status, after) = http_request(port2, "GET", &blob_path, std::slice::from_ref(&auth), b"");
    assert_eq!(
        status, 200,
        "artifact written before the restart must survive it (volume not persisting?)"
    );
    assert_eq!(
        sha256_digest(&after),
        expected,
        "digest after restart must equal the digest written before it"
    );
    podman_checked(&["stop", "-t", "15", &c2]);
    podman_checked(&["rm", "-f", &c2]);

    // Negative control: a fresh volume must NOT serve the artifact, proving the
    // assertions above discriminate persisted data from a lucky default.
    let empty_dir = TempDir::new().expect("create empty data dir");
    prepare_container_data_dir(empty_dir.path());
    let empty_mount = format!("{}:/app/data:rw,Z", empty_dir.path().display());
    let c3 = format!("hodei-hsp287-c3-{}", std::process::id());
    let _g3 = ContainerGuard::new(&c3);
    let port3 = run_authenticated_container(&c3, &empty_mount, &policy_mount);
    let (status, _) = http_request(port3, "GET", &blob_path, &[auth], b"");
    assert_ne!(status, 200, "a fresh volume must not serve the artifact");
    podman_checked(&["stop", "-t", "15", &c3]);
}

#[test]
fn release_packaging_correlates_tarball_oci_and_rejects_tampering() {
    let dist = packaged_dist();

    let verify_script = Path::new(REPO_ROOT).join("scripts/verify-release.sh");
    let ok_out = Command::new(&verify_script)
        .arg(dist)
        .output()
        .expect("run verify-release.sh");
    assert!(
        ok_out.status.success(),
        "verify-release.sh failed on valid bundle:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&ok_out.stdout),
        String::from_utf8_lossy(&ok_out.stderr)
    );

    let manifest_path = fs::read_dir(dist)
        .expect("read dist")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.to_string_lossy().ends_with(".manifest.json"))
        .expect("manifest.json must exist");
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read manifest"))
            .expect("parse manifest");

    assert!(
        dist.join(manifest["tarball"]["filename"].as_str().unwrap())
            .is_file(),
        "correlated tarball missing"
    );
    let oci_archive_name = manifest["oci"]["archive"].as_str().unwrap();
    let oci_archive = dist.join(oci_archive_name);
    assert!(oci_archive.is_file(), "correlated OCI archive missing");
    assert!(
        manifest["oci"]["digest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"),
        "OCI digest must start with sha256:"
    );

    // Tamper check in an isolated copy directory.
    let tampered_dir = TempDir::new().expect("temp tampered dir");
    for entry in fs::read_dir(dist)
        .expect("read dist")
        .filter_map(Result::ok)
    {
        if entry.path().is_file() {
            fs::copy(entry.path(), tampered_dir.path().join(entry.file_name()))
                .expect("copy release asset");
        }
    }
    let tampered_oci = tampered_dir.path().join(oci_archive_name);
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&tampered_oci)
        .expect("open oci archive for tampering");
    file.write_all(b"tamper").expect("append tamper byte");
    drop(file);

    let tampered_out = Command::new(&verify_script)
        .arg(tampered_dir.path())
        .output()
        .expect("run verify-release.sh on tampered bundle");
    assert!(
        !tampered_out.status.success(),
        "verify-release.sh must reject tampered assets"
    );
}

#[test]
fn oci_image_runs_as_non_root_without_build_tools_or_secrets() {
    let _ = packaged_dist();

    let uid_out = podman_checked(&[
        "run",
        "--rm",
        "--entrypoint",
        "/bin/sh",
        IMAGE_TAG,
        "-c",
        "id -u && id -g",
    ]);
    let ids: Vec<&str> = uid_out.lines().collect();
    assert_eq!(ids, vec!["10001", "10001"], "must run as UID/GID 10001");

    // Verify no build tools exist in the runtime image.
    let tools_out = podman(&[
        "run",
        "--rm",
        "--entrypoint",
        "/bin/sh",
        IMAGE_TAG,
        "-c",
        "for t in cargo rustc rustup gcc g++ cc make clang cmake pkg-config; do if command -v $t >/dev/null 2>&1; then echo FOUND:$t; exit 1; fi; done",
    ]);
    assert!(
        tools_out.status.success(),
        "build tool found in runtime image: {}",
        String::from_utf8_lossy(&tools_out.stdout)
    );

    // Verify no baked-in tokens/secrets in /app/config (ignoring comments) or container env.
    let secrets_out = podman(&[
        "run",
        "--rm",
        "--entrypoint",
        "/bin/sh",
        IMAGE_TAG,
        "-c",
        "grep -rnE '^[^#]*(auth_jwt_secret|AWS_SECRET_ACCESS_KEY|AWS_ACCESS_KEY_ID)\\s*=' /app/config || env | grep -E '^(HODEI_SECURITY__AUTH_JWT_SECRET|AWS_SECRET_ACCESS_KEY|AWS_ACCESS_KEY_ID)='",
    ]);
    assert_eq!(
        secrets_out.status.code(),
        Some(1),
        "unexpected secret pattern in /app/config: {}",
        String::from_utf8_lossy(&secrets_out.stdout)
    );
}

#[test]
fn podman_standalone_readonly_persists_across_restart_and_handles_sigterm() {
    let _ = packaged_dist();

    let data_dir = TempDir::new().expect("create data temp dir");
    prepare_container_data_dir(data_dir.path());
    let data_mount = format!("{}:/app/data:rw,Z", data_dir.path().display());

    // First boot with --read-only rootfs and non-root UID.
    let c1_name = format!("hodei-hsp242-c1-{}", std::process::id());
    let _g1 = ContainerGuard::new(&c1_name);
    let port1 = free_port();
    let port_map1 = format!("127.0.0.1:{port1}:3000");

    podman_checked(&[
        "run",
        "-d",
        "--name",
        &c1_name,
        "--read-only",
        "--tmpfs",
        "/tmp:rw,noexec,nosuid,size=64m",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges:true",
        "-v",
        &data_mount,
        "-p",
        &port_map1,
        "-e",
        "RUN_MODE=standalone",
        IMAGE_TAG,
    ]);

    wait_until_healthy(port1, &c1_name);
    assert!(
        data_dir.path().join("hodei.rocksdb").exists(),
        "RocksDB directory must be persisted in the mounted /app/data volume"
    );

    // Graceful SIGTERM shutdown via `podman stop`.
    podman_checked(&["stop", "-t", "15", &c1_name]);
    let exit1 = podman_checked(&["inspect", "--format", "{{.State.ExitCode}}", &c1_name]);
    assert_eq!(
        exit1.trim(),
        "0",
        "SIGTERM must trigger graceful shutdown (exit 0), got {}",
        exit1.trim()
    );
    let logs1 = podman_checked(&["logs", &c1_name]);
    assert!(
        logs1.contains("shut down gracefully") || logs1.contains("Received SIGTERM signal"),
        "expected graceful shutdown log in:\n{logs1}"
    );

    // Second boot reusing the exact same persisted volume.
    let c2_name = format!("hodei-hsp242-c2-{}", std::process::id());
    let _g2 = ContainerGuard::new(&c2_name);
    let port2 = free_port();
    let port_map2 = format!("127.0.0.1:{port2}:3000");

    podman_checked(&[
        "run",
        "-d",
        "--name",
        &c2_name,
        "--read-only",
        "--tmpfs",
        "/tmp:rw,noexec,nosuid,size=64m",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges:true",
        "-v",
        &data_mount,
        "-p",
        &port_map2,
        "-e",
        "RUN_MODE=standalone",
        IMAGE_TAG,
    ]);

    wait_until_healthy(port2, &c2_name);
    podman_checked(&["stop", "-t", "15", &c2_name]);
    let exit2 = podman_checked(&["inspect", "--format", "{{.State.ExitCode}}", &c2_name]);
    assert_eq!(exit2.trim(), "0", "second boot SIGTERM must exit 0");
}

#[test]
fn external_dgraph_s3_profile_fails_closed_without_touching_rocksdb() {
    let _ = packaged_dist();

    let data_dir = TempDir::new().expect("create data temp dir");
    prepare_container_data_dir(data_dir.path());
    let data_mount = format!("{}:/app/data:rw,Z", data_dir.path().display());

    // 1. RUN_MODE=external (db_type=dgraph, backend=s3) must fail closed and
    //    never create RocksDB or filesystem CAS directories.
    let out = podman(&[
        "run",
        "--rm",
        "--read-only",
        "--tmpfs",
        "/tmp:rw,noexec,nosuid,size=64m",
        "-v",
        &data_mount,
        "-e",
        "RUN_MODE=external",
        "-e",
        "HODEI_ROCKSDB__PATH=/app/data/forbidden.rocksdb",
        "-e",
        "HODEI_CONTENT__CAS_PATH=/app/data/forbidden.cas",
        IMAGE_TAG,
    ]);
    assert!(
        !out.status.success(),
        "external Dgraph/S3 profile must fail closed when unimplemented/unreachable"
    );
    assert!(
        !data_dir.path().join("forbidden.rocksdb").exists(),
        "external profile must NOT touch RocksDB path"
    );
    assert!(
        !data_dir.path().join("forbidden.cas").exists(),
        "external profile must NOT touch filesystem CAS path"
    );

    // 2. Unreachable S3 content backend must also fail closed without touching cas_path.
    let s3_out = podman(&[
        "run",
        "--rm",
        "--read-only",
        "--tmpfs",
        "/tmp:rw,noexec,nosuid,size=64m",
        "-v",
        &data_mount,
        "-e",
        "RUN_MODE=standalone",
        "-e",
        "HODEI_CONTENT__BACKEND=s3",
        "-e",
        "HODEI_CONTENT__S3__BUCKET=unreachable-bucket",
        "-e",
        "HODEI_CONTENT__S3__ENDPOINT=127.0.0.1:1",
        "-e",
        "HODEI_CONTENT__S3__REGION=eu-west-1",
        "-e",
        "HODEI_CONTENT__S3__TLS=false",
        "-e",
        "HODEI_CONTENT__CAS_PATH=/app/data/forbidden-s3.cas",
        "-e",
        "AWS_ACCESS_KEY_ID=test",
        "-e",
        "AWS_SECRET_ACCESS_KEY=test",
        IMAGE_TAG,
    ]);
    assert!(
        !s3_out.status.success(),
        "unreachable S3 backend must fail closed"
    );
    assert!(
        !data_dir.path().join("forbidden-s3.cas").exists(),
        "S3 profile must NOT fall back to or create cas_path"
    );
}

/// Second architecture (HSP-287): builds and correlates the aarch64 tarball and
/// linux/arm64 image next to the host ones, with an OCI image index.
///
/// The script is opt-in (`HODEI_TEST_AARCH64=1`) because it builds two
/// architectures. When it reports NOT_RUN (exit 77: opt-in missing or a tool
/// absent) this test fails unless `HODEI_ALLOW_NOT_RUN=1` is set, and even then
/// the NOT_RUN line is printed: it is never a silent green.
#[test]
#[ignore = "HSP-287: builds x86_64 + aarch64 (minutes); run with `HODEI_TEST_AARCH64=1 cargo test --test hsp242_oci_image -- --ignored`"]
fn second_architecture_bundle_is_built_and_correlated() {
    let script = Path::new(REPO_ROOT).join("scripts/test-release-aarch64.sh");
    let out = Command::new("bash")
        .arg(&script)
        .current_dir(REPO_ROOT)
        .output()
        .expect("run scripts/test-release-aarch64.sh");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    match out.status.code() {
        Some(0) => {}
        Some(77) => {
            eprintln!("NOT_RUN: second architecture\n{stdout}\n{stderr}");
            assert!(
                std::env::var("HODEI_ALLOW_NOT_RUN").as_deref() == Ok("1"),
                "second architecture is NOT_RUN (set HODEI_ALLOW_NOT_RUN=1 to accept):\n{stderr}"
            );
        }
        code => panic!(
            "second architecture test failed ({code:?}):\nstdout:\n{stdout}\nstderr:\n{stderr}"
        ),
    }
}
