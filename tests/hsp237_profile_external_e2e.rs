//! HSP-237: functional walk of the external profile (Dgraph lab + S3).
//!
//! This suite closes the audit gap "readiness/liveness are tested, but never
//! against a deployment that has an external Dgraph and an external S3 at
//! hand". It brings up, with podman, the very Dgraph Zero+Alpha of
//! `deploy/dgraph/compose.yaml` (image pinned by digest, internal network,
//! telemetry off, ACL on) and a LocalStack S3 (the same image as the HSP-138
//! series), starts the real binary on the S3 content backend, and measures
//! `/health/ready` and `/health/live` over HTTP while the dependencies come
//! and go.
//!
//! # What is real and what is not (no invented PASS)
//!
//! - **S3 is a required dependency** of the profile: the probe issues one
//!   `HeadBucket`. Stopping the S3 container flips the readiness to 503 and
//!   restoring it brings the 200 back, while the liveness stays 200.
//! - **Dgraph is NOT wired into the binary yet** (`database.db_type` accepts
//!   only `rocksdb`, see `backend_selection_failclosed`). So the readiness
//!   probe does not ask Dgraph anything. This suite proves exactly that: the
//!   Dgraph lab is up and healthy next to the service, the service refuses
//!   `db_type = "dgraph"` at startup (fail closed), and stopping the Dgraph
//!   lab does not change the readiness. When the Dgraph catalog lands, the
//!   "dgraph stopped" step must change expectation to 503.
//! - The probe network budget is measured on the S3 container log: a burst of
//!   readiness calls inside one verdict TTL costs at most one `HeadBucket`
//!   and zero listings.
//!
//! # Guard
//!
//! The suite needs podman and the images. It runs only with
//! `HSP237_PROFILE_E2E=1` (set by `scripts/hsp237-profile-e2e.sh`); without
//! it the test prints why it skipped and passes vacuously, like the
//! `HODEI_GRAPH_IT_ENDPOINT` suites. Evidence is written to
//! `target/uat/hsp237-profile-e2e/` (gitignored).

mod support;

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use support::Deployment;

/// Dgraph image of `deploy/dgraph/compose.yaml`, pinned by tag and digest.
const DGRAPH_IMAGE: &str = "docker.io/dgraph/dgraph:v25.5.1@sha256:f7459f391921c94db78f8205de9b4624210e2396c6b4ef5736838a906a38becb";
/// S3 image of the HSP-138 series.
const S3_IMAGE: &str = "docker.io/localstack/localstack:4.14";
/// The credential secret; it must never show up in a body or in the log.
const SENTINEL: &str = "hsp237-profile-sentinel-secret-not-for-logs";

fn podman(args: &[&str]) -> Result<String, String> {
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

/// Everything created for the run; removed on drop even if a step panics.
struct Lab {
    suffix: String,
    network: String,
    volumes: Vec<String>,
    containers: Vec<String>,
    secret_dir: PathBuf,
}

impl Lab {
    fn new() -> Self {
        let suffix = format!("{}", std::process::id());
        Self {
            network: format!("hsp237-net-{suffix}"),
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

fn http(port: u16, path: &str) -> (u16, String) {
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

fn wait_until(what: &str, seconds: u64, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    panic!("timed out waiting for {what}");
}

/// Wait out the one-second verdict cache so a probe measures the deployment.
fn past_the_cache() {
    std::thread::sleep(Duration::from_millis(1600));
}

fn count_in_log(container: &str, needle: &str) -> usize {
    podman(&["logs", container])
        .map(|log| log.matches(needle).count())
        .unwrap_or(0)
}

#[tokio::test]
async fn the_external_profile_readiness_walk() {
    if std::env::var("HSP237_PROFILE_E2E").as_deref() != Ok("1") {
        eprintln!(
            "SKIPPED: set HSP237_PROFILE_E2E=1 (scripts/hsp237-profile-e2e.sh) to run the podman walk"
        );
        return;
    }
    let evidence_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/uat/hsp237-profile-e2e");
    std::fs::create_dir_all(&evidence_dir).expect("the evidence directory");
    let mut evidence = String::from("# HSP-237 external profile walk\n\n");
    let mut step = |line: &str| {
        eprintln!("{line}");
        let _ = writeln!(evidence, "- {line}");
    };

    let mut lab = Lab::new();
    let suffix = lab.suffix.clone();

    // ---- Dgraph lab (deploy/dgraph/compose.yaml, raw podman mapping).
    lab.secret_dir = std::env::temp_dir().join(format!("hsp237-secret-{suffix}"));
    std::fs::create_dir_all(&lab.secret_dir).unwrap();
    let hmac = lab.secret_dir.join("hmac-secret");
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut bytes))
        .expect("random bytes");
    std::fs::write(&hmac, bytes).unwrap();
    podman(&["network", "create", "--internal", &lab.network]).expect("the internal network");
    let zero = format!("hsp237-zero-{suffix}");
    let alpha = format!("hsp237-alpha-{suffix}");
    let s3 = format!("hsp237-s3-{suffix}");
    for name in ["zero", "alpha"] {
        let volume = format!("hsp237-{name}-data-{suffix}");
        podman(&["volume", "create", &volume]).expect("the volume");
        lab.volumes.push(volume);
    }
    let alpha_port = support::free_port();
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
        &format!("hsp237-zero-data-{suffix}:/dgraph-data"),
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
        &format!("127.0.0.1:{alpha_port}:8080"),
        "-v",
        &format!("hsp237-alpha-data-{suffix}:/dgraph-data"),
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
        "--telemetry",
        "reports=false",
    ])
    .expect("alpha starts");
    wait_until("dgraph alpha /health", 90, || {
        let (code, body) = http(alpha_port, "/health");
        code == 200 && body.contains("healthy")
    });
    step(&format!(
        "dgraph lab healthy (alpha HTTP on 127.0.0.1:{alpha_port}, image {DGRAPH_IMAGE})"
    ));

    // ---- S3 (LocalStack), fixed port so a stop/start keeps the endpoint.
    let s3_port = support::free_port();
    lab.containers.push(s3.clone());
    podman(&[
        "run",
        "-d",
        "--name",
        &s3,
        "-p",
        &format!("127.0.0.1:{s3_port}:4566"),
        S3_IMAGE,
    ])
    .expect("s3 starts");
    let endpoint = format!("http://127.0.0.1:{s3_port}");
    wait_until("localstack s3", 120, || {
        let (code, body) = http(s3_port, "/_localstack/health");
        code == 200
            && (body.contains("\"s3\": \"available\"") || body.contains("\"s3\": \"running\""))
    });
    let bucket = format!("hsp237-profile-{suffix}");
    let make_bucket = |endpoint: String, bucket: String| async move {
        let sdk = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new("us-east-1"))
            .credentials_provider(aws_credential_types::Credentials::from_keys(
                "test", "test", None,
            ))
            .endpoint_url(endpoint)
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
    };
    make_bucket(endpoint.clone(), bucket.clone()).await;
    step(&format!(
        "s3 lab up with bucket {bucket} (image {S3_IMAGE})"
    ));

    // ---- The real binary on the S3 profile.
    let deployment = Deployment::new();
    let environment: Vec<(String, String)> = vec![
        ("HODEI_CONTENT__BACKEND".into(), "s3".into()),
        ("HODEI_CONTENT__S3__BUCKET".into(), bucket.clone()),
        ("HODEI_CONTENT__S3__ENDPOINT".into(), endpoint.clone()),
        ("HODEI_CONTENT__S3__REGION".into(), "us-east-1".into()),
        ("AWS_ACCESS_KEY_ID".into(), "test".into()),
        ("AWS_SECRET_ACCESS_KEY".into(), SENTINEL.into()),
    ];
    let refs: Vec<(&str, &str)> = environment
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let service = deployment.start(&refs);
    let port = service.port;

    // 1. Both available: ready and live are 200.
    past_the_cache();
    let (ready, ready_body) = http(port, "/health/ready");
    let (live, live_body) = http(port, "/health/live");
    step(&format!(
        "both available: GET /health/ready -> {ready} {ready_body}; GET /health/live -> {live} {live_body}"
    ));
    assert_eq!(
        ready, 200,
        "ready with Dgraph and S3 available: {ready_body}"
    );
    assert_eq!(live, 200, "live with Dgraph and S3 available");

    // 2. Probe budget: a burst inside one TTL costs at most one HeadBucket, no listing.
    past_the_cache();
    let heads_before = count_in_log(&s3, "HeadBucket");
    let lists_before = count_in_log(&s3, "ListObjects");
    for _ in 0..20 {
        assert_eq!(http(port, "/health/ready").0, 200);
    }
    std::thread::sleep(Duration::from_secs(1));
    let heads = count_in_log(&s3, "HeadBucket") - heads_before;
    let lists = count_in_log(&s3, "ListObjects") - lists_before;
    step(&format!(
        "probe budget: 20 readiness calls inside one verdict TTL -> {heads} HeadBucket, {lists} ListObjects on the S3 log"
    ));
    assert!(
        heads <= 2,
        "20 sondeos seguidos deben costar como mucho una comprobacion (2 con borde de TTL), got {heads}"
    );
    assert_eq!(lists, 0, "the probe must not walk the catalog of objects");

    // 3. Stop Dgraph: the profile does not require it yet, readiness is unchanged.
    podman(&["stop", "-t", "0", &alpha]).expect("alpha stops");
    past_the_cache();
    let (ready, _) = http(port, "/health/ready");
    let (live, _) = http(port, "/health/live");
    step(&format!(
        "dgraph stopped: ready -> {ready}; live -> {live} (Dgraph is not a required dependency of the binary yet)"
    ));
    assert_eq!(ready, 200);
    assert_eq!(live, 200);
    podman(&["start", &alpha]).expect("alpha restarts");

    // 4. Stop S3: readiness leaves 200, liveness stays 200, body leaks nothing.
    podman(&["stop", "-t", "0", &s3]).expect("s3 stops");
    past_the_cache();
    let (ready, ready_body) = http(port, "/health/ready");
    let (live, live_body) = http(port, "/health/live");
    step(&format!(
        "s3 stopped: ready -> {ready} {ready_body}; live -> {live} {live_body}"
    ));
    assert_eq!(
        ready, 503,
        "ready must not be 200 with the S3 down: {ready_body}"
    );
    assert_eq!(live, 200, "live must stay 200 with the S3 down");
    for body in [&ready_body, &live_body] {
        for forbidden in [
            SENTINEL,
            "AWS_",
            "127.0.0.1",
            "localhost",
            "http://",
            "https://",
            bucket.as_str(),
            "secret",
        ] {
            assert!(
                !body.contains(forbidden),
                "the body leaks {forbidden:?}: {body}"
            );
        }
    }

    // 5. Restore S3 (LocalStack is in-memory: recreate the bucket as an operator would).
    podman(&["start", &s3]).expect("s3 restarts");
    wait_until("localstack s3 again", 120, || {
        let (code, body) = http(s3_port, "/_localstack/health");
        code == 200
            && (body.contains("\"s3\": \"available\"") || body.contains("\"s3\": \"running\""))
    });
    make_bucket(endpoint.clone(), bucket.clone()).await;
    wait_until("readiness 200 again", 30, || {
        http(port, "/health/ready").0 == 200
    });
    let (ready, ready_body) = http(port, "/health/ready");
    step(&format!("s3 restored: ready -> {ready} {ready_body}"));
    assert_eq!(ready, 200);

    // 6. The service log never repeats the secret.
    let log = deployment.the_log();
    assert!(
        !log.contains(SENTINEL),
        "the service log carries the secret"
    );
    step("service log carries no credential secret");

    // 7. Dgraph catalog is refused fail-closed (so it cannot be a probed dependency yet).
    let refusal = Command::new(support::BIN)
        .env("HODEI_DATABASE__DB_TYPE", "dgraph")
        .env("HODEI_SERVER__HOST", "127.0.0.1")
        .env("HODEI_SERVER__PORT", support::free_port().to_string())
        .env(
            "HODEI_ROCKSDB__PATH",
            deployment.directory.path().join("refusal-db"),
        )
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&refusal.stdout),
        String::from_utf8_lossy(&refusal.stderr)
    );
    step(&format!(
        "db_type=dgraph refused at startup: exit {:?}",
        refusal.status.code()
    ));
    assert!(
        !refusal.status.success() && said.to_lowercase().contains("dgraph"),
        "fail-closed expected: {said}"
    );

    drop(service);
    std::fs::write(evidence_dir.join("report.md"), &evidence).expect("the evidence is written");
}
