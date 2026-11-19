//! The reproducible performance and SLA lab (HSP-140, UAT PERF-01).
//!
//! This binary answers one question and answers it with evidence: **what
//! does Hodei cost**, on this machine, at these sizes, under these
//! limits. It is not a test: a run takes minutes, and a test that takes
//! minutes is a test nobody runs. It is also not a benchmark toy: every
//! number it writes carries the commit, the command, the host state and
//! the configuration that produced it, so that the next run can be
//! compared against it without guessing.
//!
//! ```text
//! perf_lab run --profile lab --replicas 5 --out target/uat
//! ```
//!
//! # Method
//!
//! - **Open-loop driver.** Each request is scheduled at an absolute
//!   instant (`t0 + i / rate`) and fired when that instant arrives,
//!   whether the driver is ready or not. The lag between the scheduled
//!   instant and the emission (`schedule_delay`) is recorded per request:
//!   a run whose driver saturated itself is marked invalid instead of
//!   quietly measuring the scheduler of the client.
//! - **Warm and cold are separated physically.** Cold scenes evict the
//!   content from the page cache first (`fsync` + `POSIX_FADV_DONTNEED`
//!   over the CAS files) so the read really walks the disk, without
//!   root. A "cold" read that immediately follows a write is warm by
//!   construction and is never labeled cold here.
//! - **Errors are never throughput.** Failed and timed-out requests are
//!   counted in their own columns; only successes feed `rps` and
//!   `mb_per_s`.
//! - **p99 requires 1000 samples.** Below that threshold the JSON says
//!   `null` and why; a percentile computed over sixty samples is noise
//!   wearing a percentile's clothes.
//! - **Replicas.** Five replicas of the WHOLE run, each on a fresh
//!   directory (a fresh database, a fresh content store, a fresh page
//!   cache footprint). Five concurrent service processes on the same
//!   embedded RocksDB directory are not possible -- the directory is
//!   locked by the process that holds it (src/bootstrap.rs) and the HSP-125
//!   spike measured the shared-host contention that five of them would
//!   create -- so the replication is of the RUN, and that choice is
//!   written in the report with its justification.
//! - **The corpus is deterministic.** A seeded generator keyed by the
//!   size: the same profile always produces the same bytes, so the
//!   digests, the on-disk footprint and the cache behaviour repeat.
//!
//! # Profiles
//!
//! - `smoke`: minutes. A shape check, not a measurement.
//! - `lab`: the reference numbers. 100k catalog assets, warm and cold
//!   scenes, all protocol surfaces, five replicas.
//! - `full`: the lab plus the 1 GiB big-blob scenario and a 1M catalog.
//!   The 10M catalog is optional in the acceptance criterion and is not
//!   run here; the skip is declared, not hidden.
//!
//! The heavy seeds (bulk catalog rows, evidence envelopes) touch the
//! database directly and therefore run in a CHILD of this same binary
//! with the service stopped: the RocksDB directory is single-owner. The
//! driver only ever speaks HTTP to the real service.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::net::{TcpListener, TcpStream};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256, Sha512};

/// The JWT secret of the lab deployment. The same the test harness uses:
/// the lab is not a production deployment and the credentials never
/// leave the machine.
const SECRET: &[u8] = b"raw-conformity-secret-long-enough";
const ISSUER: &str = "https://issuer.example";
const AUDIENCE: &str = "hodei-artifacts";
const ACCOUNT: &str = "acme";
const USER: &str = "alice";

/// How many samples make a percentile worth printing.
const P99_MIN_SAMPLES: usize = 1000;

/// The report refuses to call a run valid above this emission lag.
const DRIVER_LAG_TOLERANCE_FACTOR: f64 = 10.0;

/// The account the permission probe speaks with when it must be refused.
const OTHER_ACCOUNT: &str = "other";

/// The name of every workload, in the order the lab runs them. The order
/// is part of the protocol: two runs that executed different orders are
/// not comparable, and the hash of this list travels in the report.
const WORKLOAD_ORDER: &[&str] = &[
    "raw-write-1kib",
    "raw-write-64kib",
    "raw-write-1mib",
    "raw-write-64mib",
    "raw-write-1gib",
    "raw-read-hit-1kib",
    "raw-read-hit-64kib",
    "raw-read-hit-1mib",
    "raw-read-hit-64mib",
    "raw-read-miss-1kib",
    "raw-read-miss-64kib",
    "raw-read-miss-1mib",
    "raw-read-miss-64mib",
    "raw-read-cold-1kib",
    "raw-read-cold-64kib",
    "raw-read-cold-1mib",
    "raw-read-cold-64mib",
    "raw-read-hit-1gib",
    "raw-read-cold-1gib",
    "oci-push-blob",
    "oci-push-manifest",
    "oci-pull-manifest",
    "oci-pull-blob",
    "npm-publish",
    "npm-download",
    "pypi-upload",
    "pypi-index",
    "pypi-file",
    "maven-upload",
    "maven-download",
    "proxy-miss",
    "proxy-hit",
    "search-package",
    "search-digest",
    "capsule-read",
    "policy-evaluate",
    "gc-plan",
];

// ──────────────────────────────────────────────────────────────────────
// The shape of the load: profiles, sizes and counts
// ──────────────────────────────────────────────────────────────────────

/// One measured size point.
#[derive(Debug, Clone, Copy)]
struct SizePoint {
    /// The blob size in bytes.
    bytes: usize,
    /// Whether this point belongs to the separate big-blob scenario.
    big: bool,
    /// Measured writes (open-loop).
    write_count: usize,
    write_rate: f64,
    /// Measured warm reads over already-present assets.
    hit_count: usize,
    hit_rate: f64,
    /// Measured first-time reads (misses against the content cache of
    /// the process: fresh paths, nothing read before).
    miss_count: usize,
    miss_rate: f64,
    /// Measured cold reads after the page-cache eviction.
    cold_count: usize,
    cold_rate: f64,
}

/// Everything a profile decides. Parsed, never guessed: an unknown
/// profile is an error, not a default.
#[derive(Debug, Clone)]
struct Profile {
    name: &'static str,
    points: Vec<SizePoint>,
    /// Bulk catalog rows seeded directly for the search workloads.
    catalog_assets: usize,
    /// Pieces of evidence seeded for the capsule workload.
    evidence: usize,
    /// Per-protocol request counts (npm, pypi, maven, OCI).
    protocol_count: usize,
    protocol_rate: f64,
    /// The auxiliary workloads.
    capsule_count: usize,
    capsule_rate: f64,
    /// Catalog searches: each one walks the catalog and costs seconds at
    /// the reference size, so the rate is of the order of one per second
    /// and the count stays small on purpose.
    search_count: usize,
    search_rate: f64,
    evaluate_count: usize,
    evaluate_rate: f64,
    gc_count: usize,
    gc_rate: f64,
    proxy_count: usize,
    proxy_rate: f64,
    /// Requests fired before the measured window and discarded.
    warmup: usize,
}

impl Profile {
    /// The profile by name.
    fn parse(name: &str) -> Result<Self, String> {
        let profile = match name {
            "smoke" => Profile {
                name: "smoke",
                points: vec![
                    SizePoint {
                        bytes: 1024,
                        big: false,
                        write_count: 40,
                        write_rate: 80.0,
                        hit_count: 40,
                        hit_rate: 150.0,
                        miss_count: 20,
                        miss_rate: 80.0,
                        cold_count: 20,
                        cold_rate: 40.0,
                    },
                    SizePoint {
                        bytes: 65_536,
                        big: false,
                        write_count: 40,
                        write_rate: 60.0,
                        hit_count: 40,
                        hit_rate: 80.0,
                        miss_count: 20,
                        miss_rate: 40.0,
                        cold_count: 20,
                        cold_rate: 20.0,
                    },
                ],
                catalog_assets: 1_000,
                evidence: 50,
                protocol_count: 30,
                protocol_rate: 20.0,
                capsule_count: 20,
                capsule_rate: 10.0,
                search_count: 5,
                search_rate: 1.0,
                evaluate_count: 30,
                evaluate_rate: 30.0,
                gc_count: 5,
                gc_rate: 1.0,
                proxy_count: 30,
                proxy_rate: 15.0,
                warmup: 5,
            },
            "lab" => Profile {
                name: "lab",
                points: vec![
                    SizePoint {
                        bytes: 1024,
                        big: false,
                        write_count: 1000,
                        write_rate: 500.0,
                        hit_count: 1000,
                        hit_rate: 800.0,
                        miss_count: 400,
                        miss_rate: 400.0,
                        cold_count: 400,
                        cold_rate: 300.0,
                    },
                    SizePoint {
                        bytes: 65_536,
                        big: false,
                        write_count: 1000,
                        write_rate: 250.0,
                        hit_count: 1000,
                        hit_rate: 400.0,
                        miss_count: 300,
                        miss_rate: 200.0,
                        cold_count: 300,
                        cold_rate: 150.0,
                    },
                    SizePoint {
                        bytes: 1_048_576,
                        big: false,
                        write_count: 300,
                        write_rate: 80.0,
                        hit_count: 300,
                        hit_rate: 120.0,
                        miss_count: 120,
                        miss_rate: 60.0,
                        cold_count: 120,
                        cold_rate: 40.0,
                    },
                    SizePoint {
                        bytes: 67_108_864,
                        big: false,
                        write_count: 60,
                        write_rate: 8.0,
                        hit_count: 120,
                        hit_rate: 10.0,
                        miss_count: 30,
                        miss_rate: 4.0,
                        cold_count: 40,
                        cold_rate: 4.0,
                    },
                ],
                catalog_assets: 100_000,
                evidence: 5_000,
                protocol_count: 200,
                protocol_rate: 40.0,
                capsule_count: 400,
                capsule_rate: 60.0,
                search_count: 20,
                search_rate: 1.0,
                evaluate_count: 1000,
                evaluate_rate: 200.0,
                gc_count: 30,
                gc_rate: 2.0,
                proxy_count: 300,
                proxy_rate: 60.0,
                warmup: 20,
            },
            "full" => {
                let mut lab = Self::parse("lab")?;
                lab.name = "full";
                lab.points.push(SizePoint {
                    bytes: 1_073_741_824,
                    big: true,
                    write_count: 2,
                    write_rate: 0.2,
                    hit_count: 4,
                    hit_rate: 0.4,
                    miss_count: 0,
                    miss_rate: 1.0,
                    cold_count: 2,
                    cold_rate: 0.2,
                });
                lab.catalog_assets = 1_000_000;
                lab.evidence = 20_000;
                lab
            }
            other => {
                return Err(format!(
                    "unknown profile '{other}'; expected smoke, lab or full"
                ));
            }
        };
        Ok(profile)
    }

    /// A stable digest of everything the profile decides. It is written
    /// in the report so that two runs can prove they ran the same load.
    fn fingerprint(&self) -> String {
        let mut text = String::new();
        text.push_str(self.name);
        for point in &self.points {
            text.push_str(&format!(
                "|{};{};{},{},{},{},{},{},{},{}",
                point.bytes,
                point.big,
                point.write_count,
                point.write_rate,
                point.hit_count,
                point.hit_rate,
                point.miss_count,
                point.miss_rate,
                point.cold_count,
                point.cold_rate
            ));
        }
        text.push_str(&format!(
            "|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            self.catalog_assets,
            self.evidence,
            self.protocol_count,
            self.protocol_rate,
            self.capsule_count,
            self.capsule_rate,
            self.search_count,
            self.search_rate,
            self.evaluate_count,
            self.evaluate_rate,
            self.gc_count,
            self.gc_rate,
            self.warmup
        ));
        hex_digest(text.as_bytes())
    }
}

// ──────────────────────────────────────────────────────────────────────
// Deterministic corpus and honest percentiles
// ──────────────────────────────────────────────────────────────────────

/// The bytes of an asset of the given size, keyed by its slot.
///
/// Deterministic on purpose: the seed is (size, slot), so the same
/// profile always produces the same bytes, with the same digests. The
/// slot matters as much as the size: assets of one size that shared
/// their content would collapse into a single blob in the CAS, and the
/// write workload would measure "the content already exists" instead of
/// a write.
fn corpus(size: usize, slot: usize) -> Vec<u8> {
    let mut state = (size as u64) ^ 0x5DEECE66D ^ (slot as u64).wrapping_mul(0x9E3779B97F4A7C15);
    let mut bytes = Vec::with_capacity(size);
    while bytes.len() < size {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        bytes.push((state >> 33) as u8);
    }
    bytes
}

/// The sha256 of bytes, in hexadecimal.
fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The percentile of a sorted list, nearest-rank with rounding.
fn percentile(sorted: &[f64], quantile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() as f64 - 1.0) * quantile).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

/// A percentile that admits it is not trustworthy below its threshold.
///
/// p99 over fewer than a thousand samples is reported as null with the
/// reason, because the number would be a rumor about the tail.
fn percentile_honest(sorted: &[f64], quantile: f64, samples: usize) -> serde_json::Value {
    if quantile > 0.98 && samples < P99_MIN_SAMPLES {
        return serde_json::json!({
            "value_ms": null,
            "reason": format!(
                "p99 needs at least {P99_MIN_SAMPLES} samples; this workload has {samples}"
            ),
        });
    }
    serde_json::json!({ "value_ms": round3(percentile(sorted, quantile)) })
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    percentile(&sorted, 0.5)
}

fn spread_pct(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let middle = median(values);
    if middle <= 0.0 {
        return 0.0;
    }
    let lowest = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let highest = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    round3((highest - lowest) / middle * 100.0)
}

/// Whether the driver invalidated its own measurement.
///
/// The emission lag is compared against the requested spacing: a run
/// whose requests left the schedule later than the tolerance factor of
/// the spacing measured the client, not the service.
fn driver_saturated(lag_p95_ms: f64, rate: f64) -> bool {
    if rate <= 0.0 {
        return false;
    }
    let spacing_ms = 1000.0 / rate;
    lag_p95_ms > spacing_ms * DRIVER_LAG_TOLERANCE_FACTOR
}

// ──────────────────────────────────────────────────────────────────────
// Requests and the open-loop driver
// ──────────────────────────────────────────────────────────────────────

/// A single request as the driver sees it.
#[derive(Clone)]
struct RequestSpec {
    method: reqwest::Method,
    path: String,
    headers: Vec<(String, String)>,
    body: Option<Arc<Vec<u8>>>,
    /// The exact success status expected; `None` accepts any 2xx.
    expect: Option<u16>,
}

fn bearer(token: &str) -> Vec<(String, String)> {
    vec![("authorization".to_string(), format!("Bearer {token}"))]
}

/// One measured workload.
struct Workload {
    name: &'static str,
    count: usize,
    warmup: usize,
    rate: f64,
    timeout: Duration,
    /// Evict the CAS from the page cache before the measured window.
    evict_first: bool,
    make: Box<dyn Fn(usize) -> RequestSpec + Send + Sync>,
}

/// One finished request, with the honesty of its own birth time.
struct Sample {
    /// Seconds between the scheduled instant and the emission.
    schedule_delay_ms: f64,
    /// Seconds between the emission and the last byte.
    latency_ms: f64,
    ok: bool,
    timed_out: bool,
    bytes_in: u64,
    bytes_out: u64,
}

/// The procfs counters of one process, in seconds and KiB.
struct ProcCounters {
    cpu_seconds: Option<f64>,
    rss_kib: Option<f64>,
}

fn proc_counters(pid: u32) -> ProcCounters {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return ProcCounters {
            cpu_seconds: None,
            rss_kib: None,
        };
    };
    // The comm field may contain spaces and parentheses: everything
    // after the LAST ')' is positional. utime and stime are the fields
    // 12 and 13 counting from the state after comm, at 100 ticks/s.
    let cpu_seconds = stat.rfind(')').and_then(|cut| {
        let fields: Vec<&str> = stat[cut + 1..].split_whitespace().collect();
        let utime = fields.get(11)?.parse::<f64>().ok()?;
        let stime = fields.get(12)?.parse::<f64>().ok()?;
        Some((utime + stime) / 100.0)
    });
    let rss_kib = std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|status| {
            status.lines().find_map(|line| {
                line.strip_prefix("VmRSS:")
                    .and_then(|rest| rest.trim().trim_end_matches(" kB").parse::<f64>().ok())
            })
        });
    ProcCounters {
        cpu_seconds,
        rss_kib,
    }
}

/// The peak-rss watcher of one process while a workload runs.
struct Watcher {
    stop: Arc<AtomicBool>,
    peak_kib: Arc<Mutex<Option<f64>>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Watcher {
    fn start(pid: u32) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let peak = Arc::new(Mutex::new(None));
        let (stop_inner, peak_inner) = (Arc::clone(&stop), Arc::clone(&peak));
        let handle = std::thread::spawn(move || {
            while !stop_inner.load(Ordering::Relaxed) {
                if let Some(rss) = proc_counters(pid).rss_kib {
                    let mut guard = peak_inner.lock().expect("the peak");
                    *guard = Some(guard.map_or(rss, |old: f64| old.max(rss)));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        Self {
            stop,
            peak_kib: peak,
            handle: Some(handle),
        }
    }

    fn stop(mut self) -> Option<f64> {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        self.peak_kib.lock().expect("the peak").take()
    }
}

/// Fires one request and says what happened: status, success, timeout,
/// bytes served. A body that breaks halfway is a failure even when the
/// status arrived, and a timeout is counted as a timeout and never as a
/// success.
async fn fire(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    spec: RequestSpec,
) -> (u16, bool, bool, u64) {
    let expect = spec.expect;
    let mut request = client.request(spec.method.clone(), format!("{base}{}", spec.path));
    if spec.headers.is_empty() {
        request = request.header("authorization", format!("Bearer {token}"));
    } else {
        for (name, value) in &spec.headers {
            request = request.header(name.as_str(), value.as_str());
        }
    }
    if let Some(body) = &spec.body {
        request = request.body(bytes::Bytes::copy_from_slice(body));
    }
    match request.send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            let ok = match expect {
                Some(expected) => status == expected,
                None => (200..300).contains(&status),
            };
            match response.bytes().await {
                Ok(data) => (status, ok, false, data.len() as u64),
                // The status arrived but the body broke: a read that
                // cannot be completed is not a success. It is counted
                // as an error, not as a timeout -- a timeout at this
                // level cannot be told apart from a broken body.
                Err(_) => (status, false, false, 0),
            }
        }
        Err(error) => {
            let timed_out = error.is_timeout();
            (0, false, timed_out, 0)
        }
    }
}

/// The statistics of one finished workload.
#[derive(Serialize)]
struct WorkloadStats {
    name: &'static str,
    count: usize,
    warmup: usize,
    rate_rps_requested: f64,
    ok: usize,
    errors: usize,
    timeouts: usize,
    /// Successes only: failures never inflate throughput.
    throughput_rps: f64,
    mb_per_s: f64,
    latency_ms: serde_json::Value,
    schedule_delay_ms: serde_json::Value,
    driver_saturated: bool,
    window_wall_ms: f64,
    service: serde_json::Value,
    driver: serde_json::Value,
    bytes: serde_json::Value,
}

/// Drives one workload open-loop against the service.
async fn drive(
    workload: &Workload,
    base: &str,
    token: &str,
    service_pid: u32,
    cas: &Path,
) -> WorkloadStats {
    let client = reqwest::Client::builder()
        .timeout(workload.timeout)
        .tcp_nodelay(true)
        .build()
        .expect("the driver client");

    // ── Warmup: fired sequentially, discarded. The first reads of a
    // scene pay for the connection pool and for cold internal caches;
    // that cost is real but it is not what the workload measures. The
    // warmup slots are BEYOND the measured range (count + i), so no
    // measured request collides with the warmup's own writes.
    for index in 0..workload.warmup {
        let spec = (workload.make)(workload.count + index);
        let _ = fire(&client, base, token, spec).await;
    }

    if workload.evict_first {
        evict_page_cache(cas);
    }

    let service_before = proc_counters(service_pid);
    let driver_before = proc_counters(std::process::id());
    let watcher = Watcher::start(service_pid);

    let interval = Duration::from_secs_f64(1.0 / workload.rate.max(f64::EPSILON));
    let window_start = tokio::time::Instant::now();
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<Sample>();
    let mut tasks = Vec::with_capacity(workload.count);
    for index in 0..workload.count {
        let spec = (workload.make)(index);
        let bytes_in = spec.body.as_ref().map_or(0, |body| body.len() as u64);
        let scheduled = window_start + interval * index as u32;
        let sender = sender.clone();
        let client = client.clone();
        let base = base.to_string();
        let token = token.to_string();
        tasks.push(tokio::spawn(async move {
            tokio::time::sleep_until(scheduled).await;
            let emitted = tokio::time::Instant::now();
            let schedule_delay_ms = (emitted - scheduled).as_secs_f64() * 1000.0;
            let (_status, ok, timed_out, bytes_out) = fire(&client, &base, &token, spec).await;
            let finished = tokio::time::Instant::now();
            sender
                .send(Sample {
                    schedule_delay_ms,
                    latency_ms: (finished - emitted).as_secs_f64() * 1000.0,
                    ok,
                    timed_out,
                    bytes_in,
                    bytes_out,
                })
                .expect("the collector is alive");
        }));
    }
    drop(sender);
    let mut samples: Vec<Sample> = Vec::with_capacity(workload.count);
    while let Some(sample) = receiver.recv().await {
        samples.push(sample);
    }
    for task in tasks {
        let _ = task.await;
    }
    let wall_ms = window_start.elapsed().as_secs_f64() * 1000.0;

    let peak_kib = watcher.stop();
    let service_after = proc_counters(service_pid);
    let driver_after = proc_counters(std::process::id());

    let mut latencies: Vec<f64> = samples
        .iter()
        .filter(|s| s.ok)
        .map(|s| s.latency_ms)
        .collect();
    latencies.sort_by(f64::total_cmp);
    let mut delays: Vec<f64> = samples.iter().map(|s| s.schedule_delay_ms).collect();
    delays.sort_by(f64::total_cmp);

    let ok = samples.iter().filter(|s| s.ok).count();
    let timeouts = samples.iter().filter(|s| s.timed_out).count();
    let errors = samples.len() - ok - timeouts;
    let served_bytes: u64 = samples.iter().map(|s| s.bytes_in + s.bytes_out).sum();
    let in_bytes: u64 = samples.iter().filter(|s| s.ok).map(|s| s.bytes_in).sum();
    let out_bytes: u64 = samples.iter().filter(|s| s.ok).map(|s| s.bytes_out).sum();
    let seconds = wall_ms / 1000.0;
    let lag_p95 = percentile(&delays, 0.95);

    let cpu_delta = match (service_before.cpu_seconds, service_after.cpu_seconds) {
        (Some(a), Some(b)) => Some(round3(b - a)),
        _ => None,
    };
    let driver_cpu_delta = match (driver_before.cpu_seconds, driver_after.cpu_seconds) {
        (Some(a), Some(b)) => Some(round3(b - a)),
        _ => None,
    };

    WorkloadStats {
        name: workload.name,
        count: workload.count,
        warmup: workload.warmup,
        rate_rps_requested: workload.rate,
        ok,
        errors,
        timeouts,
        throughput_rps: if seconds > 0.0 {
            round3(ok as f64 / seconds)
        } else {
            0.0
        },
        mb_per_s: if seconds > 0.0 {
            round3(served_bytes as f64 / 1_048_576.0 / seconds)
        } else {
            0.0
        },
        latency_ms: serde_json::json!({
            "p50": percentile_honest(&latencies, 0.50, ok),
            "p95": percentile_honest(&latencies, 0.95, ok),
            "p99": percentile_honest(&latencies, 0.99, ok),
            "max_ms": round3(percentile(&latencies, 1.0)),
        }),
        schedule_delay_ms: serde_json::json!({
            "p50": round3(percentile(&delays, 0.50)),
            "p95": round3(percentile(&delays, 0.95)),
        }),
        driver_saturated: driver_saturated(lag_p95, workload.rate),
        window_wall_ms: round3(wall_ms),
        service: serde_json::json!({
            "cpu_seconds": cpu_delta,
            "peak_rss_kib": peak_kib.map(round3),
        }),
        driver: serde_json::json!({ "cpu_seconds": driver_cpu_delta }),
        bytes: serde_json::json!({
            "requested_in": in_bytes,
            "served_out": out_bytes,
            "total_wire_estimate": served_bytes,
        }),
    }
}

// ──────────────────────────────────────────────────────────────────────
// The cold scene: drop the pages without root
// ──────────────────────────────────────────────────────────────────────

/// Forces every CAS file out of the page cache.
///
/// `fsync` first: a dirty page cannot be dropped, and a "cold" read of
/// bytes that were never written to the disk would measure the write
/// back. Then `POSIX_FADV_DONTNEED`, which asks the kernel to forget the
/// cached pages of that file -- unprivileged, targeted, and honest about
/// what it does NOT do: it does not stop other processes from refilling
/// the cache, so no other Hodei process may run during a cold scene.
fn evict_page_cache(root: &Path) {
    fn walk(dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path);
                continue;
            }
            let Ok(file) = std::fs::File::open(&path) else {
                continue;
            };
            unsafe {
                libc::fsync(file.as_raw_fd());
                libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
            }
        }
    }
    walk(root);
    std::thread::sleep(Duration::from_millis(250));
}

// ──────────────────────────────────────────────────────────────────────
// The proxy upstream: a tiny fixed-content server
// ──────────────────────────────────────────────────────────────────────

/// A server that answers every GET with the same body, in a thread.
struct Upstream {
    port: u16,
    hits: Arc<std::sync::atomic::AtomicUsize>,
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Upstream {
    fn start(body: Arc<Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("the upstream binds");
        let port = listener.local_addr().expect("the upstream address").port();
        let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (hits_inner, stop_inner, body_inner) =
            (Arc::clone(&hits), Arc::clone(&stop), Arc::clone(&body));
        let handle = std::thread::spawn(move || {
            listener.set_nonblocking(true).expect("nonblocking");
            while !stop_inner.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        hits_inner.fetch_add(1, Ordering::Relaxed);
                        let mut buffer = [0u8; 4096];
                        let _ = std::io::Read::read(&mut stream, &mut buffer);
                        let answer = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body_inner.len()
                        );
                        let _ = stream.write_all(answer.as_bytes());
                        let _ = stream.write_all(&body_inner);
                        let _ = stream.flush();
                        let _ = stream.shutdown(std::net::Shutdown::Both);
                    }
                    Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            port,
            hits,
            stop,
            handle: Some(handle),
        }
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::Relaxed)
    }

    fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Upstream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

// ──────────────────────────────────────────────────────────────────────
// The deployment: directories, policy files, the service child
// ──────────────────────────────────────────────────────────────────────

/// The Cedar schema of the lab: the actions the harnesses use, plus the
/// administrative surface (GC and policy evaluation are admin routes).
fn lab_schema() -> String {
    serde_json::json!({
        "": { "entityTypes": {}, "actions": {} },
        "Iam": {
            "entityTypes": { "User": { "shape": { "type": "Record", "attributes": {} } } },
            "actions": {}
        },
        "Artifacts": {
            "entityTypes": {
                "Repository": { "memberOfTypes": ["Artifacts::Repository"], "shape": { "type": "Record", "attributes": {} } },
                "Artifact": { "shape": { "type": "Record", "attributes": {} } }
            },
            "actions": {
                "CreateRepository": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
                "ReadRepository": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
                "PromoteAsset": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
                "RevokePromotion": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
                "ReadCapsule": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Artifact"] } },
                "UploadAsset": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
                "DownloadAsset": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } }
            }
        },
        "Admin": {
            "entityTypes": { "AdminApi": { "shape": { "type": "Record", "attributes": {} } } },
            "actions": {
                "AccessAdminApi": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Admin::AdminApi"] } }
            }
        }
    })
    .to_string()
}

/// The policy: alice uploads, downloads, reads capsules and enters the
/// administrative surface. The second account exists only for the
/// permission probe, and is granted nothing.
fn lab_policy() -> String {
    let mut text = String::new();
    for action in ["UploadAsset", "DownloadAsset", "ReadCapsule"] {
        text.push_str(&format!(
            "permit(principal == Iam::User::\"hrn:hodei:iam::{ACCOUNT}:User/{USER}\", action == Artifacts::Action::\"{action}\", resource);\n"
        ));
    }
    text.push_str(&format!(
        "permit(principal == Iam::User::\"hrn:hodei:iam::{ACCOUNT}:User/{USER}\", action == Admin::Action::\"AccessAdminApi\", resource);\n"
    ));
    text
}

/// The JWT of a lab user, minted the same way the harnesses mint theirs.
fn token(account: &str, user: &str) -> String {
    #[derive(Serialize)]
    struct Claims {
        sub: String,
        account: String,
        exp: usize,
        iss: String,
        aud: String,
    }
    encode(
        &Header::new(Algorithm::HS256),
        &Claims {
            sub: format!("hrn:hodei:iam::{account}:User/{user}"),
            account: account.to_string(),
            exp: 4_000_000_000,
            iss: ISSUER.to_string(),
            aud: AUDIENCE.to_string(),
        },
        &EncodingKey::from_secret(SECRET),
    )
    .expect("the token is signed")
}

/// One replica of the run: its directory, its service, its upstream.
///
/// Dropping it kills the service and releases the directories unless
/// the operator asked to keep them (`--keep`).
struct Replica {
    root: PathBuf,
    db: PathBuf,
    cas: PathBuf,
    policy: PathBuf,
    schema: PathBuf,
    log: PathBuf,
    child: Option<Child>,
    port: u16,
    keep: bool,
}

impl Replica {
    fn new(stamp: &str, index: usize, keep: bool) -> Result<Self, String> {
        let root = Path::new("target/perf-lab")
            .join(stamp)
            .join(format!("replica-{index}"));
        std::fs::create_dir_all(&root).map_err(|e| format!("the replica dir: {e}"))?;
        let me = Self {
            root: root.clone(),
            db: root.join("db"),
            cas: root.join("cas"),
            policy: root.join("policies.cedar"),
            schema: root.join("schema.json"),
            log: root.join("service.log"),
            child: None,
            port: 0,
            keep,
        };
        std::fs::write(&me.policy, lab_policy()).map_err(|e| e.to_string())?;
        std::fs::write(&me.schema, lab_schema()).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&me.db).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&me.cas).map_err(|e| e.to_string())?;
        Ok(me)
    }

    /// Runs a seed of this binary with the service NOT in place: the
    /// database directory is owned by one process at a time.
    fn run_seed_child(&mut self, mode: &str, extra: &[(&str, String)]) -> Result<(), String> {
        let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
        command.arg(mode);
        command.env("HODEI_PERF_DB", &self.db);
        command.env("HODEI_PERF_CAS", &self.cas);
        for (name, value) in extra {
            command.env(name, value);
        }
        let status = command
            .status()
            .map_err(|e| format!("the seed child: {e}"))?;
        if !status.success() {
            return Err(format!("the seed child {mode} failed with {status}"));
        }
        Ok(())
    }

    /// Starts the service on a free port. Retries with fresh ports: a
    /// port can be taken between the probe and the bind.
    fn start(&mut self, upstream_port: u16) -> Result<(), String> {
        let binary = std::env::var("HODEI_PERF_SERVICE_BIN").unwrap_or_else(|_| {
            let here = std::env::current_exe().expect("the lab binary has a path");
            here.parent()
                .map(|dir| dir.join("hodei-artifacts-api"))
                .unwrap_or_else(|| PathBuf::from("hodei-artifacts-api"))
                .to_string_lossy()
                .into_owned()
        });
        for attempt in 0..8 {
            let probe =
                TcpListener::bind("127.0.0.1:0").map_err(|e| format!("no free port: {e}"))?;
            let port = probe.local_addr().map_err(|e| e.to_string())?.port();
            drop(probe);
            let log = std::fs::File::create(&self.log).map_err(|e| e.to_string())?;
            let stdout = log.try_clone().map_err(|e| e.to_string())?;
            let mut command = Command::new(&binary);
            command
                .env("HODEI_SERVER__HOST", "127.0.0.1")
                .env("HODEI_SERVER__PORT", port.to_string())
                .env("HODEI_SERVER__REQUEST_TIMEOUT_SECS", "600")
                .env("HODEI_SERVER__MAX_BODY_SIZE", (2usize << 30).to_string())
                .env("HODEI_ROCKSDB__PATH", &self.db)
                .env("HODEI_CONTENT__CAS_PATH", &self.cas)
                .env("HODEI_REPOSITORY__DURABLE", "true")
                .env("HODEI_SECURITY__RAW_API_ENABLED", "true")
                .env("HODEI_SECURITY__OCI_API_ENABLED", "true")
                .env("HODEI_SECURITY__ADMIN_API_ENABLED", "true")
                .env(
                    "HODEI_SECURITY__AUTH_JWT_SECRET",
                    String::from_utf8_lossy(SECRET).into_owned(),
                )
                .env("HODEI_SECURITY__AUTH_ISSUER", ISSUER)
                .env("HODEI_SECURITY__AUTH_AUDIENCE", AUDIENCE)
                .env("HODEI_SECURITY__AUTHORIZATION_POLICY_PATH", &self.policy)
                .env("HODEI_SECURITY__AUTHORIZATION_SCHEMA_PATH", &self.schema)
                .env(
                    "HODEI_PROXY__ALLOWED_HOSTS",
                    format!("127.0.0.1:{upstream_port};private;http"),
                )
                .env("RUST_LOG", "info")
                .stdin(Stdio::null())
                .stdout(Stdio::from(stdout))
                .stderr(Stdio::from(log));
            let mut child = command.spawn().map_err(|e| format!("the service: {e}"))?;
            let deadline = std::time::Instant::now() + Duration::from_secs(60);
            loop {
                if bound_port(&self.log) == Some(port)
                    && TcpStream::connect(("127.0.0.1", port)).is_ok()
                    && matches!(child.try_wait(), Ok(None))
                {
                    self.child = Some(child);
                    self.port = port;
                    return Ok(());
                }
                if matches!(child.try_wait(), Ok(Some(_))) || std::time::Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            let _ = attempt;
        }
        Err(format!(
            "the service did not open a port; the log says:\n{}",
            std::fs::read_to_string(&self.log).unwrap_or_default()
        ))
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Replica {
    fn drop(&mut self) {
        self.stop();
        if !self.keep {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

/// The port the service said it bound, from its log, escape colors off.
fn bound_port(log: &Path) -> Option<u16> {
    let text = std::fs::read_to_string(log).ok()?;
    let plain: String = {
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c != '\u{1b}' {
                out.push(c);
                continue;
            }
            for escaped in chars.by_ref() {
                if escaped.is_ascii_alphabetic() {
                    break;
                }
            }
        }
        out
    };
    for line in plain.lines() {
        if let Some(rest) = line.split("Listening on http://").nth(1) {
            let address = rest.split_whitespace().next()?;
            return address.rsplit(':').next()?.trim().parse().ok();
        }
    }
    None
}

// ──────────────────────────────────────────────────────────────────────
// Seeds, done directly on the database with the service stopped
// ──────────────────────────────────────────────────────────────────────

/// Creates the repositories the HTTP workloads publish into.
async fn seed_repositories(db: &str, upstream_port: u16) -> Result<(), String> {
    use hodei_catalog::{RepositoryFormat, RepositoryMode};
    let engine = surrealdb::Surreal::new::<surrealdb::engine::local::RocksDb>(db.to_string())
        .await
        .map_err(|e| e.to_string())?;
    engine
        .use_ns("hodei")
        .use_db("artifacts")
        .await
        .map_err(|e| e.to_string())?;
    let store = hodei_catalog_storage::SurrealCatalogStore::with_authorizer(
        engine.clone(),
        Arc::new(hodei_catalog::AllowAllAuthorizer),
    );
    store.initialize().await.map_err(|e| e.to_string())?;

    let plain: &[(&str, RepositoryFormat)] = &[
        ("lab-raw", RepositoryFormat::Raw),
        ("lab-oci", RepositoryFormat::Oci),
        ("lab-npm", RepositoryFormat::Npm),
        ("lab-pypi", RepositoryFormat::PyPi),
        ("lab-maven", RepositoryFormat::Maven2),
    ];
    for (key, format) in plain {
        let mut config = hodei_catalog::RepositoryConfig::new(hodei_catalog::DigestScope::Global);
        if *key == "lab-raw" {
            config.config_settings = serde_json::json!({ "raw": { "replace": "allow" } });
        }
        create(
            &store,
            ACCOUNT,
            key,
            *format,
            RepositoryMode::Hosted,
            config,
        )
        .await?;
    }
    for index in 0..SEARCH_REPOSITORIES {
        create(
            &store,
            ACCOUNT,
            &format!("search-{index}"),
            RepositoryFormat::Raw,
            RepositoryMode::Hosted,
            hodei_catalog::RepositoryConfig::new(hodei_catalog::DigestScope::Global),
        )
        .await?;
    }

    // The proxy repository: upstream is the lab's own fixed server, and
    // the cache has no TTL so every read after the first is a hit --
    // which is exactly the comparison the workload makes.
    let proxy = serde_json::json!({
        "upstream": {
            "id": "lab",
            "url": format!("http://127.0.0.1:{upstream_port}/"),
            "scopes": ["/"],
            "max_bytes": 4194304,
        },
        "cache": {},
    });
    let mut config = hodei_catalog::RepositoryConfig::new(hodei_catalog::DigestScope::Global);
    config.config_settings = serde_json::json!({ "proxy": proxy });
    create(
        &store,
        ACCOUNT,
        "lab-proxy",
        RepositoryFormat::Raw,
        RepositoryMode::Proxy,
        config,
    )
    .await?;

    drop(store);
    drop(engine);
    Ok(())
}

async fn create(
    store: &hodei_catalog_storage::SurrealCatalogStore,
    account: &str,
    key: &str,
    format: hodei_catalog::RepositoryFormat,
    mode: hodei_catalog::RepositoryMode,
    config: hodei_catalog::RepositoryConfig,
) -> Result<(), String> {
    use hodei_catalog::CatalogWriter;
    store
        .create_repository(
            account,
            hodei_catalog::RepositoryDescriptor {
                hrn: hodei_catalog::repository_hrn("hodei", "artifacts", "eu", account, key),
                account: account.to_string(),
                repo_key: key.to_string(),
                format,
                mode,
                config,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
        )
        .await
        .map_err(|e| format!("repository {key}: {e}"))?;
    Ok(())
}

const SEARCH_REPOSITORIES: usize = 4;
const VERSIONS_PER_PACKAGE: usize = 4;

#[derive(Debug, Serialize)]
struct AssetRow {
    account: String,
    repository: String,
    asset_path: String,
    package_name: String,
    package_version: String,
    digest_algorithm: String,
    digest: String,
    size_bytes: i64,
    subject: Option<String>,
    updated_at: surrealdb::sql::Datetime,
}

#[derive(Debug, Serialize)]
struct VersionRow {
    account: String,
    repository: String,
    package_name: String,
    package_version: String,
    digest_algorithm: String,
    digest: String,
    size_bytes: i64,
    published_at: surrealdb::sql::Datetime,
}

#[derive(Debug, Serialize)]
struct TagRow {
    account: String,
    repository: String,
    tag_name: String,
    package_name: String,
    package_version: String,
    updated_at: surrealdb::sql::Datetime,
}

/// The bulk seed of the catalog: the same shape search_benchmark uses,
/// because the question is what a search COSTS against a catalog of the
/// reference size, and the honest way to get there is the bulk insert.
/// The publish path would take hours and would measure the publish.
async fn seed_catalog(db: &str, assets: usize) -> Result<f64, String> {
    let started = std::time::Instant::now();
    let engine = surrealdb::Surreal::new::<surrealdb::engine::local::RocksDb>(db.to_string())
        .await
        .map_err(|e| e.to_string())?;
    engine
        .use_ns("hodei")
        .use_db("artifacts")
        .await
        .map_err(|e| e.to_string())?;
    let store = hodei_catalog_storage::SurrealCatalogStore::with_authorizer(
        engine.clone(),
        Arc::new(hodei_catalog::AllowAllAuthorizer),
    );
    store.initialize().await.map_err(|e| e.to_string())?;

    let now = surrealdb::sql::Datetime::from(chrono::Utc::now());
    let mut index = 0usize;
    while index < assets {
        let end = (index + 5_000).min(assets);
        let mut asset_rows = Vec::new();
        let mut version_rows = Vec::new();
        for row in index..end {
            let package = format!("pkg-{}", row / VERSIONS_PER_PACKAGE);
            let version = format!("1.{}.0", row % VERSIONS_PER_PACKAGE);
            let repository = format!(
                "search-{}",
                (row / VERSIONS_PER_PACKAGE) % SEARCH_REPOSITORIES
            );
            // Every asset has its own content: if the digests
            // repeated, the CAS would store one blob for a hundred
            // thousand assets and the search would not be searching a
            // catalog of real assets.
            let digest = hex_digest(&corpus(1024, row));
            asset_rows.push(AssetRow {
                account: ACCOUNT.to_string(),
                repository: repository.clone(),
                asset_path: format!("{package}/{version}/{package}-{version}.tar.gz"),
                package_name: package.clone(),
                package_version: version.clone(),
                digest_algorithm: "sha256".to_string(),
                digest: digest.clone(),
                size_bytes: 1024,
                subject: None,
                updated_at: now.clone(),
            });
            version_rows.push(VersionRow {
                account: ACCOUNT.to_string(),
                repository,
                package_name: package,
                package_version: version,
                digest_algorithm: "sha256".to_string(),
                digest,
                size_bytes: 1024,
                published_at: now.clone(),
            });
        }
        store
            .db()
            .query("INSERT INTO catalog_asset $rows")
            .bind(("rows", asset_rows))
            .await
            .map_err(|e| e.to_string())?
            .check()
            .map_err(|e| e.to_string())?;
        store
            .db()
            .query("INSERT INTO catalog_version $rows")
            .bind(("rows", version_rows))
            .await
            .map_err(|e| e.to_string())?
            .check()
            .map_err(|e| e.to_string())?;
        index = end;
    }
    let mut tag_rows = Vec::new();
    for slot in 0..SEARCH_REPOSITORIES {
        let row = slot * VERSIONS_PER_PACKAGE;
        tag_rows.push(TagRow {
            account: ACCOUNT.to_string(),
            repository: format!("search-{slot}"),
            tag_name: "latest".to_string(),
            package_name: format!("pkg-{}", row / VERSIONS_PER_PACKAGE),
            package_version: format!("1.{}.0", row % VERSIONS_PER_PACKAGE),
            updated_at: now.clone(),
        });
    }
    store
        .db()
        .query("INSERT INTO catalog_tag $rows")
        .bind(("rows", tag_rows))
        .await
        .map_err(|e| e.to_string())?
        .check()
        .map_err(|e| e.to_string())?;
    drop(store);
    drop(engine);
    Ok(started.elapsed().as_secs_f64())
}

/// The HRN a capsule is asked for: an artifact of the lab account.
fn artifact_hrn(index: usize) -> String {
    format!("hrn:hodei:artifacts:eu:{ACCOUNT}:artifact/lab-{index}")
}

/// The bulk seed of evidence for the capsule workload.
///
/// Same engine, same namespace and database as the service: the capsule
/// handler reads the evidence of a SUBJECT, so every envelope names one
/// artifact of the lab account and the content lands in the same CAS the
/// service serves -- the capsule is then read over HTTP against a real,
/// deployed stack, not against a stub.
async fn seed_evidence(db: &str, cas: &str, count: usize) -> Result<f64, String> {
    let started = std::time::Instant::now();
    let engine = surrealdb::Surreal::new::<surrealdb::engine::local::RocksDb>(db.to_string())
        .await
        .map_err(|e| e.to_string())?;
    engine
        .use_ns("hodei")
        .use_db("artifacts")
        .await
        .map_err(|e| e.to_string())?;

    // The evidence store of the domain needs a *writing* content store;
    // the CAS-backed adapter of the investigation surface only reads --
    // the service never publishes through it. The seed writes the bytes
    // itself into the same CAS the service serves, so the capsule read
    // finds the content behind its digest: ingest records the envelope,
    // and the store hands back what was put.
    let blobs: Arc<dyn hodei_hub::blob::BlobStore> =
        Arc::new(hodei_hub::FileBlobStore::new(cas).map_err(|e| e.to_string())?);
    struct SeedingContentStore(Arc<dyn hodei_hub::blob::BlobStore>);
    #[async_trait::async_trait]
    impl hodei_evidence::ports::ContentStore for SeedingContentStore {
        async fn put(
            &self,
            content: Vec<u8>,
        ) -> Result<hodei_evidence::ContentDigest, hodei_evidence::ports::StoreError> {
            self.0
                .put(&content)
                .await
                .map_err(|e| hodei_evidence::ports::StoreError::Unavailable(e.to_string()))
        }
        async fn get(
            &self,
            digest: &hodei_evidence::ContentDigest,
        ) -> Result<Option<Vec<u8>>, hodei_evidence::ports::StoreError> {
            match self.0.get(digest).await {
                Ok(bytes) => Ok(Some(bytes)),
                Err(hodei_hub::blob::BlobError::NotFound) => Ok(None),
                Err(e) => Err(hodei_evidence::ports::StoreError::Unavailable(
                    e.to_string(),
                )),
            }
        }
        async fn size(
            &self,
            digest: &hodei_evidence::ContentDigest,
        ) -> Result<Option<u64>, hodei_evidence::ports::StoreError> {
            match self.0.length(digest).await {
                Ok(length) => Ok(Some(length)),
                Err(hodei_hub::blob::BlobError::NotFound) => Ok(None),
                Err(e) => Err(hodei_evidence::ports::StoreError::Unavailable(
                    e.to_string(),
                )),
            }
        }
    }
    let content: Arc<dyn hodei_evidence::ports::ContentStore> =
        Arc::new(SeedingContentStore(Arc::clone(&blobs)));
    let engine = Arc::new(engine);
    let ledger = Arc::new(hodei_evidence_storage::SurrealEventLedger::new(Arc::clone(
        &engine,
    )));
    let evidence_store = Arc::new(hodei_evidence_storage::SurrealEvidenceStore::new(
        Arc::clone(&engine),
    ));
    let authenticator = Arc::new(hodei_evidence_storage::HmacEnvelopeAuthenticator::new());
    authenticator.register(
        "perf-lab",
        hodei_evidence_storage::ProducerCredentials {
            secret: b"the-secret-of-the-perf-lab".to_vec(),
            accounts: vec![ACCOUNT.to_string()],
        },
    );
    let ingestion = hodei_evidence::EvidenceIngestion::new(
        content,
        evidence_store,
        ledger,
        authenticator.clone(),
    );

    for index in 0..count {
        let body = format!("perf-lab evidence #{index}").into_bytes();
        let mut envelope = hodei_evidence::IngestionEnvelope {
            producer: hodei_evidence::Producer::new("connector", "perf-lab").with_version("1.0.0"),
            tenant_hrn: kernel::Hrn::from_string(&format!(
                "hrn:hodei:iam:eu:{ACCOUNT}:User/{USER}"
            ))
            .expect("the tenant of the lab is a valid HRN"),
            subject_refs: vec![kernel::Hrn::from_string(&artifact_hrn(index % 50))
                .expect("the subject of the lab is a valid HRN")],
            evidence_type: hodei_evidence::EvidenceType::Provenance,
            schema_uri: None,
            schema_version: None,
            source: hodei_evidence::SourceRef::by_external_id(format!("perf-evd-{index}")),
            content_digest: hodei_evidence::ContentDigest::of_bytes(&body),
            relation_claim: None,
            observed_at: chrono::Utc::now(),
            valid_from: None,
            verification: hodei_evidence::EvidenceVerification::verified("signature"),
            normalization: hodei_evidence::Normalization::new("perf-lab", "1.0.0"),
            correlation_id: None,
            signature: String::new(),
        };
        envelope.signature = authenticator
            .sign(&envelope)
            .expect("the producer is known");
        ingestion
            .ingest(hodei_evidence::IngestionRequest {
                envelope,
                content: body,
            })
            .await
            .map_err(|e| format!("evidence {index}: {e}"))?;
    }
    drop(engine);
    Ok(started.elapsed().as_secs_f64())
}

// ──────────────────────────────────────────────────────────────────────
// Workload construction for one replica
// ──────────────────────────────────────────────────────────────────────

/// Everything a workload builder needs that is shared and stable.
struct Ctx {
    token: String,
    /// The digest of slot 0 of the one-mebibyte corpus: the blob
    /// oci-push-blob stores first and oci-pull-blob then reads.
    oci_blob_digest: String,
}

fn timeout_for(size: usize) -> Duration {
    // 1 KiB requests need seconds; a 1 GiB body over the loopback needs
    // minutes. The service timeout was raised to 600 s; the client gives
    // the same room so a timeout means the service, not the client.
    Duration::from_secs(60.max((size as u64) / (32 * 1024 * 1024)))
}

/// Builds every workload of a profile, in the canonical order.
fn build_workloads(profile: &Profile, ctx: &Ctx) -> Vec<Workload> {
    let mut workloads = Vec::new();
    let warmup = profile.warmup;

    for point in &profile.points {
        let prefix = format!("w{}", point.bytes);
        let name = size_label(point.bytes, "write");
        let auth_ctx_token = ctx.token.clone();
        let size = point.bytes;
        workloads.push(Workload {
            name: leak(name),
            count: point.write_count,
            warmup: warmup.min(point.write_count),
            rate: point.write_rate,
            timeout: timeout_for(point.bytes),
            evict_first: false,
            make: Box::new(move |index: usize| RequestSpec {
                method: reqwest::Method::PUT,
                path: format!("/repository/lab-raw/{prefix}/{index}/blob"),
                headers: bearer(&auth_ctx_token),
                // Every slot writes its own content: the seed is the
                // slot, so the CAS stores as many blobs as requests.
                body: Some(Arc::new(corpus(size, index))),
                expect: None,
            }),
        });
    }

    for point in &profile.points {
        if point.hit_count == 0 {
            continue;
        }
        let prefix = format!("w{}", point.bytes);
        let name = size_label(point.bytes, "read-hit");
        let token = ctx.token.clone();
        let slots = point.write_count;
        workloads.push(Workload {
            name: leak(name),
            count: point.hit_count,
            warmup: warmup.min(point.hit_count),
            rate: point.hit_rate,
            timeout: timeout_for(point.bytes),
            evict_first: false,
            // Hits read the assets the write scene wrote: the index
            // wraps around the written slots so every read finds a
            // blob, even when the scene reads more times than it
            // wrote.
            make: Box::new(move |index: usize| RequestSpec {
                method: reqwest::Method::GET,
                path: format!("/repository/lab-raw/{prefix}/{}/blob", index % slots),
                headers: bearer(&token),
                body: None,
                expect: Some(200),
            }),
        });
    }

    for point in &profile.points {
        if point.miss_count == 0 {
            continue;
        }
        let prefix = format!("m{}", point.bytes);
        let name = size_label(point.bytes, "read-miss");
        let token = ctx.token.clone();
        workloads.push(Workload {
            name: leak(name),
            count: point.miss_count,
            warmup: 0,
            rate: point.miss_rate,
            timeout: timeout_for(point.bytes),
            evict_first: false,
            // The assets of this scene were PUT by the pre-write phase
            // and have never been READ: a miss is "first read of bytes
            // that are already on disk", which is neither the warm hit
            // nor the evicted cold scene.
            make: Box::new(move |index: usize| RequestSpec {
                method: reqwest::Method::GET,
                path: format!("/repository/lab-raw/{prefix}/{index}/blob"),
                headers: bearer(&token),
                body: None,
                expect: Some(200),
            }),
        });
    }

    for point in &profile.points {
        if point.cold_count == 0 {
            continue;
        }
        let prefix = format!("w{}", point.bytes);
        let name = size_label(point.bytes, "read-cold");
        let token = ctx.token.clone();
        let slots = point.write_count;
        workloads.push(Workload {
            name: leak(name),
            count: point.cold_count,
            warmup: 0,
            rate: point.cold_rate,
            timeout: timeout_for(point.bytes),
            evict_first: true,
            make: Box::new(move |index: usize| RequestSpec {
                method: reqwest::Method::GET,
                path: format!("/repository/lab-raw/{prefix}/{}/blob", index % slots),
                headers: bearer(&token),
                body: None,
                expect: Some(200),
            }),
        });
    }

    // ── OCI ───────────────────────────────────────────────────────────
    let token = ctx.token.clone();
    let blob_size = 1_048_576;
    workloads.push(Workload {
        name: "oci-push-blob",
        count: profile.protocol_count,
        warmup: warmup.min(profile.protocol_count),
        rate: profile.protocol_rate,
        timeout: Duration::from_secs(60),
        evict_first: false,
        make: Box::new(move |index: usize| {
            // Each slot pushes its own bytes and declares its own
            // digest: the push is a real store, not a re-existence.
            let body = Arc::new(corpus(blob_size, index));
            let digest = hex_digest(&body);
            RequestSpec {
                method: reqwest::Method::POST,
                path: format!("/v2/lab-oci/blobs/uploads/?digest=sha256:{digest}"),
                headers: bearer(&token),
                body: Some(body),
                expect: None,
            }
        }),
    });

    let token = ctx.token.clone();
    let manifest_digest = ctx.oci_blob_digest.clone();
    workloads.push(Workload {
        name: "oci-push-manifest",
        count: profile.protocol_count / 4,
        warmup: 2,
        rate: profile.protocol_rate / 2.0,
        timeout: Duration::from_secs(60),
        evict_first: false,
        make: Box::new(move |index: usize| {
            // One tag per slot: re-PUTting the same tag with the same
            // bytes would measure the idempotence, not the publish.
            let manifest = serde_json::json!({
                "schemaVersion": 2,
                "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "config": { "mediaType": "application/vnd.oci.image.config.v1+json", "digest": format!("sha256:{manifest_digest}"), "size": 1_048_576u64 },
                "layers": [ { "mediaType": "application/vnd.oci.image.layer.v1.tar", "digest": format!("sha256:{manifest_digest}"), "size": 1_048_576u64 } ],
            })
            .to_string();
            RequestSpec {
                method: reqwest::Method::PUT,
                path: format!("/v2/lab-oci/manifests/lab-{index}"),
                headers: {
                    let mut headers = bearer(&token);
                    headers.push(("content-type".to_string(), "application/vnd.oci.image.manifest.v1+json".to_string()));
                    headers
                },
                body: Some(Arc::new(manifest.into_bytes())),
                expect: None,
            }
        }),
    });

    let token = ctx.token.clone();
    let pushed_tags = (profile.protocol_count / 4).max(1);
    workloads.push(Workload {
        name: "oci-pull-manifest",
        count: profile.protocol_count,
        warmup: warmup.min(profile.protocol_count),
        rate: profile.protocol_rate,
        timeout: Duration::from_secs(60),
        evict_first: false,
        make: Box::new(move |index: usize| RequestSpec {
            // The tags the push scene published (and the ones its
            // warmup published beyond the range): a pull of a tag
            // that does not exist would measure a 404.
            method: reqwest::Method::GET,
            path: format!("/v2/lab-oci/manifests/lab-{}", index % pushed_tags),
            headers: {
                let mut headers = bearer(&token);
                headers.push((
                    "accept".to_string(),
                    "application/vnd.oci.image.manifest.v1+json".to_string(),
                ));
                headers
            },
            body: None,
            expect: Some(200),
        }),
    });

    let token = ctx.token.clone();
    let digest = ctx.oci_blob_digest.clone();
    workloads.push(Workload {
        name: "oci-pull-blob",
        count: profile.protocol_count,
        warmup: warmup.min(profile.protocol_count),
        rate: profile.protocol_rate,
        timeout: Duration::from_secs(60),
        evict_first: false,
        make: Box::new(move |_index: usize| RequestSpec {
            method: reqwest::Method::GET,
            path: format!("/v2/lab-oci/blobs/sha256:{digest}"),
            headers: bearer(&token),
            body: None,
            expect: Some(200),
        }),
    });

    // ── npm ───────────────────────────────────────────────────────────
    let token = ctx.token.clone();
    let tarball_size = 32_768;
    workloads.push(Workload {
        name: "npm-publish",
        count: profile.protocol_count,
        warmup: warmup.min(profile.protocol_count),
        rate: profile.protocol_rate,
        timeout: Duration::from_secs(60),
        evict_first: false,
        make: Box::new(move |index: usize| {
            // One package per slot: the name in the URL, the name in
            // the body and the bytes of the tarball all agree, and no
            // two slots share their content.
            let tarball = corpus(tarball_size, index);
            RequestSpec {
                method: reqwest::Method::PUT,
                path: format!("/npm/lab-npm/@acme%2flabwidget-{index}"),
                headers: {
                    let mut headers = bearer(&token);
                    headers.push(("content-type".to_string(), "application/json".to_string()));
                    headers
                },
                body: Some(Arc::new(build_npm_publish(index, &tarball))),
                expect: None,
            }
        }),
    });

    let token = ctx.token.clone();
    workloads.push(Workload {
        name: "npm-download",
        count: profile.protocol_count,
        warmup: warmup.min(profile.protocol_count),
        rate: profile.protocol_rate,
        timeout: Duration::from_secs(60),
        evict_first: false,
        make: Box::new(move |index: usize| RequestSpec {
            method: reqwest::Method::GET,
            path: format!("/npm/lab-npm/@acme/labwidget-{index}/-/labwidget-{index}-1.0.0.tgz"),
            headers: bearer(&token),
            body: None,
            expect: Some(200),
        }),
    });

    // ── PyPI ──────────────────────────────────────────────────────────
    let token = ctx.token.clone();
    let wheel_size = 65_536;
    workloads.push(Workload {
        name: "pypi-upload",
        count: profile.protocol_count,
        warmup: warmup.min(profile.protocol_count),
        rate: profile.protocol_rate,
        timeout: Duration::from_secs(60),
        evict_first: false,
        make: Box::new(move |index: usize| {
            let wheel = corpus(wheel_size, index);
            let wheel_sha = hex_digest(&wheel);
            RequestSpec {
                method: reqwest::Method::POST,
                path: "/pypi/lab-pypi/".to_string(),
                headers: {
                    let mut headers = bearer(&token);
                    headers.push((
                        "content-type".to_string(),
                        "multipart/form-data; boundary=hodeipypiboundary".to_string(),
                    ));
                    headers
                },
                body: Some(Arc::new(build_pypi_upload(index, &wheel, &wheel_sha))),
                expect: None,
            }
        }),
    });

    let token = ctx.token.clone();
    workloads.push(Workload {
        name: "pypi-index",
        count: profile.protocol_count,
        warmup: warmup.min(profile.protocol_count),
        rate: profile.protocol_rate,
        timeout: Duration::from_secs(60),
        evict_first: false,
        make: Box::new(move |index: usize| RequestSpec {
            method: reqwest::Method::GET,
            path: format!("/pypi/lab-pypi/simple/labpkg{index}/"),
            headers: bearer(&token),
            body: None,
            expect: Some(200),
        }),
    });

    let token = ctx.token.clone();
    workloads.push(Workload {
        name: "pypi-file",
        count: profile.protocol_count,
        warmup: warmup.min(profile.protocol_count),
        rate: profile.protocol_rate,
        timeout: Duration::from_secs(60),
        evict_first: false,
        make: Box::new(move |index: usize| RequestSpec {
            method: reqwest::Method::GET,
            path: format!("/pypi/lab-pypi/files/labpkg{index}/labpkg{index}-1.0-py3-none-any.whl"),
            headers: bearer(&token),
            body: None,
            expect: Some(200),
        }),
    });

    // ── Maven ─────────────────────────────────────────────────────────
    let token = ctx.token.clone();
    let jar_size = 65_536;
    workloads.push(Workload {
        name: "maven-upload",
        count: profile.protocol_count,
        warmup: warmup.min(profile.protocol_count),
        rate: profile.protocol_rate,
        timeout: Duration::from_secs(60),
        evict_first: false,
        make: Box::new(move |index: usize| RequestSpec {
            method: reqwest::Method::PUT,
            path: format!("/repository/lab-maven/com/acme/lib{index}/1.0/lib{index}-1.0.jar"),
            headers: bearer(&token),
            body: Some(Arc::new(corpus(jar_size, index))),
            expect: None,
        }),
    });

    let token = ctx.token.clone();
    workloads.push(Workload {
        name: "maven-download",
        count: profile.protocol_count,
        warmup: warmup.min(profile.protocol_count),
        rate: profile.protocol_rate,
        timeout: Duration::from_secs(60),
        evict_first: false,
        make: Box::new(move |index: usize| RequestSpec {
            method: reqwest::Method::GET,
            path: format!("/repository/lab-maven/com/acme/lib{index}/1.0/lib{index}-1.0.jar"),
            headers: bearer(&token),
            body: None,
            expect: Some(200),
        }),
    });

    // ── Proxy ─────────────────────────────────────────────────────────
    let token = ctx.token.clone();
    let count = profile.proxy_count;
    workloads.push(Workload {
        name: "proxy-miss",
        count,
        warmup: 0,
        rate: profile.proxy_rate,
        timeout: Duration::from_secs(60),
        evict_first: false,
        make: Box::new(move |index: usize| RequestSpec {
            // Unique paths: none of them was read before, so the cache
            // answers every one of them with a trip to the upstream.
            method: reqwest::Method::GET,
            path: format!("/repository/lab-proxy/object/{index}/blob"),
            headers: bearer(&token),
            body: None,
            expect: Some(200),
        }),
    });

    let token = ctx.token.clone();
    workloads.push(Workload {
        name: "proxy-hit",
        count: count * 4,
        warmup: warmup.min(count * 4),
        rate: profile.proxy_rate * 4.0,
        timeout: Duration::from_secs(60),
        evict_first: false,
        make: Box::new(move |index: usize| RequestSpec {
            // The paths the miss scene just cached: the same object,
            // now served by the cache instead of the upstream.
            method: reqwest::Method::GET,
            path: format!("/repository/lab-proxy/object/{}/blob", index % count),
            headers: bearer(&token),
            body: None,
            expect: Some(200),
        }),
    });

    // ── Catalog search ────────────────────────────────────────────────
    let token = ctx.token.clone();
    let packages = profile.catalog_assets / VERSIONS_PER_PACKAGE;
    workloads.push(Workload {
        name: "search-package",
        count: profile.search_count,
        warmup: 3,
        rate: profile.search_rate,
        timeout: Duration::from_secs(120),
        evict_first: false,
        make: Box::new(move |index: usize| RequestSpec {
            method: reqwest::Method::GET,
            path: format!("/api/v1/search?package=pkg-{}", index % packages),
            headers: bearer(&token),
            body: None,
            expect: Some(200),
        }),
    });

    let token = ctx.token.clone();
    // The digest of asset N is the digest of its content, and the seed
    // wrote that content from the same deterministic corpus: recomputing
    // it here costs one sha256 of 1 KiB per request and keeps the query
    // honest -- it asks for a digest that is really in the catalog.
    let assets = profile.catalog_assets;
    workloads.push(Workload {
        name: "search-digest",
        count: profile.search_count,
        warmup: 3,
        rate: profile.search_rate,
        timeout: Duration::from_secs(120),
        evict_first: false,
        make: Box::new(move |index: usize| RequestSpec {
            method: reqwest::Method::GET,
            path: format!(
                "/api/v1/search?digest=sha256:{}",
                hex_digest(&corpus(1024, index % assets))
            ),
            headers: bearer(&token),
            body: None,
            expect: Some(200),
        }),
    });

    // ── Capsule ───────────────────────────────────────────────────────
    let token = ctx.token.clone();
    workloads.push(Workload {
        name: "capsule-read",
        count: profile.capsule_count,
        warmup: warmup.min(profile.capsule_count),
        rate: profile.capsule_rate,
        timeout: Duration::from_secs(60),
        evict_first: false,
        make: Box::new(move |index: usize| RequestSpec {
            method: reqwest::Method::GET,
            path: format!("/api/v1/capsules?hrn={}", artifact_hrn(index % 50)),
            headers: bearer(&token),
            body: None,
            expect: Some(200),
        }),
    });

    // ── Authorization and administration ─────────────────────────────
    let token = ctx.token.clone();
    let evaluation = serde_json::json!({
        "principal_hrn": format!("hrn:hodei:iam::{ACCOUNT}:User/{USER}"),
        "action": "ReadRepository",
        "resource_hrn": "hrn:hodei:artifacts:eu:acme:repository/lab-raw",
        "policies": [format!(
            "permit(principal == Iam::User::\"hrn:hodei:iam::{ACCOUNT}:User/{USER}\", action == Artifacts::Action::\"ReadRepository\", resource);"
        )],
        "context": {},
        "schema_version": null,
        "evaluation_mode": "BestEffortNoSchema",
    })
    .to_string();
    workloads.push(Workload {
        name: "policy-evaluate",
        count: profile.evaluate_count,
        warmup: warmup.min(profile.evaluate_count),
        rate: profile.evaluate_rate,
        timeout: Duration::from_secs(60),
        evict_first: false,
        make: Box::new(move |_index: usize| RequestSpec {
            method: reqwest::Method::POST,
            path: "/api/v1/policies/evaluate".to_string(),
            headers: {
                let mut headers = bearer(&token);
                headers.push(("content-type".to_string(), "application/json".to_string()));
                headers
            },
            body: Some(Arc::new(evaluation.clone().into_bytes())),
            expect: None,
        }),
    });

    let token = ctx.token.clone();
    workloads.push(Workload {
        name: "gc-plan",
        count: profile.gc_count,
        warmup: 1,
        rate: profile.gc_rate,
        timeout: Duration::from_secs(120),
        evict_first: false,
        make: Box::new(move |_index: usize| RequestSpec {
            method: reqwest::Method::GET,
            path: "/api/v1/gc/plan".to_string(),
            headers: bearer(&token),
            body: None,
            expect: Some(200),
        }),
    });

    // Keep the canonical order: names not in WORKLOAD_ORDER are a bug.
    let known: std::collections::HashSet<&str> = WORKLOAD_ORDER.iter().copied().collect();
    workloads.retain(|workload| known.contains(workload.name));
    workloads.sort_by_key(|workload| {
        WORKLOAD_ORDER
            .iter()
            .position(|name| *name == workload.name)
            .unwrap_or(usize::MAX)
    });
    workloads
}

/// A stable label for a size point: `raw-write-1mib`.
fn size_label(bytes: usize, kind: &str) -> String {
    let unit = match bytes {
        1024 => "1kib",
        65_536 => "64kib",
        1_048_576 => "1mib",
        67_108_864 => "64mib",
        1_073_741_824 => "1gib",
        other => return format!("raw-{kind}-{other}b"),
    };
    format!("raw-{kind}-{unit}")
}

/// Extends a `&'static str` lifetime for labels built at runtime.
///
/// The set of labels is fixed by the profiles, so the leak is bounded
/// (a handful of short strings per process) and never repeats in a way
/// that matters; the alternative, `Box::leak` in a loop, would grow per
/// request, and this is not per request.
fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// The body of an `npm publish` of one package.
fn build_npm_publish(index: usize, tarball: &[u8]) -> Vec<u8> {
    let name = format!("@acme/labwidget-{index}");
    let base = format!("labwidget-{index}");
    let integrity = format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(Sha512::digest(tarball))
    );
    let file = format!("{base}-1.0.0.tgz");
    let body = serde_json::json!({
        "_id": name,
        "name": name,
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": { "name": name, "version": "1.0.0" } },
        "_attachments": { file: {
            "content_type": "application/octet-stream",
            "data": base64::engine::general_purpose::STANDARD.encode(tarball),
            "length": tarball.len(),
            "integrity": integrity,
        } },
    });
    serde_json::to_vec(&body).expect("the publication serializes")
}

/// The multipart body of a `twine upload`.
fn build_pypi_upload(index: usize, wheel: &[u8], sha256: &str) -> Vec<u8> {
    let boundary = "hodeipypiboundary";
    // The name carries no dash: the wheel filename is parsed as
    // `{distribution}-{version}-{python}-{abi}-{platform}.whl`, and a
    // distribution with a dash inside would make the server read the
    // slot number as the version and refuse the upload.
    let name = format!("labpkg{index}");
    let filename = format!("labpkg{index}-1.0-py3-none-any.whl");
    let mut body: Vec<u8> = Vec::new();
    for (field, value) in [
        (":action", "file_upload"),
        ("protocol_version", "1"),
        ("name", name.as_str()),
        ("version", "1.0"),
        ("metadata_version", "2.1"),
        ("sha256_digest", sha256),
    ] {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"content\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(wheel);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

// ──────────────────────────────────────────────────────────────────────
// Host state and disk accounting
// ──────────────────────────────────────────────────────────────────────

fn disk_usage(path: &Path) -> Option<u64> {
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let c_path = std::ffi::CString::new(path.to_string_lossy().as_bytes()).ok()?;
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) };
    if rc != 0 {
        return None;
    }
    let used_blocks = stat.f_blocks.checked_sub(stat.f_bfree)?;
    Some(used_blocks * stat.f_frsize as u64)
}

fn host_state(profile: &Profile, cas: &Path) -> serde_json::Value {
    let nproc = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(0);
    let governor = std::fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    let loadavg = std::fs::read_to_string("/proc/loadavg")
        .unwrap_or_default()
        .trim()
        .to_string();
    let meminfo = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let mem_total = meminfo
        .lines()
        .find_map(|line| {
            line.strip_prefix("MemTotal:")
                .map(|rest| rest.trim().to_string())
        })
        .unwrap_or_default();
    let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .unwrap_or_default()
        .trim()
        .to_string();
    serde_json::json!({
        "nproc": nproc,
        "cpu_governor": governor,
        "loadavg": loadavg,
        "mem_total": mem_total,
        "kernel": kernel,
        "cas_device": format!("{:?}", cas),
        "profile_fingerprint": profile.fingerprint(),
        "workload_order_hash": hex_digest(WORKLOAD_ORDER.join("|").as_bytes()),
        "config_hash": hex_digest(format!("{}|{}", lab_policy(), lab_schema()).as_bytes()),
    })
}

fn the_commit() -> String {
    Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                String::from_utf8(out.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

// ──────────────────────────────────────────────────────────────────────
// Permission probe: the semantics check, not a performance one
// ──────────────────────────────────────────────────────────────────────

/// The permission semantics of AC#3 run as a probe, sequentially and
/// outside every measured window. If the probe fails, the replica's
/// numbers are still written but the replica is marked not comparable.
async fn permission_probe(base: &str, ctx: &Ctx) -> serde_json::Value {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("probe client");
    let existing = format!("{base}/repository/lab-raw/w1024/0/blob");
    let anonymous = client
        .get(&existing)
        .send()
        .await
        .map(|r| r.status().as_u16())
        .unwrap_or(0);
    let intruder = client
        .get(&existing)
        .header(
            "authorization",
            format!("Bearer {}", token(OTHER_ACCOUNT, "mallory")),
        )
        .send()
        .await
        .map(|r| r.status().as_u16())
        .unwrap_or(0);
    let owner = client
        .get(&existing)
        .header("authorization", format!("Bearer {}", ctx.token))
        .send()
        .await
        .map(|r| r.status().as_u16())
        .unwrap_or(0);
    serde_json::json!({
        "anonymous": { "status": anonymous, "expected": 401, "pass": anonymous == 401 },
        "other_account": { "status": intruder, "expected": 404, "pass": intruder == 404 },
        "owner": { "status": owner, "expected": 200, "pass": owner == 200 },
    })
}

// ──────────────────────────────────────────────────────────────────────
// The miss-scene pre-write
// ──────────────────────────────────────────────────────────────────────

/// Writes the assets the miss scenes read, before the miss window, so
/// that "miss" means "never read by the cache", "present on disk".
/// The same path the GET of the scene will ask for, byte for byte.
async fn prewrite_misses(base: &str, ctx: &Ctx, profile: &Profile) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .expect("the prewrite client");
    for point in &profile.points {
        for index in 0..point.miss_count {
            let body = corpus(point.bytes, index);
            let response = client
                .put(format!(
                    "{base}/repository/lab-raw/m{}/{index}/blob",
                    point.bytes
                ))
                .header("authorization", format!("Bearer {}", ctx.token))
                .body(body)
                .send()
                .await
                .map_err(|e| format!("prewrite: {e}"))?;
            if !response.status().is_success() {
                return Err(format!("prewrite status {}", response.status()));
            }
        }
    }
    Ok(())
}

// ──────────────────────────────────────────────────────────────────────
// The run of one replica
// ──────────────────────────────────────────────────────────────────────

async fn run_replica(
    stamp: &str,
    index: usize,
    profile: &Profile,
    keep: bool,
) -> Result<serde_json::Value, String> {
    let mut replica = Replica::new(stamp, index, keep)?;
    let proxy_body = Arc::new(corpus(65_536, 0));
    let upstream = Upstream::start(Arc::clone(&proxy_body));
    let upstream_before = upstream.hits();

    // Seeds: children with the service stopped (the database directory
    // is owned by one process).
    replica.run_seed_child(
        "--seed-repos",
        &[("HODEI_PERF_UPSTREAM", upstream.port.to_string())],
    )?;
    let catalog_seconds = {
        let started = std::time::Instant::now();
        replica.run_seed_child(
            "--seed-catalog",
            &[("HODEI_PERF_ASSETS", profile.catalog_assets.to_string())],
        )?;
        started.elapsed().as_secs_f64()
    };
    let evidence_seconds = {
        let started = std::time::Instant::now();
        replica.run_seed_child(
            "--seed-evidence",
            &[("HODEI_PERF_EVIDENCE", profile.evidence.to_string())],
        )?;
        started.elapsed().as_secs_f64()
    };

    replica.start(upstream.port)?;
    let base = format!("http://127.0.0.1:{}", replica.port);
    let service_pid = replica.child.as_ref().expect("the service is up").id();

    let ctx = Ctx {
        token: token(ACCOUNT, USER),
        oci_blob_digest: hex_digest(&corpus(1_048_576, 0)),
    };

    prewrite_misses(&base, &ctx, profile).await?;

    let disk_before = disk_usage(Path::new("."));
    let mut results = Vec::new();
    let mut invalid = Vec::new();
    let workloads = build_workloads(profile, &ctx);
    for workload in &workloads {
        let stats = drive(workload, &base, &ctx.token, service_pid, &replica.cas).await;
        if stats.driver_saturated {
            invalid.push(workload.name);
        }
        results.push(serde_json::to_value(&stats).expect("the stats serialize"));
    }
    let disk_after = disk_usage(Path::new("."));
    let upstream_hits = upstream.hits() - upstream_before;

    let probe = permission_probe(&base, &ctx).await;
    let probe_ok = probe["anonymous"]["pass"] == serde_json::json!(true)
        && probe["other_account"]["pass"] == serde_json::json!(true)
        && probe["owner"]["pass"] == serde_json::json!(true);

    let served_bytes: u64 = results
        .iter()
        .filter_map(|r| r["bytes"]["total_wire_estimate"].as_u64())
        .sum();
    let disk_delta = match (disk_before, disk_after) {
        (Some(a), Some(b)) if b >= a => Some(b - a),
        _ => None,
    };

    let value = serde_json::json!({
        "replica": index,
        "seeds": {
            "catalog_assets": profile.catalog_assets,
            "catalog_bulk_seconds": round3(catalog_seconds),
            "evidence": profile.evidence,
            "evidence_seconds": round3(evidence_seconds),
        },
        "workloads": results,
        "driver_invalid_workloads": invalid,
        "permission_probe": probe,
        "permission_probe_ok": probe_ok,
        "disk": {
            "used_before_bytes": disk_before,
            "used_after_bytes": disk_after,
            "delta_bytes": disk_delta,
        },
        "cost_per_gib": {
            "definition": "amplification of the resource: the bytes the filesystem grew during the measured windows divided by the bytes the workloads moved. A number far above 1 says the service costs more disk than the traffic it served.",
            "served_bytes": served_bytes,
            "disk_delta_per_served_byte": match disk_delta {
                Some(delta) if served_bytes > 0 => serde_json::json!(round3(delta as f64 / served_bytes as f64)),
                _ => serde_json::Value::Null,
            },
        },
        "proxy_upstream_hits": upstream_hits,
        "wire_bytes_estimate_total": served_bytes,
    });
    replica.stop();
    upstream.stop();
    Ok(value)
}

// ──────────────────────────────────────────────────────────────────────
// Aggregation, baseline and budget
// ──────────────────────────────────────────────────────────────────────

/// One measured number per workload, from one replica.
fn scalar_of(stats: &serde_json::Value, pointer: &str) -> Option<f64> {
    let path: Vec<&str> = pointer.split('.').collect();
    let mut node = stats;
    for key in path {
        node = node.get(key)?;
    }
    node.as_f64()
}

fn aggregate(replicas: &[serde_json::Value]) -> serde_json::Value {
    let mut by_name: BTreeMap<&'static str, Vec<serde_json::Value>> = BTreeMap::new();
    for replica in replicas {
        let Some(workloads) = replica["workloads"].as_array() else {
            continue;
        };
        for stats in workloads {
            let Some(name) = stats["name"].as_str() else {
                continue;
            };
            // The lifetime of the key comes from WORKLOAD_ORDER; a
            // workload outside it was already dropped by the builder.
            let key: &'static str = leak(name.to_string());
            by_name.entry(key).or_default().push(stats.clone());
        }
    }
    let mut aggregate = serde_json::Map::new();
    for (name, runs) in by_name {
        let p99s: Vec<f64> = runs
            .iter()
            .filter_map(|r| scalar_of(r, "latency_ms.p99.value_ms"))
            .collect();
        let p50s: Vec<f64> = runs
            .iter()
            .filter_map(|r| scalar_of(r, "latency_ms.p50.value_ms"))
            .collect();
        let rps: Vec<f64> = runs
            .iter()
            .filter_map(|r| r["throughput_rps"].as_f64())
            .collect();
        let errors: usize = runs
            .iter()
            .filter_map(|r| r["errors"].as_u64())
            .map(|e| e as usize)
            .sum();
        let timeouts: usize = runs
            .iter()
            .filter_map(|r| r["timeouts"].as_u64())
            .map(|e| e as usize)
            .sum();
        let saturated: Vec<&str> = runs
            .iter()
            .filter(|r| r["driver_saturated"] == serde_json::json!(true))
            .filter_map(|r| r["name"].as_str())
            .collect();
        let entry = serde_json::json!({
            "replicas_with_samples": runs.len(),
            "p50_ms_median": if p50s.is_empty() { serde_json::Value::Null } else { serde_json::json!(round3(median(&p50s))) },
            "p95_ms_median": if runs.is_empty() { serde_json::Value::Null } else { serde_json::json!(round3(median(&runs.iter().filter_map(|r| scalar_of(r, "latency_ms.p95.value_ms")).collect::<Vec<f64>>()))) },
            "p99_ms_median": if p99s.is_empty() { serde_json::json!(null) } else { serde_json::json!(round3(median(&p99s))) },
            "p99_note": if p99s.is_empty() { "no replica reached 1000 samples for this workload" } else { "" },
            "p99_spread_pct": spread_pct(&p99s),
            "throughput_rps_median": if rps.is_empty() { serde_json::Value::Null } else { serde_json::json!(round3(median(&rps))) },
            "throughput_spread_pct": spread_pct(&rps),
            "errors_total": errors,
            "timeouts_total": timeouts,
            "saturated_replicas": saturated.len(),
        });
        aggregate.insert(name.to_string(), entry);
    }
    serde_json::Value::Object(aggregate)
}

/// The baseline and the regression budget.
///
/// The budget is the proposal of the task (p99 +10%, throughput -5%)
/// CALIBRATED against the noise the replicas actually showed: where the
/// spread between replicas is wider than the tolerance, the tolerance
/// must grow or the flag is meaningless, and the report says which is
/// which instead of pretending the measurement is steadier than it is.
fn budget_of(aggregate: &serde_json::Value) -> serde_json::Value {
    let mut per_workload = serde_json::Map::new();
    let mut worst_p99_spread = 0.0f64;
    let mut worst_rps_spread = 0.0f64;
    if let Some(map) = aggregate.as_object() {
        for (name, entry) in map {
            let p99_spread = entry["p99_spread_pct"].as_f64().unwrap_or(0.0);
            let rps_spread = entry["throughput_spread_pct"].as_f64().unwrap_or(0.0);
            worst_p99_spread = worst_p99_spread.max(p99_spread);
            worst_rps_spread = worst_rps_spread.max(rps_spread);
            per_workload.insert(
                name.clone(),
                serde_json::json!({
                    "baseline_p99_ms": entry["p99_ms_median"],
                    "baseline_rps": entry["throughput_rps_median"],
                    "noise_p99_spread_pct": p99_spread,
                    "noise_rps_spread_pct": rps_spread,
                    "budget_p99_increase_pct": 10.0,
                    "budget_throughput_drop_pct": 5.0,
                    "budget_covers_noise": p99_spread <= 10.0 && rps_spread <= 5.0,
                }),
            );
        }
    }
    serde_json::json!({
        "proposal": { "p99_increase_pct": 10.0, "throughput_drop_pct": 5.0 },
        "observed": {
            "worst_p99_replica_spread_pct": round3(worst_p99_spread),
            "worst_throughput_replica_spread_pct": round3(worst_rps_spread),
        },
        "per_workload": per_workload,
    })
}

// ──────────────────────────────────────────────────────────────────────
// Skips declared with a motive: not a single number is invented
// ──────────────────────────────────────────────────────────────────────

fn declared_skips(profile: &Profile) -> Vec<serde_json::Value> {
    let mut skips = vec![
        serde_json::json!({
            "id": "catalog-10m",
            "motive": "the 10M catalog is optional in the acceptance criterion and was not executed: seeding and measuring it costs hours and tens of gigabytes on this host; the growth is reported from 100k and 1M."
        }),
        serde_json::json!({
            "id": "s3-backend",
            "motive": "the S3 content backend was not exercised: it needs an external object store, and starting one inside this run would change the host state the report describes."
        }),
        serde_json::json!({
            "id": "tls-termination",
            "motive": "TLS is terminated by the ingress in production; the lab measures plain HTTP on the loopback so the wire adds no noise to the service times. Checksums are verified by the CAS on every read and write regardless."
        }),
        serde_json::json!({
            "id": "nexus-artifactory",
            "motive": "no competitor product was measured: the acceptance criterion forbids inventing figures of competitors, and none was executed here."
        }),
        serde_json::json!({
            "id": "gc-sweep",
            "motive": "the sweep was not measured: it mutates the content store while other numbers are being taken, and a measurement that changes what it measures is not comparable."
        }),
    ];
    if profile.name != "full" {
        skips.push(serde_json::json!({
            "id": "big-blob-1gib",
            "motive": format!("the 1 GiB scenario runs only in the 'full' profile; this run used '{}'.", profile.name)
        }));
    }
    if profile.name != "full" && profile.catalog_assets < 1_000_000 {
        skips.push(serde_json::json!({
            "id": "catalog-1m",
            "motive": format!("the 1M catalog runs in the 'full' profile; this run used {} assets.", profile.catalog_assets)
        }));
    }
    skips
}

// ──────────────────────────────────────────────────────────────────────
// The report: assembled once, written atomically, snapshotted as it grows
// ──────────────────────────────────────────────────────────────────────

/// The whole report from the replicas measured so far.
///
/// `report_state` is `partial` while the run is still going and
/// `complete` once every requested replica answered. A partial report
/// is valid JSON whose aggregates may be null: the first replica of a
/// workload under a thousand samples has no p99 by construction, and a
/// snapshot must not lie about it with a fake zero.
fn build_report(
    profile: &Profile,
    replicas_requested: usize,
    replicas: &[serde_json::Value],
    failures: &[serde_json::Value],
    command: &str,
    stamp: &str,
    started_at: &str,
    report_state: &str,
) -> serde_json::Value {
    let aggregate = aggregate(replicas);
    let budget = budget_of(&aggregate);
    let cas_probe = Path::new("target/perf-lab");
    let now = chrono::Utc::now().to_rfc3339();
    serde_json::json!({
        "meta": {
            "task": "HSP-140",
            "uat": "PERF-01",
            "tool": "perf_lab",
            "version": env!("CARGO_PKG_VERSION"),
            "commit": the_commit(),
            "command": command,
            "profile": profile.name,
            "replicas_requested": replicas_requested,
            "replicas_completed": replicas.len(),
            "report_state": report_state,
            "stamp": stamp,
            "started_at": started_at,
            "snapshot_at": now,
            "finished_at": if report_state == "complete" { serde_json::json!(now) } else { serde_json::Value::Null },
        },
        "declarations": {
            "replication": {
                "mode": format!("{} replicas of the RUN, each on a fresh database and a fresh content store", replicas_requested),
                "justification": "the embedded RocksDB directory is locked by the single process that holds it (src/bootstrap.rs), and the HSP-125 spike measured the host contention that several services sharing one machine would create: five CONCURRENT services would measure the contention, not the service. Five replicas of the whole run give the run-to-run noise the budget has to cover."
            },
            "limits": {
                "max_body_size": "raised to 2 GiB (default 10 MiB) for the big-blob scenario",
                "request_timeout_secs": "600 (default 30) so a timeout means the service, not the client",
                "tls": "plain HTTP on the loopback; see skips",
            },
            "cache": {
                "warm": "read after the same bytes were read during the warmup; pages are in the cache",
                "miss": "never read since the run started, present on disk",
                "cold": "fsync + POSIX_FADV_DONTNEED over the whole CAS before the window",
                "comparable": "every replica evicts, warms and misses with the same corpus and the same order",
            },
            "errors": "failed and timed-out requests are counted in their own columns and never in the throughput",
        },
        "host_state": host_state(profile, cas_probe),
        "skips_declared": declared_skips(profile),
        "replica_failures": failures,
        "replicas": replicas,
        "aggregate": aggregate,
        "baseline_and_budget": budget,
    })
}

/// The report on disk. Written to a sibling and renamed over the final
/// name: a reader that catches the file mid-run never sees a truncated
/// JSON, only the previous snapshot or the new one.
fn write_report(path: &Path, report: &serde_json::Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(report).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.part");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// The snapshot after each replica: the measurement already taken is on
/// disk before the next one starts. HSP-140 lost an hour of seeding and
/// measuring when a watchdog killed a run that wrote its report only at
/// the end; that must not happen to a long run again.
fn write_incremental_snapshot(
    out: &Path,
    stamp: &str,
    profile: &Profile,
    replicas_requested: usize,
    command: &str,
    started_at: &str,
    replicas: &[serde_json::Value],
    failures: &[serde_json::Value],
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let path = out.join(format!("perf-lab-{stamp}.json"));
    let report = build_report(
        profile,
        replicas_requested,
        replicas,
        failures,
        command,
        stamp,
        started_at,
        "partial",
    );
    write_report(&path, &report)?;
    Ok(path)
}

// ──────────────────────────────────────────────────────────────────────
// Main
// ──────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct RunArgs {
    profile: String,
    replicas: usize,
    out: PathBuf,
    keep: bool,
}

fn parse_args() -> Result<Option<RunArgs>, String> {
    let argv: Vec<String> = std::env::args().collect();
    if argv.iter().any(|a| a.starts_with("--seed")) {
        return Ok(None); // seed child mode
    }
    if argv.len() > 1 && argv[1] == "run" {
        let mut args = RunArgs {
            profile: "lab".into(),
            replicas: 5,
            out: PathBuf::from("target/uat"),
            keep: false,
        };
        let mut i = 2;
        while i < argv.len() {
            match argv[i].as_str() {
                "--profile" => {
                    i += 1;
                    args.profile = argv.get(i).cloned().ok_or("--profile needs a value")?;
                }
                "--replicas" => {
                    i += 1;
                    args.replicas = argv
                        .get(i)
                        .ok_or("--replicas needs a value")?
                        .parse::<usize>()
                        .map_err(|e| e.to_string())?;
                    if args.replicas == 0 {
                        return Err("at least one replica".into());
                    }
                }
                "--out" => {
                    i += 1;
                    args.out = PathBuf::from(argv.get(i).ok_or("--out needs a value")?);
                }
                "--keep" => args.keep = true,
                other => return Err(format!("unknown option {other}")),
            }
            i += 1;
        }
        Profile::parse(&args.profile)?; // validate now, not at 3am
        return Ok(Some(args));
    }
    Ok(None)
}

#[tokio::main]
async fn main() {
    if let Err(error) = the_main().await {
        eprintln!("perf_lab: {error}");
        std::process::exit(1);
    }
}

async fn the_main() -> Result<(), String> {
    let argv: Vec<String> = std::env::args().collect();
    let mode = argv.get(1).map(String::as_str).unwrap_or("run");

    // ── Seed child modes: direct on the database, service stopped ────
    match mode {
        "--seed-repos" => {
            let db = std::env::var("HODEI_PERF_DB").map_err(|e| e.to_string())?;
            let upstream = std::env::var("HODEI_PERF_UPSTREAM").map_err(|e| e.to_string())?;
            return seed_repositories(&db, upstream.parse::<u16>().map_err(|e| e.to_string())?)
                .await;
        }
        "--seed-catalog" => {
            let db = std::env::var("HODEI_PERF_DB").map_err(|e| e.to_string())?;
            let assets: usize = std::env::var("HODEI_PERF_ASSETS")
                .unwrap_or_else(|_| "100000".into())
                .parse::<usize>()
                .map_err(|e| e.to_string())?;
            let seconds = seed_catalog(&db, assets).await?;
            println!("seeded {assets} assets in {seconds:.1} s");
            return Ok(());
        }
        "--seed-evidence" => {
            let db = std::env::var("HODEI_PERF_DB").map_err(|e| e.to_string())?;
            let cas = std::env::var("HODEI_PERF_CAS").map_err(|e| e.to_string())?;
            let count: usize = std::env::var("HODEI_PERF_EVIDENCE")
                .unwrap_or_else(|_| "5000".into())
                .parse::<usize>()
                .map_err(|e| e.to_string())?;
            let seconds = seed_evidence(&db, &cas, count).await?;
            println!("seeded {count} pieces of evidence in {seconds:.1} s");
            return Ok(());
        }
        _ => {}
    }

    let Some(args) = parse_args()? else {
        eprintln!(
            "usage: perf_lab run [--profile smoke|lab|full] [--replicas N] [--out DIR] [--keep]\n\
             seed modes are internal: --seed-repos / --seed-catalog / --seed-evidence"
        );
        return Err("no run arguments".into());
    };
    let profile = Profile::parse(&args.profile)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let started_at = chrono::Utc::now().to_rfc3339();
    let command = std::env::args().collect::<Vec<_>>().join(" ");

    println!(
        "perf_lab: profile={} replicas={} catalog_assets={} evidence={}",
        profile.name,
        profile.replicas_label(args.replicas),
        profile.catalog_assets,
        profile.evidence
    );

    let mut replicas = Vec::new();
    let mut failures = Vec::new();
    for index in 1..=args.replicas {
        println!("── replica {index}/{}", args.replicas);
        match run_replica(&stamp, index, &profile, args.keep).await {
            Ok(value) => replicas.push(value),
            Err(error) => {
                eprintln!("replica {index} failed: {error}");
                failures.push(serde_json::json!({"replica": index, "error": error}));
            }
        }
        if !replicas.is_empty() {
            match write_incremental_snapshot(
                &args.out,
                &stamp,
                &profile,
                args.replicas,
                &command,
                &started_at,
                &replicas,
                &failures,
            ) {
                Ok(path) => println!("partial snapshot: {}", path.display()),
                Err(error) => eprintln!("snapshot could not be written: {error}"),
            }
        }
    }
    if replicas.is_empty() {
        return Err("every replica failed; there is no measurement".into());
    }

    let report = build_report(
        &profile,
        args.replicas,
        &replicas,
        &failures,
        &command,
        &stamp,
        &started_at,
        "complete",
    );

    std::fs::create_dir_all(&args.out).map_err(|e| e.to_string())?;
    let path = args.out.join(format!("perf-lab-{stamp}.json"));
    write_report(&path, &report)?;
    println!("report: {}", path.display());

    // A compact human line per workload: what the board will read.
    println!("\nworkload                        p50 ms    p99 ms    rps      spread p99 %");
    if let Some(map) = report["aggregate"].as_object() {
        for (name, entry) in map {
            println!(
                "{name:<30}{:<10}{:<10}{:<9}{:.1}",
                entry["p50_ms_median"].to_string(),
                entry["p99_ms_median"].to_string(),
                entry["throughput_rps_median"].to_string(),
                entry["p99_spread_pct"].as_f64().unwrap_or(0.0)
            );
        }
    }
    Ok(())
}

impl Profile {
    fn replicas_label(&self, replicas: usize) -> String {
        format!("{replicas}")
    }
}

// ──────────────────────────────────────────────────────────────────────
// Form tests: pure, fast, CI-safe (pipeline.kts runs --lib --bins)
// ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_corpus_is_deterministic_per_size_and_slot() {
        let first = corpus(4096, 7);
        let second = corpus(4096, 7);
        assert_eq!(
            first, second,
            "the same size and slot must be the same bytes"
        );
        assert_eq!(first.len(), 4096);
        assert_ne!(
            corpus(4097, 7),
            first,
            "a different size is a different content"
        );
        assert_ne!(
            corpus(4096, 8),
            first,
            "a different slot is a different content"
        );
        assert_eq!(hex_digest(&corpus(1024, 3)), hex_digest(&corpus(1024, 3)));
    }

    #[test]
    fn the_percentile_reads_the_sorted_list() {
        let sorted: Vec<f64> = (1..=1000).map(|v| v as f64).collect();
        // Nearest-rank on 0-based positions with rounding: over one
        // thousand samples the rank of p50 is 501, not 500, and the
        // test says so explicitly because someone will be tempted to
        // "fix" it.
        assert_eq!(percentile(&sorted, 0.5), 501.0);
        assert_eq!(percentile(&sorted, 0.95), 950.0);
        assert_eq!(percentile(&sorted, 0.99), 990.0);
        assert_eq!(percentile(&sorted, 1.0), 1000.0);
        assert_eq!(percentile(&[], 0.5), 0.0);
    }

    #[test]
    fn p99_refuses_small_samples_instead_of_lying() {
        let sorted: Vec<f64> = (1..=999).map(|v| v as f64).collect();
        let honest = percentile_honest(&sorted, 0.99, sorted.len());
        assert_eq!(honest["value_ms"], serde_json::Value::Null);
        assert!(honest["reason"].as_str().expect("reason").contains("1000"));
        let enough: Vec<f64> = (1..=1000).map(|v| v as f64).collect();
        let honest = percentile_honest(&enough, 0.99, enough.len());
        assert_eq!(honest["value_ms"], serde_json::json!(990.0));
    }

    #[test]
    fn the_driver_saturation_check_blames_the_client_only_when_it_is_the_client() {
        // 100 rps -> 10 ms of spacing; ten times the spacing is the line.
        assert!(!driver_saturated(50.0, 100.0));
        assert!(driver_saturated(150.0, 100.0));
        assert!(!driver_saturated(1_000.0, 0.0));
    }

    #[test]
    fn profiles_are_named_and_never_invented() {
        assert!(Profile::parse("smoke").is_ok());
        assert!(Profile::parse("lab").is_ok());
        let full = Profile::parse("full").expect("full exists");
        assert!(full
            .points
            .iter()
            .any(|p| p.big && p.bytes == 1_073_741_824));
        assert!(full.catalog_assets >= 1_000_000);
        let lab = Profile::parse("lab").expect("lab exists");
        assert!(!lab.points.iter().any(|p| p.big));
        assert_eq!(lab.catalog_assets, 100_000);
        let error = Profile::parse("prod").expect_err("prod is not a profile");
        assert!(error.contains("unknown profile"));
    }

    #[test]
    fn the_fingerprint_covers_the_numbers_it_claims() {
        let lab = Profile::parse("lab").expect("lab");
        let mut changed = lab.clone();
        changed.catalog_assets += 1;
        assert_ne!(lab.fingerprint(), changed.fingerprint());
        assert_eq!(
            lab.fingerprint(),
            Profile::parse("lab").expect("lab").fingerprint()
        );
    }

    #[test]
    fn the_workload_order_is_the_protocol_and_it_is_stable() {
        let hash = hex_digest(WORKLOAD_ORDER.join("|").as_bytes());
        assert_eq!(hash, hex_digest(WORKLOAD_ORDER.join("|").as_bytes()));
        assert!(WORKLOAD_ORDER.contains(&"raw-read-cold-64kib"));
        assert!(WORKLOAD_ORDER.contains(&"raw-read-miss-1mib"));
        // Warm, miss and cold are three different scenes of the same
        // size: their names may not collide.
        let mut seen = std::collections::HashSet::new();
        for name in WORKLOAD_ORDER {
            assert!(seen.insert(name), "duplicate workload name {name}");
        }
    }

    #[test]
    fn size_labels_are_the_workload_names() {
        assert_eq!(size_label(1024, "write"), "raw-write-1kib");
        assert_eq!(size_label(1_048_576, "read-cold"), "raw-read-cold-1mib");
        assert_eq!(size_label(1_073_741_824, "read-hit"), "raw-read-hit-1gib");
    }

    #[test]
    fn the_aggregation_median_and_spread_are_the_numbers_reported() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(spread_pct(&[10.0, 12.0, 11.0]).round(), 18.0);
        assert_eq!(spread_pct(&[5.0]), 0.0);
    }

    #[test]
    fn the_incremental_snapshot_is_valid_json_after_one_replica() {
        // The regression the snapshot exists for: a long run killed
        // mid-way must leave on disk what it already measured. One
        // replica is the worst moment to lose work, so that is the
        // shape tested here.
        let replica = serde_json::json!({
            "replica": 1,
            "workloads": [{
                "name": "raw-write-1kib",
                "count": 40,
                "errors": 0,
                "timeouts": 0,
                "throughput_rps": 80.5,
                "latency_ms": {
                    "p50": { "value_ms": 7.8 },
                    "p95": { "value_ms": 8.7 },
                    "p99": { "value_ms": null, "reason": "p99 needs at least 1000 samples" },
                },
                "driver_saturated": false,
            }],
        });
        let dir = tempfile::tempdir().expect("tempdir");
        let profile = Profile::parse("smoke").expect("smoke");
        let path = write_incremental_snapshot(
            dir.path(),
            "test-stamp",
            &profile,
            5,
            "perf_lab run --test",
            "2026-11-19T00:00:00+00:00",
            &[replica],
            &[],
        )
        .expect("the snapshot writes");
        let text = std::fs::read_to_string(&path).expect("the snapshot is readable");
        let report: serde_json::Value =
            serde_json::from_str(&text).expect("a partial snapshot is valid JSON");
        assert_eq!(report["meta"]["report_state"], "partial");
        assert_eq!(report["meta"]["replicas_completed"], 1);
        assert_eq!(report["meta"]["replicas_requested"], 5);
        assert_eq!(report["meta"]["finished_at"], serde_json::Value::Null);
        // The first replica has no p99 by construction: honest null,
        // not an invented zero.
        assert_eq!(
            report["aggregate"]["raw-write-1kib"]["p99_ms_median"],
            serde_json::Value::Null
        );
        // Renaming leaves no .part behind.
        assert!(!dir.path().join("perf-lab-test-stamp.json.part").exists());
    }
}
