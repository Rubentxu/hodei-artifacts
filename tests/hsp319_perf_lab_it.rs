//! The performance lab of the graph profile (HSP-319, slice S1).
//!
//! A deliberately small, reproducible load lab: it boots the production
//! binary on `RUN_MODE=dgraph` against a REAL Dgraph alpha (the same
//! `HODEI_GRAPH_IT_*` environment as every `*_it` target), publishes a
//! parameterised number of assets through the raw surface, then reads and
//! searches them back, and reports p50/p95/p99 and throughput per workload.
//!
//! ```text
//! HODEI_GRAPH_IT_ENDPOINT=127.0.0.1:9080 HODEI_GRAPH_IT_ADMIN=127.0.0.1:8080 \
//! HODEI_GRAPH_IT_USER=groot HODEI_GRAPH_IT_PASSWORD=password \
//!   cargo test --test hsp319_perf_lab_it -- --scales 1000,10000 --compare docs/perf/baseline-graph.json
//! ```
//!
//! # What it is and what it is not
//!
//! - **Closed loop.** `--concurrency` requests are in flight at any time and a
//!   new one starts when one finishes. It measures the service under a fixed
//!   concurrency, not at a fixed arrival rate: the latencies of a saturated
//!   service are therefore optimistic (coordinated omission) and the report
//!   says so in `parameters.driver`.
//! - **Errors are never throughput.** Failed requests are counted in their own
//!   column and only successes feed `throughput_rps`. A run with errors is
//!   invalid and the process exits non-zero.
//! - **p99 needs 1000 samples.** Below that the JSON says `null` and why.
//! - **Only measured scales are claimed.** The report lists 10M and 100M as
//!   `not_run`; they belong to HSP-216 (S4).
//! - Each scale runs on a fresh ACL namespace of the alpha (the `Deployment`
//!   harness), so scales never see each other's data and the alpha may be
//!   shared. The content backend is the filesystem of the temp directory: the
//!   lab measures the graph profile, not an object store.
//!
//! # Modes
//!
//! - default: run and write the result to `--out` (`target/perf-lab/last.json`).
//! - `--record PATH`: also write the result to PATH (the versioned baseline).
//! - `--compare PATH`: compare against a baseline and exit non-zero if the p95
//!   of any shared (scale, workload) worsens by more than `--threshold-pct`
//!   AND by more than `--min-delta-ms` (the floor keeps sub-millisecond noise
//!   from failing a run), or if any request failed.
//! - `--inject-delay-ms N`: adds N ms to every measured request. It exists to
//!   prove that `--compare` fails when it must (mutation check); never record
//!   a baseline with it.
//!
//! Without `HODEI_GRAPH_IT_ENDPOINT` the target skips itself (exit 0), like the
//! other `*_it` targets, and says so.

mod support;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use futures_util::StreamExt as _;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use support::{Deployment, FIRST_ACCOUNT};

/// The schema version of the result file; bump it when the shape changes.
const SCHEMA_VERSION: u64 = 1;
/// How many samples make a p99 worth printing.
const P99_MIN_SAMPLES: usize = 1000;
/// The repository every scale publishes into.
const REPOSITORY: &str = "perf";
/// The workloads of a scale, in the order they run and are reported.
const WORKLOADS: [&str; 4] = ["publish", "read", "search_text", "search_digest"];

/// The parameters of a run.
struct Options {
    scales: Vec<usize>,
    concurrency: usize,
    samples: usize,
    payload_bytes: usize,
    out: PathBuf,
    record: Option<PathBuf>,
    compare: Option<PathBuf>,
    threshold_pct: f64,
    min_delta_ms: f64,
    inject_delay_ms: u64,
}

impl Options {
    fn parse() -> Result<Self, String> {
        let mut options = Self {
            scales: vec![1000],
            concurrency: 8,
            samples: 1000,
            payload_bytes: 1024,
            out: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/perf-lab/last.json"),
            record: None,
            compare: None,
            threshold_pct: 50.0,
            min_delta_ms: 2.0,
            inject_delay_ms: 0,
        };
        let mut args = std::env::args().skip(1);
        while let Some(flag) = args.next() {
            // The libtest flags the pipeline passes to every target are not ours.
            if flag.starts_with("--test-threads")
                || matches!(
                    flag.as_str(),
                    "--nocapture" | "--show-output" | "--quiet" | "-q"
                )
            {
                if flag == "--test-threads" {
                    args.next();
                }
                continue;
            }
            let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
            match flag.as_str() {
                "--scales" => {
                    options.scales = value("--scales")?
                        .split(',')
                        .map(|scale| {
                            scale
                                .trim()
                                .parse::<usize>()
                                .map_err(|_| format!("`{scale}` is not a scale"))
                        })
                        .collect::<Result<_, _>>()?;
                }
                "--concurrency" => options.concurrency = number(&value("--concurrency")?)?,
                "--samples" => options.samples = number(&value("--samples")?)?,
                "--payload-bytes" => options.payload_bytes = number(&value("--payload-bytes")?)?,
                "--out" => options.out = PathBuf::from(value("--out")?),
                "--record" => options.record = Some(PathBuf::from(value("--record")?)),
                "--compare" => options.compare = Some(PathBuf::from(value("--compare")?)),
                "--threshold-pct" => {
                    options.threshold_pct = value("--threshold-pct")?
                        .parse()
                        .map_err(|_| "--threshold-pct is not a number".to_string())?;
                }
                "--min-delta-ms" => {
                    options.min_delta_ms = value("--min-delta-ms")?
                        .parse()
                        .map_err(|_| "--min-delta-ms is not a number".to_string())?;
                }
                "--inject-delay-ms" => {
                    options.inject_delay_ms = number::<u64>(&value("--inject-delay-ms")?)?;
                }
                other => return Err(format!("unknown argument `{other}`")),
            }
        }
        if options.scales.is_empty() || options.scales.contains(&0) {
            return Err("every scale must be at least 1".to_string());
        }
        if options.concurrency == 0 || options.samples == 0 {
            return Err("--concurrency and --samples must be at least 1".to_string());
        }
        if options.payload_bytes < 64 {
            return Err("--payload-bytes must be at least 64".to_string());
        }
        Ok(options)
    }
}

fn number<T: std::str::FromStr>(text: &str) -> Result<T, String> {
    text.trim()
        .parse()
        .map_err(|_| format!("`{text}` is not a number"))
}

/// What one workload measured.
struct Outcome {
    latencies_ms: Vec<f64>,
    errors: usize,
    first_error: Option<String>,
    wall: Duration,
}

/// Runs `count` operations with `concurrency` in flight. The latency of each
/// one is measured around the whole operation, including the injected delay.
async fn drive<F, Fut>(count: usize, concurrency: usize, delay_ms: u64, operation: F) -> Outcome
where
    F: Fn(usize) -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    let started = Instant::now();
    let mut latencies_ms = Vec::with_capacity(count);
    let mut errors = 0usize;
    let mut first_error = None;
    let mut stream = futures_util::stream::iter(0..count)
        .map(|index| {
            let future = operation(index);
            async move {
                let begin = Instant::now();
                if delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
                let result = future.await;
                (begin.elapsed(), result)
            }
        })
        .buffer_unordered(concurrency);
    while let Some((elapsed, result)) = stream.next().await {
        match result {
            Ok(()) => latencies_ms.push(elapsed.as_secs_f64() * 1000.0),
            Err(error) => {
                errors += 1;
                first_error.get_or_insert(error);
            }
        }
    }
    Outcome {
        latencies_ms,
        errors,
        first_error,
        wall: started.elapsed(),
    }
}

/// Nearest-rank percentile over a sorted slice.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// The report of one workload.
fn summarize(outcome: &Outcome) -> Value {
    let mut sorted = outcome.latencies_ms.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let successes = sorted.len();
    let wall = outcome.wall.as_secs_f64();
    let (p50, p95, p99) = if successes == 0 {
        (Value::Null, Value::Null, Value::Null)
    } else {
        (
            json!(round3(percentile(&sorted, 50.0))),
            json!(round3(percentile(&sorted, 95.0))),
            if successes >= P99_MIN_SAMPLES {
                json!(round3(percentile(&sorted, 99.0)))
            } else {
                Value::Null
            },
        )
    };
    json!({
        "samples": successes,
        "errors": outcome.errors,
        "first_error": outcome.first_error,
        "p50_ms": p50,
        "p95_ms": p95,
        "p99_ms": p99,
        "p99_note": if successes >= P99_MIN_SAMPLES { Value::Null } else {
            json!(format!("fewer than {P99_MIN_SAMPLES} successful samples"))
        },
        "throughput_rps": if wall > 0.0 { json!(round3(successes as f64 / wall)) } else { Value::Null },
        "wall_s": round3(wall),
    })
}

/// A deterministic payload: the same index always yields the same bytes, and
/// no two indexes share a digest.
fn payload(index: usize, size: usize) -> Vec<u8> {
    let mut bytes = format!("hsp319-perf-asset-{index:08}|").into_bytes();
    let mut state = 0x9E37_79B9_7F4A_7C15u64 ^ index as u64;
    while bytes.len() < size {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        bytes.push(b'a' + (state % 26) as u8);
    }
    bytes.truncate(size);
    bytes
}

fn asset_path(index: usize) -> String {
    format!("dir{:02}/asset-{index:08}.bin", index % 64)
}

fn digest_of(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// A deterministic pseudo-random asset index below `scale` for the n-th sample.
fn sample_index(n: usize, scale: usize) -> usize {
    let mixed = (n as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 17;
    (mixed % scale as u64) as usize
}

fn the_policy() -> String {
    let mut policy = support::the_policy();
    for action in ["CreateRepository", "ReadRepository"] {
        policy.push_str(&format!(
            "permit(principal == Iam::User::\"hrn:hodei:iam::{FIRST_ACCOUNT}:User/alice\", action == Artifacts::Action::\"{action}\", resource);\n"
        ));
    }
    policy
}

/// Measures one scale on a fresh namespace and a fresh service.
async fn run_scale(scale: usize, options: &Options) -> Result<Value, String> {
    let deployment = Deployment::new();
    deployment.write_policy(&the_policy());
    let service = deployment.start(&[]);
    let base = format!("http://127.0.0.1:{}", service.port);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(options.concurrency)
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| format!("http client: {error}"))?;

    let created = client
        .post(format!("{base}/api/v1/catalog/repositories"))
        .bearer_auth(&token)
        .json(&json!({"name": REPOSITORY, "format": "raw", "mode": "hosted"}))
        .send()
        .await
        .map_err(|error| format!("create repository: {error}"))?;
    if created.status().as_u16() != 201 {
        return Err(format!(
            "the repository was not created ({}); service log:\n{}",
            created.status(),
            deployment.the_log()
        ));
    }

    let size = options.payload_bytes;
    let delay = options.inject_delay_ms;
    let c = options.concurrency;
    let samples = options.samples;

    eprintln!("[perf-lab] scale {scale}: publish {scale} assets");
    let publish = drive(scale, c, delay, |index| {
        let (client, token) = (client.clone(), token.clone());
        let url = format!("{base}/repository/{REPOSITORY}/{}", asset_path(index));
        async move {
            let response = client
                .put(url)
                .bearer_auth(token)
                .body(payload(index, size))
                .send()
                .await
                .map_err(|error| format!("publish: {error}"))?;
            match response.status().as_u16() {
                200 | 201 => Ok(()),
                status => Err(format!("publish answered {status}")),
            }
        }
    })
    .await;

    eprintln!("[perf-lab] scale {scale}: read {samples} assets");
    let read = drive(samples, c, delay, |n| {
        let index = sample_index(n, scale);
        let (client, token) = (client.clone(), token.clone());
        let url = format!("{base}/repository/{REPOSITORY}/{}", asset_path(index));
        async move {
            let response = client
                .get(url)
                .bearer_auth(token)
                .send()
                .await
                .map_err(|error| format!("read: {error}"))?;
            let status = response.status().as_u16();
            let body = response
                .bytes()
                .await
                .map_err(|error| format!("read body: {error}"))?;
            if status != 200 {
                return Err(format!("read answered {status}"));
            }
            if digest_of(&body) != digest_of(&payload(index, size)) {
                return Err("the served bytes are not the published bytes".to_string());
            }
            Ok(())
        }
    })
    .await;

    eprintln!("[perf-lab] scale {scale}: search by text {samples} times");
    let search_text = drive(samples, c, delay, |n| {
        let index = sample_index(n, scale);
        let (client, token) = (client.clone(), token.clone());
        let url =
            format!("{base}/api/v1/search?repository={REPOSITORY}&text=asset-{index:08}&limit=20");
        async move { search_finds(&client, &token, &url, &asset_path(index)).await }
    })
    .await;

    eprintln!("[perf-lab] scale {scale}: search by digest {samples} times");
    let search_digest = drive(samples, c, delay, |n| {
        let index = sample_index(n, scale);
        let (client, token) = (client.clone(), token.clone());
        let url = format!(
            "{base}/api/v1/search?repository={REPOSITORY}&digest=sha256:{}&limit=20",
            digest_of(&payload(index, size))
        );
        async move { search_finds(&client, &token, &url, &asset_path(index)).await }
    })
    .await;

    let outcomes = [publish, read, search_text, search_digest];
    let mut workloads = serde_json::Map::new();
    for (name, outcome) in WORKLOADS.iter().zip(outcomes.iter()) {
        workloads.insert((*name).to_string(), summarize(outcome));
    }
    Ok(json!({ "assets": scale, "workloads": workloads }))
}

/// One search that must answer 200 and name the asset it was asked about.
async fn search_finds(
    client: &reqwest::Client,
    token: &str,
    url: &str,
    path: &str,
) -> Result<(), String> {
    let response = client
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|error| format!("search: {error}"))?;
    let status = response.status().as_u16();
    let body = response
        .text()
        .await
        .map_err(|error| format!("search body: {error}"))?;
    if status != 200 {
        return Err(format!("search answered {status}"));
    }
    if !body.contains(path) {
        return Err(format!("the search did not return {path}"));
    }
    Ok(())
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn proc_field(file: &str, key: &str) -> Option<String> {
    std::fs::read_to_string(file)
        .ok()?
        .lines()
        .find(|line| line.starts_with(key))
        .and_then(|line| line.split_once(':'))
        .map(|(_, value)| value.trim().to_string())
}

fn hardware() -> Value {
    json!({
        "cpu_model": proc_field("/proc/cpuinfo", "model name"),
        "logical_cores": std::thread::available_parallelism().map(|n| n.get()).ok(),
        "mem_total_kib": proc_field("/proc/meminfo", "MemTotal")
            .and_then(|value| value.trim_end_matches("kB").trim().parse::<u64>().ok()),
        "kernel": std::fs::read_to_string("/proc/sys/kernel/osrelease").ok().map(|k| k.trim().to_string()),
    })
}

/// The p95 of (scale, workload) in a result document.
fn p95_of(document: &Value, scale: u64, workload: &str) -> Option<(f64, u64)> {
    let entry = document["scales"]
        .as_array()?
        .iter()
        .find(|entry| entry["assets"].as_u64() == Some(scale))?;
    let workload = &entry["workloads"][workload];
    Some((workload["p95_ms"].as_f64()?, workload["errors"].as_u64()?))
}

/// The verdict of `--compare`: one line per shared (scale, workload), and the
/// list of regressions.
fn compare(
    current: &Value,
    baseline: &Value,
    threshold_pct: f64,
    min_delta_ms: f64,
) -> Vec<String> {
    let mut failures = Vec::new();
    let Some(scales) = current["scales"].as_array() else {
        return vec!["the current result has no scales".to_string()];
    };
    for entry in scales {
        let Some(scale) = entry["assets"].as_u64() else {
            continue;
        };
        for workload in WORKLOADS {
            let Some((now, errors)) = p95_of(current, scale, workload) else {
                failures.push(format!("{scale} assets / {workload}: the run has no p95"));
                continue;
            };
            if errors > 0 {
                failures.push(format!(
                    "{scale} assets / {workload}: {errors} failed requests"
                ));
                continue;
            }
            let Some((before, _)) = p95_of(baseline, scale, workload) else {
                eprintln!(
                    "[compare] {scale} assets / {workload}: not in the baseline, not compared"
                );
                continue;
            };
            let limit = before * (1.0 + threshold_pct / 100.0);
            let regressed = now > limit && now - before > min_delta_ms;
            eprintln!(
                "[compare] {scale} assets / {workload}: p95 {before:.3} ms -> {now:.3} ms (limit {limit:.3} ms, floor +{min_delta_ms} ms) {}",
                if regressed { "REGRESSION" } else { "ok" }
            );
            if regressed {
                failures.push(format!(
                    "{scale} assets / {workload}: p95 {before:.3} ms -> {now:.3} ms exceeds +{threshold_pct}%"
                ));
            }
        }
    }
    failures
}

fn write_json(path: &Path, document: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(document).map_err(|error| error.to_string())?;
    std::fs::write(path, format!("{text}\n"))
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn main() {
    // Skips (exit 0) without an alpha, like every `*_it` target.
    support::graph_profile::skip_unless_configured();
    let options = match Options::parse() {
        Ok(options) => options,
        Err(error) => {
            eprintln!("perf-lab: {error}");
            std::process::exit(2);
        }
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("the runtime builds");
    let result = runtime.block_on(async {
        let mut scales = Vec::new();
        for scale in &options.scales {
            scales.push(run_scale(*scale, &options).await?);
        }
        Ok::<_, String>(scales)
    });
    let scales = match result {
        Ok(scales) => scales,
        Err(error) => {
            eprintln!("perf-lab: the run is invalid: {error}");
            std::process::exit(1);
        }
    };

    let dirty = command_output("git", &["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    let document = json!({
        "schema_version": SCHEMA_VERSION,
        "lab": "hsp319-graph-s1",
        "recorded_at_utc": chrono::Utc::now().to_rfc3339(),
        "commit": command_output("git", &["rev-parse", "HEAD"]),
        "dirty_tree": dirty,
        "hardware": hardware(),
        "dgraph_image": std::env::var("HODEI_PERF_DGRAPH_IMAGE").ok(),
        "parameters": {
            "driver": "closed-loop (coordinated omission is not corrected)",
            "concurrency": options.concurrency,
            "samples_per_read_and_search": options.samples,
            "payload_bytes": options.payload_bytes,
            "content_backend": "filesystem",
            "profile": "graph (RUN_MODE=dgraph)",
            "build": if cfg!(debug_assertions) { "debug (unoptimized service binary)" } else { "release" },
            "inject_delay_ms": options.inject_delay_ms,
        },
        "scales": scales,
        "not_run": [
            {"assets": 10_000_000, "reason": "not measured by S1; belongs to HSP-216 (S4)"},
            {"assets": 100_000_000, "reason": "not measured by S1; belongs to HSP-216 (S4)"},
        ],
    });

    println!(
        "{}",
        serde_json::to_string_pretty(&document).expect("the result serializes")
    );
    if let Err(error) = write_json(&options.out, &document) {
        eprintln!("perf-lab: {error}");
        std::process::exit(1);
    }

    let mut failed = false;
    // A request that failed makes the run invalid, with or without a baseline.
    for entry in document["scales"].as_array().into_iter().flatten() {
        for workload in WORKLOADS {
            let stats = &entry["workloads"][workload];
            if stats["errors"].as_u64().unwrap_or(0) > 0 {
                eprintln!(
                    "perf-lab: {} assets / {workload}: {} failed requests; first: {}",
                    entry["assets"], stats["errors"], stats["first_error"]
                );
                failed = true;
            }
        }
    }
    if let Some(path) = &options.record {
        if failed || options.inject_delay_ms > 0 {
            eprintln!("perf-lab: refusing to record a baseline from an invalid or degraded run");
            std::process::exit(1);
        }
        if let Err(error) = write_json(path, &document) {
            eprintln!("perf-lab: {error}");
            std::process::exit(1);
        }
        eprintln!("[perf-lab] baseline written to {}", path.display());
    }
    if let Some(path) = &options.compare {
        let baseline: Value = match std::fs::read_to_string(path)
            .map_err(|error| error.to_string())
            .and_then(|text| serde_json::from_str(&text).map_err(|error| error.to_string()))
        {
            Ok(baseline) => baseline,
            Err(error) => {
                eprintln!(
                    "perf-lab: cannot read the baseline {}: {error}",
                    path.display()
                );
                std::process::exit(2);
            }
        };
        let failures = compare(
            &document,
            &baseline,
            options.threshold_pct,
            options.min_delta_ms,
        );
        for failure in &failures {
            eprintln!("perf-lab: FAIL {failure}");
        }
        failed |= !failures.is_empty();
        if failures.is_empty() {
            eprintln!(
                "[perf-lab] compare: no p95 regression beyond +{}%",
                options.threshold_pct
            );
        }
    }
    std::process::exit(i32::from(failed));
}
