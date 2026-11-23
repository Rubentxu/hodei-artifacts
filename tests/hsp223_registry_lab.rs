//! The laboratory of the registry (HSP-223 / HCR-01), against the wire.
//!
//! The service is a **real process** with a real catalog, a real store of
//! bytes and real Cedar; the upstream is a real local HTTP server that
//! this file counts. What is measured is what a person sees: what comes
//! back, what the upstream receives, and what the run **proves**.
//!
//! The laboratory is agnostic of the format: the adapter of each format
//! keeps its own grammar, and what is shared is the harness -- the
//! deployment, the `curl`, the `sha256sum` and the upstream with the
//! counter.
//!
//! Three acceptance criteria are demonstrated here, and none of them is
//! demonstrated by the absence of a failure:
//!
//! - **AC1**: a first fetch misses, a second one hits, and the hit is
//!   served with the upstream **gone**: the counter of the upstream does
//!   not grow and the digest is the same one.
//! - **AC2**: an invalid credential and a corrupt digest are both
//!   refused **with a cause**, and the report tells `PASS`, `FAIL` and
//!   `NOT_RUN` apart. A case that did not run is never green.
//! - **AC3**: the laboratory never touches the HOME of whoever runs it,
//!   never sends a bearer to another origin, works without network with a
//!   local fixture, and only reaches the real network when somebody asks
//!   for it explicitly and within a budget of bytes and of time.

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use support::lab::{
    Budget, Doc, Expectation, HomeSnapshot, LabCase, LabHome, Outcome, REAL_MODE_VARIABLE, Summary,
    Upstream, real_mode_enabled, record_registry_lab,
};
use support::{Curl, Deployment, FIRST_ACCOUNT};

/// The bytes of the fixture that the upstream serves.
const FIXTURE: &[u8] = b"the bytes of the fixture that the upstream of the laboratory serves";

/// Where the fixture lives in the repository.
const ARTIFACT: &str = "fixtures/lab.bin";

/// The secret that the proxy sends to its own upstream.
const UPSTREAM_SECRET: &str = "secret-of-the-laboratory";

/// The cache of the proxy: the copy is fresh for an hour.
const CACHE_FRESH: &str = r#"{"ttl": {"content_secs": 3600, "metadata_secs": 3600}}"#;

/// The upstream of the real mode: the Maven Central of the internet.
const REAL_UPSTREAM: &str = "https://repo.maven.apache.org/maven2/";

/// The artifact that the real mode asks for.
const REAL_ARTIFACT: &str = "org/apache/commons/commons-lang3/3.12.0/commons-lang3-3.12.0.pom";

/// The seeder of the catalog.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

// ---------------------------------------------------------------------------
// The laboratory.
// ---------------------------------------------------------------------------

/// The laboratory: a deployment, an upstream and a HOME that is not yours.
struct Lab {
    deployment: Deployment,
    upstream: Upstream,
    home: LabHome,
}

impl Lab {
    /// A laboratory with the proxies it is given.
    ///
    /// The proxies are built with the upstream that the laboratory starts,
    /// because a proxy without an upstream is a proxy that cannot be
    /// asked anything.
    fn new<F>(proxies: F) -> Self
    where
        F: FnOnce(&Upstream) -> Vec<(&'static str, serde_json::Value)>,
    {
        let upstream = Upstream::start();
        let proxies = proxies(&upstream);
        let deployment = Deployment::new();
        deployment.seed_proxies(&[], &[], &proxies);
        Self {
            deployment,
            upstream,
            home: LabHome::new(),
        }
    }

    /// The environment of the service: the isolated HOME and the egress.
    ///
    /// The HOME is the first thing on the list on purpose: a service that
    /// writes where it always writes is a service that writes in the HOME
    /// of whoever runs the laboratory.
    fn environment(&self, extra: &[(&str, &str)]) -> Vec<(String, String)> {
        let mut environment = self.home.environment();
        environment.push((
            "HODEI_PROXY__ALLOWED_HOSTS".to_string(),
            format!("127.0.0.1:{};private;http", self.upstream.port()),
        ));
        for (name, value) in extra {
            environment.push((name.to_string(), value.to_string()));
        }
        environment
    }

    /// Starts the service with the environment of the laboratory.
    fn start(&self, extra: &[(&str, &str)]) -> support::Service {
        let environment = self.environment(extra);
        let borrowed: Vec<(&str, &str)> = environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        self.deployment.start(&borrowed)
    }

    /// A file of the laboratory, in its own directory.
    fn file(&self, name: &str) -> PathBuf {
        self.deployment.directory.path().join(name)
    }
}

/// The configuration of a proxy repository of the laboratory.
fn proxy_of(upstream: &Upstream, cache: &str, credential: Option<&str>) -> serde_json::Value {
    let mut configuration = serde_json::json!({
        "upstream": {
            "id": "lab",
            "url": upstream.base(),
            "scopes": ["/"],
            "max_bytes": 4096,
        },
        "cache": serde_json::from_str::<serde_json::Value>(cache).expect("the cache is json"),
    });
    if let Some(reference) = credential {
        configuration["upstream"]["credential_ref"] = reference.into();
    }
    configuration
}

/// The configuration of a proxy that lets a redirect cross the origin.
fn proxy_with_redirects(upstream: &Upstream, credential: Option<&str>) -> serde_json::Value {
    let mut configuration = proxy_of(upstream, CACHE_FRESH, credential);
    configuration["upstream"]["redirects"] = serde_json::json!({ "max": 3, "cross_origin": true });
    configuration
}

/// A reply of the service.
struct Reply {
    status: u16,
    body: Vec<u8>,
    cache: Option<String>,
    command: String,
}

impl Reply {
    /// The body, as text.
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// Asks the service for a path of a repository, and leaves the bytes in a
/// file.
fn fetch(port: u16, repository: &str, path: &str, into: &Path) -> Reply {
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let _ = std::fs::remove_file(into);
    let curl = Curl::call(
        port,
        "GET",
        &format!("/repository/{repository}/{path}"),
        &alice,
        None,
        None,
        Some(into),
    );
    let command = curl.shown();
    let answer = curl.run();
    Reply {
        status: answer.status,
        body: std::fs::read(into).unwrap_or_default(),
        cache: answer.header("x-hodei-cache"),
        command,
    }
}

// ---------------------------------------------------------------------------
// AC1: a miss, a hit, and the hit with the upstream gone.
// ---------------------------------------------------------------------------

#[test]
fn ac1_the_hit_is_served_without_the_upstream_and_with_the_same_digest() {
    let lab = Lab::new(|upstream| vec![("pxy", proxy_of(upstream, CACHE_FRESH, None))]);
    lab.upstream.put(ARTIFACT, FIXTURE, None);

    let fixture = lab.file("fixture.bin");
    std::fs::write(&fixture, FIXTURE).expect("the fixture is written");
    let into = lab.file("response.bin");

    let mut service = lab.start(&[]);
    let port = service.port;

    // The first fetch does not have it: it goes out and comes back.
    let first = fetch(port, "pxy", ARTIFACT, &into);
    assert_eq!(first.status, 200, "{}", first.text());
    assert_eq!(first.cache.as_deref(), Some("miss"), "{}", first.text());
    assert_eq!(lab.upstream.hits(ARTIFACT), 1);
    let first_digest = support::sha256_of(&into);
    assert_eq!(first_digest, support::sha256_of(&fixture));

    // The second one is a hit, and a hit does not ask anybody.
    let second = fetch(port, "pxy", ARTIFACT, &into);
    assert_eq!(second.status, 200, "{}", second.text());
    assert_eq!(second.cache.as_deref(), Some("hit"), "{}", second.text());
    assert_eq!(
        lab.upstream.hits(ARTIFACT),
        1,
        "the hit went out to the network"
    );

    // The upstream goes away: it does not accept connections anymore.
    lab.upstream.disconnect();
    let before = lab.upstream.total();

    // The hit is served anyway, out of the copy that is already there.
    let third = fetch(port, "pxy", ARTIFACT, &into);
    assert_eq!(third.status, 200, "{}", third.text());
    assert_eq!(third.cache.as_deref(), Some("hit"), "{}", third.text());
    assert_eq!(
        lab.upstream.total(),
        before,
        "the hit touched the network with the upstream gone"
    );
    let third_digest = support::sha256_of(&into);
    assert_eq!(third_digest, first_digest, "the digest changed");
    assert_eq!(third_digest, support::sha256_of(&fixture));

    record_registry_lab(
        LabCase::new(
            "ac1-hit-without-the-upstream",
            "el HIT se resuelve sin trafico externo y con el mismo digest",
        )
        .command(&third.command)
        .origin(&lab.upstream.origin())
        .sha256(&third_digest)
        .passed(),
    );
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// AC2: the refusals, and the count that does not lie.
// ---------------------------------------------------------------------------

#[test]
fn ac2_an_invalid_credential_is_refused_with_a_cause() {
    let lab = Lab::new(|upstream| {
        vec![(
            "pxy-auth",
            proxy_of(upstream, CACHE_FRESH, Some("env:LAB_TOKEN")),
        )]
    });
    // The upstream refuses the credential: it answers with a 401.
    lab.upstream
        .put_doc(ARTIFACT, Doc::new(b"never served").with_status(401));

    let into = lab.file("response.bin");
    let mut service = lab.start(&[
        ("HODEI_PROXY__ALLOWED_CREDENTIALS", "LAB_TOKEN"),
        ("LAB_TOKEN", UPSTREAM_SECRET),
    ]);
    let port = service.port;

    let reply = fetch(port, "pxy-auth", ARTIFACT, &into);
    let refused = reply.status >= 400;
    assert!(
        refused,
        "the registry served the content with an invalid credential: {}",
        reply.text()
    );
    assert!(
        !reply
            .body
            .windows(FIXTURE.len())
            .any(|window| window == FIXTURE),
        "the bytes of the upstream were served anyway"
    );

    let cause = format!("HTTP {}: {}", reply.status, reply.text());
    let mut case = LabCase::new(
        "ac2-invalid-credential",
        "una credencial invalida se rechaza, y el rechazo dice por que",
    )
    .expecting(Expectation::Refused)
    .command(&reply.command)
    .origin(&lab.upstream.origin());
    if refused {
        case = case.refused(cause);
    } else {
        case = case.failed(cause);
    }
    record_registry_lab(case);
    support::stop(&mut service);
}

#[test]
fn ac2_a_corrupt_digest_is_refused_with_a_cause() {
    let lab = Lab::new(|upstream| vec![("pxy-digest", proxy_of(upstream, CACHE_FRESH, None))]);
    // The upstream declares a digest that is not the one of the bytes.
    let wrong = "0".repeat(64);
    lab.upstream
        .put_doc(ARTIFACT, Doc::new(FIXTURE).with_declared_sha256(&wrong));

    let into = lab.file("response.bin");
    let mut service = lab.start(&[]);
    let port = service.port;

    let reply = fetch(port, "pxy-digest", ARTIFACT, &into);
    let refused = reply.status >= 400;
    assert!(
        refused,
        "the registry served content whose digest does not match: {}",
        reply.text()
    );
    assert!(
        !reply
            .body
            .windows(FIXTURE.len())
            .any(|window| window == FIXTURE),
        "the bytes with a corrupt digest were served anyway"
    );

    let cause = format!("HTTP {}: {}", reply.status, reply.text());
    let mut case = LabCase::new(
        "ac2-corrupt-digest",
        "un digest corrupto se rechaza, y el rechazo dice por que",
    )
    .expecting(Expectation::Refused)
    .command(&reply.command)
    .origin(&lab.upstream.origin());
    if refused {
        case = case.refused(cause);
    } else {
        case = case.failed(cause);
    }
    record_registry_lab(case);
    support::stop(&mut service);
}

#[test]
fn ac2_the_summary_counts_the_skipped_cases_apart_and_never_calls_them_green() {
    let cases = vec![
        LabCase::new("one", "one").passed(),
        LabCase::new("two", "two").failed("the cause of the failure"),
        LabCase::new("three", "three").not_run("the real mode is opt-in"),
        LabCase::new("four", "four").not_run("the real mode is opt-in"),
    ];
    let summary = Summary::of(&cases);

    assert_eq!(summary.pass, 1);
    assert_eq!(summary.fail, 1);
    assert_eq!(summary.not_run, 2);
    // The count of the green cases is the count of the passes: the ones
    // that did not run are never part of it.
    assert_eq!(summary.green_cases(), 1);
    assert!(!summary.green, "a skipped case was counted as green");
    assert!(!summary.complete);
    assert!(Outcome::Pass.is_green());
    assert!(!Outcome::Fail.is_green());
    assert!(!Outcome::NotRun.is_green());

    record_registry_lab(
        LabCase::new(
            "ac2-the-count-does-not-lie",
            "los casos que no se ejecutan no cuentan como verdes",
        )
        .command("Summary::of(casos)")
        .passed(),
    );
}

// ---------------------------------------------------------------------------
// AC3: the isolation, the bearer, the fixture and the opt-in.
// ---------------------------------------------------------------------------

#[test]
fn ac3_the_laboratory_never_touches_the_home_of_the_user() {
    let user_home = PathBuf::from(std::env::var("HOME").expect("the home of the user"));
    // The tree of the HOME of the user, before and after: the files it
    // has and the bytes they weigh.
    let before = HomeSnapshot::capture(&user_home, 2);
    // The trace of the laboratory in that HOME: anything whose name talks
    // about it.
    let before_trace = HomeSnapshot::capture_matching(&user_home, 2, "hodei");
    let before_probes = the_probes(&user_home);

    let lab = Lab::new(|upstream| vec![("pxy", proxy_of(upstream, CACHE_FRESH, None))]);
    lab.upstream.put(ARTIFACT, FIXTURE, None);
    let into = lab.file("response.bin");
    let mut service = lab.start(&[]);
    let port = service.port;

    // The service runs with the HOME of the laboratory, and not with the
    // one of whoever runs the laboratory.
    let home_of_the_service = the_home_of_the_process(service.child.id());
    if let Some(home) = home_of_the_service.as_deref() {
        assert_eq!(
            home,
            lab.home.path().to_string_lossy(),
            "the service does not run with the HOME of the laboratory"
        );
    }
    assert!(
        !lab.home.path().starts_with(&user_home),
        "the HOME of the laboratory is inside the HOME of the user"
    );

    let reply = fetch(port, "pxy", ARTIFACT, &into);
    assert_eq!(reply.status, 200, "{}", reply.text());
    support::stop(&mut service);

    let after = HomeSnapshot::capture(&user_home, 2);
    let after_trace = HomeSnapshot::capture_matching(&user_home, 2, "hodei");
    let after_probes = the_probes(&user_home);

    assert_eq!(
        before.fingerprint(),
        after.fingerprint(),
        "the files of the HOME of the user changed"
    );
    let trace = before_trace.differences(&after_trace);
    assert!(
        trace.is_empty(),
        "the laboratory left a trace in the HOME of the user: {trace:?}"
    );
    assert_eq!(
        before_probes, after_probes,
        "the laboratory left a directory of its own in the HOME of the user"
    );

    record_registry_lab(
        LabCase::new(
            "ac3-the-home-of-the-user-is-untouched",
            "el HOME de quien ejecuta el laboratorio no cambia",
        )
        .command(&format!(
            "find {} -maxdepth 2 -type f -printf '%s\\n'",
            user_home.display()
        ))
        .origin(&lab.upstream.origin())
        .passed(),
    );
}

/// The places where a service would put its configuration and its cache.
const THE_PLACES_OF_A_SERVICE: &[&str] = &[
    ".hodei",
    ".config/hodei",
    ".cache/hodei",
    ".local/share/hodei",
    ".local/state/hodei",
    ".var/app/hodei",
];

/// Which of the places of a service exist in a HOME.
fn the_probes(home: &Path) -> Vec<String> {
    THE_PLACES_OF_A_SERVICE
        .iter()
        .filter(|place| home.join(place).exists())
        .map(|place| place.to_string())
        .collect()
}

#[test]
fn ac3_the_bearer_never_reaches_another_origin() {
    // The origin of the upstream, and another one that the redirect
    // points to.
    let foreign = Upstream::start();
    let lab = Lab::new(|upstream| {
        vec![(
            "pxy-cross",
            proxy_with_redirects(upstream, Some("env:LAB_TOKEN")),
        )]
    });
    // The upstream sends the client to another origin.
    lab.upstream.put_doc(
        ARTIFACT,
        Doc::new(b"").redirect(&format!("{}/{}", foreign.base(), ARTIFACT)),
    );
    foreign.put(ARTIFACT, FIXTURE, None);

    let into = lab.file("response.bin");
    let mut service = lab.start(&[
        ("HODEI_PROXY__ALLOWED_CREDENTIALS", "LAB_TOKEN"),
        ("LAB_TOKEN", UPSTREAM_SECRET),
        (
            "HODEI_PROXY__ALLOWED_HOSTS",
            format!(
                "127.0.0.1:{};private;http,127.0.0.1:{};private;http",
                lab.upstream.port(),
                foreign.port()
            )
            .as_str(),
        ),
    ]);
    let port = service.port;

    let alice = support::token(FIRST_ACCOUNT, "alice");
    let reply = fetch(port, "pxy-cross", ARTIFACT, &into);
    assert_eq!(reply.status, 200, "{}", reply.text());

    // The upstream of the proxy receives its own credential.
    let own = lab.upstream.requests();
    assert!(!own.is_empty(), "the upstream was never asked");
    for seen in &own {
        assert_eq!(
            seen.authorization.as_deref(),
            Some(format!("Bearer {UPSTREAM_SECRET}").as_str()),
            "{}",
            seen.path
        );
    }

    // The other origin receives nothing: the bearer is not its own.
    let other = foreign.requests();
    assert!(!other.is_empty(), "the other origin was never asked");
    for seen in &other {
        assert_eq!(
            seen.authorization, None,
            "the bearer reached another origin: {}",
            seen.path
        );
    }

    // And the credential of whoever asks is not the credential of the
    // upstream: it never leaves the service.
    for seen in own.iter().chain(other.iter()) {
        assert!(
            !seen
                .authorization
                .as_deref()
                .unwrap_or_default()
                .contains(&alice),
            "the credential of the caller reached the upstream"
        );
    }

    record_registry_lab(
        LabCase::new(
            "ac3-the-bearer-stays-in-its-origin",
            "el bearer viaja a su origen y a ningun otro",
        )
        .command(&reply.command)
        .origin(&foreign.origin())
        .sha256(&support::sha256_of(&into))
        .passed(),
    );
    support::stop(&mut service);
}

#[test]
fn ac3_the_fixture_is_served_without_network_when_the_service_is_offline() {
    let lab = Lab::new(|upstream| vec![("pxy", proxy_of(upstream, CACHE_FRESH, None))]);
    lab.upstream.put(ARTIFACT, FIXTURE, None);
    let fixture = lab.file("fixture.bin");
    std::fs::write(&fixture, FIXTURE).expect("the fixture is written");
    let into = lab.file("response.bin");

    let mut service = lab.start(&[]);
    let port = service.port;
    let online = fetch(port, "pxy", ARTIFACT, &into);
    assert_eq!(online.status, 200, "{}", online.text());
    let digest = support::sha256_of(&into);
    support::stop(&mut service);

    // The service does not go out: it serves the copy it has.
    let before = lab.upstream.total();
    let mut offline = lab.start(&[("HODEI_PROXY__OFFLINE", "true")]);
    let port = offline.port;
    let reply = fetch(port, "pxy", ARTIFACT, &into);
    assert_eq!(reply.status, 200, "{}", reply.text());
    assert_eq!(
        support::sha256_of(&into),
        digest,
        "the offline copy is not the same content"
    );
    assert_eq!(
        lab.upstream.total(),
        before,
        "the offline service touched the network"
    );

    // What it does not have is not a 404 that lies: it is a 503.
    let unknown = fetch(port, "pxy", "fixtures/never.bin", &into);
    assert_eq!(unknown.status, 503, "{}", unknown.text());

    record_registry_lab(
        LabCase::new(
            "ac3-offline-with-a-local-fixture",
            "el modo sin red sirve el fixture local con el mismo digest",
        )
        .command(&reply.command)
        .origin(&lab.upstream.origin())
        .sha256(&digest)
        .passed(),
    );
    support::stop(&mut offline);
}

#[test]
fn ac3_the_real_mode_is_opt_in_and_bounded_by_bytes_and_by_time() {
    let budget = Budget::from_environment();
    let case = LabCase::new(
        "ac3-real-upstream",
        "el registro contra un upstream real, con presupuesto de bytes y de tiempo",
    )
    .expecting(Expectation::Succeeds)
    .command(&format!(
        "curl http://127.0.0.1:<puerto>/repository/real/{REAL_ARTIFACT}"
    ));

    if !real_mode_enabled() {
        // Without the opt-in the case does not run, and a case that does
        // not run is not a green case.
        record_registry_lab(case.not_run(format!(
            "the real mode is opt-in: set {REAL_MODE_VARIABLE}=1 to run it"
        )));
    } else {
        record_registry_lab(the_real_case(&budget));
    }

    // The budget is a budget: what does not fit in it does not pass.
    assert!(
        budget
            .permits(budget.max_bytes, Duration::from_secs(budget.max_seconds))
            .is_ok()
    );
    assert!(
        budget
            .permits(budget.max_bytes + 1, Duration::ZERO)
            .is_err()
    );
    assert!(
        budget
            .permits(0, Duration::from_secs(budget.max_seconds + 1))
            .is_err()
    );

    // And the budget reaches the service, which refuses to bring more
    // than it allows.
    let environment = budget.service_environment();
    assert!(
        environment
            .iter()
            .any(|(name, value)| name == "HODEI_PROXY__MAX_BYTES"
                && value == &budget.max_bytes.to_string())
    );
    assert!(
        environment
            .iter()
            .any(|(name, value)| name == "HODEI_PROXY__TOTAL_TIMEOUT_SECS"
                && value == &budget.max_seconds.to_string())
    );
}

/// The real case: the registry against a real upstream, within the budget.
fn the_real_case(budget: &Budget) -> LabCase {
    let case = LabCase::new(
        "ac3-real-upstream",
        "el registro contra un upstream real, con presupuesto de bytes y de tiempo",
    )
    .expecting(Expectation::Succeeds);

    let lab = Lab::new(|upstream| {
        let mut configuration = proxy_of(upstream, CACHE_FRESH, None);
        configuration["upstream"]["url"] = REAL_UPSTREAM.into();
        // The upstream of the real mode obeys the budget of the
        // laboratory: the service does not bring more than it allows.
        configuration["upstream"]["max_bytes"] = budget.max_bytes.into();
        vec![("real", configuration)]
    });

    let mut environment: Vec<(String, String)> = vec![(
        "HODEI_PROXY__ALLOWED_HOSTS".to_string(),
        "repo.maven.apache.org".to_string(),
    )];
    environment.extend(budget.service_environment());
    let borrowed: Vec<(&str, &str)> = environment
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let mut service = lab.deployment.start(&borrowed);
    let port = service.port;
    let into = lab.file("response.bin");

    let started = Instant::now();
    let reply = fetch(port, "real", REAL_ARTIFACT, &into);
    let elapsed = started.elapsed();
    let bytes = std::fs::metadata(&into).map(|meta| meta.len()).unwrap_or(0);
    support::stop(&mut service);

    let case = case
        .command(&reply.command)
        .origin(REAL_UPSTREAM)
        .sha256(&support::sha256_of(&into));
    if reply.status != 200 {
        return case.failed(format!("HTTP {}: {}", reply.status, reply.text()));
    }
    match budget.permits(bytes, elapsed) {
        Ok(()) => case.passed(),
        Err(cause) => case.failed(cause),
    }
}

/// The HOME of a running process, read from the system.
fn the_home_of_the_process(pid: u32) -> Option<String> {
    let environ = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    environ.split(|byte| *byte == 0).find_map(|entry| {
        let text = String::from_utf8_lossy(entry);
        text.strip_prefix("HOME=").map(str::to_string)
    })
}
