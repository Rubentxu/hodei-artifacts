//! HSP-237: readiness and liveness are two different questions.
//!
//! What is demonstrated here:
//!
//! - against the **real service**: the really started deployment
//!   says `200` on `/health/ready` and on `/health/live`; when the content
//!   store the profile requires is hot-disconnected, `/health/ready`
//!   turns `503` **without** saying where the store is, `/health/live`
//!   keeps saying `200`, and when the store is put back readiness returns to
//!   `200`;
//! - against the **probe**: a dependency the profile does not require is not
//!   checked and cannot make readiness fail; a dependency that
//!   does not answer produces `timeout` and a `503` without hanging; the probe
//!   checks each dependency once and caches its verdict for a second,
//!   so that two consecutive probes do not spend the budget twice;
//!   and the data path does not go through the probe.
//!
//! This suite's deployment runs in the **graph profile**: the catalog
//! is a namespace of a real Dgraph and the content is in a directory
//! of the deployment. The profile requires two dependencies, `graph` and
//! `content_store`, and the suite **hot-disconnects** both: the
//! content directory (it is moved) and the alpha (the relay through
//! which the service reaches it is cut). In both cases readiness turns `503`
//! without saying where the store is, liveness stays at `200`, and when
//! it is put back readiness returns.
//!
//! What is **not** demonstrated here: the profile with external S3; this
//! deployment's content is the directory, not a bucket.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;

use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use hodei_artifacts_api::authorization::AuthorizerPort;
use hodei_artifacts_api::readiness::{
    CachedProbe, Dependency, DependencyFailure, ReadinessProbe, VERDICT_TTL,
};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::{
    AccountId, AuthenticationError, AuthorizationOutcome, AuthorizationRequest, Credentials,
    PrincipalAuthenticator, PrincipalClaims, PrincipalId,
};
use serde_json::Value;
use support::lab::{Expectation, LabCase, Scope, describe_registry_lab, run_registry_lab_case};
use support::{Answer, Curl, Deployment};

#[path = "support/graph_profile.rs"]
mod graph_profile;
use graph_profile::Workdir;
use tower::ServiceExt;

/// The case: with the required store there, readiness says it can
/// serve.
const AC1_READY_200: &str = "ac1-the-readiness-is-200-when-the-required-storage-is-there";
/// The case: when the required store is disconnected, readiness stops being
/// `200`.
const AC1_READY_503: &str = "ac1-the-readiness-is-not-200-when-the-required-storage-is-gone";
/// The case: the process lives even if the store is not there.
const AC1_LIVE_200: &str = "ac1-the-liveness-is-200-when-the-required-storage-is-gone";
/// The case: putting the store back restores readiness.
const AC1_READY_BACK: &str = "ac1-the-readiness-comes-back-when-the-required-storage-returns";
/// The case: when the alpha is cut hot, readiness stops being `200`
/// and does not say where the alpha is.
const AC1_GRAPH_503: &str = "ac1-the-readiness-is-not-200-when-the-graph-is-cut";
/// The case: the process lives even if the alpha is cut.
const AC1_GRAPH_LIVE: &str = "ac1-the-liveness-is-200-when-the-graph-is-cut";
/// The case: putting the alpha back restores readiness.
const AC1_GRAPH_BACK: &str = "ac1-the-readiness-comes-back-when-the-graph-returns";
/// The case: the graph profile with its content is ready and requires the
/// alpha and the content store.
const AC2_STANDALONE: &str = "ac2-the-graph-profile-is-ready";
/// The case: the health body does not say where the deployment is.
const AC2_NO_LEAK: &str = "ac2-the-body-of-the-health-endpoints-does-not-carry-the-deployment";
/// The case: a dependency the profile does not require cannot make
/// readiness fail.
const AC2_OPTIONAL: &str =
    "ac2-a-dependency-that-the-profile-does-not-require-cannot-fail-the-readiness";
/// The case: a dependency that does not answer produces a `timeout` and a
/// `503`, and does not hang the request.
const AC2_TIMEOUT: &str = "ac2-a-dependency-that-does-not-answer-produces-a-timeout-and-a-503";
/// The case: the probe checks each required dependency once.
const AC3_BOUNDED: &str = "ac3-the-probe-checks-every-required-dependency-once";
/// The case: two consecutive probes share the verdict.
const AC3_CACHE: &str = "ac3-two-readiness-in-a-row-share-the-verdict";
/// The case: the data path does not go through the probe.
const AC3_DATA_PATH: &str = "ac3-the-data-path-does-not-go-through-the-readiness-probe";

/// The cases a full run has to execute.
///
/// They are declared **before** running: a run that only reached some
/// says which ones did not run, and a report with a case that did not run is never
/// green.
const THE_CASES: &[(&str, &str)] = &[
    (
        AC1_READY_200,
        "con el almacen exigido ahi, la readiness dice que se puede servir",
    ),
    (
        AC1_READY_503,
        "al desconectar el almacen exigido, la readiness deja de ser 200",
    ),
    (
        AC1_LIVE_200,
        "el proceso vive aunque el almacen exigido no este",
    ),
    (
        AC1_READY_BACK,
        "devolver el almacen exigido devuelve la readiness",
    ),
    (
        AC1_GRAPH_503,
        "al cortar el alpha en caliente, la readiness deja de ser 200 y no dice donde esta",
    ),
    (
        AC1_GRAPH_LIVE,
        "el proceso vive aunque el alpha este cortado",
    ),
    (AC1_GRAPH_BACK, "devolver el alpha devuelve la readiness"),
    (
        AC2_STANDALONE,
        "el perfil de grafo esta listo y exige el alpha y el contenido",
    ),
    (
        AC2_NO_LEAK,
        "el cuerpo de la salud no dice donde esta el despliegue",
    ),
    (
        AC2_OPTIONAL,
        "una dependencia que el perfil no exige no puede fallar el readiness",
    ),
    (
        AC2_TIMEOUT,
        "una dependencia que no contesta produce un timeout y un 503, y no cuelga",
    ),
    (
        AC3_BOUNDED,
        "el probe comprueba cada dependencia exigida una vez y no la recorre",
    ),
    (
        AC3_CACHE,
        "dos readiness seguidas comparten el veredicto y no gastan presupuesto",
    ),
    (
        AC3_DATA_PATH,
        "el camino de los datos no pasa por el probe de readiness",
    ),
];

/// A fake probe: counts the checks and answers what it is
/// told.
///
/// The real binary does not admit a fake probe -- its own is decided by the
/// profile --, so this suite's harness injects it into the process.
struct Counting {
    required: Vec<Dependency>,
    checks: AtomicUsize,
    answer: Result<(), DependencyFailure>,
}

impl Counting {
    fn new(required: Vec<Dependency>, answer: Result<(), DependencyFailure>) -> Self {
        Self {
            required,
            checks: AtomicUsize::new(0),
            answer,
        }
    }

    /// How many checks have been made.
    fn checks(&self) -> usize {
        self.checks.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl ReadinessProbe for Counting {
    fn required(&self) -> Vec<Dependency> {
        self.required.clone()
    }

    async fn check(&self, _dependency: Dependency) -> Result<(), DependencyFailure> {
        self.checks.fetch_add(1, Ordering::Relaxed);
        self.answer
    }
}

/// A probe that never answers.
struct Silent;

#[async_trait]
impl ReadinessProbe for Silent {
    fn required(&self) -> Vec<Dependency> {
        vec![Dependency::ContentStore]
    }

    async fn check(&self, _dependency: Dependency) -> Result<(), DependencyFailure> {
        tokio::time::sleep(Duration::from_secs(30)).await;
        Ok(())
    }
}

/// A relay to the alpha that the test can cut: cutting it drops all
/// open connections and stops listening, so that the service's next
/// call really fails. It can be brought up again at the
/// same address.
struct Relay {
    address: String,
    target: String,
    tasks: JoinSet<()>,
}

impl Relay {
    async fn bind(address: &str, target: String) -> Relay {
        let listener = TcpListener::bind(address).await.expect("relay binds");
        let address = listener.local_addr().expect("relay address").to_string();
        let mut tasks = JoinSet::new();
        let forward = target.clone();
        tasks.spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                let Ok((mut inbound, _)) = listener.accept().await else {
                    return;
                };
                let target = forward.clone();
                connections.spawn(async move {
                    if let Ok(mut outbound) = TcpStream::connect(target).await {
                        let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                    }
                });
            }
        });
        Relay {
            address,
            target,
            tasks,
        }
    }

    async fn to(target: String) -> Relay {
        Relay::bind("127.0.0.1:0", target).await
    }

    /// Cut it: no connection survives and nothing listens.
    async fn cut(&mut self) {
        self.tasks.abort_all();
        while self.tasks.join_next().await.is_some() {}
    }

    /// Put it back on the same address.
    async fn restore(&mut self) {
        let again = Relay::bind(&self.address, self.target.clone()).await;
        self.tasks = again.tasks;
    }
}

/// A fake credential: the `acme` account and the `alice` person.
///
/// The case does not measure authentication: it measures the probe's path.
struct Alice;

#[async_trait]
impl PrincipalAuthenticator for Alice {
    async fn authenticate(
        &self,
        _credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        Ok(PrincipalClaims::new(
            PrincipalId::new("hrn:hodei:iam::acme:User/alice").expect("the principal"),
            AccountId::new("acme").expect("the account"),
        ))
    }
}

/// An authorizer that permits everything: the case does not measure authorization.
struct AllowAll;

impl AuthorizerPort for AllowAll {
    fn authorize(&self, _request: &AuthorizationRequest) -> AuthorizationOutcome {
        AuthorizationOutcome::allow()
    }
}

/// The in-process application, with the probe it is told.
///
/// The catalog is a Dgraph namespace and is what the data path reads:
/// without it, the content surface would have nothing to read.
async fn the_app(probe: Arc<dyn ReadinessProbe>) -> (Router, Workdir) {
    let tmp = Workdir::new();
    let mut config = AppConfig::default();
    tmp.apply(&mut config);
    config.security.raw_api_enabled = true;

    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .expect("the bootstrap")
    .with_authenticator(Arc::new(Alice))
    .with_authorization(Arc::new(AllowAll))
    .with_readiness(probe);

    (build_router(state, &config), tmp)
}

/// A request to the in-process router, with its code and body.
async fn request(app: &Router, uri: &str) -> (StatusCode, String) {
    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header("authorization", "Bearer alice")
        .body(Body::empty())
        .expect("the request");
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("the router answers");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("the body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

/// A file for a response body.
fn next(name: &str) -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let number = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{number}"))
}

/// A request to the real service, with its body and with the command
/// that made it.
///
/// The command written in the report is the one that was executed: a
/// report that says otherwise is an opinion.
fn call(port: u16, path: &str) -> (Answer, String) {
    let into = next("answer");
    let curl = Curl::call(port, "GET", path, "", None, None, Some(&into));
    let shown = curl.shown();
    (curl.run(), shown)
}

/// The text of a response.
fn text_of(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

/// The time to wait for the short cache to expire.
///
/// The verdict is cached for a second: checking before it expires
/// would measure the cache, not the deployment.
fn the_cache_is_gone() -> Duration {
    VERDICT_TTL + Duration::from_millis(500)
}

/// Runs a laboratory case.
fn a_case(
    id: &str,
    title: &str,
    scope: Scope,
    expectation: Expectation,
    body: impl FnOnce() -> LabCase,
) {
    describe_registry_lab(
        "Laboratorio de la readiness — informe",
        &[
            "- **El perfil del laboratorio es el de grafo.** El catalogo es un namespace de un Dgraph real y el contenido esta en un directorio del despliegue. El caso del contenido en S3 externo no aplica a este despliegue y no se maquilla.",
            "- **El binario real.** Los casos de AC1 y del perfil se miden contra el servicio arrancado de verdad, no contra una funcion.",
            "- **El probe se mide aparte.** Los casos de timeout, de cache y de presupuesto inyectan un probe de mentira: el binario real decide su probe y no admite otro.",
        ],
    );
    run_registry_lab_case(THE_CASES, id, title, scope, expectation, body);
}

#[test]
fn la_readiness_y_la_liveness_dicen_cosas_distintas() {
    // The runtime lives for the whole test: it keeps the relay alive while
    // the sync cases run, and it runs the in-process cases at the end.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("the runtime");

    // ---- The real deployment, reaching the alpha through a relay.
    let deployment = Deployment::new();
    let tenant = deployment.graph.as_ref().expect("the graph profile");
    let mut relay = runtime.block_on(Relay::to(tenant.endpoint.clone()));
    let service = deployment.start(&[("HODEI_GRAPH__ENDPOINT", relay.address.as_str())]);
    let port = service.port;

    // ---- AC1: with the store there, it can serve.
    a_case(
        AC1_READY_200,
        "con el almacen exigido ahi, la readiness dice que se puede servir",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let (ready, ready_command) = call(port, "/health/ready");
            let (live, live_command) = call(port, "/health/live");
            if ready.status != 200 {
                return LabCase::new(
                    AC1_READY_200,
                    "con el almacen exigido ahi, la readiness dice que se puede servir",
                )
                .failed(format!(
                    "the readiness answered {}: {}",
                    ready.status,
                    text_of(&ready)
                ));
            }
            if live.status != 200 {
                return LabCase::new(
                    AC1_READY_200,
                    "con el almacen exigido ahi, la readiness dice que se puede servir",
                )
                .failed(format!(
                    "the liveness answered {}: {}",
                    live.status,
                    text_of(&live)
                ));
            }
            LabCase::new(
                AC1_READY_200,
                "con el almacen exigido ahi, la readiness dice que se puede servir",
            )
            .command(&format!("{ready_command} y {live_command}"))
            .passed()
        },
    );

    // ---- The required store is disconnected **hot**. It is not deleted
    // before starting: a store that is not there at startup is another
    // scenario, and this is the one to be measured.
    let cas = std::path::PathBuf::from(&deployment.cas);
    let away = deployment.directory.path().join("cas-away");
    std::fs::rename(&cas, &away).expect("the store is moved away");
    std::thread::sleep(the_cache_is_gone());

    // ---- AC1: readiness stops being 200.
    a_case(
        AC1_READY_503,
        "al desconectar el almacen exigido, la readiness deja de ser 200",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let (ready, command) = call(port, "/health/ready");
            if ready.status != 503 {
                return LabCase::new(
                    AC1_READY_503,
                    "al desconectar el almacen exigido, la readiness deja de ser 200",
                )
                .failed(format!(
                    "the readiness answered {}: {}",
                    ready.status,
                    text_of(&ready)
                ));
            }
            let body = text_of(&ready);
            if body.contains(&deployment.cas) {
                return LabCase::new(
                    AC1_READY_503,
                    "al desconectar el almacen exigido, la readiness deja de ser 200",
                )
                .failed("the body of the readiness names the path of the store".to_string());
            }
            LabCase::new(
                AC1_READY_503,
                "al desconectar el almacen exigido, la readiness deja de ser 200",
            )
            .command(&command)
            .passed()
        },
    );

    // ---- AC1: the process lives.
    a_case(
        AC1_LIVE_200,
        "el proceso vive aunque el almacen exigido no este",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let (live, command) = call(port, "/health/live");
            if live.status != 200 {
                return LabCase::new(
                    AC1_LIVE_200,
                    "el proceso vive aunque el almacen exigido no este",
                )
                .failed(format!(
                    "the liveness answered {}: {}",
                    live.status,
                    text_of(&live)
                ));
            }
            LabCase::new(
                AC1_LIVE_200,
                "el proceso vive aunque el almacen exigido no este",
            )
            .command(&command)
            .passed()
        },
    );

    // ---- AC2: the body carries nothing of the deployment.
    a_case(
        AC2_NO_LEAK,
        "el cuerpo de la salud no dice donde esta el despliegue",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let (ready, ready_command) = call(port, "/health/ready");
            let (live, live_command) = call(port, "/health/live");
            let root = deployment.directory.path().to_string_lossy().into_owned();
            for (what, body) in [
                ("the readiness", text_of(&ready)),
                ("the liveness", text_of(&live)),
            ] {
                for forbidden in [
                    root.as_str(),
                    deployment.cas.as_str(),
                    "http://",
                    "https://",
                    "AKIA",
                    "minio",
                    "secret",
                    "password",
                    "bucket",
                ] {
                    if body.contains(forbidden) {
                        return LabCase::new(
                            AC2_NO_LEAK,
                            "el cuerpo de la salud no dice donde esta el despliegue",
                        )
                        .failed(format!("the body of {what} carries {forbidden:?}: {body}"));
                    }
                }
            }
            LabCase::new(
                AC2_NO_LEAK,
                "el cuerpo de la salud no dice donde esta el despliegue",
            )
            .command(&format!("{ready_command} y {live_command}"))
            .passed()
        },
    );

    // ---- AC1: the store comes back.
    std::fs::rename(&away, &cas).expect("the store is moved back");
    std::thread::sleep(the_cache_is_gone());

    a_case(
        AC1_READY_BACK,
        "devolver el almacen exigido devuelve la readiness",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let (ready, command) = call(port, "/health/ready");
            if ready.status != 200 {
                return LabCase::new(
                    AC1_READY_BACK,
                    "devolver el almacen exigido devuelve la readiness",
                )
                .failed(format!(
                    "the readiness answered {}: {}",
                    ready.status,
                    text_of(&ready)
                ));
            }
            LabCase::new(
                AC1_READY_BACK,
                "devolver el almacen exigido devuelve la readiness",
            )
            .command(&command)
            .passed()
        },
    );

    // ---- The alpha is cut **hot**: the service already started against
    // it, and suddenly it does not answer.
    runtime.block_on(relay.cut());
    std::thread::sleep(the_cache_is_gone());

    a_case(
        AC1_GRAPH_503,
        "al cortar el alpha en caliente, la readiness deja de ser 200 y no dice donde esta",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let title =
                "al cortar el alpha en caliente, la readiness deja de ser 200 y no dice donde esta";
            let (ready, command) = call(port, "/health/ready");
            if ready.status != 503 {
                return LabCase::new(AC1_GRAPH_503, title).failed(format!(
                    "the readiness answered {}: {}",
                    ready.status,
                    text_of(&ready)
                ));
            }
            let body = text_of(&ready);
            // It names the dependency that fails, and only its class.
            if !body.contains("graph") {
                return LabCase::new(AC1_GRAPH_503, title)
                    .failed(format!("the body does not name the graph: {body}"));
            }
            let alpha = tenant.endpoint.as_str();
            for forbidden in [alpha, relay.address.as_str(), "127.0.0.1", "groot"] {
                if body.contains(forbidden) {
                    return LabCase::new(AC1_GRAPH_503, title)
                        .failed(format!("the body carries {forbidden:?}: {body}"));
                }
            }
            LabCase::new(AC1_GRAPH_503, title)
                .command(&command)
                .passed()
        },
    );

    a_case(
        AC1_GRAPH_LIVE,
        "el proceso vive aunque el alpha este cortado",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let title = "el proceso vive aunque el alpha este cortado";
            let (live, command) = call(port, "/health/live");
            if live.status != 200 {
                return LabCase::new(AC1_GRAPH_LIVE, title).failed(format!(
                    "the liveness answered {}: {}",
                    live.status,
                    text_of(&live)
                ));
            }
            LabCase::new(AC1_GRAPH_LIVE, title)
                .command(&command)
                .passed()
        },
    );

    runtime.block_on(relay.restore());
    std::thread::sleep(the_cache_is_gone());

    a_case(
        AC1_GRAPH_BACK,
        "devolver el alpha devuelve la readiness",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let title = "devolver el alpha devuelve la readiness";
            let (ready, command) = call(port, "/health/ready");
            if ready.status != 200 {
                return LabCase::new(AC1_GRAPH_BACK, title).failed(format!(
                    "the readiness answered {}: {}",
                    ready.status,
                    text_of(&ready)
                ));
            }
            LabCase::new(AC1_GRAPH_BACK, title)
                .command(&command)
                .passed()
        },
    );

    // ---- AC2: the standalone profile.
    a_case(
        AC2_STANDALONE,
        "el perfil de grafo esta listo y exige el alpha y el contenido",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let (ready, command) = call(port, "/health/ready");
            if ready.status != 200 {
                return LabCase::new(
                    AC2_STANDALONE,
                    "el perfil de grafo esta listo y exige el alpha y el contenido",
                )
                .failed(format!(
                    "the readiness answered {}: {}",
                    ready.status,
                    text_of(&ready)
                ));
            }
            let body: Value = serde_json::from_str(&text_of(&ready)).unwrap_or(Value::Null);
            let names: Vec<String> = body["dependencies"]
                .as_array()
                .map(|dependencies| {
                    dependencies
                        .iter()
                        .filter_map(|dependency| dependency["name"].as_str())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            // This deployment's profile requires the alpha and the content
            // store. It does not require the trust policy: it is loaded into
            // memory at startup and cannot be lost hot.
            if names != vec!["graph".to_string(), "content_store".to_string()] {
                return LabCase::new(
                    AC2_STANDALONE,
                    "el perfil de grafo esta listo y exige el alpha y el contenido",
                )
                .failed(format!("the dependencies of the profile are {names:?}"));
            }
            LabCase::new(
                AC2_STANDALONE,
                "el perfil de grafo esta listo y exige el alpha y el contenido",
            )
            .command(&command)
            .passed()
        },
    );

    // ---- The rest, in-process: the real binary decides its probe and does not
    // admit another, so the harness injects a fake one.
    runtime.block_on(async {
        // ---- AC2: a dependency the profile does not require.
        let probe = Arc::new(Counting::new(
            vec![Dependency::Graph],
            Err(DependencyFailure::Unreachable),
        ));
        let report = CachedProbe::new(probe.clone()).report().await;
        let checks = probe.checks();
        a_case(
            AC2_OPTIONAL,
            "una dependencia que el perfil no exige no puede fallar el readiness",
            Scope::Product,
            Expectation::Succeeds,
            || {
                if report.ready {
                    return LabCase::new(AC2_OPTIONAL, "una dependencia que el perfil no exige no puede fallar el readiness").failed(
                        "the readiness is ready although the catalog is not there".to_string(),
                    );
                }
                // The dependency that is not required does not appear in the verdict
                // and is not checked: it cannot make readiness fail.
                if report.failing != vec![(Dependency::Graph, DependencyFailure::Unreachable)] {
                    return LabCase::new(AC2_OPTIONAL, "una dependencia que el perfil no exige no puede fallar el readiness").failed(format!(
                        "the verdict names {:?}",
                        report.failing
                    ));
                }
                if checks != 1 {
                    return LabCase::new(AC2_OPTIONAL, "una dependencia que el perfil no exige no puede fallar el readiness").failed(format!(
                        "the probe checked {checks} dependencies"
                    ));
                }
                LabCase::new(AC2_OPTIONAL, "una dependencia que el perfil no exige no puede fallar el readiness")
                    .command("el probe del perfil, en proceso, con una dependencia que se exige y otra que no")
                    .passed()
            },
        );

        // ---- AC2: a dependency that does not answer.
        let (app, _tmp) = the_app(Arc::new(CachedProbe::new(Arc::new(Silent)))).await;
        let started = Instant::now();
        let (status, body) = request(&app, "/health/ready").await;
        let elapsed = started.elapsed();
        a_case(
            AC2_TIMEOUT,
            "una dependencia que no contesta produce un timeout y un 503, y no cuelga",
            Scope::Product,
            Expectation::Succeeds,
            || {
                if status != StatusCode::SERVICE_UNAVAILABLE {
                    return LabCase::new(AC2_TIMEOUT, "una dependencia que no contesta produce un timeout y un 503, y no cuelga").failed(format!(
                        "the readiness answered {status}: {body}"
                    ));
                }
                if !body.contains("timeout") {
                    return LabCase::new(AC2_TIMEOUT, "una dependencia que no contesta produce un timeout y un 503, y no cuelga").failed(format!(
                        "the body does not name the timeout: {body}"
                    ));
                }
                // The budget is one second per dependency: the
                // request does not hang, and does not wait for the dependency
                // to answer.
                if elapsed > Duration::from_secs(5) {
                    return LabCase::new(AC2_TIMEOUT, "una dependencia que no contesta produce un timeout y un 503, y no cuelga").failed(format!(
                        "the readiness took {elapsed:?}"
                    ));
                }
                LabCase::new(AC2_TIMEOUT, "una dependencia que no contesta produce un timeout y un 503, y no cuelga")
                    .command(&format!("GET /health/ready (en proceso, probe que no contesta) -> {status} en {elapsed:?}"))
                    .passed()
            },
        );

        // ---- AC3: one check per dependency.
        let probe = Arc::new(Counting::new(
            vec![Dependency::Graph, Dependency::ContentStore],
            Ok(()),
        ));
        let report = CachedProbe::new(probe.clone()).report().await;
        let checks = probe.checks();
        a_case(
            AC3_BOUNDED,
            "el probe comprueba cada dependencia exigida una vez y no la recorre",
            Scope::Product,
            Expectation::Succeeds,
            || {
                if !report.ready {
                    return LabCase::new(AC3_BOUNDED, "el probe comprueba cada dependencia exigida una vez y no la recorre").failed(
                        "the verdict is not ready".to_string(),
                    );
                }
                if checks != 2 {
                    return LabCase::new(AC3_BOUNDED, "el probe comprueba cada dependencia exigida una vez y no la recorre").failed(format!(
                        "the probe checked {checks} dependencies instead of one per required dependency"
                    ));
                }
                LabCase::new(AC3_BOUNDED, "el probe comprueba cada dependencia exigida una vez y no la recorre")
                    .command(&format!("el probe del perfil, en proceso, con dos dependencias exigidas: {checks} comprobaciones"))
                    .passed()
            },
        );

        // ---- AC3: the short cache.
        let probe = Arc::new(Counting::new(vec![Dependency::ContentStore], Ok(())));
        let (app, _tmp) = the_app(Arc::new(CachedProbe::new(probe.clone()))).await;
        let (first, _) = request(&app, "/health/ready").await;
        let (second, _) = request(&app, "/health/ready").await;
        let checks = probe.checks();
        a_case(
            AC3_CACHE,
            "dos readiness seguidas comparten el veredicto y no gastan presupuesto",
            Scope::Product,
            Expectation::Succeeds,
            || {
                if first != StatusCode::OK || second != StatusCode::OK {
                    return LabCase::new(AC3_CACHE, "dos readiness seguidas comparten el veredicto y no gastan presupuesto").failed(format!(
                        "the readiness answered {first} and {second}"
                    ));
                }
                if checks != 1 {
                    return LabCase::new(AC3_CACHE, "dos readiness seguidas comparten el veredicto y no gastan presupuesto").failed(format!(
                        "the probe was executed {checks} times instead of once"
                    ));
                }
                LabCase::new(AC3_CACHE, "dos readiness seguidas comparten el veredicto y no gastan presupuesto")
                    .command(&format!("GET /health/ready dos veces (en proceso): {first} y {second}, con {checks} comprobaciones"))
                    .passed()
            },
        );

        // ---- AC3: the data path.
        let probe = Arc::new(Counting::new(
            vec![Dependency::ContentStore],
            Err(DependencyFailure::Unreachable),
        ));
        let (app, _tmp) = the_app(Arc::new(CachedProbe::new(probe.clone()))).await;
        // Readiness runs the probe and fails: the profile requires a
        // dependency that is not there.
        let (ready, _) = request(&app, "/health/ready").await;
        let checks_after_the_readiness = probe.checks();
        // And now a data-path request: the content surface,
        // which is the one that reads the catalog.
        let (data, data_body) = request(&app, "/repository/releases/nothing.txt").await;
        let checks_after_the_data = probe.checks();
        a_case(
            AC3_DATA_PATH,
            "el camino de los datos no pasa por el probe de readiness",
            Scope::Product,
            Expectation::Succeeds,
            || {
                if ready != StatusCode::SERVICE_UNAVAILABLE {
                    return LabCase::new(AC3_DATA_PATH, "el camino de los datos no pasa por el probe de readiness").failed(format!(
                        "the readiness answered {ready} although the store is not there"
                    ));
                }
                if data == StatusCode::SERVICE_UNAVAILABLE {
                    return LabCase::new(AC3_DATA_PATH, "el camino de los datos no pasa por el probe de readiness").failed(
                        "the data path answered 503: the probe is in the data path".to_string(),
                    );
                }
                // The body is not empty: the response was written by the
                // surface's handler and not by the router. Without this, a
                // router `404` would be indistinguishable from a catalog
                // `404`, and the case would measure nothing.
                if data_body.is_empty() {
                    return LabCase::new(AC3_DATA_PATH, "el camino de los datos no pasa por el probe de readiness").failed(
                        "the data path did not answer with a body: the route did not reach the handler".to_string(),
                    );
                }
                if checks_after_the_data != checks_after_the_readiness {
                    return LabCase::new(AC3_DATA_PATH, "el camino de los datos no pasa por el probe de readiness").failed(format!(
                        "the probe ran {} times for a data request",
                        checks_after_the_data
                    ));
                }
                LabCase::new(AC3_DATA_PATH, "el camino de los datos no pasa por el probe de readiness")
                    .command(&format!(
                        "GET /health/ready -> {ready} y GET /repository/releases/nothing.txt -> {data} (en proceso), con {checks_after_the_data} comprobaciones del probe"
                    ))
                    .passed()
            },
        );
    });
}
