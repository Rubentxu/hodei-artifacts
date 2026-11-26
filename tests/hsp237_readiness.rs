//! HSP-237: la readiness y la liveness son dos preguntas distintas.
//!
//! Lo que se demuestra aqui:
//!
//! - contra el **servicio de verdad**: el despliegue arrancado de verdad
//!   dice `200` en `/health/ready` y en `/health/live`; al desconectar en
//!   caliente el almacen de contenido que el perfil exige, `/health/ready`
//!   pasa a `503` **sin** decir donde esta el almacen, `/health/live`
//!   sigue diciendo `200`, y al devolver el almacen la readiness vuelve a
//!   `200`;
//! - contra el **probe**: una dependencia que el perfil no exige no se
//!   comprueba y no puede hacer fallar el readiness; una dependencia que
//!   no contesta produce `timeout` y un `503` sin colgarse; el probe
//!   comprueba cada dependencia una vez y cachea su veredicto un segundo,
//!   de manera que dos sondeos seguidos no gastan el presupuesto dos
//!   veces; y el camino de los datos no pasa por el probe.
//!
//! El despliegue de esta suite corre en el **perfil de grafo**: el catalogo
//! es un namespace de un Dgraph real y el contenido esta en un directorio
//! del despliegue. El perfil exige dos dependencias, `graph` y
//! `content_store`, y la suite desconecta **en caliente** las dos: el
//! directorio de contenido (se mueve) y el alpha (se corta el relay por el
//! que el servicio lo alcanza). En los dos casos la readiness pasa a `503`
//! sin decir donde esta el almacen, la liveness sigue en `200`, y al
//! devolverlo la readiness vuelve.
//!
//! Lo que **no** se demuestra aqui: el perfil con S3 externo; el contenido
//! de este despliegue es el directorio, no un bucket.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;
use std::time::{Duration, Instant};

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

/// El caso: con el almacen exigido ahi, la readiness dice que se puede
/// servir.
const AC1_READY_200: &str = "ac1-the-readiness-is-200-when-the-required-storage-is-there";
/// El caso: al desconectar el almacen exigido, la readiness deja de ser
/// `200`.
const AC1_READY_503: &str = "ac1-the-readiness-is-not-200-when-the-required-storage-is-gone";
/// El caso: el proceso vive aunque el almacen no este.
const AC1_LIVE_200: &str = "ac1-the-liveness-is-200-when-the-required-storage-is-gone";
/// El caso: devolver el almacen devuelve la readiness.
const AC1_READY_BACK: &str = "ac1-the-readiness-comes-back-when-the-required-storage-returns";
/// El caso: al cortar el alpha en caliente, la readiness deja de ser `200`
/// y no dice donde esta el alpha.
const AC1_GRAPH_503: &str = "ac1-the-readiness-is-not-200-when-the-graph-is-cut";
/// El caso: el proceso vive aunque el alpha este cortado.
const AC1_GRAPH_LIVE: &str = "ac1-the-liveness-is-200-when-the-graph-is-cut";
/// El caso: devolver el alpha devuelve la readiness.
const AC1_GRAPH_BACK: &str = "ac1-the-readiness-comes-back-when-the-graph-returns";
/// El caso: el perfil de grafo con su contenido esta listo y exige el
/// alpha y el almacen de contenido.
const AC2_STANDALONE: &str = "ac2-the-graph-profile-is-ready";
/// El caso: el cuerpo de la salud no dice donde esta el despliegue.
const AC2_NO_LEAK: &str = "ac2-the-body-of-the-health-endpoints-does-not-carry-the-deployment";
/// El caso: una dependencia que el perfil no exige no puede fallar el
/// readiness.
const AC2_OPTIONAL: &str =
    "ac2-a-dependency-that-the-profile-does-not-require-cannot-fail-the-readiness";
/// El caso: una dependencia que no contesta produce un `timeout` y un
/// `503`, y no cuelga la peticion.
const AC2_TIMEOUT: &str = "ac2-a-dependency-that-does-not-answer-produces-a-timeout-and-a-503";
/// El caso: el probe comprueba cada dependencia exigida una vez.
const AC3_BOUNDED: &str = "ac3-the-probe-checks-every-required-dependency-once";
/// El caso: dos sondeos seguidos comparten el veredicto.
const AC3_CACHE: &str = "ac3-two-readiness-in-a-row-share-the-verdict";
/// El caso: el camino de los datos no pasa por el probe.
const AC3_DATA_PATH: &str = "ac3-the-data-path-does-not-go-through-the-readiness-probe";

/// Los casos que una ejecucion completa tiene que ejecutar.
///
/// Se declaran **antes** de correr: una ejecucion que solo llego a algunos
/// dice cuales no corrieron, y un informe con un caso que no corrio nunca es
/// verde.
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
    (
        AC1_GRAPH_BACK,
        "devolver el alpha devuelve la readiness",
    ),
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

/// Un probe de mentira: cuenta las comprobaciones y responde lo que se le
/// dice.
///
/// El binario real no admite un probe de mentira --el suyo lo decide el
/// perfil--, asi que el arnes de esta suite lo inyecta en el proceso.
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

    /// Cuantas comprobaciones se han hecho.
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

/// Un probe que no contesta nunca.
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

/// Un relay hacia el alpha que el test puede cortar: cortarlo suelta todas
/// las conexiones abiertas y deja de escuchar, de modo que la siguiente
/// llamada del servicio falla de verdad. Se puede levantar de nuevo en la
/// misma direccion.
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

/// Una credencial de mentira: la cuenta `acme` y la persona `alice`.
///
/// El caso no mide la autenticacion: mide el camino del probe.
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

/// Un autorizador que lo permite todo: el caso no mide la autorizacion.
struct AllowAll;

impl AuthorizerPort for AllowAll {
    fn authorize(&self, _request: &AuthorizationRequest) -> AuthorizationOutcome {
        AuthorizationOutcome::allow()
    }
}

/// La aplicacion en proceso, con el probe que se le dice.
///
/// El catalogo es un namespace del Dgraph y es lo que lee el camino de los
/// datos: sin el, la superficie de contenido no tendria nada que leer.
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

/// Una peticion al router en proceso, con su codigo y su cuerpo.
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

/// Un fichero para el cuerpo de una respuesta.
fn next(name: &str) -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let number = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{number}"))
}

/// Una peticion al servicio de verdad, con su cuerpo y con el comando
/// que la hizo.
///
/// El comando que se escribe en el informe es el que se ejecuto: un
/// informe que dice otra cosa es una opinion.
fn call(port: u16, path: &str) -> (Answer, String) {
    let into = next("answer");
    let curl = Curl::call(port, "GET", path, "", None, None, Some(&into));
    let shown = curl.shown();
    (curl.run(), shown)
}

/// El texto de una respuesta.
fn text_of(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

/// El tiempo que hay que esperar para que el cache corto expire.
///
/// El veredicto se cachea un segundo: comprobarlo antes de que expire
/// mediria el cache, no el despliegue.
fn the_cache_is_gone() -> Duration {
    VERDICT_TTL + Duration::from_millis(500)
}

/// Corre un caso del laboratorio.
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

    // ---- El despliegue de verdad, alcanzando el alpha por un relay.
    let deployment = Deployment::new();
    let tenant = deployment.graph.as_ref().expect("the graph profile");
    let mut relay = runtime.block_on(Relay::to(tenant.endpoint.clone()));
    let service = deployment.start(&[("HODEI_GRAPH__ENDPOINT", relay.address.as_str())]);
    let port = service.port;

    // ---- AC1: con el almacen ahi, se puede servir.
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

    // ---- El almacen exigido se desconecta **en caliente**. No se borra
    // antes de arrancar: un almacen que no esta al arrancar es otro
    // escenario, y este es el que se quiere medir.
    let cas = std::path::PathBuf::from(&deployment.cas);
    let away = deployment.directory.path().join("cas-away");
    std::fs::rename(&cas, &away).expect("the store is moved away");
    std::thread::sleep(the_cache_is_gone());

    // ---- AC1: la readiness deja de ser 200.
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

    // ---- AC1: el proceso vive.
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

    // ---- AC2: el cuerpo no lleva nada del despliegue.
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

    // ---- AC1: el almacen vuelve.
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

    // ---- El alpha se corta **en caliente**: el servicio ya arranco contra
    // el, y de pronto no contesta.
    runtime.block_on(relay.cut());
    std::thread::sleep(the_cache_is_gone());

    a_case(
        AC1_GRAPH_503,
        "al cortar el alpha en caliente, la readiness deja de ser 200 y no dice donde esta",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let title = "al cortar el alpha en caliente, la readiness deja de ser 200 y no dice donde esta";
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
            LabCase::new(AC1_GRAPH_503, title).command(&command).passed()
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
            LabCase::new(AC1_GRAPH_LIVE, title).command(&command).passed()
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
            LabCase::new(AC1_GRAPH_BACK, title).command(&command).passed()
        },
    );

    // ---- AC2: el perfil standalone.
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
            // El perfil de este despliegue exige el alpha y el almacen de
            // contenido. No exige la politica de confianza: se carga en
            // memoria al arrancar y no se puede perder en caliente.
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

    // ---- El resto, en proceso: el binario real decide su probe y no
    // admite otro, asi que el arnes inyecta uno de mentira.
    runtime.block_on(async {
        // ---- AC2: una dependencia que el perfil no exige.
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
                // La dependencia que no se exige no aparece en el veredicto
                // y no se comprueba: no puede hacer fallar el readiness.
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

        // ---- AC2: una dependencia que no contesta.
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
                // El presupuesto es de un segundo por dependencia: la
                // peticion no cuelga, y no espera a que la dependencia
                // conteste.
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

        // ---- AC3: una comprobacion por dependencia.
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

        // ---- AC3: el cache corto.
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

        // ---- AC3: el camino de los datos.
        let probe = Arc::new(Counting::new(
            vec![Dependency::ContentStore],
            Err(DependencyFailure::Unreachable),
        ));
        let (app, _tmp) = the_app(Arc::new(CachedProbe::new(probe.clone()))).await;
        // La readiness corre el probe y falla: el perfil exige una
        // dependencia que no esta.
        let (ready, _) = request(&app, "/health/ready").await;
        let checks_after_the_readiness = probe.checks();
        // Y ahora una peticion del camino de los datos: la superficie de
        // contenido, que es la que lee el catalogo.
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
                // El cuerpo no esta vacio: la respuesta la escribio el
                // handler de la superficie y no el router. Sin esto, un
                // `404` del router seria indistinguible de un `404` del
                // catalogo, y el caso no mediria nada.
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
