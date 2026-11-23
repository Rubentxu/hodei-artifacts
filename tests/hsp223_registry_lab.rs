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
//!   served with the upstream **gone**: the upstream does not accept
//!   connections anymore and the digest is the same one.
//! - **AC2**: an invalid credential and a corrupt digest are both
//!   refused **with a cause**, and the report tells `PASS`, `FAIL` and
//!   `NOT_RUN` apart. A case that did not run is never green.
//! - **AC3**: the laboratory never touches the HOME of whoever runs it,
//!   never sends a bearer to another origin, works without network with a
//!   local fixture, refuses what is beyond its budget, and only reaches
//!   the real network when somebody asks for it explicitly.
//!
//! A case is **inscribed before it runs** and replaced when it ends: a
//! case that dies in the middle leaves a `NOT_RUN` in the report, and a
//! panic leaves a `FAIL` with its cause. A report that says `PASS` says
//! that somebody measured it.

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use support::lab::{
    Budget, Doc, Expectation, HomeSnapshot, LabCase, LabHome, Outcome, REAL_MODE_VARIABLE, Scope,
    Summary, Upstream, real_mode_enabled, run_registry_lab_case,
};
use support::{Curl, Deployment, FIRST_ACCOUNT};

/// The bytes of the fixture that the upstream serves.
const FIXTURE: &[u8] = b"the bytes of the fixture that the upstream of the laboratory serves";

/// Where the fixture lives in the repository.
const ARTIFACT: &str = "fixtures/lab.bin";

/// The secret that the proxy sends to its own upstream.
const UPSTREAM_SECRET: &str = "secret-of-the-laboratory";

/// The secret that the proxy sends when it is wrong.
const WRONG_SECRET: &str = "secret-that-is-not-the-one";

/// The cache of the proxy: the copy is fresh for an hour.
const CACHE_FRESH: &str = r#"{"ttl": {"content_secs": 3600, "metadata_secs": 3600}}"#;

/// The upstream of the real mode: the Maven Central of the internet.
const REAL_UPSTREAM: &str = "https://repo.maven.apache.org/maven2/";

/// The artifact that the real mode asks for.
const REAL_ARTIFACT: &str = "org/apache/commons/commons-lang3/3.12.0/commons-lang3-3.12.0.pom";

/// The cases that a complete run has to execute.
///
/// The report declares them, so a run that only reached one of them says
/// so: the ones that did not run count as `NOT_RUN`, and the report is
/// never green.
const THE_CASES: &[(&str, &str)] = &[
    (
        "ac1-hit-without-the-upstream",
        "el HIT se resuelve sin trafico externo y con el mismo digest",
    ),
    (
        "ac2-the-credential-travels",
        "la credencial del proxy llega a su upstream y el contenido se sirve",
    ),
    (
        "ac2-invalid-credential",
        "una credencial invalida se rechaza, y el rechazo dice por que",
    ),
    (
        "ac2-corrupt-digest",
        "un digest corrupto se rechaza, y el rechazo dice por que",
    ),
    (
        "ac2-the-count-does-not-lie",
        "los casos que no se ejecutan no cuentan como verdes",
    ),
    (
        "ac3-the-home-of-the-user-is-untouched",
        "el HOME de quien ejecuta el laboratorio no cambia",
    ),
    (
        "ac3-the-bearer-stays-in-its-origin",
        "el bearer viaja a su origen y a ningun otro",
    ),
    (
        "ac3-offline-with-a-local-fixture",
        "el modo sin red sirve el fixture local con el mismo digest",
    ),
    (
        "ac3-the-service-refuses-what-is-beyond-the-budget",
        "el servicio rechaza lo que no cabe en el presupuesto",
    ),
    (
        "ac3-the-budget-of-the-laboratory-is-a-budget",
        "el presupuesto del laboratorio recusa lo que no cabe",
    ),
    (
        "ac3-real-upstream",
        "el registro contra un upstream real, con presupuesto de bytes y de tiempo",
    ),
];

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

/// Runs a case of the laboratory.
///
/// The case is **inscribed before it runs**, with `NOT_RUN`, and replaced
/// when it ends: a case that dies in the middle leaves a `NOT_RUN` in the
/// report and not a `PASS` that nobody measured.
///
/// And a panic does not take the report with it: the panic is caught, the
/// case is inscribed as the failure it is, and the panic is resumed so
/// that the suite still fails.
///
/// The arnés es el del laboratorio: lo ejecuta `support::lab`, y esta
/// suite solo declara que casos son los suyos.
fn a_case(
    id: &str,
    title: &str,
    scope: Scope,
    expectation: Expectation,
    body: impl FnOnce() -> LabCase,
) {
    run_registry_lab_case(THE_CASES, id, title, scope, expectation, body);
}

// ---------------------------------------------------------------------------
// AC1: a miss, a hit, and the hit with the upstream gone.
// ---------------------------------------------------------------------------

#[test]
fn ac1_the_hit_is_served_without_the_upstream_and_with_the_same_digest() {
    a_case(
        "ac1-hit-without-the-upstream",
        "el HIT se resuelve sin trafico externo y con el mismo digest",
        Scope::Product,
        Expectation::Succeeds,
        || {
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

            // The upstream goes away: it does not accept connections
            // anymore. The counter cannot grow once the port is closed,
            // so what proves the offline hit is that the upstream is
            // verifiably dead and the content comes back the same.
            lab.upstream.disconnect();
            assert!(
                !lab.upstream.accepts_connections(),
                "the upstream is still listening: the evidence of the offline hit is not what it claims to be"
            );

            let third = fetch(port, "pxy", ARTIFACT, &into);
            assert_eq!(third.status, 200, "{}", third.text());
            assert_eq!(third.cache.as_deref(), Some("hit"), "{}", third.text());
            let third_digest = support::sha256_of(&into);
            assert_eq!(third_digest, first_digest, "the digest changed");
            assert_eq!(third_digest, support::sha256_of(&fixture));

            support::stop(&mut service);
            LabCase::new(
                "ac1-hit-without-the-upstream",
                "el HIT se resuelve sin trafico externo y con el mismo digest",
            )
            .command(&third.command)
            .origin(&lab.upstream.origin())
            .sha256(&third_digest)
            .passed()
        },
    );
}

// ---------------------------------------------------------------------------
// AC2: the credential, the refusals, and the count that does not lie.
// ---------------------------------------------------------------------------

/// A laboratory with two proxies: one with the right credential and one
/// with a wrong one, against an upstream that **demands** a credential.
fn the_lab_of_the_credentials() -> Lab {
    let lab = Lab::new(|upstream| {
        vec![
            (
                "pxy-auth",
                proxy_of(upstream, CACHE_FRESH, Some("env:LAB_TOKEN")),
            ),
            (
                "pxy-auth-bad",
                proxy_of(upstream, CACHE_FRESH, Some("env:LAB_TOKEN_BAD")),
            ),
        ]
    });
    // The upstream only answers to the credential it demands.
    lab.upstream
        .require_authorization(&format!("Bearer {UPSTREAM_SECRET}"));
    lab.upstream.put(ARTIFACT, FIXTURE, None);
    lab
}

/// The environment of the service in the cases of the credential.
fn the_environment_of_the_credentials() -> [(&'static str, &'static str); 3] {
    [
        (
            "HODEI_PROXY__ALLOWED_CREDENTIALS",
            "LAB_TOKEN,LAB_TOKEN_BAD",
        ),
        ("LAB_TOKEN", UPSTREAM_SECRET),
        ("LAB_TOKEN_BAD", WRONG_SECRET),
    ]
}

#[test]
fn ac2_the_credential_travels_to_the_upstream() {
    a_case(
        "ac2-the-credential-travels",
        "la credencial del proxy llega a su upstream y el contenido se sirve",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = the_lab_of_the_credentials();
            let into = lab.file("response.bin");
            let mut service = lab.start(&the_environment_of_the_credentials());
            let port = service.port;

            // The proxy sends the credential that the upstream demands, and
            // the upstream answers with the content.
            let reply = fetch(port, "pxy-auth", ARTIFACT, &into);
            assert_eq!(reply.status, 200, "{}", reply.text());
            assert_eq!(reply.body, FIXTURE);
            let seen = lab.upstream.requests();
            assert!(!seen.is_empty(), "the upstream was never asked");
            for one in &seen {
                assert_eq!(
                    one.authorization.as_deref(),
                    Some(format!("Bearer {UPSTREAM_SECRET}").as_str()),
                    "{}",
                    one.path
                );
            }

            support::stop(&mut service);
            LabCase::new(
                "ac2-the-credential-travels",
                "la credencial del proxy llega a su upstream y el contenido se sirve",
            )
            .command(&reply.command)
            .origin(&lab.upstream.origin())
            .sha256(&support::sha256_of(&into))
            .passed()
        },
    );
}

#[test]
fn ac2_an_invalid_credential_is_refused_with_a_cause() {
    a_case(
        "ac2-invalid-credential",
        "una credencial invalida se rechaza, y el rechazo dice por que",
        Scope::Product,
        Expectation::Refused,
        || {
            let lab = the_lab_of_the_credentials();
            let into = lab.file("response.bin");
            let mut service = lab.start(&the_environment_of_the_credentials());
            let port = service.port;

            // The same upstream, the same path and the same client: only
            // the credential of the proxy is wrong. The upstream rejects
            // it, and the registry does not serve what it did not get.
            let reply = fetch(port, "pxy-auth-bad", ARTIFACT, &into);
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
            // The upstream did receive the credential, and it was the
            // wrong one: the rejection is of the credential and not of
            // the absence of it.
            let seen = lab.upstream.requests();
            let wrong = seen
                .iter()
                .find(|one| one.path == ARTIFACT)
                .expect("the upstream was asked");
            assert_eq!(
                wrong.authorization.as_deref(),
                Some(format!("Bearer {WRONG_SECRET}").as_str()),
                "the upstream did not receive the credential of the proxy"
            );

            support::stop(&mut service);
            let cause = format!("HTTP {}: {}", reply.status, reply.text());
            let case = LabCase::new(
                "ac2-invalid-credential",
                "una credencial invalida se rechaza, y el rechazo dice por que",
            )
            .command(&reply.command)
            .origin(&lab.upstream.origin());
            if refused {
                case.refused(cause)
            } else {
                case.failed(cause)
            }
        },
    );
}

#[test]
fn ac2_a_corrupt_digest_is_refused_with_a_cause() {
    a_case(
        "ac2-corrupt-digest",
        "un digest corrupto se rechaza, y el rechazo dice por que",
        Scope::Product,
        Expectation::Refused,
        || {
            let lab =
                Lab::new(|upstream| vec![("pxy-digest", proxy_of(upstream, CACHE_FRESH, None))]);
            // The upstream declares a digest that is not the one of the
            // bytes.
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

            support::stop(&mut service);
            let cause = format!("HTTP {}: {}", reply.status, reply.text());
            let case = LabCase::new(
                "ac2-corrupt-digest",
                "un digest corrupto se rechaza, y el rechazo dice por que",
            )
            .command(&reply.command)
            .origin(&lab.upstream.origin());
            if refused {
                case.refused(cause)
            } else {
                case.failed(cause)
            }
        },
    );
}

#[test]
fn ac2_the_summary_counts_the_skipped_cases_apart_and_never_calls_them_green() {
    a_case(
        "ac2-the-count-does-not-lie",
        "los casos que no se ejecutan no cuentan como verdes",
        Scope::Harness,
        Expectation::Succeeds,
        || {
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
            // The count of the green cases is the count of the passes: the
            // ones that did not run are never part of it.
            assert_eq!(summary.green_cases(), 1);
            assert!(!summary.green, "a skipped case was counted as green");
            assert!(!summary.complete);
            assert!(Outcome::Pass.is_green());
            assert!(!Outcome::Fail.is_green());
            assert!(!Outcome::NotRun.is_green());

            LabCase::new(
                "ac2-the-count-does-not-lie",
                "los casos que no se ejecutan no cuentan como verdes",
            )
            .command("Summary::of(casos)")
            .passed()
        },
    );
}

// ---------------------------------------------------------------------------
// AC3: the isolation, the bearer, the fixture, the budget and the opt-in.
// ---------------------------------------------------------------------------

/// The places where a service would put its configuration and its cache.
///
/// The ones of depth three and deeper are there on purpose: a service
/// that caches writes `~/.config/hodei/<fichero>`, and a probe that only
/// looked two levels down would not see it.
const THE_PLACES_OF_A_SERVICE: &[&str] = &[
    ".hodei",
    ".config/hodei",
    ".cache/hodei",
    ".local/share/hodei",
    ".local/state/hodei",
    ".var/app/hodei",
];

/// What is in the places of a service, and whether they exist.
fn the_probes(home: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for place in THE_PLACES_OF_A_SERVICE {
        let directory = home.join(place);
        found.push(format!("{place}: {}", directory.exists()));
        collect(&directory, &directory, 2, &mut found);
    }
    found.sort();
    found
}

/// What is under a directory, up to a number of levels.
fn collect(directory: &Path, root: &Path, levels: usize, found: &mut Vec<String>) {
    if levels == 0 {
        return;
    }
    let Ok(reader) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in reader.flatten() {
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_symlink() {
            continue;
        }
        let name = path
            .strip_prefix(root)
            .map(|relative| relative.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string_lossy().into_owned());
        found.push(format!("{name} {}", metadata.len()));
        if metadata.is_dir() {
            collect(&path, root, levels - 1, found);
        }
    }
}

#[test]
fn ac3_the_laboratory_never_touches_the_home_of_the_user() {
    a_case(
        "ac3-the-home-of-the-user-is-untouched",
        "el HOME de quien ejecuta el laboratorio no cambia",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let user_home = PathBuf::from(std::env::var("HOME").expect("the home of the user"));
            // The trace of the laboratory in that HOME: anything whose
            // name talks about it.
            let before_trace = HomeSnapshot::capture_matching(&user_home, 2, "hodei");
            let before_probes = the_probes(&user_home);

            let lab = Lab::new(|upstream| vec![("pxy", proxy_of(upstream, CACHE_FRESH, None))]);
            lab.upstream.put(ARTIFACT, FIXTURE, None);
            let into = lab.file("response.bin");
            let mut service = lab.start(&[]);
            let port = service.port;

            // The service runs with the HOME of the laboratory, and not
            // with the one of whoever runs the laboratory. Reading the
            // environment of the process is not optional: without that
            // datum the isolation is not proved, and a case that is not
            // proved cannot come out green.
            let home_of_the_service = the_home_of_the_process(service.child.id());
            let Some(home) = home_of_the_service.as_deref() else {
                support::stop(&mut service);
                return LabCase::new(
                    "ac3-the-home-of-the-user-is-untouched",
                    "el HOME de quien ejecuta el laboratorio no cambia",
                )
                .not_run(
                    "the environment of the service could not be read: the isolation is not proved",
                );
            };
            assert_eq!(
                home,
                lab.home.path().to_string_lossy(),
                "the service does not run with the HOME of the laboratory"
            );
            assert!(
                !lab.home.path().starts_with(&user_home),
                "the HOME of the laboratory is inside the HOME of the user"
            );

            let reply = fetch(port, "pxy", ARTIFACT, &into);
            assert_eq!(reply.status, 200, "{}", reply.text());
            support::stop(&mut service);

            let after_trace = HomeSnapshot::capture_matching(&user_home, 2, "hodei");
            let after_probes = the_probes(&user_home);

            let trace = before_trace.differences(&after_trace);
            assert!(
                trace.is_empty(),
                "the laboratory left a trace in the HOME of the user: {trace:?}"
            );
            assert_eq!(
                before_probes, after_probes,
                "the laboratory left something of its own in the HOME of the user"
            );

            LabCase::new(
                "ac3-the-home-of-the-user-is-untouched",
                "el HOME de quien ejecuta el laboratorio no cambia",
            )
            .command(&format!(
                "find {} -maxdepth 2 -iname '*hodei*' -print",
                user_home.display()
            ))
            .origin(&lab.upstream.origin())
            .passed()
        },
    );
}

#[test]
fn ac3_the_bearer_never_reaches_another_origin() {
    a_case(
        "ac3-the-bearer-stays-in-its-origin",
        "el bearer viaja a su origen y a ningun otro",
        Scope::Product,
        Expectation::Succeeds,
        || {
            // The origin of the upstream, and another one that the
            // redirect points to.
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
            let allowed = format!(
                "127.0.0.1:{};private;http,127.0.0.1:{};private;http",
                lab.upstream.port(),
                foreign.port()
            );
            let mut service = lab.start(&[
                ("HODEI_PROXY__ALLOWED_CREDENTIALS", "LAB_TOKEN"),
                ("LAB_TOKEN", UPSTREAM_SECRET),
                ("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str()),
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

            // The other origin receives nothing: the bearer is not its
            // own.
            let other = foreign.requests();
            assert!(!other.is_empty(), "the other origin was never asked");
            for seen in &other {
                assert_eq!(
                    seen.authorization, None,
                    "the bearer reached another origin: {}",
                    seen.path
                );
            }

            // And the credential of whoever asks is not the credential of
            // the upstream: it never leaves the service.
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

            support::stop(&mut service);
            LabCase::new(
                "ac3-the-bearer-stays-in-its-origin",
                "el bearer viaja a su origen y a ningun otro",
            )
            .command(&reply.command)
            .origin(&foreign.origin())
            .sha256(&support::sha256_of(&into))
            .passed()
        },
    );
}

#[test]
fn ac3_the_fixture_is_served_without_network_when_the_service_is_offline() {
    a_case(
        "ac3-offline-with-a-local-fixture",
        "el modo sin red sirve el fixture local con el mismo digest",
        Scope::Product,
        Expectation::Succeeds,
        || {
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

            support::stop(&mut offline);
            LabCase::new(
                "ac3-offline-with-a-local-fixture",
                "el modo sin red sirve el fixture local con el mismo digest",
            )
            .command(&reply.command)
            .origin(&lab.upstream.origin())
            .sha256(&digest)
            .passed()
        },
    );
}

#[test]
fn ac3_the_service_refuses_what_is_beyond_the_budget() {
    a_case(
        "ac3-the-service-refuses-what-is-beyond-the-budget",
        "el servicio rechaza lo que no cabe en el presupuesto",
        Scope::Product,
        Expectation::Refused,
        || {
            // The budget of the laboratory, small on purpose so that the
            // upstream can serve more than it allows without costing
            // anything.
            let budget = Budget {
                max_bytes: 1024,
                max_seconds: 30,
            };
            let lab = Lab::new(|upstream| {
                let mut configuration = proxy_of(upstream, CACHE_FRESH, None);
                configuration["upstream"]["max_bytes"] = budget.max_bytes.into();
                vec![("pxy-budget", configuration)]
            });
            // The upstream serves four times the budget.
            let body = vec![b'x'; (budget.max_bytes * 4) as usize];
            lab.upstream.put(ARTIFACT, &body, None);

            let into = lab.file("response.bin");
            let owned = budget.service_environment();
            let borrowed: Vec<(&str, &str)> = owned
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str()))
                .collect();
            let mut service = lab.start(&borrowed);
            let port = service.port;

            let reply = fetch(port, "pxy-budget", ARTIFACT, &into);
            let refused = reply.status >= 400;
            assert!(
                refused,
                "the service served {} bytes with a budget of {}: {}",
                body.len(),
                budget.max_bytes,
                reply.text()
            );
            assert!(
                !reply
                    .body
                    .windows(body.len())
                    .any(|window| window == body.as_slice()),
                "the bytes beyond the budget were served anyway"
            );
            // The budget of the laboratory refuses it too: the two
            // budgets are the same one.
            assert!(budget.permits(body.len() as u64, Duration::ZERO).is_err());

            support::stop(&mut service);
            let cause = format!(
                "HTTP {}: {} (the budget allows {} bytes and the content was {})",
                reply.status,
                reply.text(),
                budget.max_bytes,
                body.len()
            );
            let case = LabCase::new(
                "ac3-the-service-refuses-what-is-beyond-the-budget",
                "el servicio rechaza lo que no cabe en el presupuesto",
            )
            .command(&reply.command)
            .origin(&lab.upstream.origin());
            if refused {
                case.refused(cause)
            } else {
                case.failed(cause)
            }
        },
    );
}

#[test]
fn ac3_the_real_mode_is_opt_in_and_bounded_by_bytes_and_by_time() {
    a_case(
        "ac3-real-upstream",
        "el registro contra un upstream real, con presupuesto de bytes y de tiempo",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let budget = Budget::from_environment();
            let case = LabCase::new(
                "ac3-real-upstream",
                "el registro contra un upstream real, con presupuesto de bytes y de tiempo",
            )
            .command(&format!(
                "curl http://127.0.0.1:<puerto>/repository/real/{REAL_ARTIFACT}"
            ));

            if !real_mode_enabled() {
                // Without the opt-in the case does not run, and a case
                // that does not run is not a green case.
                return case.not_run(format!(
                    "the real mode is opt-in: set {REAL_MODE_VARIABLE}=1 to run it"
                ));
            }
            the_real_case(&budget)
        },
    );
}

/// The budget is a budget: what does not fit in it does not pass.
#[test]
fn ac3_the_budget_of_the_laboratory_is_a_budget() {
    a_case(
        "ac3-the-budget-of-the-laboratory-is-a-budget",
        "el presupuesto del laboratorio recusa lo que no cabe",
        Scope::Harness,
        Expectation::Succeeds,
        || {
            let budget = Budget::from_environment();
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

            // And the budget reaches the service, which refuses to bring
            // more than it allows.
            let environment = budget.service_environment();
            assert!(environment.iter().any(|(name, value)| {
                name == "HODEI_PROXY__MAX_BYTES" && value == &budget.max_bytes.to_string()
            }));
            assert!(environment.iter().any(|(name, value)| {
                name == "HODEI_PROXY__TOTAL_TIMEOUT_SECS"
                    && value == &budget.max_seconds.to_string()
            }));

            LabCase::new(
                "ac3-the-budget-of-the-laboratory-is-a-budget",
                "el presupuesto del laboratorio recusa lo que no cabe",
            )
            .command("Budget::permits(bytes, elapsed)")
            .passed()
        },
    );
}

/// The real case: the registry against a real upstream, within the budget.
fn the_real_case(budget: &Budget) -> LabCase {
    let lab = Lab::new(|upstream| {
        let mut configuration = proxy_of(upstream, CACHE_FRESH, None);
        configuration["upstream"]["url"] = REAL_UPSTREAM.into();
        // The upstream of the real mode obeys the budget of the
        // laboratory: the service does not bring more than it allows.
        configuration["upstream"]["max_bytes"] = budget.max_bytes.into();
        vec![("real", configuration)]
    });

    // The service of the real mode also runs with the HOME of the
    // laboratory: no mode of the laboratory touches the HOME of whoever
    // runs it.
    let mut environment: Vec<(String, String)> = vec![(
        "HODEI_PROXY__ALLOWED_HOSTS".to_string(),
        "repo.maven.apache.org".to_string(),
    )];
    environment.extend(budget.service_environment());
    let borrowed: Vec<(&str, &str)> = environment
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let mut service = lab.start(&borrowed);
    let port = service.port;
    let into = lab.file("response.bin");

    let started = Instant::now();
    let reply = fetch(port, "real", REAL_ARTIFACT, &into);
    let elapsed = started.elapsed();
    let bytes = std::fs::metadata(&into).map(|meta| meta.len()).unwrap_or(0);
    support::stop(&mut service);

    let case = LabCase::new(
        "ac3-real-upstream",
        "el registro contra un upstream real, con presupuesto de bytes y de tiempo",
    )
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
