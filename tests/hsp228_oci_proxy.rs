//! HSP-228: OCI proxy: pull remoto por digest y etiqueta.
//!
//! Lo que se demuestra aqui, contra el **service de verdad**:
//!
//! - un manifiesto y sus blobs se sacan de un upstream de distribucion a
//!   traves de Hodei, **byte a byte**, y el `Docker-Content-Digest` que
//!   dice la respuesta es el del contenido servido;
//! - un indice (un manifest-list) conserva sus bytes y las referencias
//!   entre objetos que declara;
//! - una etiqueta que se mueve upstream se revalida y se mueve; un digest
//!   **jamás** cambia: lo que no sea el digest pedido no se sirve;
//! - un reto que apunta a un realm que no es el declarado, y una
//!   redireccion a otro origen, **no** llevan la credencial a ningun sitio;
//! - sin red solo se sirve lo integro y con recibo, y las puertas --Cedar y
//!   la confianza-- se ejecutan tambien cuando el contenido sale de la
//!   cache.
//!
//! El upstream es un registro de distribucion del laboratorio: pide un
//! token, lo emite en su realm, y sabe hacer las dos cosas hostiles. Por
//! defecto el camino **no usa la red**: el upstream es local y el contenido
//! es un fixture.

mod support;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderName, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::json;
use sha2::{Digest, Sha256};
use support::lab::{Expectation, LabCase, Scope, describe_registry_lab, run_registry_lab_case};
use support::{Curl, Deployment, FIRST_ACCOUNT};

/// La clave del repositorio OCI proxy del despliegue.
const REPOSITORY: &str = "oci-proxy";

/// El nombre con el que se pide al upstream.
const NAME: &str = "oci-proxy";

/// La etiqueta que se pide.
const TAG: &str = "latest";

/// La cabecera que dice el digest del contenido servido.
const DOCKER_CONTENT_DIGEST: HeaderName = HeaderName::from_static("docker-content-digest");

/// La credencial que el upstream exige en su realm.
const CREDENTIAL: &str = "secreto-del-laboratorio";

/// El token que el upstream emite.
const TOKEN: &str = "token-del-laboratorio";

/// El nombre de la variable de entorno que lleva la credencial.
const CREDENTIAL_VARIABLE: &str = "LAB_OCI_CREDENTIAL";

/// El seeder del catálogo.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

// ---------------------------------------------------------------------------
// El registro de distribucion del laboratorio.
// ---------------------------------------------------------------------------

/// Como se comporta el registro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TheBehaviour {
    /// Pide token, sirve manifiestos y blobs.
    Normal,
    /// Su reto apunta a un realm que **no** es el declarado.
    HostileRealm,
    /// El manifiesto responde con una redireccion a otro origen.
    Redirect,
}

/// Lo que el registro ha recibido.
#[derive(Debug, Clone)]
struct Seen {
    /// El camino pedido.
    path: String,
    /// La cabecera `Authorization`, si la hubo.
    authorization: Option<String>,
}

/// El estado del registro.
struct TheWorld {
    /// Los manifiestos, por referencia (`latest`, `sha256:...`).
    manifests: Mutex<HashMap<String, Vec<u8>>>,
    /// Los blobs, por digest.
    blobs: Mutex<HashMap<String, Vec<u8>>>,
    /// Lo que el registro ha recibido.
    seen: Mutex<Vec<Seen>>,
    /// Como se comporta.
    behaviour: Mutex<TheBehaviour>,
    /// El origen al que apunta el reto hostil, o al que manda la
    /// redireccion.
    elsewhere: Mutex<String>,
    /// El origen de este registro, para el reto.
    origin: Mutex<String>,
}

/// El registro de distribucion del laboratorio.
struct TheRegistry {
    port: u16,
    world: Arc<TheWorld>,
}

impl TheRegistry {
    /// Arranca un registro vacio.
    fn start() -> Self {
        let world = Arc::new(TheWorld {
            manifests: Mutex::new(HashMap::new()),
            blobs: Mutex::new(HashMap::new()),
            seen: Mutex::new(Vec::new()),
            behaviour: Mutex::new(TheBehaviour::Normal),
            elsewhere: Mutex::new(String::new()),
            origin: Mutex::new(String::new()),
        });
        let port = serve(Arc::clone(&world));
        let registry = Self { port, world };
        *registry.world.origin.lock().expect("the origin") = registry.origin();
        registry
    }

    /// El puerto donde escucha.
    fn port(&self) -> u16 {
        self.port
    }

    /// El origen del registro.
    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// La base del registro, con barra final.
    fn base(&self) -> String {
        format!("{}/", self.origin())
    }

    /// Publica un manifiesto, bajo una referencia.
    fn publish(&self, reference: &str, bytes: Vec<u8>) {
        self.world
            .manifests
            .lock()
            .expect("the manifests")
            .insert(reference.to_string(), bytes);
    }

    /// Publica un blob.
    fn publish_blob(&self, digest: &str, bytes: Vec<u8>) {
        self.world
            .blobs
            .lock()
            .expect("the blobs")
            .insert(digest.to_string(), bytes);
    }

    /// Cambia el comportamiento del registro.
    fn behave(&self, behaviour: TheBehaviour) {
        *self.world.behaviour.lock().expect("the behaviour") = behaviour;
    }

    /// A donde apunta el reto hostil, o a donde manda la redireccion.
    fn elsewhere(&self, origin: &str) {
        *self.world.elsewhere.lock().expect("the elsewhere") = origin.to_string();
    }

    /// Lo que el registro ha recibido.
    fn seen(&self) -> Vec<Seen> {
        self.world.seen.lock().expect("the log").clone()
    }

    /// Cuantas veces le han pedido un camino.
    fn hits(&self, path: &str) -> usize {
        self.seen().iter().filter(|one| one.path == path).count()
    }
}

/// Arranca un servidor con el router del registro y devuelve el puerto.
fn serve(world: Arc<TheWorld>) -> u16 {
    let app = Router::new()
        .route("/token", get(the_token_endpoint))
        .route("/v2/{*path}", get(the_distribution))
        .with_state(world);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("the registry binds");
    let port = listener.local_addr().expect("the address").port();
    listener
        .set_nonblocking(true)
        .expect("the registry accepts");
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("the runtime of the registry");
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("the listener");
            let _ = axum::serve(listener, app).await;
        });
    });
    port
}

/// El endpoint de token del registro.
///
/// Solo entrega el token a quien presenta la credencial, y dice quien es:
/// un realm abierto es un realm por el que pasa cualquiera.
async fn the_token_endpoint(State(_world): State<Arc<TheWorld>>, headers: HeaderMap) -> Response {
    let expected = format!("Basic {}", base64_of(&format!("lab:{CREDENTIAL}")));
    match headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        Some(value) if value == expected => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            json!({ "token": TOKEN, "expires_in": 300 }).to_string(),
        )
            .into_response(),
        _ => (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Basic realm=\"registry\"")],
            "{}".to_string(),
        )
            .into_response(),
    }
}

/// La superficie de la distribucion: manifiestos y blobs.
async fn the_distribution(
    State(world): State<Arc<TheWorld>>,
    Path(path): Path<String>,
    headers: HeaderMap,
) -> Response {
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    world.seen.lock().expect("the log").push(Seen {
        path: path.clone(),
        authorization: authorization.clone(),
    });

    let behaviour = *world.behaviour.lock().expect("the behaviour");
    let origin = world.origin.lock().expect("the origin").clone();
    let elsewhere = world.elsewhere.lock().expect("the elsewhere").clone();

    // El reto: el registro pide un token en su realm. El registro hostil
    // lo pide en el realm de **otro**, que es lo que un upstream puede
    // hacer.
    let realm = match behaviour {
        TheBehaviour::HostileRealm => format!("{elsewhere}/token"),
        _ => format!("{origin}/token"),
    };
    let challenge = format!("Bearer realm=\"{realm}\",service=\"lab\"");

    // Sin el token, el registro responde con el reto.
    let authorized = authorization.as_deref() == Some(&format!("Bearer {TOKEN}"));
    if !authorized {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, challenge.as_str())],
            "{}".to_string(),
        )
            .into_response();
    }

    // La redireccion: el manifiesto se ofrece en otro origen.
    if behaviour == TheBehaviour::Redirect {
        return (
            StatusCode::FOUND,
            [(header::LOCATION, format!("{elsewhere}/v2/{path}").as_str())],
            "".to_string(),
        )
            .into_response();
    }

    // `v2/<nombre>/manifests/<referencia>` y `v2/<nombre>/blobs/<digest>`.
    if let Some((_name, reference)) = path.split_once("/manifests/") {
        let manifest = world
            .manifests
            .lock()
            .expect("the manifests")
            .get(reference)
            .cloned();
        let Some(bytes) = manifest else {
            return (StatusCode::NOT_FOUND, "{}".to_string()).into_response();
        };
        let digest = sha256_of(&bytes);
        let media_type = if String::from_utf8_lossy(&bytes).contains("\"manifests\"") {
            "application/vnd.oci.image.index.v1+json"
        } else {
            "application/vnd.oci.image.manifest.v1+json"
        };
        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, media_type.to_string()),
                (header::ETAG, format!("\"{digest}\"")),
                (DOCKER_CONTENT_DIGEST, format!("sha256:{digest}")),
            ],
            bytes,
        )
            .into_response();
    }

    if let Some((_name, reference)) = path.split_once("/blobs/") {
        // El blob se busca por su hexadecimal: la ruta lo nombra con su
        // algoritmo, y el almacen lo guarda sin el.
        let digest = reference.strip_prefix("sha256:").unwrap_or(reference);
        let blob = world.blobs.lock().expect("the blobs").get(digest).cloned();
        let Some(bytes) = blob else {
            return (StatusCode::NOT_FOUND, "{}".to_string()).into_response();
        };
        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/octet-stream".to_string()),
                (
                    DOCKER_CONTENT_DIGEST,
                    format!("sha256:{}", sha256_of(&bytes)),
                ),
            ],
            bytes,
        )
            .into_response();
    }

    (StatusCode::NOT_FOUND, "{}".to_string()).into_response()
}

// ---------------------------------------------------------------------------
// El origen hostil.
// ---------------------------------------------------------------------------

/// El origen hostil: solo registra lo que le llega.
struct TheHostile {
    port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl TheHostile {
    /// Arranca un origen que solo registra lo que le llega.
    fn start() -> Self {
        let seen: Arc<Mutex<Vec<Seen>>> = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route(
                "/{*path}",
                get({
                    let seen = Arc::clone(&seen);
                    async move |Path(path): Path<String>, headers: HeaderMap| {
                        seen.lock().expect("the log").push(Seen {
                            path,
                            authorization: headers
                                .get(header::AUTHORIZATION)
                                .and_then(|value| value.to_str().ok())
                                .map(str::to_string),
                        });
                        (StatusCode::OK, "robado".to_string())
                    }
                }),
            )
            .with_state(());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("the hostile binds");
        let port = listener.local_addr().expect("the address").port();
        listener.set_nonblocking(true).expect("the hostile accepts");
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("the runtime of the hostile");
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener).expect("the listener");
                let _ = axum::serve(listener, app).await;
            });
        });
        Self { port, seen }
    }

    /// El origen del hostil.
    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Lo que el hostil ha recibido.
    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().expect("the log").clone()
    }
}

// ---------------------------------------------------------------------------
// El despliegue.
// ---------------------------------------------------------------------------

/// El despliegue con el registro de distribucion delante.
struct TheLab {
    deployment: Deployment,
    registry: TheRegistry,
    hostile: TheHostile,
}

impl TheLab {
    /// Un despliegue con el registro sembrado y el servicio parado.
    fn new() -> Self {
        let deployment = Deployment::new();
        let registry = TheRegistry::start();
        let hostile = TheHostile::start();
        let lab = Self {
            deployment,
            registry,
            hostile,
        };
        lab.seed();
        lab
    }

    /// Siembra el repositorio OCI proxy.
    ///
    /// El cache no tiene TTL: todo se revalida, que es lo que hace falta
    /// para ver la revalidacion por `ETag` y el movimiento de una etiqueta.
    fn seed(&self) {
        self.deployment.seed_repositories(&[json!({
            "key": REPOSITORY,
            "format": "oci",
            "proxy": {
                "upstream": {
                    "id": "lab",
                    "url": self.registry.base(),
                    "scopes": ["v2/"],
                    "credential_ref": format!("env:{CREDENTIAL_VARIABLE}"),
                    "auth": {
                        "realm": format!("{}token", self.registry.base()),
                        "service": "lab",
                        "username": "lab"
                    },
                    "redirects": { "max": 3, "cross_origin": true }
                },
                "cache": {
                    "ttl": { "content_secs": 0, "metadata_secs": 0 },
                    "negative_secs": 0
                }
            }
        })]);
    }

    /// El entorno del servicio: la salida permitida y la credencial.
    fn the_environment(&self) -> Vec<(String, String)> {
        let allowed = format!(
            "127.0.0.1:{};private;http,127.0.0.1:{};private;http",
            self.registry.port(),
            self.hostile.port
        );
        vec![
            ("HODEI_PROXY__ALLOWED_HOSTS".to_string(), allowed),
            (
                "HODEI_PROXY__ALLOWED_CREDENTIALS".to_string(),
                CREDENTIAL_VARIABLE.to_string(),
            ),
            (CREDENTIAL_VARIABLE.to_string(), CREDENTIAL.to_string()),
        ]
    }

    /// Arranca el servicio con la salida permitida.
    fn start(&self) -> support::Service {
        let environment = self.the_environment();
        let borrowed: Vec<(&str, &str)> = environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        self.deployment.start(&borrowed)
    }

    /// Arranca el servicio en el puerto dado, con lo que se le anade.
    fn start_on(&self, port: u16, extra: &[(&str, &str)]) -> support::Service {
        let mut environment = self.the_environment();
        environment.extend(
            extra
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string())),
        );
        let borrowed: Vec<(&str, &str)> = environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        self.deployment.start_on(port, &borrowed)
    }

    /// Un laboratorio **sin** el repositorio proxy sembrado: el catalogo
    /// solo tiene las cuentas, y la politica deja a la primera persona
    /// crear repositorios. Lo que haya de existir lo crea la API publica.
    fn unseeded() -> Self {
        let deployment = Deployment::new();
        let registry = TheRegistry::start();
        let hostile = TheHostile::start();
        deployment.seed();
        deployment.write_policy(&format!(
            "{}permit(principal == Iam::User::\"hrn:hodei:iam::{FIRST_ACCOUNT}:User/alice\", action == Artifacts::Action::\"CreateRepository\", resource);\n",
            support::the_policy()
        ));
        Self {
            deployment,
            registry,
            hostile,
        }
    }

    /// El cuerpo publico que crea el repositorio OCI proxy del laboratorio.
    fn the_creation(&self, upstream_url: &str) -> serde_json::Value {
        json!({
            "name": REPOSITORY,
            "format": "oci",
            "mode": "proxy",
            "settings": {
                "proxy": {
                    "upstream": {
                        "id": "lab",
                        "url": upstream_url,
                        "scopes": ["v2/"],
                        "credential_ref": format!("env:{CREDENTIAL_VARIABLE}"),
                        "auth": {
                            "realm": format!("{}token", self.registry.base()),
                            "service": "lab",
                            "username": "lab"
                        },
                        "redirects": { "max": 3, "cross_origin": true }
                    },
                    "cache": {
                        "ttl": { "content_secs": 0, "metadata_secs": 0 },
                        "negative_secs": 0
                    }
                }
            }
        })
    }

    /// Crea un repositorio por `POST /api/v1/catalog/repositories`.
    fn create(&self, port: u16, body: &serde_json::Value) -> support::Answer {
        let file = self.a_file("creation.json");
        std::fs::write(&file, body.to_string()).expect("the body is written");
        let into = self.a_file("creation-answer");
        Curl::call(
            port,
            "POST",
            "/api/v1/catalog/repositories",
            &the_client_token(),
            Some(&file),
            Some("application/json"),
            Some(&into),
        )
        .run()
    }

    /// Un fichero del despliegue donde dejar el cuerpo de una respuesta.
    fn a_file(&self, name: &str) -> PathBuf {
        self.deployment.directory.path().join(name)
    }
}

// ---------------------------------------------------------------------------
// Las utilidades.
// ---------------------------------------------------------------------------

/// El digest de unos bytes.
fn sha256_of(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Un texto en base64.
fn base64_of(text: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(text)
}

/// El token de la persona que descarga.
fn the_client_token() -> String {
    support::token(FIRST_ACCOUNT, "alice")
}

/// Un manifiesto de imagen con sus blobs.
fn the_manifest() -> (Vec<u8>, Vec<(String, Vec<u8>)>) {
    let config = b"{\"architecture\":\"amd64\"}".to_vec();
    let layer = b"contenido-de-la-capa".to_vec();
    let config_digest = sha256_of(&config);
    let layer_digest = sha256_of(&layer);
    let manifest = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": format!("sha256:{config_digest}"),
            "size": config.len()
        },
        "layers": [{
            "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
            "digest": format!("sha256:{layer_digest}"),
            "size": layer.len()
        }]
    })
    .to_string()
    .into_bytes();
    (
        manifest,
        vec![(config_digest, config), (layer_digest, layer)],
    )
}

/// Un indice de imagenes: un manifiesto que apunta a otros.
fn the_index(first: &str, second: &str) -> Vec<u8> {
    json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "manifests": [
            {
                "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "digest": format!("sha256:{first}"),
                "size": 100,
                "platform": { "os": "linux", "architecture": "amd64" }
            },
            {
                "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "digest": format!("sha256:{second}"),
                "size": 200,
                "platform": { "os": "linux", "architecture": "arm64" }
            }
        ]
    })
    .to_string()
    .into_bytes()
}

/// Siembra el contenido del registro.
///
/// Devuelve el manifiesto y el indice, para poder compararlos con lo que
/// sirve el registro.
fn the_content(registry: &TheRegistry) -> (Vec<u8>, Vec<u8>) {
    let (manifest, blobs) = the_manifest();
    for (digest, bytes) in &blobs {
        registry.publish_blob(digest, bytes.clone());
    }
    let digest = sha256_of(&manifest);
    registry.publish(TAG, manifest.clone());
    registry.publish(&format!("sha256:{digest}"), manifest.clone());

    let index = the_index(&digest, &"0".repeat(64));
    let index_digest = sha256_of(&index);
    registry.publish("indice", index.clone());
    registry.publish(&format!("sha256:{index_digest}"), index.clone());

    (manifest, index)
}

// ---------------------------------------------------------------------------
// Los casos.
// ---------------------------------------------------------------------------

/// Los casos que una ejecucion completa tiene que hacer.
const THE_CASES: &[(&str, &str)] = &[
    (
        "ac1_manifest",
        "El manifiesto del upstream llega byte a byte",
    ),
    (
        "ac1_index",
        "El indice conserva sus bytes y sus referencias",
    ),
    ("ac1_blob", "El blob del upstream llega byte a byte"),
    ("ac1_digest", "El manifiesto se pide por digest"),
    (
        "ac2_tag_moves",
        "Una etiqueta que se mueve upstream se revalida y se mueve",
    ),
    ("ac2_digest_never_changes", "Un digest jamas cambia"),
    (
        "ac2_hostile_realm",
        "Un realm que no es el declarado no recibe la credencial",
    ),
    (
        "ac2_redirect",
        "Una redireccion a otro origen no lleva la credencial",
    ),
    ("ac3_head", "El HEAD dice lo mismo que el GET"),
    (
        "ac3_not_modified",
        "El 304 se responde al cliente que ya lo tiene",
    ),
    (
        "ac3_offline",
        "Sin red solo se sirve lo integro y con recibo",
    ),
    (
        "ac3_gate_on_hit",
        "Las puertas se ejecutan tambien en un acierto de la cache",
    ),
    (
        "ac4_public_creation",
        "El proxy OCI se crea por la API publica y se descarga",
    ),
    (
        "ac4_public_creation_refused",
        "Un proxy OCI con upstream invalido se rechaza con diagnostico",
    ),
];

/// El titulo y el descargo del informe.
fn the_report() {
    describe_registry_lab(
        "Laboratorio de OCI proxy — informe",
        &[
            "- **Ni un cliente real.** Lo que se ha ejecutado es el camino de la distribución contra un upstream local: no se ha hablado con `podman`, `skopeo` ni `oras`, y este informe no dice nada de ellos.",
            "- **Ni la red real.** Por defecto el laboratorio no sale a la red: el upstream es un registro local. El contenido es un fixture, y lo que se demuestra es el camino, no la integridad de ese contenido.",
            "- **Ni la superficie de push.** Solo se ha medido el pull: manifiesto y blob, por etiqueta y por digest.",
            "- **Ni el laboratorio.** Los casos de `scope: harness` miden el arnés —el recuento— y no el producto. Los dos recuentos van aparte.",
        ],
    );
}

/// El camino del manifiesto.
fn the_manifest_path(reference: &str) -> String {
    format!("/v2/{NAME}/manifests/{reference}")
}

/// El camino del blob.
fn the_blob_path(digest: &str) -> String {
    format!("/v2/{NAME}/blobs/{digest}")
}

#[test]
fn ac1_manifest() {
    run_registry_lab_case(
        THE_CASES,
        "ac1_manifest",
        "El manifiesto del upstream llega byte a byte",
        Scope::Product,
        Expectation::Succeeds,
        || {
            the_report();
            let lab = TheLab::new();
            let (manifest, _) = the_content(&lab.registry);
            let into = lab.a_file("manifiesto");
            let mut service = lab.start();
            let answer = Curl::call(
                service.port,
                "GET",
                &the_manifest_path(TAG),
                &the_client_token(),
                None,
                None,
                Some(&into),
            )
            .run();
            let digest = answer.header("Docker-Content-Digest");
            let etag = answer.header("ETag");
            let content_type = answer.header("Content-Type");
            support::stop(&mut service);

            let case = || {
                LabCase::new(
                    "ac1_manifest",
                    "El manifiesto del upstream llega byte a byte",
                )
            };
            if answer.status != 200 {
                let log = lab.deployment.the_log();
                let tail: Vec<&str> = log
                    .lines()
                    .filter(|line| {
                        line.contains("WARN") || line.contains("ERROR") || line.contains("publish")
                    })
                    .take(6)
                    .collect();
                return case().failed(format!(
                    "el estado fue {} y el cuerpo fue {}; el log dice: {}",
                    answer.status,
                    String::from_utf8_lossy(&answer.body),
                    tail.join(" | ")
                ));
            }
            // Los bytes son los del upstream, no una reescritura.
            if answer.body != manifest {
                return case().failed("los bytes servidos no son los del upstream");
            }
            let expected = format!("sha256:{}", sha256_of(&manifest));
            if digest.as_deref() != Some(expected.as_str()) {
                return case().failed(format!(
                    "el digest servido fue {digest:?} y deberia ser {expected}"
                ));
            }
            if etag.is_none() {
                return case().failed("la respuesta no lleva ETag");
            }
            if content_type.as_deref() != Some("application/vnd.oci.image.manifest.v1+json") {
                return case().failed(format!("el tipo de medio fue {content_type:?}"));
            }
            // El upstream recibio la peticion.
            // El registro ve el camino sin el prefijo de la superficie.
            if lab.registry.hits(&format!("{NAME}/manifests/{TAG}")) == 0 {
                return case().failed("el upstream no recibio la peticion del manifiesto");
            }
            case()
                .command("curl /v2/oci-proxy/manifests/latest")
                .origin(&lab.registry.origin())
                .sha256(&sha256_of(&manifest))
                .passed()
        },
    );
}

#[test]
fn ac1_index() {
    run_registry_lab_case(
        THE_CASES,
        "ac1_index",
        "El indice conserva sus bytes y sus referencias",
        Scope::Product,
        Expectation::Succeeds,
        || {
            the_report();
            let lab = TheLab::new();
            let (_, index) = the_content(&lab.registry);
            let into = lab.a_file("indice");
            let mut service = lab.start();
            let answer = Curl::call(
                service.port,
                "GET",
                &the_manifest_path("indice"),
                &the_client_token(),
                None,
                None,
                Some(&into),
            )
            .run();
            let content_type = answer.header("Content-Type");
            support::stop(&mut service);

            let case = || {
                LabCase::new(
                    "ac1_index",
                    "El indice conserva sus bytes y sus referencias",
                )
            };
            if answer.status != 200 {
                return case().failed(format!("el estado fue {}", answer.status));
            }
            if content_type.as_deref() != Some("application/vnd.oci.image.index.v1+json") {
                return case().failed(format!("el tipo de medio fue {content_type:?}"));
            }
            // Las referencias entre objetos siguen ahi: un indice que
            // pierde sus digest no es el mismo indice.
            let served: serde_json::Value =
                serde_json::from_slice(&answer.body).unwrap_or(serde_json::Value::Null);
            if served["manifests"].as_array().map(|list| list.len()) != Some(2) {
                return case().failed("el indice servido no tiene las referencias que declara");
            }
            let expected: serde_json::Value =
                serde_json::from_slice(&index).unwrap_or(serde_json::Value::Null);
            if served != expected {
                return case().failed("el indice servido no es el del upstream");
            }
            case()
                .command("curl /v2/oci-proxy/manifests/indice")
                .origin(&lab.registry.origin())
                .sha256(&sha256_of(&index))
                .passed()
        },
    );
}

#[test]
fn ac1_blob() {
    run_registry_lab_case(
        THE_CASES,
        "ac1_blob",
        "El blob del upstream llega byte a byte",
        Scope::Product,
        Expectation::Succeeds,
        || {
            the_report();
            let lab = TheLab::new();
            let (_, blobs) = the_manifest();
            the_content(&lab.registry);
            let into = lab.a_file("blob");
            let mut service = lab.start();
            let answer = Curl::call(
                service.port,
                "GET",
                &the_blob_path(&format!("sha256:{}", blobs[0].0)),
                &the_client_token(),
                None,
                None,
                Some(&into),
            )
            .run();
            support::stop(&mut service);

            let case = || LabCase::new("ac1_blob", "El blob del upstream llega byte a byte");
            if answer.status != 200 {
                return case().failed(format!("el estado fue {}", answer.status));
            }
            if answer.body != blobs[0].1 {
                return case().failed("los bytes del blob no son los del upstream");
            }
            let expected = format!("sha256:{}", blobs[0].0);
            if answer.header("Docker-Content-Digest").as_deref() != Some(expected.as_str()) {
                return case().failed("el digest del blob no es el del contenido");
            }
            case()
                .command("curl /v2/oci-proxy/blobs/sha256:...")
                .origin(&lab.registry.origin())
                .sha256(&blobs[0].0)
                .passed()
        },
    );
}

#[test]
fn ac1_digest() {
    run_registry_lab_case(
        THE_CASES,
        "ac1_digest",
        "El manifiesto se pide por digest",
        Scope::Product,
        Expectation::Succeeds,
        || {
            the_report();
            let lab = TheLab::new();
            let (manifest, _) = the_content(&lab.registry);
            let digest = sha256_of(&manifest);
            let into = lab.a_file("por-digest");
            let mut service = lab.start();
            let answer = Curl::call(
                service.port,
                "GET",
                &the_manifest_path(&format!("sha256:{digest}")),
                &the_client_token(),
                None,
                None,
                Some(&into),
            )
            .run();
            support::stop(&mut service);

            let case = || LabCase::new("ac1_digest", "El manifiesto se pide por digest");
            if answer.status != 200 {
                return case().failed(format!("el estado fue {}", answer.status));
            }
            if answer.header("Docker-Content-Digest").as_deref()
                != Some(format!("sha256:{digest}").as_str())
            {
                return case().failed("el digest servido no es el pedido");
            }
            if answer.body != manifest {
                return case().failed("los bytes servidos no son los del upstream");
            }
            case()
                .command("curl /v2/oci-proxy/manifests/sha256:...")
                .origin(&lab.registry.origin())
                .sha256(&digest)
                .passed()
        },
    );
}

#[test]
fn ac2_tag_moves() {
    run_registry_lab_case(
        THE_CASES,
        "ac2_tag_moves",
        "Una etiqueta que se mueve upstream se revalida y se mueve",
        Scope::Product,
        Expectation::Succeeds,
        || {
            the_report();
            let lab = TheLab::new();
            let (manifest, _) = the_content(&lab.registry);
            let into = lab.a_file("la-etiqueta");
            let mut service = lab.start();
            let first = Curl::call(
                service.port,
                "GET",
                &the_manifest_path(TAG),
                &the_client_token(),
                None,
                None,
                Some(&into),
            )
            .run();
            // El upstream mueve la etiqueta: el mismo nombre, otro
            // contenido.
            let moved =
                b"{\"schemaVersion\":2,\"mediaType\":\"application/vnd.oci.image.manifest.v1+json\",\"layers\":[]}"
                    .to_vec();
            lab.registry.publish(TAG, moved.clone());
            let second = Curl::call(
                service.port,
                "GET",
                &the_manifest_path(TAG),
                &the_client_token(),
                None,
                None,
                Some(&into),
            )
            .run();
            support::stop(&mut service);

            let case = || {
                LabCase::new(
                    "ac2_tag_moves",
                    "Una etiqueta que se mueve upstream se revalida y se mueve",
                )
            };
            if first.status != 200 || second.status != 200 {
                return case().failed(format!(
                    "los estados fueron {} y {}",
                    first.status, second.status
                ));
            }
            let moved_digest = format!("sha256:{}", sha256_of(&moved));
            if second.header("Docker-Content-Digest").as_deref() != Some(moved_digest.as_str()) {
                return case().failed("la etiqueta no se movio: el digest servido es el viejo");
            }
            if sha256_of(&manifest) == sha256_of(&moved) {
                return case().failed("el contenido movido es el mismo que el anterior");
            }
            if second.body != moved {
                return case().failed("los bytes movidos no llegaron al cliente");
            }
            case()
                .command("curl /v2/oci-proxy/manifests/latest (dos veces)")
                .origin(&lab.registry.origin())
                .sha256(&sha256_of(&moved))
                .passed()
        },
    );
}

#[test]
fn ac2_digest_never_changes() {
    run_registry_lab_case(
        THE_CASES,
        "ac2_digest_never_changes",
        "Un digest jamas cambia",
        Scope::Product,
        Expectation::Refused,
        || {
            the_report();
            let lab = TheLab::new();
            let (manifest, _) = the_content(&lab.registry);
            let digest = sha256_of(&manifest);
            let into = lab.a_file("la-mentira");
            let mut service = lab.start();
            // El upstream miente: ofrece otros bytes para ese digest.
            let lie = b"contenido-que-no-es-el-que-se-pidio".to_vec();
            lab.registry
                .publish(&format!("sha256:{digest}"), lie.clone());
            let answer = Curl::call(
                service.port,
                "GET",
                &the_manifest_path(&format!("sha256:{digest}")),
                &the_client_token(),
                None,
                None,
                Some(&into),
            )
            .run();
            support::stop(&mut service);

            let case = || LabCase::new("ac2_digest_never_changes", "Un digest jamas cambia");
            if answer.status == 200 {
                return case()
                    .failed("el upstream ofrecio otros bytes para un digest y se sirvieron");
            }
            if answer.body == lie {
                return case().failed("los bytes que no son los del digest llegaron al cliente");
            }
            case()
                .command("curl /v2/oci-proxy/manifests/sha256:...")
                .origin(&lab.registry.origin())
                .passed()
        },
    );
}

#[test]
fn ac2_hostile_realm() {
    run_registry_lab_case(
        THE_CASES,
        "ac2_hostile_realm",
        "Un realm que no es el declarado no recibe la credencial",
        Scope::Product,
        Expectation::Refused,
        || {
            the_report();
            let lab = TheLab::new();
            the_content(&lab.registry);
            lab.registry.elsewhere(&lab.hostile.origin());
            lab.registry.behave(TheBehaviour::HostileRealm);
            let mut service = lab.start();
            let answer = Curl::call(
                service.port,
                "GET",
                &the_manifest_path(TAG),
                &the_client_token(),
                None,
                None,
                None,
            )
            .run();
            support::stop(&mut service);

            let case = || {
                LabCase::new(
                    "ac2_hostile_realm",
                    "Un realm que no es el declarado no recibe la credencial",
                )
            };
            if answer.status == 200 {
                return case().failed("el pull funciono con un realm que no es el declarado");
            }
            // El origen hostil no recibio la credencial.
            for seen in lab.hostile.seen() {
                if seen.authorization.is_some() {
                    return case().failed("la credencial llego al realm que no es el declarado");
                }
            }
            case()
                .command("curl /v2/oci-proxy/manifests/latest")
                .origin(&lab.registry.origin())
                .passed()
        },
    );
}

#[test]
fn ac2_redirect() {
    run_registry_lab_case(
        THE_CASES,
        "ac2_redirect",
        "Una redireccion a otro origen no lleva la credencial",
        Scope::Product,
        Expectation::Refused,
        || {
            the_report();
            let lab = TheLab::new();
            let (manifest, _) = the_content(&lab.registry);
            let digest = sha256_of(&manifest);
            lab.registry.elsewhere(&lab.hostile.origin());
            lab.registry.behave(TheBehaviour::Redirect);
            let mut service = lab.start();
            let answer = Curl::call(
                service.port,
                "GET",
                &the_manifest_path(&format!("sha256:{digest}")),
                &the_client_token(),
                None,
                None,
                None,
            )
            .run();
            support::stop(&mut service);

            let case = || {
                LabCase::new(
                    "ac2_redirect",
                    "Una redireccion a otro origen no lleva la credencial",
                )
            };
            // El contenido de un digest **fijado** no se sustituye: lo que
            // llega de otro origen no es lo que se pidio, y no se sirve.
            if answer.status == 200 {
                return case().failed("el pull funciono con una redireccion a otro origen");
            }
            if answer.body == b"robado".to_vec() {
                return case().failed("el contenido del otro origen llego al cliente");
            }
            // El origen hostil recibio la peticion, pero **sin** la
            // credencial: la credencial solo viaja a su origen.
            for seen in lab.hostile.seen() {
                if seen.authorization.is_some() {
                    return case().failed("la credencial llego al origen de la redireccion");
                }
            }
            case()
                .command("curl /v2/oci-proxy/manifests/sha256:...")
                .origin(&lab.registry.origin())
                .passed()
        },
    );
}

#[test]
fn ac3_head() {
    run_registry_lab_case(
        THE_CASES,
        "ac3_head",
        "El HEAD dice lo mismo que el GET",
        Scope::Product,
        Expectation::Succeeds,
        || {
            the_report();
            let lab = TheLab::new();
            let (manifest, _) = the_content(&lab.registry);
            let into = lab.a_file("el-get");
            let mut service = lab.start();
            let get = Curl::call(
                service.port,
                "GET",
                &the_manifest_path(TAG),
                &the_client_token(),
                None,
                None,
                Some(&into),
            )
            .run();
            let head = Curl::call(
                service.port,
                "HEAD",
                &the_manifest_path(TAG),
                &the_client_token(),
                None,
                None,
                None,
            )
            .run();
            support::stop(&mut service);

            let case = || LabCase::new("ac3_head", "El HEAD dice lo mismo que el GET");
            if get.status != 200 || head.status != 200 {
                return case().failed(format!(
                    "los estados fueron {} y {}",
                    get.status, head.status
                ));
            }
            if head.size != 0 {
                return case().failed("el HEAD trajo cuerpo");
            }
            for name in ["Content-Length", "Docker-Content-Digest", "Content-Type"] {
                if get.header(name) != head.header(name) {
                    return case().failed(format!(
                        "la cabecera {name} no es la misma en el HEAD que en el GET"
                    ));
                }
            }
            case()
                .command("curl --head /v2/oci-proxy/manifests/latest")
                .origin(&lab.registry.origin())
                .sha256(&sha256_of(&manifest))
                .passed()
        },
    );
}

#[test]
fn ac3_not_modified() {
    run_registry_lab_case(
        THE_CASES,
        "ac3_not_modified",
        "El 304 se responde al cliente que ya lo tiene",
        Scope::Product,
        Expectation::Succeeds,
        || {
            the_report();
            let lab = TheLab::new();
            let (manifest, _) = the_content(&lab.registry);
            let into = lab.a_file("el-304");
            let mut service = lab.start();
            let first = Curl::call(
                service.port,
                "GET",
                &the_manifest_path(TAG),
                &the_client_token(),
                None,
                None,
                Some(&into),
            )
            .run();
            let etag = first.header("ETag").unwrap_or_default();
            let answer = Curl::call_with_headers(
                service.port,
                "GET",
                &the_manifest_path(TAG),
                &the_client_token(),
                None,
                &[("If-None-Match", etag.as_str())],
                Some(&into),
            )
            .run();
            support::stop(&mut service);

            let case = || {
                LabCase::new(
                    "ac3_not_modified",
                    "El 304 se responde al cliente que ya lo tiene",
                )
            };
            if answer.status != 304 {
                return case().failed(format!("el estado fue {}", answer.status));
            }
            if answer.size != 0 {
                return case().failed("el 304 trajo cuerpo");
            }
            case()
                .command("curl -H 'If-None-Match: ...' /v2/oci-proxy/manifests/latest")
                .origin(&lab.registry.origin())
                .sha256(&sha256_of(&manifest))
                .passed()
        },
    );
}

#[test]
fn ac3_offline() {
    run_registry_lab_case(
        THE_CASES,
        "ac3_offline",
        "Sin red solo se sirve lo integro y con recibo",
        Scope::Product,
        Expectation::Succeeds,
        || {
            the_report();
            let lab = TheLab::new();
            let (manifest, _) = the_content(&lab.registry);
            let into = lab.a_file("sin-red");
            let mut service = lab.start();
            let first = Curl::call(
                service.port,
                "GET",
                &the_manifest_path(TAG),
                &the_client_token(),
                None,
                None,
                Some(&into),
            )
            .run();
            let port = service.port;
            support::stop(&mut service);
            if first.status != 200 {
                return LabCase::new(
                    "ac3_offline",
                    "Sin red solo se sirve lo integro y con recibo",
                )
                .failed("el primer pull no funciono");
            }

            // El servicio vuelve **sin red**, en el mismo puerto.
            let mut offline = lab.start_on(port, &[("HODEI_PROXY__OFFLINE", "true")]);
            let answer = Curl::call(
                offline.port,
                "GET",
                &the_manifest_path(TAG),
                &the_client_token(),
                None,
                None,
                Some(&into),
            )
            .run();
            support::stop(&mut offline);

            let case = || {
                LabCase::new(
                    "ac3_offline",
                    "Sin red solo se sirve lo integro y con recibo",
                )
            };
            if answer.status != 200 {
                let log = lab.deployment.the_log();
                let tail: Vec<&str> = log
                    .lines()
                    .filter(|line| {
                        line.contains("WARN") || line.contains("ERROR") || line.contains("publish")
                    })
                    .take(4)
                    .collect();
                return case().failed(format!(
                    "el estado sin red fue {} y el cuerpo fue {}; el log dice: {}",
                    answer.status,
                    String::from_utf8_lossy(&answer.body),
                    tail.join(" | ")
                ));
            }
            let served: serde_json::Value =
                serde_json::from_slice(&answer.body).unwrap_or(serde_json::Value::Null);
            let expected: serde_json::Value =
                serde_json::from_slice(&manifest).unwrap_or(serde_json::Value::Null);
            if served != expected {
                return case().failed("el contenido sin red no es el que se trajo");
            }
            case()
                .command("curl /v2/oci-proxy/manifests/latest (sin red)")
                .origin(&lab.registry.origin())
                .sha256(&sha256_of(&manifest))
                .passed()
        },
    );
}

#[test]
fn ac3_gate_on_hit() {
    run_registry_lab_case(
        THE_CASES,
        "ac3_gate_on_hit",
        "Las puertas se ejecutan tambien en un acierto de la cache",
        Scope::Product,
        Expectation::Refused,
        || {
            the_report();
            let lab = TheLab::new();
            the_content(&lab.registry);
            let mut service = lab.start();
            let first = Curl::call(
                service.port,
                "GET",
                &the_manifest_path(TAG),
                &the_client_token(),
                None,
                None,
                None,
            )
            .run();
            let port = service.port;
            support::stop(&mut service);
            if first.status != 200 {
                return LabCase::new(
                    "ac3_gate_on_hit",
                    "Las puertas se ejecutan tambien en un acierto de la cache",
                )
                .failed("el primer pull no funciono");
            }

            // La politica cambia: la persona ya no puede descargar. El
            // contenido esta en la cache, y aun asi no se sirve.
            lab.deployment.write_policy("");
            let mut service = lab.start_on(port, &[]);
            let answer = Curl::call(
                service.port,
                "GET",
                &the_manifest_path(TAG),
                &the_client_token(),
                None,
                None,
                None,
            )
            .run();
            support::stop(&mut service);

            let case = || {
                LabCase::new(
                    "ac3_gate_on_hit",
                    "Las puertas se ejecutan tambien en un acierto de la cache",
                )
            };
            if answer.status != 403 {
                return case().failed(format!(
                    "el estado con la politica cerrada fue {} y deberia ser 403",
                    answer.status
                ));
            }
            case()
                .command("curl /v2/oci-proxy/manifests/latest (politica cerrada)")
                .origin(&lab.registry.origin())
                .passed()
        },
    );
}

#[test]
fn ac4_public_creation() {
    run_registry_lab_case(
        THE_CASES,
        "ac4_public_creation",
        "El proxy OCI se crea por la API publica y se descarga",
        Scope::Product,
        Expectation::Succeeds,
        || {
            the_report();
            let lab = TheLab::unseeded();
            let (manifest, _) = the_content(&lab.registry);
            let (_, blobs) = the_manifest();
            let case = || {
                LabCase::new(
                    "ac4_public_creation",
                    "El proxy OCI se crea por la API publica y se descarga",
                )
            };
            let mut service = lab.start();

            // The capability is announced: OCI proxy is creatable.
            let capabilities = lab.a_file("capabilities");
            let announced = Curl::call(
                service.port,
                "GET",
                "/api/v1/capabilities",
                &the_client_token(),
                None,
                None,
                Some(&capabilities),
            )
            .run();
            let text = String::from_utf8_lossy(&announced.body).into_owned();
            let document: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            let oci_proxy = document.to_string().contains("\"oci\"")
                && document
                    .get("formats")
                    .and_then(|formats| formats.as_array())
                    .map(|formats| {
                        formats.iter().any(|format| {
                            format.to_string().contains("\"oci\"")
                                && format.to_string().contains("\"proxy\"")
                        })
                    })
                    .unwrap_or(false);
            if !oci_proxy {
                support::stop(&mut service);
                return case().failed(format!("capabilities no anuncia oci proxy: {text}"));
            }

            let created = lab.create(service.port, &lab.the_creation(&lab.registry.base()));
            if created.status != 201 {
                support::stop(&mut service);
                return case().failed(format!(
                    "la creacion por la API publica fallo: {} {}",
                    created.status,
                    String::from_utf8_lossy(&created.body)
                ));
            }

            let into = lab.a_file("manifiesto");
            let answer = Curl::call(
                service.port,
                "GET",
                &the_manifest_path(TAG),
                &the_client_token(),
                None,
                None,
                Some(&into),
            )
            .run();
            let first_blob = blobs[0].1.clone();
            let blob_digest = format!("sha256:{}", blobs[0].0);
            let into_blob = lab.a_file("blob");
            let blob = Curl::call(
                service.port,
                "GET",
                &the_blob_path(&blob_digest),
                &the_client_token(),
                None,
                None,
                Some(&into_blob),
            )
            .run();
            support::stop(&mut service);

            if answer.status != 200 || answer.body != manifest {
                return case().failed(format!(
                    "el manifiesto no llego integro: {} ({} bytes)",
                    answer.status,
                    answer.body.len()
                ));
            }
            if blob.status != 200 || blob.body != first_blob {
                return case().failed(format!("el blob no llego integro: {}", blob.status));
            }
            case()
                .command("curl POST /api/v1/catalog/repositories; GET manifest + blob")
                .origin(&lab.registry.origin())
                .sha256(&sha256_of(&manifest))
                .passed()
        },
    );
}

#[test]
fn ac4_public_creation_refused() {
    run_registry_lab_case(
        THE_CASES,
        "ac4_public_creation_refused",
        "Un proxy OCI con upstream invalido se rechaza con diagnostico",
        Scope::Product,
        Expectation::Refused,
        || {
            the_report();
            let lab = TheLab::unseeded();
            let case = || {
                LabCase::new(
                    "ac4_public_creation_refused",
                    "Un proxy OCI con upstream invalido se rechaza con diagnostico",
                )
            };
            let mut service = lab.start();

            // Without upstream: refused with a diagnosis.
            let missing = lab.create(
                service.port,
                &json!({
                    "name": REPOSITORY,
                    "format": "oci",
                    "mode": "proxy",
                    "settings": { "proxy": {} }
                }),
            );
            // An upstream whose URL is not a URL: refused with a diagnosis.
            let invalid = lab.create(service.port, &lab.the_creation("no es una url"));
            // An upstream outside the egress allowlist: the pull is refused.
            let outside = lab.create(service.port, &lab.the_creation("http://203.0.113.9:5000/"));
            let pulled = if outside.status == 201 {
                Some(
                    Curl::call(
                        service.port,
                        "GET",
                        &the_manifest_path(TAG),
                        &the_client_token(),
                        None,
                        None,
                        None,
                    )
                    .run(),
                )
            } else {
                None
            };
            support::stop(&mut service);

            for (name, answer) in [("sin upstream", &missing), ("url invalida", &invalid)] {
                let text = String::from_utf8_lossy(&answer.body).into_owned();
                if answer.status != 400 || text.trim().is_empty() {
                    return case().failed(format!(
                        "{name}: se esperaba 400 con diagnostico y fue {} {text}",
                        answer.status
                    ));
                }
            }
            let rejected_at_creation = outside.status == 400;
            let refused_at_pull = pulled
                .as_ref()
                .map(|answer| answer.status >= 400 && lab.registry.seen().is_empty())
                .unwrap_or(false);
            if !rejected_at_creation && !refused_at_pull {
                return case().failed(format!(
                    "un upstream fuera de la allowlist no se rechazo: creacion {} y descarga {:?}",
                    outside.status,
                    pulled.map(|answer| answer.status)
                ));
            }
            case()
                .command("curl POST /api/v1/catalog/repositories (config invalida)")
                .origin(&lab.registry.origin())
                .passed()
        },
    );
}
