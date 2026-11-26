//! The performance lab of the graph profile (HSP-319, slices S1 and S2).
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
//! - `--compare PATH`: compare against a baseline and exit non-zero if, for
//!   any shared (scale, workload), the p95 worsens by more than
//!   `--threshold-pct` AND by more than `--min-delta-ms` (the floor keeps
//!   sub-millisecond noise from failing a run), the p99 worsens by more than
//!   `--p99-threshold-pct` AND `--p99-min-delta-ms` (only when both sides have
//!   a p99, which needs 1000 successful samples), the throughput drops by more
//!   than `--throughput-drop-pct` AND `--min-throughput-delta-rps`, or if any
//!   request failed. One line per metric is printed.
//! - `--repeat N`: runs the same configuration N times (each on a fresh
//!   namespace and service) and reports the median of every metric plus its
//!   dispersion (min, max, MAD) under `repeat`, to calibrate the noise.
//! - Resource sampling (HSP-140, HSP-215): every ~250 ms the lab samples the
//!   CPU and RSS of the service process (`/proc/<pid>/stat` and `status`) during
//!   each load. With `--dgraph-container NAME` (or `HODEI_GRAPH_IT_CONTAINER`)
//!   it also samples that container with `podman stats --no-stream`, after
//!   checking that it publishes the alpha's port, that its ID stays the same and
//!   that its memory limit does not change (a reading that disagrees is
//!   discarded and counted). Without a container the JSON says
//!   `dgraph_resources: "not_sampled"`.
//! - `--payload-bytes 1KiB,64KiB,1MiB`: several payload sizes in one run; each
//!   size is its own (scale, size) row on a fresh namespace.
//! - `--dist uniform|zipf` (`--zipf-s`, `--seed`): how read and search pick
//!   the asset. Zipf is seeded, so the same options give the same accesses.
//! - Replica validity (HSP-173): after setup and after every workload the lab
//!   probes the liveness (`GET /health/live`) and a permission probe (an
//!   authorised `GET /api/v1/catalog/repositories`). A dead service is
//!   `INVALID_SERVICE_DIED`; a permission probe with status 0 or 401/403 is
//!   `INVALID_PERMISSION_PROBE`. Either aborts at once, names the scenario and
//!   the last good one with a timestamp, and the result is evidence of the
//!   failure: `--record` and `--compare` refuse it.
//! - `--inject-kill WORKLOAD`: kills the service halfway through that workload
//!   (mutation check of the liveness probe; never record with it).
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
use support::{Deployment, FIRST_ACCOUNT, Service};

/// The schema version of the result file; bump it when the shape changes.
const SCHEMA_VERSION: u64 = 2;
/// How many samples make a p99 worth printing.
const P99_MIN_SAMPLES: usize = 1000;
/// The repository every scale publishes into.
const REPOSITORY: &str = "perf";
/// The payload size the first baseline rows were recorded with.
const DEFAULT_PAYLOAD: usize = 1024;
/// The workloads of a scale, in the order they run and are reported.
const WORKLOADS: [&str; 4] = ["publish", "read", "search_text", "search_digest"];

/// The parameters of a run.
struct Options {
    scales: Vec<usize>,
    concurrency: usize,
    samples: usize,
    payload_bytes: Vec<usize>,
    dist: Dist,
    zipf_s: f64,
    seed: u64,
    inject_kill: Option<String>,
    out: PathBuf,
    record: Option<PathBuf>,
    compare: Option<PathBuf>,
    threshold_pct: f64,
    min_delta_ms: f64,
    inject_delay_ms: u64,
    p99_threshold_pct: f64,
    p99_min_delta_ms: f64,
    throughput_drop_pct: f64,
    min_throughput_delta_rps: f64,
    repeat: usize,
    container: Option<String>,
    /// Resolved from `container` once, before the first load.
    dgraph: Dgraph,
}

impl Options {
    fn parse() -> Result<Self, String> {
        let mut options = Self {
            scales: vec![1000],
            concurrency: 4,
            samples: 1000,
            payload_bytes: vec![DEFAULT_PAYLOAD],
            dist: Dist::Uniform,
            zipf_s: 1.0,
            seed: 319,
            inject_kill: None,
            out: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/perf-lab/last.json"),
            record: None,
            compare: None,
            threshold_pct: 50.0,
            min_delta_ms: 2.0,
            inject_delay_ms: 0,
            p99_threshold_pct: 100.0,
            p99_min_delta_ms: 5.0,
            throughput_drop_pct: 30.0,
            min_throughput_delta_rps: 5.0,
            repeat: 1,
            container: std::env::var("HODEI_GRAPH_IT_CONTAINER")
                .ok()
                .filter(|name| !name.is_empty()),
            dgraph: Dgraph::NotSampled("no container was given".to_string()),
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
                "--payload-bytes" => {
                    options.payload_bytes = value("--payload-bytes")?
                        .split(',')
                        .map(size)
                        .collect::<Result<_, _>>()?;
                }
                "--dist" => {
                    options.dist = match value("--dist")?.as_str() {
                        "uniform" => Dist::Uniform,
                        "zipf" => Dist::Zipf,
                        other => return Err(format!("`{other}` is not a distribution")),
                    };
                }
                "--zipf-s" => {
                    options.zipf_s = value("--zipf-s")?
                        .parse()
                        .map_err(|_| "--zipf-s is not a number".to_string())?;
                }
                "--seed" => options.seed = number::<u64>(&value("--seed")?)?,
                "--inject-kill" => {
                    let workload = value("--inject-kill")?;
                    if !WORKLOADS.contains(&workload.as_str()) {
                        return Err(format!("`{workload}` is not a workload"));
                    }
                    options.inject_kill = Some(workload);
                }
                "--out" => options.out = PathBuf::from(value("--out")?),
                "--record" => options.record = Some(PathBuf::from(value("--record")?)),
                "--compare" => options.compare = Some(PathBuf::from(value("--compare")?)),
                "--threshold-pct" => options.threshold_pct = float(&value("--threshold-pct")?)?,
                "--min-delta-ms" => options.min_delta_ms = float(&value("--min-delta-ms")?)?,
                "--p99-threshold-pct" => {
                    options.p99_threshold_pct = float(&value("--p99-threshold-pct")?)?;
                }
                "--p99-min-delta-ms" => {
                    options.p99_min_delta_ms = float(&value("--p99-min-delta-ms")?)?;
                }
                "--throughput-drop-pct" => {
                    options.throughput_drop_pct = float(&value("--throughput-drop-pct")?)?;
                }
                "--min-throughput-delta-rps" => {
                    options.min_throughput_delta_rps =
                        float(&value("--min-throughput-delta-rps")?)?;
                }
                "--repeat" => options.repeat = number(&value("--repeat")?)?,
                "--dgraph-container" => options.container = Some(value("--dgraph-container")?),
                "--inject-delay-ms" => {
                    options.inject_delay_ms = number::<u64>(&value("--inject-delay-ms")?)?;
                }
                other => return Err(format!("unknown argument `{other}`")),
            }
        }
        if options.scales.is_empty() || options.scales.contains(&0) {
            return Err("every scale must be at least 1".to_string());
        }
        if options.repeat == 0 {
            return Err("--repeat must be at least 1".to_string());
        }
        if options.concurrency == 0 || options.samples == 0 {
            return Err("--concurrency and --samples must be at least 1".to_string());
        }
        if options.payload_bytes.iter().any(|bytes| *bytes < 64) {
            return Err("every --payload-bytes size must be at least 64".to_string());
        }
        if !(options.zipf_s > 0.0 && options.zipf_s.is_finite()) {
            return Err("--zipf-s must be a positive number".to_string());
        }
        Ok(options)
    }
}

fn number<T: std::str::FromStr>(text: &str) -> Result<T, String> {
    text.trim()
        .parse()
        .map_err(|_| format!("`{text}` is not a number"))
}

fn float(text: &str) -> Result<f64, String> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or_else(|| format!("`{text}` is not a non-negative number"))
}

/// A size such as `1024`, `64KiB` or `1MiB`.
fn size(text: &str) -> Result<usize, String> {
    let text = text.trim();
    let (digits, unit) = match text.find(|c: char| !c.is_ascii_digit()) {
        Some(at) => text.split_at(at),
        None => (text, ""),
    };
    let factor = match unit {
        "" | "B" => 1,
        "KiB" => 1024,
        "MiB" => 1024 * 1024,
        "GiB" => 1024 * 1024 * 1024,
        _ => return Err(format!("`{text}` is not a size")),
    };
    digits
        .parse::<usize>()
        .map(|count| count * factor)
        .map_err(|_| format!("`{text}` is not a size"))
}

/// How read and search pick the asset.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dist {
    Uniform,
    Zipf,
}

impl Dist {
    fn name(self) -> &'static str {
        match self {
            Self::Uniform => "uniform",
            Self::Zipf => "zipf",
        }
    }
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
async fn drive<F, Fut>(
    count: usize,
    concurrency: usize,
    delay_ms: u64,
    kill_pid: Option<u32>,
    operation: F,
) -> Outcome
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
            // Mutation check: the service dies halfway through the load.
            if let (Some(pid), true) = (kill_pid, index == count / 2) {
                let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
            }
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

/// SplitMix64: a seeded, dependency-free generator for the Zipf draws.
fn splitmix(mut state: u64) -> u64 {
    state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// The asset index every sample of a run touches, and what that says about the
/// distribution (the proof that Zipf concentrates and uniform does not).
struct AccessPlan {
    indexes: Vec<usize>,
}

impl AccessPlan {
    fn new(options: &Options, scale: usize) -> Self {
        let indexes = match options.dist {
            Dist::Uniform => (0..options.samples)
                .map(|n| sample_index(n, scale))
                .collect(),
            Dist::Zipf => {
                // P(rank k) is proportional to 1 / k^s; inverse-CDF sampling.
                let mut cumulative = Vec::with_capacity(scale);
                let mut total = 0.0f64;
                for rank in 1..=scale {
                    total += (rank as f64).powf(-options.zipf_s);
                    cumulative.push(total);
                }
                // Ranks are spread over the keyspace by a stride coprime with
                // the scale, so the hot assets are not simply the first ones.
                let mut stride = (0x9E37_79B1usize % scale).max(1);
                while gcd(stride, scale) != 1 {
                    stride += 1;
                }
                (0..options.samples)
                    .map(|n| {
                        let unit = (splitmix(options.seed ^ (n as u64).wrapping_mul(0xA24B_AED4))
                            >> 11) as f64
                            / (1u64 << 53) as f64;
                        let rank = cumulative.partition_point(|c| *c < unit * total);
                        (rank.min(scale - 1) * stride) % scale
                    })
                    .collect()
            }
        };
        Self { indexes }
    }

    fn at(&self, n: usize) -> usize {
        self.indexes[n]
    }

    /// Distinct assets touched, the share of the samples on the hottest asset
    /// and on the ten hottest, and the five hottest with their counts.
    fn report(&self, options: &Options) -> Value {
        let mut counts = std::collections::HashMap::<usize, usize>::new();
        for index in &self.indexes {
            *counts.entry(*index).or_default() += 1;
        }
        let mut ranked: Vec<(usize, usize)> = counts.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let total = self.indexes.len() as f64;
        let share = |top: usize| {
            round3(ranked.iter().take(top).map(|(_, c)| *c).sum::<usize>() as f64 / total)
        };
        json!({
            "dist": options.dist.name(),
            "zipf_s": (options.dist == Dist::Zipf).then_some(options.zipf_s),
            "seed": (options.dist == Dist::Zipf).then_some(options.seed),
            "samples": self.indexes.len(),
            "distinct_assets": ranked.len(),
            "top1_share": share(1),
            "top10_share": share(10),
            "hottest": ranked.iter().take(5)
                .map(|(index, count)| json!({"asset": index, "count": count}))
                .collect::<Vec<_>>(),
        })
    }
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

// ---------------------------------------------------------------------------
// Resource sampling (HSP-140 AC2, HSP-215 AC1)
// ---------------------------------------------------------------------------

/// How often the service process is sampled.
const SAMPLE_INTERVAL: Duration = Duration::from_millis(250);

/// The Dgraph container whose resources are sampled, if any.
enum Dgraph {
    NotSampled(String),
    Sampled { name: String, id: String },
}

impl Dgraph {
    /// Resolves the container once: it must exist and publish the port of the
    /// alpha the lab talks to, otherwise nothing is sampled and the reason is
    /// declared instead of guessing.
    fn resolve(name: Option<&str>) -> Self {
        let Some(name) = name else {
            return Self::NotSampled("no container was given".to_string());
        };
        let Some(id) = command_output("podman", &["inspect", "--format", "{{.Id}}", name])
            .filter(|id| !id.is_empty())
        else {
            return Self::NotSampled(format!("container `{name}` was not found by podman"));
        };
        let endpoint = std::env::var("HODEI_GRAPH_IT_ENDPOINT").unwrap_or_default();
        let port = endpoint.rsplit(':').next().unwrap_or_default().to_string();
        let ports = command_output("podman", &["port", name]).unwrap_or_default();
        if port.is_empty() || !ports.contains(&format!(":{port}")) {
            return Self::NotSampled(format!(
                "container `{name}` does not publish the alpha port {port}"
            ));
        }
        Self::Sampled {
            name: name.to_string(),
            id,
        }
    }

    fn describe(&self) -> Value {
        match self {
            Self::NotSampled(reason) => json!({"sampled": false, "reason": reason}),
            Self::Sampled { name, id } => json!({"sampled": true, "name": name, "id": id}),
        }
    }
}

/// One reading: cumulative CPU seconds and memory in bytes at an instant.
struct Reading {
    at: Instant,
    cpu_seconds: f64,
    mem_bytes: u64,
}

/// A background thread that calls `read` until it is stopped.
struct Sampler {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: std::thread::JoinHandle<(Vec<Reading>, Vec<String>)>,
}

impl Sampler {
    fn spawn(mut read: impl FnMut() -> Result<Reading, String> + Send + 'static) -> Self {
        use std::sync::atomic::{AtomicBool, Ordering};
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let handle = std::thread::spawn(move || {
            let (mut readings, mut discarded) = (Vec::new(), Vec::new());
            loop {
                match read() {
                    Ok(reading) => readings.push(reading),
                    Err(reason) => discarded.push(reason),
                }
                if flag.load(Ordering::Relaxed) {
                    break;
                }
                let until = Instant::now() + SAMPLE_INTERVAL;
                while Instant::now() < until && !flag.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(25));
                }
            }
            (readings, discarded)
        });
        Self { stop, handle }
    }

    /// Stops the thread (after one last reading) and summarises what it saw.
    fn finish(self, memory_name: &str) -> Value {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        match self.handle.join() {
            Ok((readings, discarded)) => summarize_readings(&readings, &discarded, memory_name),
            Err(_) => json!({"sampled": false, "reason": "the sampling thread panicked"}),
        }
    }
}

fn summarize_readings(readings: &[Reading], discarded: &[String], memory_name: &str) -> Value {
    if readings.is_empty() {
        return json!({
            "sampled": false,
            "reason": "no coherent reading",
            "discarded_readings": discarded.len(),
            "discard_reasons": discarded.iter().take(3).collect::<Vec<_>>(),
        });
    }
    let mib = |bytes: u64| bytes as f64 / (1024.0 * 1024.0);
    let memory: Vec<f64> = readings.iter().map(|r| mib(r.mem_bytes)).collect();
    let (first, last) = (&readings[0], &readings[readings.len() - 1]);
    let span = last.at.duration_since(first.at).as_secs_f64();
    // CPU is in percent of ONE core (200 means two cores busy).
    let cpu_mean = (span > 0.0).then(|| (last.cpu_seconds - first.cpu_seconds) / span * 100.0);
    let cpu_max = readings
        .windows(2)
        .filter_map(|pair| {
            let dt = pair[1].at.duration_since(pair[0].at).as_secs_f64();
            (dt >= 0.1).then(|| (pair[1].cpu_seconds - pair[0].cpu_seconds) / dt * 100.0)
        })
        .fold(None, |best: Option<f64>, value| {
            Some(best.map_or(value, |b| b.max(value)))
        });
    let mut summary = json!({
        "sampled": true,
        "readings": readings.len(),
        "discarded_readings": discarded.len(),
        "discard_reasons": discarded.iter().take(3).collect::<Vec<_>>(),
        "cpu_pct_mean": cpu_mean.map(round3),
        "cpu_pct_max": cpu_max.map(round3),
    });
    summary[format!("{memory_name}_mib_max")] =
        json!(round3(memory.iter().copied().fold(0.0, f64::max)));
    summary[format!("{memory_name}_mib_mean")] =
        json!(round3(memory.iter().sum::<f64>() / memory.len() as f64));
    summary
}

/// CPU seconds and RSS of a process from `/proc`.
fn process_reader(pid: u32) -> impl FnMut() -> Result<Reading, String> + Send + 'static {
    let ticks = command_output("getconf", &["CLK_TCK"])
        .and_then(|text| text.parse::<f64>().ok())
        .unwrap_or(100.0);
    move || {
        let at = Instant::now();
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .map_err(|error| format!("/proc/{pid}/stat: {error}"))?;
        // `comm` may contain spaces: the numeric fields start after the last `)`.
        let after = &stat[stat.rfind(')').ok_or("malformed stat")? + 1..];
        let fields: Vec<&str> = after.split_whitespace().collect();
        let tick = |index: usize| {
            fields
                .get(index)
                .and_then(|field| field.parse::<f64>().ok())
                .ok_or_else(|| "malformed stat".to_string())
        };
        // Fields 14 and 15 of the line are utime and stime.
        let cpu_seconds = (tick(11)? + tick(12)?) / ticks;
        let rss_kib = proc_field(&format!("/proc/{pid}/status"), "VmRSS")
            .and_then(|value| value.trim_end_matches("kB").trim().parse::<u64>().ok())
            .ok_or("no VmRSS")?;
        Ok(Reading {
            at,
            cpu_seconds,
            mem_bytes: rss_kib * 1024,
        })
    }
}

/// `46.09MB`, `1.2GiB`, `512kB`: podman prints decimal and binary units.
fn parse_bytes(text: &str) -> Option<u64> {
    let text = text.trim();
    let at = text.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    let (digits, unit) = text.split_at(at);
    let factor = match unit {
        "B" => 1.0,
        "kB" | "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        "KiB" => 1024.0,
        "MiB" => 1024.0 * 1024.0,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((digits.parse::<f64>().ok()? * factor) as u64)
}

/// A Go duration such as `2.300738s`, `1m2.5s` or `150ms`, in seconds.
fn parse_duration(text: &str) -> Option<f64> {
    let mut rest = text.trim();
    let mut total = 0.0;
    while !rest.is_empty() {
        let at = rest.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
        let (digits, tail) = rest.split_at(at);
        let end = tail
            .find(|c: char| c.is_ascii_digit() || c == '.')
            .unwrap_or(tail.len());
        let (unit, next) = tail.split_at(end);
        let scale = match unit {
            "h" => 3600.0,
            "m" => 60.0,
            "s" => 1.0,
            "ms" => 1e-3,
            "us" | "\u{b5}s" => 1e-6,
            "ns" => 1e-9,
            _ => return None,
        };
        total += digits.parse::<f64>().ok()? * scale;
        rest = next;
    }
    Some(total)
}

/// CPU time and memory of the Dgraph container from `podman stats`. A reading
/// whose ID is not the resolved one, whose memory limit differs from the first
/// reading, or whose CPU time goes backwards is a different cgroup and is
/// discarded with the reason.
fn container_reader(
    name: String,
    full_id: String,
) -> impl FnMut() -> Result<Reading, String> + Send + 'static {
    let mut limit: Option<String> = None;
    let mut last_cpu = 0.0f64;
    move || {
        let output = Command::new("podman")
            .args(["stats", "--no-stream", "--format", "json", &name])
            .output()
            .map_err(|error| format!("podman stats: {error}"))?;
        let at = Instant::now();
        if !output.status.success() {
            return Err(format!("podman stats exited {}", output.status));
        }
        let document: Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("podman stats output: {error}"))?;
        let entry = document
            .as_array()
            .and_then(|entries| entries.first())
            .ok_or("podman stats returned no entry")?;
        let id = entry["id"].as_str().unwrap_or_default();
        if id.is_empty() || !full_id.starts_with(id) {
            return Err(format!("container id changed to `{id}`"));
        }
        let usage = entry["mem_usage"].as_str().unwrap_or_default();
        let (used, cap) = usage
            .split_once(" / ")
            .ok_or_else(|| format!("unreadable mem_usage `{usage}`"))?;
        match &limit {
            None => limit = Some(cap.to_string()),
            Some(first) if first != cap => {
                return Err(format!("memory limit changed from {first} to {cap}"));
            }
            Some(_) => {}
        }
        let mem_bytes = parse_bytes(used).ok_or_else(|| format!("unreadable memory `{used}`"))?;
        let cpu_seconds = parse_duration(entry["cpu_time"].as_str().unwrap_or_default())
            .ok_or("unreadable cpu_time")?;
        if cpu_seconds + 1e-6 < last_cpu {
            return Err(format!(
                "cpu_time went backwards ({last_cpu} -> {cpu_seconds})"
            ));
        }
        last_cpu = cpu_seconds;
        Ok(Reading {
            at,
            cpu_seconds,
            mem_bytes,
        })
    }
}

/// Runs a load while sampling the service (and the Dgraph container if given).
async fn sampled<Fut: Future<Output = Outcome>>(
    pid: u32,
    dgraph: &Dgraph,
    load: Fut,
) -> (Outcome, Value) {
    let service = Sampler::spawn(process_reader(pid));
    let container = match dgraph {
        Dgraph::Sampled { name, id } => {
            Some(Sampler::spawn(container_reader(name.clone(), id.clone())))
        }
        Dgraph::NotSampled(_) => None,
    };
    let outcome = load.await;
    // Joining waits for at most one more reading (about a second for podman).
    let service = service.finish("rss");
    let dgraph_resources = container.map_or(json!("not_sampled"), |sampler| sampler.finish("mem"));
    (
        outcome,
        json!({"service": service, "dgraph_resources": dgraph_resources}),
    )
}

// ---------------------------------------------------------------------------
// Repetitions (--repeat)
// ---------------------------------------------------------------------------

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[mid]
    } else {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    }
}

/// Median, min, max and median absolute deviation of repeated measurements.
fn dispersion(values: &[f64]) -> Value {
    let med = median(values);
    let mad = median(&values.iter().map(|v| (v - med).abs()).collect::<Vec<_>>());
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let relative = |amount: f64| (med > 0.0).then(|| round3(amount / med * 100.0));
    json!({
        "median": round3(med),
        "min": round3(min),
        "max": round3(max),
        "mad": round3(mad),
        "mad_pct": relative(mad),
        "range_pct": relative(max - min),
        "values": values.iter().map(|v| round3(*v)).collect::<Vec<_>>(),
    })
}

/// Collapses the resources of N runs: peaks keep their maximum, the rest the
/// median.
fn merge_resources(runs: &[&Value]) -> Value {
    let Some(first) = runs.first().and_then(|run| run.as_object()) else {
        return runs.first().map_or(Value::Null, |run| (*run).clone());
    };
    let mut merged = serde_json::Map::new();
    for (key, value) in first {
        let column: Vec<&Value> = runs.iter().map(|run| &run[key.as_str()]).collect();
        let merged_value = if value.is_object() {
            merge_resources(&column)
        } else if let Some(numbers) = column
            .iter()
            .map(|v| v.as_f64())
            .collect::<Option<Vec<f64>>>()
            .filter(|_| value.is_number())
        {
            let picked = if key.ends_with("_max") || key == "discarded_readings" {
                numbers.iter().copied().fold(f64::MIN, f64::max)
            } else {
                median(&numbers)
            };
            // Counts stay integers, measurements keep three decimals.
            if value.is_u64() {
                json!(picked.round() as u64)
            } else {
                json!(round3(picked))
            }
        } else {
            value.clone()
        };
        merged.insert(key.clone(), merged_value);
    }
    Value::Object(merged)
}

/// Collapses N runs of the same rows into one row each: the headline metrics
/// become the median and `repeat` carries the dispersion.
fn aggregate(runs: &[Vec<Value>]) -> Vec<Value> {
    if runs.len() == 1 {
        return runs[0].clone();
    }
    let mut rows = Vec::new();
    for (position, template) in runs[0].iter().enumerate() {
        let mut row = template.clone();
        let mut spread = serde_json::Map::new();
        for workload in WORKLOADS {
            let each: Vec<&Value> = runs
                .iter()
                .map(|run| &run[position]["workloads"][workload])
                .collect();
            let mut stats = serde_json::Map::new();
            for metric in ["p50_ms", "p95_ms", "p99_ms", "throughput_rps", "wall_s"] {
                let values: Option<Vec<f64>> = each.iter().map(|w| w[metric].as_f64()).collect();
                if let Some(values) = values {
                    row["workloads"][workload][metric] = json!(round3(median(&values)));
                    stats.insert(metric.to_string(), dispersion(&values));
                }
            }
            row["workloads"][workload]["errors"] = json!(
                each.iter()
                    .filter_map(|w| w["errors"].as_u64())
                    .sum::<u64>()
            );
            let resources: Vec<&Value> = each.iter().map(|w| &w["resources"]).collect();
            row["workloads"][workload]["resources"] = merge_resources(&resources);
            spread.insert(workload.to_string(), Value::Object(stats));
        }
        row["repeat"] = json!({
            "runs": runs.len(),
            "headline": "median of the runs",
            "dispersion": spread,
        });
        rows.push(row);
    }
    rows
}

/// A replica that cannot be measured: it never feeds an aggregate.
struct Fault {
    /// `INVALID_SERVICE_DIED`, `INVALID_PERMISSION_PROBE` or `INVALID_SETUP`.
    verdict: &'static str,
    /// The scenario during or after which the fault was seen.
    scenario: String,
    /// The last scenario whose end-of-load probes passed.
    last_good: String,
    at_utc: String,
    detail: String,
    assets: usize,
    payload_bytes: usize,
    /// What the scenarios that did run measured (evidence, not a result).
    partial: serde_json::Map<String, Value>,
}

impl Fault {
    fn setup(assets: usize, payload_bytes: usize, detail: String) -> Self {
        Self {
            verdict: "INVALID_SETUP",
            scenario: "setup".to_string(),
            last_good: "none".to_string(),
            at_utc: chrono::Utc::now().to_rfc3339(),
            detail,
            assets,
            payload_bytes,
            partial: serde_json::Map::new(),
        }
    }

    fn report(&self) -> Value {
        json!({
            "verdict": self.verdict,
            "scenario": self.scenario,
            "last_good_scenario": self.last_good,
            "detected_at_utc": self.at_utc,
            "detail": self.detail,
            "assets": self.assets,
            "payload_bytes": self.payload_bytes,
            "partial_workloads": self.partial,
        })
    }
}

/// The probes between loads: the service is alive (HSP-173 AC1) and an
/// authorised request is still answered (AC2). A failure is `(verdict, detail)`.
async fn check_replica(
    service: &mut Service,
    client: &reqwest::Client,
    base: &str,
    token: &str,
) -> Result<(), (&'static str, String)> {
    const DIED: &str = "INVALID_SERVICE_DIED";
    if let Ok(Some(status)) = service.child.try_wait() {
        return Err((DIED, format!("the service process exited ({status})")));
    }
    match client
        .get(format!("{base}/health/live"))
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => {}
        Ok(response) => {
            return Err((DIED, format!("liveness answered {}", response.status())));
        }
        Err(error) => return Err((DIED, format!("liveness probe failed: {error}"))),
    }
    match client
        .get(format!("{base}/api/v1/catalog/repositories"))
        .bearer_auth(token)
        .timeout(Duration::from_secs(10))
        .send()
        .await
    {
        Err(error) => Err((
            "INVALID_PERMISSION_PROBE",
            format!("permission probe has status 0 (no response): {error}"),
        )),
        Ok(response) if matches!(response.status().as_u16(), 401 | 403) => Err((
            "INVALID_PERMISSION_PROBE",
            format!("permission probe answered {}", response.status()),
        )),
        Ok(response) => {
            if !response.status().is_success() {
                eprintln!(
                    "[perf-lab] permission probe answered {} (not an authorisation failure)",
                    response.status()
                );
            }
            Ok(())
        }
    }
}

/// Measures one (scale, payload size) on a fresh namespace and a fresh service.
// The last checkpoint assigns `last_good` and nobody reads it: the run is over.
#[allow(unused_assignments)]
async fn run_scale(
    scale: usize,
    size: usize,
    options: &Options,
    last_good_run: &str,
) -> Result<Value, Box<Fault>> {
    let setup_fault = |detail: String| {
        let mut fault = Fault::setup(scale, size, detail);
        fault.last_good = last_good_run.to_string();
        Box::new(fault)
    };
    let deployment = Deployment::new();
    deployment.write_policy(&the_policy());
    let mut service = deployment.start(&[]);
    let base = format!("http://127.0.0.1:{}", service.port);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(options.concurrency)
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| setup_fault(format!("http client: {error}")))?;

    let created = client
        .post(format!("{base}/api/v1/catalog/repositories"))
        .bearer_auth(&token)
        .json(&json!({"name": REPOSITORY, "format": "raw", "mode": "hosted"}))
        .send()
        .await
        .map_err(|error| setup_fault(format!("create repository: {error}")))?;
    if created.status().as_u16() != 201 {
        return Err(setup_fault(format!(
            "the repository was not created ({}); service log:\n{}",
            created.status(),
            deployment.the_log()
        )));
    }

    let delay = options.inject_delay_ms;
    let c = options.concurrency;
    let samples = options.samples;
    let plan = AccessPlan::new(options, scale);
    let pid = service.child.id();
    let kill = |workload: &str| (options.inject_kill.as_deref() == Some(workload)).then_some(pid);

    let mut last_good = format!("setup(assets={scale},payload={size})");
    let mut partial = serde_json::Map::new();
    // Records what a load measured, then probes the replica: a dead replica
    // aborts at once, naming the scenario and the last good one.
    macro_rules! checkpoint {
        ($name:expr, $outcome:expr, $resources:expr) => {{
            let mut summary = summarize(&$outcome);
            summary["resources"] = $resources;
            partial.insert($name.to_string(), summary);
            if let Err((verdict, detail)) =
                check_replica(&mut service, &client, &base, &token).await
            {
                return Err(Box::new(Fault {
                    verdict,
                    scenario: format!("{}(assets={scale},payload={size})", $name),
                    last_good,
                    at_utc: chrono::Utc::now().to_rfc3339(),
                    detail,
                    assets: scale,
                    payload_bytes: size,
                    partial,
                }));
            }
            last_good = format!("{}(assets={scale},payload={size})", $name);
        }};
    }
    // The replica must be healthy before the first load.
    if let Err((verdict, detail)) = check_replica(&mut service, &client, &base, &token).await {
        let mut fault = setup_fault(detail);
        fault.verdict = verdict;
        return Err(fault);
    }

    eprintln!("[perf-lab] scale {scale} / {size} B: publish {scale} assets");
    let (publish, publish_resources) = sampled(
        pid,
        &options.dgraph,
        drive(scale, c, delay, kill("publish"), |index| {
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
        }),
    )
    .await;
    checkpoint!("publish", publish, publish_resources);

    eprintln!(
        "[perf-lab] scale {scale} / {size} B: read {samples} assets ({})",
        options.dist.name()
    );
    let (read, read_resources) = sampled(
        pid,
        &options.dgraph,
        drive(samples, c, delay, kill("read"), |n| {
            let index = plan.at(n);
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
        }),
    )
    .await;
    checkpoint!("read", read, read_resources);

    eprintln!("[perf-lab] scale {scale} / {size} B: search by text {samples} times");
    let (search_text, search_text_resources) = sampled(
        pid,
        &options.dgraph,
        drive(samples, c, delay, kill("search_text"), |n| {
            let index = plan.at(n);
            let (client, token) = (client.clone(), token.clone());
            let url = format!(
                "{base}/api/v1/search?repository={REPOSITORY}&text=asset-{index:08}&limit=20"
            );
            async move { search_finds(&client, &token, &url, &asset_path(index)).await }
        }),
    )
    .await;
    checkpoint!("search_text", search_text, search_text_resources);

    eprintln!("[perf-lab] scale {scale} / {size} B: search by digest {samples} times");
    let (search_digest, search_digest_resources) = sampled(
        pid,
        &options.dgraph,
        drive(samples, c, delay, kill("search_digest"), |n| {
            let index = plan.at(n);
            let (client, token) = (client.clone(), token.clone());
            let url = format!(
                "{base}/api/v1/search?repository={REPOSITORY}&digest=sha256:{}&limit=20",
                digest_of(&payload(index, size))
            );
            async move { search_finds(&client, &token, &url, &asset_path(index)).await }
        }),
    )
    .await;
    checkpoint!("search_digest", search_digest, search_digest_resources);

    Ok(json!({
        "assets": scale,
        "payload_bytes": size,
        "access": plan.report(options),
        "workloads": partial,
    }))
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

/// The row of (scale, payload size, distribution) in a result document. Rows
/// of the schema 1 baseline carry neither field: they are 1 KiB and uniform.
fn row_of<'a>(document: &'a Value, assets: u64, payload: u64, dist: &str) -> Option<&'a Value> {
    document["scales"].as_array()?.iter().find(|entry| {
        entry["assets"].as_u64() == Some(assets)
            && entry["payload_bytes"]
                .as_u64()
                .unwrap_or(DEFAULT_PAYLOAD as u64)
                == payload
            && entry["access"]["dist"].as_str().unwrap_or("uniform") == dist
    })
}

/// The limits of `--compare`: a metric regresses only when it is worse than
/// the percentage AND than the absolute floor.
struct Thresholds {
    p95_pct: f64,
    p95_min_delta_ms: f64,
    p99_pct: f64,
    p99_min_delta_ms: f64,
    throughput_drop_pct: f64,
    min_throughput_delta_rps: f64,
}

/// A latency metric of a workload (`null` is `None`).
fn metric_of(row: &Value, workload: &str, metric: &str) -> Option<f64> {
    row["workloads"][workload][metric].as_f64()
}

/// One latency metric (lower is better): prints a line and returns a failure.
fn compare_latency(
    label: &str,
    metric: &str,
    (before, now): (f64, f64),
    (pct, floor_ms): (f64, f64),
) -> Option<String> {
    let limit = before * (1.0 + pct / 100.0);
    let regressed = now > limit && now - before > floor_ms;
    eprintln!(
        "[compare] {label} / {metric}: {before:.3} ms -> {now:.3} ms (limit {limit:.3} ms, floor +{floor_ms} ms) {}",
        if regressed { "REGRESSION" } else { "ok" }
    );
    regressed.then(|| format!("{label} / {metric}: {before:.3} ms -> {now:.3} ms exceeds +{pct}%"))
}

/// The verdict of `--compare`: one line per shared row, workload and metric
/// (p95, p99, throughput), and the list of regressions. The p99 is compared
/// only when both sides have one (it needs 1000 successful samples, so rows of
/// an older baseline or of a short run are skipped, not failed).
fn compare(current: &Value, baseline: &Value, limits: &Thresholds) -> Vec<String> {
    let mut failures = Vec::new();
    let Some(rows) = current["scales"].as_array() else {
        return vec!["the current result has no scales".to_string()];
    };
    for row in rows {
        let Some(assets) = row["assets"].as_u64() else {
            continue;
        };
        let payload = row["payload_bytes"]
            .as_u64()
            .unwrap_or(DEFAULT_PAYLOAD as u64);
        let dist = row["access"]["dist"].as_str().unwrap_or("uniform");
        let label = format!("{assets} assets / {payload} B / {dist}");
        for workload in WORKLOADS {
            let label = format!("{label} / {workload}");
            let Some(now) = metric_of(row, workload, "p95_ms") else {
                failures.push(format!("{label}: the run has no p95"));
                continue;
            };
            let errors = row["workloads"][workload]["errors"].as_u64().unwrap_or(0);
            if errors > 0 {
                failures.push(format!("{label}: {errors} failed requests"));
                continue;
            }
            let Some(before_row) = row_of(baseline, assets, payload, dist) else {
                eprintln!("[compare] {label}: not in the baseline, not compared");
                continue;
            };
            let Some(before) = metric_of(before_row, workload, "p95_ms") else {
                eprintln!("[compare] {label}: not in the baseline, not compared");
                continue;
            };
            failures.extend(compare_latency(
                &label,
                "p95",
                (before, now),
                (limits.p95_pct, limits.p95_min_delta_ms),
            ));
            match (
                metric_of(before_row, workload, "p99_ms"),
                metric_of(row, workload, "p99_ms"),
            ) {
                (Some(before), Some(now)) => failures.extend(compare_latency(
                    &label,
                    "p99",
                    (before, now),
                    (limits.p99_pct, limits.p99_min_delta_ms),
                )),
                _ => eprintln!(
                    "[compare] {label} / p99: skipped (needs >= {P99_MIN_SAMPLES} samples on both sides)"
                ),
            }
            if let (Some(before), Some(now)) = (
                metric_of(before_row, workload, "throughput_rps"),
                metric_of(row, workload, "throughput_rps"),
            ) {
                let floor = before * (1.0 - limits.throughput_drop_pct / 100.0);
                let regressed = now < floor && before - now > limits.min_throughput_delta_rps;
                eprintln!(
                    "[compare] {label} / throughput: {before:.3} rps -> {now:.3} rps (minimum {floor:.3} rps, floor -{} rps) {}",
                    limits.min_throughput_delta_rps,
                    if regressed { "REGRESSION" } else { "ok" }
                );
                if regressed {
                    failures.push(format!(
                        "{label} / throughput: {before:.3} rps -> {now:.3} rps drops more than {}%",
                        limits.throughput_drop_pct
                    ));
                }
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
    let mut options = match Options::parse() {
        Ok(options) => options,
        Err(error) => {
            eprintln!("perf-lab: {error}");
            std::process::exit(2);
        }
    };
    options.dgraph = Dgraph::resolve(options.container.as_deref());
    if let Dgraph::NotSampled(reason) = &options.dgraph {
        eprintln!("[perf-lab] dgraph_resources: not_sampled ({reason})");
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("the runtime builds");
    let result = runtime.block_on(async {
        let mut runs = Vec::new();
        let mut last_good = "none".to_string();
        for repetition in 1..=options.repeat {
            if options.repeat > 1 {
                eprintln!("[perf-lab] repetition {repetition} of {}", options.repeat);
            }
            let mut rows = Vec::new();
            for scale in &options.scales {
                for size in &options.payload_bytes {
                    let row = run_scale(*scale, *size, &options, &last_good).await?;
                    last_good = format!("search_digest(assets={scale},payload={size})");
                    rows.push(row);
                }
            }
            runs.push(rows);
        }
        Ok::<_, Box<Fault>>(aggregate(&runs))
    });
    let (scales, fault) = match result {
        Ok(scales) => (scales, None),
        Err(fault) => (Vec::new(), Some(fault)),
    };

    let dirty = command_output("git", &["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    let document = json!({
        "schema_version": SCHEMA_VERSION,
        "lab": "hsp319-graph-s2",
        "recorded_at_utc": chrono::Utc::now().to_rfc3339(),
        "commit": command_output("git", &["rev-parse", "HEAD"]),
        "dirty_tree": dirty,
        "hardware": hardware(),
        "dgraph_image": std::env::var("HODEI_PERF_DGRAPH_IMAGE").ok(),
        "replica": match &fault {
            None => json!({"verdict": "VALID"}),
            Some(fault) => fault.report(),
        },
        "parameters": {
            "driver": "closed-loop (coordinated omission is not corrected)",
            "concurrency": options.concurrency,
            "samples_per_read_and_search": options.samples,
            "payload_bytes": options.payload_bytes,
            "dist": options.dist.name(),
            "content_backend": "filesystem",
            "profile": "graph (RUN_MODE=dgraph)",
            "build": if cfg!(debug_assertions) { "debug (unoptimized service binary)" } else { "release" },
            "inject_delay_ms": options.inject_delay_ms,
            "inject_kill": options.inject_kill,
            "repeat": options.repeat,
            "dgraph_container": options.dgraph.describe(),
            "resource_sampling": {
                "service": "/proc/<pid>/stat and status every ~250 ms (cpu_pct is percent of one core)",
                "dgraph": "podman stats --no-stream; id, memory limit and cpu_time monotonicity validated per reading",
            },
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

    if let Some(fault) = &fault {
        // An invalid replica is evidence of the failure, never a measurement.
        eprintln!(
            "perf-lab: {}: {} at {} (last good scenario: {}); {}",
            fault.verdict, fault.scenario, fault.at_utc, fault.last_good, fault.detail
        );
        if options.record.is_some() || options.compare.is_some() {
            eprintln!(
                "perf-lab: --record/--compare refused: an invalid replica never feeds an aggregate"
            );
        }
        std::process::exit(1);
    }

    let mut failed = false;
    // A request that failed makes the run invalid, with or without a baseline.
    for entry in document["scales"].as_array().into_iter().flatten() {
        for workload in WORKLOADS {
            let stats = &entry["workloads"][workload];
            if stats["errors"].as_u64().unwrap_or(0) > 0 {
                eprintln!(
                    "perf-lab: {} assets / {} B / {workload}: {} failed requests; first: {}",
                    entry["assets"], entry["payload_bytes"], stats["errors"], stats["first_error"]
                );
                failed = true;
            }
        }
    }
    if let Some(path) = &options.record {
        if failed || options.inject_delay_ms > 0 || options.inject_kill.is_some() {
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
        if baseline["replica"]["verdict"]
            .as_str()
            .is_some_and(|verdict| verdict != "VALID")
        {
            eprintln!("perf-lab: the baseline is from an invalid replica; refusing to compare");
            std::process::exit(2);
        }
        let failures = compare(
            &document,
            &baseline,
            &Thresholds {
                p95_pct: options.threshold_pct,
                p95_min_delta_ms: options.min_delta_ms,
                p99_pct: options.p99_threshold_pct,
                p99_min_delta_ms: options.p99_min_delta_ms,
                throughput_drop_pct: options.throughput_drop_pct,
                min_throughput_delta_rps: options.min_throughput_delta_rps,
            },
        );
        for failure in &failures {
            eprintln!("perf-lab: FAIL {failure}");
        }
        failed |= !failures.is_empty();
        if failures.is_empty() {
            eprintln!(
                "[perf-lab] compare: no regression of p95 (+{}%), p99 (+{}%) or throughput (-{}%)",
                options.threshold_pct, options.p99_threshold_pct, options.throughput_drop_pct
            );
        }
    }
    std::process::exit(i32::from(failed));
}
