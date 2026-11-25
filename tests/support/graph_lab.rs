//! The podman laboratory of the graph profile: a real Dgraph Zero+Alpha
//! (the `deploy/dgraph/compose.yaml` mapping, image pinned by digest, internal
//! network, telemetry off, ACL on) and a LocalStack S3.
//!
//! Shared by `hsp237_profile_external_e2e` (cargo binary) and
//! `hsp308_oci_graph_profile` (the OCI image) so the lab is defined once.
//! Everything created is removed on drop, even when a step panics.

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

/// Dgraph image of `deploy/dgraph/compose.yaml`, pinned by tag and digest.
pub const DGRAPH_IMAGE: &str = "docker.io/dgraph/dgraph:v25.5.1@sha256:f7459f391921c94db78f8205de9b4624210e2396c6b4ef5736838a906a38becb";
/// S3 image of the HSP-138 series.
pub const S3_IMAGE: &str = "docker.io/localstack/localstack:4.14";
/// The ACL guardian of the lab Dgraph (the default of a fresh cluster).
pub const DGRAPH_USER: &str = "groot";
pub const DGRAPH_PASSWORD: &str = "password";

/// Run `podman` with the default (rootless) store; stdout on success.
pub fn podman(args: &[&str]) -> Result<String, String> {
    let out = Command::new("podman")
        .args(args)
        .output()
        .map_err(|error| format!("podman is not available: {error}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "podman {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

/// One `curl` GET; returns the status code and the body.
pub fn http(port: u16, path: &str) -> (u16, String) {
    let out = Command::new("curl")
        .args([
            "-s",
            "-m",
            "10",
            "-w",
            "\n%{http_code}",
            &format!("http://127.0.0.1:{port}{path}"),
        ])
        .output()
        .expect("curl runs");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let (body, code) = text.rsplit_once('\n').unwrap_or(("", "0"));
    (code.trim().parse().unwrap_or(0), body.to_string())
}

pub fn wait_until(what: &str, seconds: u64, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    panic!("timed out waiting for {what}");
}

/// Everything created for the run; removed on drop even if a step panics.
pub struct Lab {
    /// Prefix of every resource name (`hsp237`, `hodei-hsp308`, ...).
    pub prefix: String,
    pub suffix: String,
    pub network: String,
    pub volumes: Vec<String>,
    pub containers: Vec<String>,
    pub secret_dir: PathBuf,
}

impl Lab {
    pub fn new(prefix: &str) -> Self {
        let suffix = format!("{}", std::process::id());
        Self {
            network: format!("{prefix}-net-{suffix}"),
            prefix: prefix.to_string(),
            suffix,
            volumes: Vec::new(),
            containers: Vec::new(),
            secret_dir: std::env::temp_dir(),
        }
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        for container in &self.containers {
            let _ = podman(&["rm", "-f", "-t", "0", container]);
        }
        for volume in &self.volumes {
            let _ = podman(&["volume", "rm", "-f", volume]);
        }
        let _ = podman(&["network", "rm", "-f", &self.network]);
        let _ = std::fs::remove_dir_all(&self.secret_dir);
    }
}

/// A running Dgraph lab.
pub struct Dgraph {
    /// Container name of the alpha (to stop and start it).
    pub alpha: String,
    pub http_port: u16,
    pub grpc_port: u16,
}

/// A running S3 lab.
pub struct S3 {
    pub container: String,
    pub port: u16,
    pub endpoint: String,
}

/// Start Zero and Alpha (ACL on) and wait for the alpha to be healthy.
pub fn start_dgraph(lab: &mut Lab) -> Dgraph {
    let (prefix, suffix) = (lab.prefix.clone(), lab.suffix.clone());
    lab.secret_dir = std::env::temp_dir().join(format!("{prefix}-secret-{suffix}"));
    std::fs::create_dir_all(&lab.secret_dir).unwrap();
    let hmac = lab.secret_dir.join("hmac-secret");
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut bytes))
        .expect("random bytes");
    std::fs::write(&hmac, bytes).unwrap();
    podman(&["network", "create", "--internal", &lab.network]).expect("the internal network");
    let zero = format!("{prefix}-zero-{suffix}");
    let alpha = format!("{prefix}-alpha-{suffix}");
    for name in ["zero", "alpha"] {
        let volume = format!("{prefix}-{name}-data-{suffix}");
        podman(&["volume", "create", &volume]).expect("the volume");
        lab.volumes.push(volume);
    }
    let http_port = super::free_port();
    let grpc_port = super::free_port();
    lab.containers.push(zero.clone());
    podman(&[
        "run",
        "-d",
        "--name",
        &zero,
        "--hostname",
        "dgraph-zero",
        "--network",
        &lab.network,
        "-v",
        &format!("{prefix}-zero-data-{suffix}:/dgraph-data"),
        DGRAPH_IMAGE,
        "dgraph",
        "zero",
        "--my=dgraph-zero:5080",
        "--wal=/dgraph-data/zero",
        "--telemetry",
        "reports=false",
    ])
    .expect("zero starts");
    lab.containers.push(alpha.clone());
    podman(&[
        "run",
        "-d",
        "--name",
        &alpha,
        "--hostname",
        "dgraph-alpha",
        "--network",
        &lab.network,
        "-p",
        &format!("127.0.0.1:{http_port}:8080"),
        "-p",
        &format!("127.0.0.1:{grpc_port}:9080"),
        "-v",
        &format!("{prefix}-alpha-data-{suffix}:/dgraph-data"),
        "-v",
        &format!("{}:/dgraph/secrets/hmac-secret:ro,z", hmac.display()),
        DGRAPH_IMAGE,
        "dgraph",
        "alpha",
        "--my=dgraph-alpha:7080",
        "--zero=dgraph-zero:5080",
        "--postings=/dgraph-data/p",
        "--wal=/dgraph-data/w",
        "--acl",
        "secret-file=/dgraph/secrets/hmac-secret; access-ttl=24h; refresh-ttl=72h; jwt-alg=HS256",
        // The login is only accepted from loopback by default; the host
        // reaches the published port through the podman gateway, so the lab
        // (ephemeral, internal network, random port) whitelists every source.
        "--security",
        "whitelist=0.0.0.0/0",
        "--telemetry",
        "reports=false",
    ])
    .expect("alpha starts");
    wait_until("dgraph alpha /health", 90, || {
        let (code, body) = http(http_port, "/health");
        code == 200 && body.contains("healthy")
    });
    Dgraph {
        alpha,
        http_port,
        grpc_port,
    }
}

/// Start LocalStack and wait until its S3 is available.
pub fn start_s3(lab: &mut Lab) -> S3 {
    let container = format!("{}-s3-{}", lab.prefix, lab.suffix);
    let port = super::free_port();
    lab.containers.push(container.clone());
    podman(&[
        "run",
        "-d",
        "--name",
        &container,
        "-p",
        &format!("127.0.0.1:{port}:4566"),
        S3_IMAGE,
    ])
    .expect("s3 starts");
    let s3 = S3 {
        container,
        port,
        endpoint: format!("http://127.0.0.1:{port}"),
    };
    s3.wait_available();
    s3
}

impl S3 {
    /// Wait until LocalStack reports its S3 as available (also after a restart).
    pub fn wait_available(&self) {
        wait_until("localstack s3", 120, || {
            let (code, body) = http(self.port, "/_localstack/health");
            code == 200
                && (body.contains("\"s3\": \"available\"") || body.contains("\"s3\": \"running\""))
        });
    }

    /// Create a bucket (LocalStack is in-memory: recreate it after a restart).
    pub async fn make_bucket(&self, bucket: &str) {
        let sdk = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new("us-east-1"))
            .credentials_provider(aws_credential_types::Credentials::from_keys(
                "test", "test", None,
            ))
            .endpoint_url(self.endpoint.clone())
            .load()
            .await;
        let client = aws_sdk_s3::Client::from_conf(
            aws_sdk_s3::config::Builder::from(&sdk)
                .force_path_style(true)
                .build(),
        );
        client
            .create_bucket()
            .bucket(bucket)
            .send()
            .await
            .expect("the bucket is created");
    }
}
