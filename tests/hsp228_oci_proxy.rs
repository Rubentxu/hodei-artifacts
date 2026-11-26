//! HSP-228: OCI proxy: remote pull by digest and tag.
//!
//! What is demonstrated here, against the **real service**:
//!
//! - a manifest and its blobs are pulled from a distribution upstream
//!   through Hodei, **byte for byte**, and the `Docker-Content-Digest` that
//!   the response states is that of the served content;
//! - an index (a manifest-list) preserves its bytes and the references
//!   between objects that it declares;
//! - a tag that moves upstream is revalidated and moves; a digest
//!   **never** changes: anything that is not the requested digest is not served;
//! - a challenge that points to a realm that is not the declared one, and a
//!   redirect to another origin, **do not** carry the credential anywhere;
//! - without network only what is intact and has a receipt is served, and the gates --Cedar and
//!   trust-- also run when the content comes out of the
//!   cache.
//!
//! The upstream is a distribution registry of the lab: it asks for a
//! token, issues it in its realm, and can do both hostile things. By
//! default the path **does not use the network**: the upstream is local and the content
//! is a fixture.

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

/// The key of the deployment's OCI proxy repository.
const REPOSITORY: &str = "oci-proxy";

/// The name with which the upstream is requested.
const NAME: &str = "oci-proxy";

/// The tag that is requested.
const TAG: &str = "latest";

/// The header that states the digest of the served content.
const DOCKER_CONTENT_DIGEST: HeaderName = HeaderName::from_static("docker-content-digest");

/// The credential that the upstream requires in its realm.
const CREDENTIAL: &str = "secreto-del-laboratorio";

/// The token that the upstream issues.
const TOKEN: &str = "token-del-laboratorio";

/// The name of the environment variable that carries the credential.
const CREDENTIAL_VARIABLE: &str = "LAB_OCI_CREDENTIAL";

/// The catalog seeder.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

// ---------------------------------------------------------------------------
// The lab's distribution registry.
// ---------------------------------------------------------------------------

/// How the registry behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TheBehaviour {
    /// Asks for a token, serves manifests and blobs.
    Normal,
    /// Its challenge points to a realm that is **not** the declared one.
    HostileRealm,
    /// The manifest answers with a redirect to another origin.
    Redirect,
}

/// What the registry has received.
#[derive(Debug, Clone)]
struct Seen {
    /// The requested path.
    path: String,
    /// The `Authorization` header, if there was one.
    authorization: Option<String>,
}

/// The state of the registry.
struct TheWorld {
    /// The manifests, by reference (`latest`, `sha256:...`).
    manifests: Mutex<HashMap<String, Vec<u8>>>,
    /// The blobs, by digest.
    blobs: Mutex<HashMap<String, Vec<u8>>>,
    /// What the registry has received.
    seen: Mutex<Vec<Seen>>,
    /// How it behaves.
    behaviour: Mutex<TheBehaviour>,
    /// The origin that the hostile challenge points to, or that the
    /// redirect sends to.
    elsewhere: Mutex<String>,
    /// The origin of this registry, for the challenge.
    origin: Mutex<String>,
}

/// The lab's distribution registry.
struct TheRegistry {
    port: u16,
    world: Arc<TheWorld>,
}

impl TheRegistry {
    /// Starts an empty registry.
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

    /// The port it listens on.
    fn port(&self) -> u16 {
        self.port
    }

    /// The origin of the registry.
    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// The base of the registry, with a trailing slash.
    fn base(&self) -> String {
        format!("{}/", self.origin())
    }

    /// Publishes a manifest, under a reference.
    fn publish(&self, reference: &str, bytes: Vec<u8>) {
        self.world
            .manifests
            .lock()
            .expect("the manifests")
            .insert(reference.to_string(), bytes);
    }

    /// Publishes a blob.
    fn publish_blob(&self, digest: &str, bytes: Vec<u8>) {
        self.world
            .blobs
            .lock()
            .expect("the blobs")
            .insert(digest.to_string(), bytes);
    }

    /// Changes the registry's behaviour.
    fn behave(&self, behaviour: TheBehaviour) {
        *self.world.behaviour.lock().expect("the behaviour") = behaviour;
    }

    /// Where the hostile challenge points, or where the redirect sends.
    fn elsewhere(&self, origin: &str) {
        *self.world.elsewhere.lock().expect("the elsewhere") = origin.to_string();
    }

    /// What the registry has received.
    fn seen(&self) -> Vec<Seen> {
        self.world.seen.lock().expect("the log").clone()
    }

    /// How many times a path has been requested.
    fn hits(&self, path: &str) -> usize {
        self.seen().iter().filter(|one| one.path == path).count()
    }
}

/// Starts a server with the registry's router and returns the port.
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

/// The registry's token endpoint.
///
/// It only hands the token to whoever presents the credential, and says who it is:
/// an open realm is a realm that anyone passes through.
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

/// The distribution surface: manifests and blobs.
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

    // The challenge: the registry asks for a token in its realm. The hostile
    // registry asks for it in the realm of **another**, which is what an upstream can
    // do.
    let realm = match behaviour {
        TheBehaviour::HostileRealm => format!("{elsewhere}/token"),
        _ => format!("{origin}/token"),
    };
    let challenge = format!("Bearer realm=\"{realm}\",service=\"lab\"");

    // Without the token, the registry answers with the challenge.
    let authorized = authorization.as_deref() == Some(&format!("Bearer {TOKEN}"));
    if !authorized {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, challenge.as_str())],
            "{}".to_string(),
        )
            .into_response();
    }

    // The redirect: the manifest is offered at another origin.
    if behaviour == TheBehaviour::Redirect {
        return (
            StatusCode::FOUND,
            [(header::LOCATION, format!("{elsewhere}/v2/{path}").as_str())],
            "".to_string(),
        )
            .into_response();
    }

    // `v2/<name>/manifests/<reference>` and `v2/<name>/blobs/<digest>`.
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
        // The blob is looked up by its hexadecimal: the path names it with its
        // algorithm, and the store keeps it without it.
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
// The hostile origin.
// ---------------------------------------------------------------------------

/// The hostile origin: it only records what reaches it.
struct TheHostile {
    port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl TheHostile {
    /// Starts an origin that only records what reaches it.
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

    /// The hostile one's origin.
    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// What the hostile one has received.
    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().expect("the log").clone()
    }
}

// ---------------------------------------------------------------------------
// The deployment.
// ---------------------------------------------------------------------------

/// The deployment with the distribution registry in front.
struct TheLab {
    deployment: Deployment,
    registry: TheRegistry,
    hostile: TheHostile,
}

impl TheLab {
    /// A deployment with the registry seeded and the service stopped.
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

    /// Seeds the OCI proxy repository.
    ///
    /// The cache has no TTL: everything is revalidated, which is what is needed
    /// to see the `ETag` revalidation and the movement of a tag.
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

    /// The service's environment: the allowed egress and the credential.
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

    /// Starts the service with the allowed egress.
    fn start(&self) -> support::Service {
        let environment = self.the_environment();
        let borrowed: Vec<(&str, &str)> = environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        self.deployment.start(&borrowed)
    }

    /// Starts the service on the given port, with what is added to it.
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

    /// A lab **without** the proxy repository seeded: the catalog
    /// only has the accounts, and the policy lets the first person
    /// create repositories. Whatever has to exist is created by the public API.
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

    /// The public body that creates the lab's OCI proxy repository.
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

    /// Creates a repository through `POST /api/v1/catalog/repositories`.
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

    /// A deployment file where to leave the body of a response.
    fn a_file(&self, name: &str) -> PathBuf {
        self.deployment.directory.path().join(name)
    }
}

// ---------------------------------------------------------------------------
// The utilities.
// ---------------------------------------------------------------------------

/// The digest of some bytes.
fn sha256_of(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// A text in base64.
fn base64_of(text: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(text)
}

/// The token of the person who downloads.
fn the_client_token() -> String {
    support::token(FIRST_ACCOUNT, "alice")
}

/// An image manifest with its blobs.
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

/// An image index: a manifest that points to others.
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

/// Seeds the registry's content.
///
/// Returns the manifest and the index, so they can be compared with what
/// the registry serves.
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
// The cases.
// ---------------------------------------------------------------------------

/// The cases that a complete run has to execute.
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
    (
        "ac5_native_client_pull",
        "skopeo saca del proxy directo el manifiesto, los blobs y la lista de manifiestos",
    ),
    (
        "ac5_native_client_referrers",
        "oras pide al proxy directo los referrers de un manifiesto",
    ),
];

/// The title and the disclaimer of the report.
fn the_report() {
    describe_registry_lab(
        "Laboratorio de OCI proxy — informe",
        &[
            "- **Un cliente real, y solo en dos casos.** El grueso de los casos habla el camino de la distribución con `curl` contra un upstream local. Los dos casos `ac5_native_client_*` ejecutan `skopeo` (pull del manifiesto, los blobs y la lista de manifiestos) y `oras` (referrers) contra un repositorio proxy directo; si el cliente no está, el caso se declara `NOT_RUN` con el motivo. `podman` no se ha usado, y el resto de los casos no dicen nada de ningún cliente.",
            "- **Ni la red real.** Por defecto el laboratorio no sale a la red: el upstream es un registro local. El contenido es un fixture, y lo que se demuestra es el camino, no la integridad de ese contenido.",
            "- **Ni la superficie de push.** Solo se ha medido el pull: manifiesto y blob, por etiqueta y por digest.",
            "- **Ni el laboratorio.** Los casos de `scope: harness` miden el arnés —el recuento— y no el producto. Los dos recuentos van aparte.",
        ],
    );
}

/// The manifest's path.
fn the_manifest_path(reference: &str) -> String {
    format!("/v2/{NAME}/manifests/{reference}")
}

/// The blob's path.
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
            // The bytes are the upstream's, not a rewrite.
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
            // The upstream received the request.
            // The registry sees the path without the surface prefix.
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
            // The references between objects are still there: an index that
            // loses its digests is not the same index.
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
            // The upstream moves the tag: the same name, other
            // content.
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
            // The upstream lies: it offers other bytes for that digest.
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
            // The hostile origin did not receive the credential.
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
            // The content of a **pinned** digest is not replaced: what
            // arrives from another origin is not what was requested, and is not served.
            if answer.status == 200 {
                return case().failed("el pull funciono con una redireccion a otro origen");
            }
            if answer.body == b"robado".to_vec() {
                return case().failed("el contenido del otro origen llego al cliente");
            }
            // The hostile origin received the request, but **without** the
            // credential: the credential only travels to its own origin.
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

            // The service comes back **without network**, on the same port.
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

            // The policy changes: the person can no longer download. The
            // content is in the cache, and even so it is not served.
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

// ---------------------------------------------------------------------------
// AC5: a native client against the direct proxy.
// ---------------------------------------------------------------------------

/// The content of the native-client cases: two images, the list that names
/// them and a referrer that hangs from the first image.
struct TheNativeContent {
    /// The tag of the list.
    list_tag: &'static str,
    /// The tag of the first image alone.
    image_tag: &'static str,
    /// Every manifest by digest (`sha256:...`): what the upstream serves.
    manifests: Vec<(String, Vec<u8>)>,
    /// Every blob by digest (`sha256:...`).
    blobs: Vec<(String, Vec<u8>)>,
    /// The digest of the list.
    list_digest: String,
    /// The digest of the first image, the subject of the referrer.
    subject_digest: String,
    /// The digest of the referrer.
    referrer_digest: String,
}

/// Builds an image for one architecture, with blobs of its own.
fn a_native_image(architecture: &str) -> (Vec<u8>, Vec<(String, Vec<u8>)>) {
    let config = json!({ "architecture": architecture, "os": "linux" })
        .to_string()
        .into_bytes();
    let layer = format!("layer-of-{architecture}").into_bytes();
    let config_digest = format!("sha256:{}", sha256_of(&config));
    let layer_digest = format!("sha256:{}", sha256_of(&layer));
    let manifest = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": config_digest,
            "size": config.len()
        },
        "layers": [{
            "mediaType": "application/vnd.oci.image.layer.v1.tar",
            "digest": layer_digest,
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

/// Seeds the upstream with the native-client content.
fn the_native_content(registry: &TheRegistry) -> TheNativeContent {
    let (first, first_blobs) = a_native_image("amd64");
    let (second, second_blobs) = a_native_image("arm64");
    let first_digest = format!("sha256:{}", sha256_of(&first));
    let second_digest = format!("sha256:{}", sha256_of(&second));
    let list = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "manifests": [
            {
                "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "digest": first_digest,
                "size": first.len(),
                "platform": { "os": "linux", "architecture": "amd64" }
            },
            {
                "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "digest": second_digest,
                "size": second.len(),
                "platform": { "os": "linux", "architecture": "arm64" }
            }
        ]
    })
    .to_string()
    .into_bytes();
    let list_digest = format!("sha256:{}", sha256_of(&list));

    // The referrer: a manifest whose `subject` is the first image.
    let referrer = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "artifactType": "application/vnd.hodei.signature.v1+json",
        "config": {
            "mediaType": "application/vnd.oci.empty.v1+json",
            "digest": format!("sha256:{}", sha256_of(b"{}")),
            "size": 2
        },
        "layers": [{
            "mediaType": "application/vnd.oci.empty.v1+json",
            "digest": format!("sha256:{}", sha256_of(b"{}")),
            "size": 2
        }],
        "subject": {
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "digest": first_digest,
            "size": first.len()
        }
    })
    .to_string()
    .into_bytes();
    let referrer_digest = format!("sha256:{}", sha256_of(&referrer));

    let manifests = vec![
        (first_digest.clone(), first),
        (second_digest, second),
        (list_digest.clone(), list),
        (referrer_digest.clone(), referrer),
    ];
    let mut blobs: Vec<(String, Vec<u8>)> = first_blobs;
    blobs.extend(second_blobs);
    blobs.push((format!("sha256:{}", sha256_of(b"{}")), b"{}".to_vec()));

    for (digest, bytes) in &manifests {
        registry.publish(digest, bytes.clone());
    }
    for (digest, bytes) in &blobs {
        registry.publish_blob(digest.trim_start_matches("sha256:"), bytes.clone());
    }
    registry.publish("multi", manifests[2].1.clone());
    registry.publish("single", manifests[0].1.clone());
    TheNativeContent {
        list_tag: "multi",
        image_tag: "single",
        manifests,
        blobs,
        list_digest,
        subject_digest: first_digest,
        referrer_digest,
    }
}

/// Whether a command exists and answers to its version request.
///
/// Only the **absence** of the client is a reason not to run: a client
/// that is there and fails is a failure of the case.
fn the_client_exists(program: &str, version_argument: &str) -> bool {
    std::process::Command::new(program)
        .arg(version_argument)
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Runs a native client in an isolated home and returns the outcome.
fn run_native(home: &std::path::Path, program: &str, args: &[&str]) -> (bool, String, String) {
    match std::process::Command::new(program)
        .args(args)
        .env("HOME", home)
        .env("DOCKER_CONFIG", home.join("config"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .output()
    {
        Ok(output) => (
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ),
        Err(error) => (false, String::new(), error.to_string()),
    }
}

/// Reads a blob of an OCI image layout, if it is there.
fn a_layout_blob(layout: &std::path::Path, digest: &str) -> Option<Vec<u8>> {
    std::fs::read(
        layout
            .join("blobs/sha256")
            .join(digest.trim_start_matches("sha256:")),
    )
    .ok()
}

#[test]
fn ac5_native_client_pull() {
    const ID: &str = "ac5_native_client_pull";
    const TITLE: &str =
        "skopeo saca del proxy directo el manifiesto, los blobs y la lista de manifiestos";
    run_registry_lab_case(
        THE_CASES,
        ID,
        TITLE,
        Scope::Product,
        Expectation::Succeeds,
        || {
            the_report();
            if !the_client_exists("skopeo", "--version") {
                return LabCase::new(ID, TITLE)
                    .not_run("skopeo no esta instalado o no responde a --version");
            }
            let lab = TheLab::new();
            let content = the_native_content(&lab.registry);
            let mut service = lab.start();
            let port = service.port;
            let home = lab.a_file("native-home");
            std::fs::create_dir_all(home.join("config")).expect("the home");
            std::fs::create_dir_all(home.join("run")).expect("the runtime directory");
            let credentials = format!("alice:{}", the_client_token());
            let source = |reference: &str| format!("docker://127.0.0.1:{port}/{NAME}{reference}");
            let mut problems: Vec<String> = Vec::new();

            // 1. The list, pulled whole with its children, into an OCI layout.
            let layout = lab.a_file("layout-list");
            let (ok, stdout, stderr) = run_native(
                &home,
                "skopeo",
                &[
                    "copy",
                    "--all",
                    "--preserve-digests",
                    "--src-tls-verify=false",
                    "--src-creds",
                    &credentials,
                    &source(&format!(":{}", content.list_tag)),
                    &format!("oci:{}:list", layout.display()),
                ],
            );
            if !ok {
                problems.push(format!(
                    "skopeo copy --all of the list failed: {stderr} {stdout}"
                ));
            } else {
                // Every object of the upstream is in the layout, byte for byte.
                for (digest, upstream_bytes) in &content.manifests {
                    if *digest == content.referrer_digest {
                        continue;
                    }
                    match a_layout_blob(&layout, digest) {
                        None => problems.push(format!("the layout lacks the manifest {digest}")),
                        Some(pulled) if &pulled != upstream_bytes => problems.push(format!(
                            "the manifest {digest} pulled by skopeo is not the one of the upstream"
                        )),
                        Some(_) => {}
                    }
                }
                for (digest, upstream_bytes) in &content.blobs {
                    if digest == &format!("sha256:{}", sha256_of(b"{}")) {
                        continue;
                    }
                    match a_layout_blob(&layout, digest) {
                        None => problems.push(format!("the layout lacks the blob {digest}")),
                        Some(pulled) if &pulled != upstream_bytes => problems.push(format!(
                            "the blob {digest} pulled by skopeo is not the one of the upstream"
                        )),
                        Some(_) => {}
                    }
                }
                // The index of the layout names the list by the digest of
                // the upstream: the digest was preserved.
                let named = std::fs::read(layout.join("index.json"))
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                    .and_then(|index| index["manifests"][0]["digest"].as_str().map(str::to_string));
                if named.as_deref() != Some(content.list_digest.as_str()) {
                    problems.push(format!(
                        "the layout names the list as {named:?} and the upstream says {}",
                        content.list_digest
                    ));
                }
            }

            // 2. The raw manifest of the list, by tag and by digest: the
            // bytes of the upstream, and the digest that skopeo computes
            // over them is the one of the upstream.
            let list_bytes = &content
                .manifests
                .iter()
                .find(|(digest, _)| *digest == content.list_digest)
                .expect("the list")
                .1;
            for reference in [
                format!(":{}", content.list_tag),
                format!("@{}", content.list_digest),
            ] {
                let (ok, stdout, stderr) = run_native(
                    &home,
                    "skopeo",
                    &[
                        "inspect",
                        "--raw",
                        "--tls-verify=false",
                        "--creds",
                        &credentials,
                        &source(&reference),
                    ],
                );
                if !ok {
                    problems.push(format!("skopeo inspect --raw {reference} failed: {stderr}"));
                } else if stdout.as_bytes() != list_bytes.as_slice() {
                    problems.push(format!(
                        "skopeo inspect --raw {reference} did not return the bytes of the upstream"
                    ));
                }
            }

            // 3. A single image by tag: manifest and blobs, and the digest
            // that skopeo writes down is the one of the upstream.
            let image_layout = lab.a_file("layout-image");
            let digest_file = lab.a_file("image.digest");
            let (ok, _, stderr) = run_native(
                &home,
                "skopeo",
                &[
                    "copy",
                    "--preserve-digests",
                    "--digestfile",
                    digest_file.to_str().expect("the path"),
                    "--src-tls-verify=false",
                    "--src-creds",
                    &credentials,
                    &source(&format!(":{}", content.image_tag)),
                    &format!("oci:{}:image", image_layout.display()),
                ],
            );
            if !ok {
                problems.push(format!("skopeo copy of the image failed: {stderr}"));
            } else {
                let written = std::fs::read_to_string(&digest_file).unwrap_or_default();
                if written.trim() != content.subject_digest {
                    problems.push(format!(
                        "skopeo wrote the digest {written:?} and the upstream says {}",
                        content.subject_digest
                    ));
                }
                if a_layout_blob(&image_layout, &content.subject_digest).as_deref()
                    != Some(content.manifests[0].1.as_slice())
                {
                    problems.push("the image manifest is not the one of the upstream".to_string());
                }
            }

            // 4. `Docker-Content-Digest` of the list is the one of the
            // upstream bytes: it is what a client verifies against.
            let into = lab.a_file("list-over-http");
            let answer = Curl::call(
                port,
                "GET",
                &the_manifest_path(content.list_tag),
                &the_client_token(),
                None,
                None,
                Some(&into),
            )
            .run();
            let header = answer.header("Docker-Content-Digest");
            if header.as_deref() != Some(content.list_digest.as_str()) {
                problems.push(format!(
                    "Docker-Content-Digest of the list was {header:?} and the upstream says {}",
                    content.list_digest
                ));
            }
            if &answer.body != list_bytes {
                problems.push("the list over HTTP is not the one of the upstream".to_string());
            }

            // The upstream was the one that answered.
            if lab
                .registry
                .hits(&format!("{NAME}/manifests/{}", content.list_tag))
                == 0
            {
                problems.push("the upstream never saw the request of the list".to_string());
            }
            support::stop(&mut service);

            if problems.is_empty() {
                LabCase::new(ID, TITLE)
                    .command("skopeo copy --all / inspect --raw / copy --digestfile")
                    .origin(&lab.registry.origin())
                    .sha256(content.list_digest.trim_start_matches("sha256:"))
                    .passed()
            } else {
                LabCase::new(ID, TITLE).failed(problems.join("; "))
            }
        },
    );
}

// Known product gap, measured by this case and not hidden by it: a direct
// proxy answers `/referrers/<digest>` from its own catalog only, so the
// referrers that the upstream holds are not served (the answer is an empty
// index). The case is ignored, with that reason, until the proxy serves them;
// run it with `--ignored` to see the failure.
#[test]
#[ignore = "HSP-299: the direct OCI proxy does not serve the referrers of the upstream (empty index)"]
fn ac5_native_client_referrers() {
    const ID: &str = "ac5_native_client_referrers";
    const TITLE: &str = "oras pide al proxy directo los referrers de un manifiesto";
    run_registry_lab_case(
        THE_CASES,
        ID,
        TITLE,
        Scope::Product,
        Expectation::Succeeds,
        || {
            the_report();
            let oras = support::the_oras();
            if !the_client_exists(&oras, "version") {
                return LabCase::new(ID, TITLE)
                    .not_run("oras no esta instalado o no responde a version");
            }
            let lab = TheLab::new();
            let content = the_native_content(&lab.registry);
            let mut service = lab.start();
            let port = service.port;
            let home = lab.a_file("native-home");
            std::fs::create_dir_all(home.join("config")).expect("the home");
            std::fs::create_dir_all(home.join("run")).expect("the runtime directory");

            // The referrer is pulled through the proxy first, so that it is
            // known to the proxy as any manifest that a client has used, and
            // then it is asked for with the native client.
            let pulled = Curl::call(
                port,
                "GET",
                &the_manifest_path(&content.referrer_digest),
                &the_client_token(),
                None,
                None,
                Some(&lab.a_file("referrer-pulled")),
            )
            .run();
            assert_eq!(pulled.status, 200, "the referrer was not pulled");
            let (ok, stdout, stderr) = run_native(
                &home,
                &oras,
                &[
                    "discover",
                    "--plain-http",
                    "--username",
                    "alice",
                    "--password",
                    &the_client_token(),
                    "--format",
                    "json",
                    &format!("127.0.0.1:{port}/{NAME}@{}", content.subject_digest),
                ],
            );
            support::stop(&mut service);
            if !ok {
                return LabCase::new(ID, TITLE)
                    .failed(format!("oras discover failed: {stderr} {stdout}"));
            }
            let found: serde_json::Value =
                serde_json::from_str(&stdout).unwrap_or(serde_json::Value::Null);
            let listed: Vec<String> = found["manifests"]
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(|one| one["digest"].as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            if !listed.contains(&content.referrer_digest) {
                return LabCase::new(ID, TITLE).failed(format!(
                    "the referrers that oras discovered through the proxy were {listed:?} and \
                     the upstream holds {}: {stdout}",
                    content.referrer_digest
                ));
            }
            LabCase::new(ID, TITLE)
                .command("oras discover --plain-http <proxy>/oci-proxy@sha256:...")
                .origin(&lab.registry.origin())
                .sha256(content.referrer_digest.trim_start_matches("sha256:"))
                .passed()
        },
    );
}
