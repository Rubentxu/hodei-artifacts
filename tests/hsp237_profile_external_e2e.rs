//! HSP-237 / HSP-292: functional walk of the graph profile (real Dgraph + S3).
//!
//! This suite closes the audit gap "readiness/liveness are tested, but never
//! against a deployment that has an external Dgraph and an external S3 at
//! hand". It brings up, with podman, the very Dgraph Zero+Alpha of
//! `deploy/dgraph/compose.yaml` (image pinned by digest, internal network,
//! telemetry off, ACL on, gRPC published on loopback) and a LocalStack S3
//! (the same image as the HSP-138 series), starts the real binary on the
//! graph profile (`RUN_MODE=dgraph`, a `[graph]` section, S3 content) and
//! measures `/health/ready` and `/health/live` over HTTP while the
//! dependencies come and go.
//!
//! # What is proven
//!
//! - **Dgraph is a required dependency** of the graph profile: the probe
//!   logs in with the ACL and runs one authenticated read-only DQL
//!   round-trip. Stopping the Alpha flips `/health/ready` to 503 (the body
//!   names `graph`) while `/health/live` stays 200, and restoring it brings
//!   the 200 back.
//! - **S3 is a required dependency** too: one `HeadBucket`. Stopping the S3
//!   container flips the readiness to 503, restoring it brings the 200
//!   back, and the liveness stays 200 throughout.
//! - No body carries the S3 secret, the endpoints, the bucket or the Dgraph
//!   credentials, and the service log never repeats the secrets.
//! - The probe network budget is measured on the S3 container log: a burst of
//!   readiness calls inside one verdict TTL costs at most one `HeadBucket`
//!   and zero listings.
//!
//! # Guard
//!
//! The suite needs podman and the images. It runs only with
//! `HSP237_PROFILE_E2E=1` (set by `scripts/hsp237-profile-e2e.sh`); without
//! it the test prints `SKIPPED` and passes vacuously, like the
//! `HODEI_GRAPH_IT_ENDPOINT` suites; the script turns that into `NOT_RUN`
//! (exit 77), never into a PASS. A boot of the graph profile that fails is a
//! real failure of the walk, not a skip. Evidence is written to
//! `target/uat/hsp237-profile-e2e/` (gitignored).

mod support;

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use support::Deployment;
use support::graph_lab::{
    DGRAPH_IMAGE, DGRAPH_PASSWORD, DGRAPH_USER, Lab, S3_IMAGE, http, podman, start_dgraph,
    start_s3, wait_until,
};

/// The credential secret; it must never show up in a body or in the log.
const SENTINEL: &str = "hsp237-profile-sentinel-secret-not-for-logs";

/// Wait out the one-second verdict cache so a probe measures the deployment.
fn past_the_cache() {
    std::thread::sleep(Duration::from_millis(1600));
}

/// The coarse body never carries a secret, an address, a bucket or a user.
fn assert_no_leak(body: &str, bucket: &str) {
    for forbidden in [
        SENTINEL,
        DGRAPH_PASSWORD,
        DGRAPH_USER,
        "AWS_",
        "127.0.0.1",
        "localhost",
        "http://",
        "https://",
        bucket,
        "secret",
    ] {
        assert!(
            !body.contains(forbidden),
            "the body leaks {forbidden:?}: {body}"
        );
    }
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
    let mut evidence = String::from("# HSP-237/HSP-292 graph profile walk\n\n");
    let mut step = |line: &str| {
        eprintln!("{line}");
        let _ = writeln!(evidence, "- {line}");
    };

    let mut lab = Lab::new("hsp237");
    let dgraph = start_dgraph(&mut lab);
    let (alpha, alpha_port, alpha_grpc_port) = (dgraph.alpha, dgraph.http_port, dgraph.grpc_port);
    step(&format!(
        "dgraph lab healthy (alpha HTTP on 127.0.0.1:{alpha_port}, image {DGRAPH_IMAGE})"
    ));

    // ---- S3 (LocalStack), fixed port so a stop/start keeps the endpoint.
    let s3_lab = start_s3(&mut lab);
    let (s3, endpoint) = (s3_lab.container.clone(), s3_lab.endpoint.clone());
    let bucket = format!("hsp237-profile-{}", lab.suffix);
    s3_lab.make_bucket(&bucket).await;
    step(&format!(
        "s3 lab up with bucket {bucket} (image {S3_IMAGE})"
    ));

    // ---- The real binary on the S3 profile.
    let deployment = Deployment::new();
    let environment: Vec<(String, String)> = vec![
        ("RUN_MODE".into(), "dgraph".into()),
        // The harness reads the bound port from a plain-text log line.
        ("HODEI_LOGGING__FORMAT".into(), "pretty".into()),
        (
            "HODEI_GRAPH__ENDPOINT".into(),
            format!("127.0.0.1:{alpha_grpc_port}"),
        ),
        ("HODEI_GRAPH__TLS__ENABLED".into(), "false".into()),
        ("HODEI_GRAPH__TLS__CA_CERT_PATH".into(), String::new()),
        ("HODEI_GRAPH__TLS__SERVER_NAME".into(), String::new()),
        ("HODEI_GRAPH__NAMESPACE".into(), "0".into()),
        (
            "HODEI_GRAPH__CREDENTIALS__USERID".into(),
            DGRAPH_USER.into(),
        ),
        (
            "HODEI_GRAPH__CREDENTIALS__PASSWORD".into(),
            DGRAPH_PASSWORD.into(),
        ),
        ("HODEI_CONTENT__BACKEND".into(), "s3".into()),
        ("HODEI_CONTENT__S3__TLS".into(), "false".into()),
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
    for dependency in ["graph", "content_store"] {
        assert!(
            ready_body.contains(dependency),
            "the graph profile must require {dependency}: {ready_body}"
        );
    }

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

    // 3. Stop Dgraph: the graph profile requires it, readiness goes to 503
    //    and the liveness stays 200; restoring it brings the 200 back.
    podman(&["stop", "-t", "0", &alpha]).expect("alpha stops");
    past_the_cache();
    let started = Instant::now();
    let (ready, ready_body) = http(port, "/health/ready");
    let elapsed = started.elapsed();
    let (live, live_body) = http(port, "/health/live");
    step(&format!(
        "dgraph stopped: ready -> {ready} {ready_body} (in {elapsed:?}); live -> {live} {live_body}"
    ));
    assert_eq!(
        ready, 503,
        "ready must not be 200 with Dgraph down: {ready_body}"
    );
    assert!(
        ready_body.contains("\"graph\"") && ready_body.contains("not_ready"),
        "the body must name the failing graph dependency: {ready_body}"
    );
    assert!(
        elapsed < Duration::from_millis(2500),
        "the readiness with Dgraph down took {elapsed:?}"
    );
    assert_eq!(live, 200, "live must stay 200 with Dgraph down");
    assert_no_leak(&ready_body, &bucket);
    assert_no_leak(&live_body, &bucket);
    podman(&["start", &alpha]).expect("alpha restarts");
    wait_until("dgraph alpha /health again", 120, || {
        let (code, body) = http(alpha_port, "/health");
        code == 200 && body.contains("healthy")
    });
    wait_until("readiness 200 after dgraph is back", 120, || {
        http(port, "/health/ready").0 == 200
    });
    past_the_cache();
    let (ready, ready_body) = http(port, "/health/ready");
    step(&format!("dgraph restored: ready -> {ready} {ready_body}"));
    assert_eq!(ready, 200, "ready must come back with Dgraph: {ready_body}");

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
    assert_no_leak(&ready_body, &bucket);
    assert_no_leak(&live_body, &bucket);

    // 5. Restore S3 (LocalStack is in-memory: recreate the bucket as an operator would).
    podman(&["start", &s3]).expect("s3 restarts");
    s3_lab.wait_available();
    s3_lab.make_bucket(&bucket).await;
    wait_until("readiness 200 again", 30, || {
        http(port, "/health/ready").0 == 200
    });
    let (ready, ready_body) = http(port, "/health/ready");
    step(&format!("s3 restored: ready -> {ready} {ready_body}"));
    assert_eq!(ready, 200);

    // 6. The service log never repeats a secret and the profile is the graph one.
    let log = deployment.the_log();
    for secret in [SENTINEL, DGRAPH_PASSWORD] {
        assert!(!log.contains(secret), "the service log carries a secret");
    }
    assert!(
        log.contains("Bootstrap completed successfully on the graph profile"),
        "the binary must have booted the graph profile:\n{log}"
    );
    step("service log carries no credential secret and announces the graph profile");

    // The archived log never carries the credentials of the lab.
    let archived = log
        .replace(SENTINEL, "__S3_SECRET_IN_ENVIRONMENT_ONLY__")
        .replace(DGRAPH_PASSWORD, "__DGRAPH_PASSWORD_IN_ENVIRONMENT_ONLY__");
    std::fs::write(evidence_dir.join("service.log"), archived).expect("the log is archived");

    drop(service);
    std::fs::write(evidence_dir.join("report.md"), &evidence).expect("the evidence is written");
}
