//! HSP-196: proxy, grupos virtuales y provenance sobre el motor Dgraph real.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` como todos los IT del stack (TLS +
//! ACL, el servidor vivo h186). Lo que prueba, contra el motor de verdad:
//!
//! - **AC1** el flujo proxy real (miss → hit → offline → grupo virtual)
//!   sobre el catálogo Dgraph y el libro de procedencia Dgraph
//!   (`src/proxy/dgraph_ledger.rs`), con un upstream local controlado:
//!   la primera lectura baja, verifica y deja UN recibo; la segunda sirve
//!   como propio sin molestar al upstream; offline solo sirve lo verificado
//!   y no toca la red; el grupo virtual recorre los miembros en orden y
//!   gana el hosted, sin ampliar permisos ni cambiar la resolución (la
//!   salida a hosts no autorizados sigue denegada, el scope del upstream
//!   sigue acotado).
//! - **AC2** el recibo con su digest sobrevive al reinicio del proceso
//!   (cliente, plano de proxy y CAS recreados sobre el MISMO grafo y los
//!   MISMOs bytes): tras el reinicio la ruta se sirve como hit sin salir a
//!   la red y el recibo sigue ahí, idéntico; `record` repetido del mismo
//!   id es replay sin duplicar; la clave lleva la cuenta, así que dos
//!   cuentas con el mismo id no comparten fila (sin duplicidad entre
//!   repos).
//!
//! El censo es SIEMPRE por membresía de claves propias (índice exacto por
//! `subject`/`key` con el tenant de la prueba): nunca globales, porque el
//! alpha se comparte con otros carriles (HSP-192, HSP-201).
//!
//! Suite SERIALIZADA (`--test-threads=1`): la apertura bajo carga es el
//! flake conocido del stack (HSP-219) y cada prueba abre su propio rig.

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use cedar_policy::Entities;
use chrono::Utc;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::proxy::dgraph_ledger::{DgraphProvenanceLedger, receipt_key};
use hodei_artifacts_api::proxy::fetch::{FetchLimits, ProxyFetcher};
use hodei_artifacts_api::proxy::{ProxyPlane, egress::AllowedHost};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use hodei_catalog::{
    AllowAllAuthorizer, CatalogReader, CatalogWriter, DigestScope, ProvenanceLedger,
    RepositoryConfig, RepositoryDescriptor, RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_catalog_storage::DgraphCatalogStore;
use hodei_evidence::ContentDigest;
use hodei_graph_client::{GraphClient, GraphClientConfig};
use hodei_hub::blob::{BlobStore, FileBlobStore};
use serde_json::json;
use tempfile::{TempDir, tempdir};
use tower::ServiceExt;

/// El cuerpo que vive en el upstream para `pkg/lib.bin`.
const BODY: &[u8] = b"content that lives upstream";
/// El cuerpo de la ruta compartida hosted+proxy (gana el hosted).
const SHARED: &[u8] = b"shared content with two possible homes";

/// El entorno del grafo, o `None` con el IT saltado (gated como el stack).
fn config_from_env() -> Option<GraphClientConfig> {
    let endpoint = std::env::var("HODEI_GRAPH_IT_ENDPOINT").ok()?;
    let ca = std::env::var("HODEI_GRAPH_IT_CA").unwrap_or_default();
    let server_name = std::env::var("HODEI_GRAPH_IT_SERVER_NAME").unwrap_or_default();
    let user = std::env::var("HODEI_GRAPH_IT_USER").unwrap_or_default();
    let password = std::env::var("HODEI_GRAPH_IT_PASSWORD").unwrap_or_default();
    let mut text = format!(
        "endpoint = \"{endpoint}\"\n\n[tls]\nenabled = {enabled}\nca_cert_path = \"{ca}\"\n",
        enabled = !ca.is_empty()
    );
    if !server_name.is_empty() {
        text.push_str(&format!("server_name = \"{server_name}\"\n"));
    }
    if !user.is_empty() {
        text.push_str(&format!(
            "\n[credentials]\nuserid = \"{user}\"\npassword = \"{password}\"\n"
        ));
    }
    Some(GraphClientConfig::from_toml_str(&text).expect("the IT configuration validates"))
}

/// Un cliente nuevo: el IT simula el reinicio del proceso re-conectando.
async fn fresh_client() -> Option<Arc<GraphClient>> {
    let Some(config) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return None;
    };
    let mut last = String::new();
    for attempt in 1..=3 {
        let mut client = match GraphClient::connect(config.clone()).await {
            Ok(client) => client,
            Err(error) => {
                last = error.to_string();
                tokio::time::sleep(Duration::from_millis(250 * attempt as u64)).await;
                continue;
            }
        };
        match client.login().await {
            Ok(()) => return Some(Arc::new(client)),
            Err(error) => {
                last = error.to_string();
                tokio::time::sleep(Duration::from_millis(250 * attempt as u64)).await;
            }
        }
    }
    panic!("Dgraph did not open after 3 attempts: {last}");
}

/// La cola del reloj: separa dos ejecuciones de la misma prueba.
fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

/// Una cuenta por prueba y por ejecución: `hsp196_<tag>_<cola>`, el
/// baseline por tenant que exige el encargo. Ninguna prueba se encuentra
/// con lo que otra dejó, y el censo va solo por sus claves.
fn account(tag: &str) -> String {
    format!("hsp196_{tag}_{}", &nonce()[4..])
}

// ---------------------------------------------------------------------------
// El upstream controlado: un servidor HTTP plano con respuestas escritas.
// ---------------------------------------------------------------------------

/// Un upstream local: sirve las rutas sembradas, 404 el resto, y guarda
/// cuántas veces vio cada una.
struct Upstream {
    port: u16,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Upstream {
    fn start(routes: Vec<(&str, Vec<u8>)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("the upstream binds");
        let port = listener.local_addr().expect("the address").port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        let table: Vec<(String, Vec<u8>)> = routes
            .into_iter()
            .map(|(path, body)| (path.to_string(), body))
            .collect();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let log = Arc::clone(&log);
                let table = table.clone();
                std::thread::spawn(move || serve(stream, &table, &log));
            }
        });
        Self { port, seen }
    }

    fn total(&self) -> usize {
        self.seen.lock().expect("the log").len()
    }
}

fn serve(mut stream: TcpStream, table: &[(String, Vec<u8>)], log: &Mutex<Vec<String>>) {
    let mut request = Vec::new();
    let mut buffer = [0u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
    }
    let text = String::from_utf8_lossy(&request).into_owned();
    let Some(path) = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
    else {
        return;
    };
    let path = path.trim_start_matches('/').to_string();
    log.lock().expect("the log").push(path.clone());
    let answer = match table.iter().find(|(key, _)| *key == path) {
        Some((_, body)) => format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        ),
        None => {
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
        }
    };
    let _ = stream.write_all(answer.as_bytes());
    if let Some((_, body)) = table.iter().find(|(key, _)| key == &path) {
        let _ = stream.write_all(body);
    }
    let _ = stream.flush();
}

// ---------------------------------------------------------------------------
// El vertical: catálogo Dgraph + libro Dgraph + plano de proxy + router.
// ---------------------------------------------------------------------------

struct ContractAuthenticator {
    account: String,
}

#[async_trait]
impl PrincipalAuthenticator for ContractAuthenticator {
    async fn authenticate(
        &self,
        credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        match credentials {
            Credentials::Bearer(token) if token == "alice" => Ok(PrincipalClaims::new(
                PrincipalId::new(format!("hrn:hodei:iam::{}:User/alice", self.account)).unwrap(),
                AccountId::new(self.account.clone()).unwrap(),
            )),
            _ => Err(AuthenticationError::InvalidCredentials),
        }
    }
}

/// Cedar: `alice` descarga y publica en CADA repositorio del despliegue.
/// El grupo no da permisos: la política por miembro es lo que se prueba.
fn cedar_for(account: &str, repos: &[&str]) -> Arc<dyn AuthorizerPort> {
    let mut policy = String::new();
    for repo in repos {
        policy.push_str(&format!(
            r#"permit(
        principal == Iam::User::"hrn:hodei:iam::{account}:User/alice",
        action == Artifacts::Action::"UploadAsset",
        resource == Artifacts::Repository::"hrn:hodei:artifacts:eu:{account}:repository/{repo}"
    );
    permit(
        principal == Iam::User::"hrn:hodei:iam::{account}:User/alice",
        action == Artifacts::Action::"DownloadAsset",
        resource == Artifacts::Repository::"hrn:hodei:artifacts:eu:{account}:repository/{repo}"
    );
    "#
        ));
    }
    Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            &policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .expect("authorizer"),
    )))
}

fn descriptor(
    account: &str,
    repo_key: &str,
    mode: RepositoryMode,
    settings: serde_json::Value,
) -> RepositoryDescriptor {
    let mut config = RepositoryConfig::new(DigestScope::Global);
    config.config_settings = settings;
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", account, repo_key),
        account: account.to_string(),
        repo_key: repo_key.to_string(),
        format: RepositoryFormat::Raw,
        mode,
        config,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn upstream_of(id: &str, url: &str, scopes: &[&str]) -> serde_json::Value {
    json!({
        "proxy": { "upstream": {
            "id": id,
            "url": url,
            "scopes": scopes,
            "max_bytes": 65536,
        } }
    })
}

/// El grafo y el CAS que todas las vidas del servicio comparten: es el
/// estado durable que sobrevive al reinicio.
struct Shared {
    tmp: TempDir,
    client: Arc<GraphClient>,
    store: Arc<DgraphCatalogStore>,
}

/// Una instancia del servicio: plano de proxy sobre el libro Dgraph.
///
/// `rocksdb` vive en su propio subdirectorio: dos instancias abiertas a la
/// vez no pelean por el lock, y el estado durable del proxy es TODO: el
/// grafo y el CAS, que es lo que se quiere poner a prueba. Un `open` nuevo
/// con el mismo grafo y el mismo CAS es el reinicio del proceso.
struct Instance {
    app: Router,
    ledger: Arc<DgraphProvenanceLedger>,
    #[allow(dead_code)]
    local: TempDir,
}

impl Instance {
    /// Abre una vida del servicio sobre el rig compartido. `offline=true`
    /// es el interruptor del operador (`HODEI_PROXY__OFFLINE`).
    async fn open(shared: &Shared, account: &str, offline: bool) -> Self {
        let local = tempdir().expect("the local dir");
        let ledger = Arc::new(
            DgraphProvenanceLedger::open(shared.client.clone())
                .await
                .expect("the dgraph ledger opens"),
        );
        let mut config = AppConfig::default();
        config.security.raw_api_enabled = true;
        // The graph profile is the only authority: the bootstrap connects to the
        // same alpha the test seeds and its catalog is replaced just below.
        config.graph = config_from_env();
        config.content.cas_path = local
            .path()
            .join("bootstrap-cas")
            .to_string_lossy()
            .into_owned();
        let state = bootstrap(
            &config,
            BootstrapConfig {
                register_iam_schema: false,
                schema_version: None,
                validate_schemas: false,
            },
        )
        .await
        .expect("bootstrap must succeed");

        let plane = Arc::new(ProxyPlane::new(
            ProxyFetcher::new(
                hodei_artifacts_api::proxy::egress::EgressPolicy::new(vec![AllowedHost {
                    host: "127.0.0.1".to_string(),
                    port: Some(shared_lab::port()),
                    allow_private: true,
                    allow_http: true,
                }])
                .expect("the lab egress"),
                FetchLimits {
                    connect_timeout: Duration::from_secs(2),
                    total_timeout: Duration::from_secs(30),
                    max_bytes: 1 << 20,
                },
                4,
            )
            .with_allowed_credentials(std::iter::empty::<String>())
            .with_offline(offline),
            ledger.clone(),
        ));

        let reader: Arc<dyn CatalogReader> = shared.store.clone();
        let writer: Arc<dyn CatalogWriter> = shared.store.clone();
        let content: Arc<dyn BlobStore> =
            Arc::new(FileBlobStore::new(shared.tmp.path().join("cas")).expect("the CAS"));

        let state = state
            .with_catalog(reader, writer)
            .with_content_store(content)
            .with_proxy_plane(plane)
            .with_authenticator(Arc::new(ContractAuthenticator {
                account: account.to_string(),
            }))
            .with_authorization(cedar_for(
                account,
                &["hos", "pxy", "pxy-scoped", "pxy-ghost", "grp"],
            ));

        Self {
            app: build_router(state, &config),
            ledger,
            local,
        }
    }
}

/// El upstream de la corrida: una sola instancia, sembrada y con su
/// contador de peticiones. La egress lo autoriza por puerto exacto.
mod shared_lab {
    use super::*;
    static LAB: std::sync::OnceLock<Upstream> = std::sync::OnceLock::new();

    fn lab() -> &'static Upstream {
        LAB.get_or_init(|| {
            Upstream::start(vec![
                ("pkg/lib.bin", BODY.to_vec()),
                ("shared/file.bin", SHARED.to_vec()),
                ("ok/inside.txt", b"inside the scope".to_vec()),
            ])
        })
    }

    pub(super) fn port() -> u16 {
        lab().port
    }

    pub(super) fn total() -> usize {
        lab().total()
    }
}

// ---------------------------------------------------------------------------
// HTTP del vertical.
// ---------------------------------------------------------------------------

struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl Answer {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }
}

async fn send(app: &Router, method: Method, uri: &str, body: Option<Vec<u8>>) -> Answer {
    let mut builder = Request::builder().method(method).uri(uri);
    builder = builder.header(header::AUTHORIZATION, "Bearer alice");
    let request = builder
        .body(body.map(Body::from).unwrap_or_else(Body::empty))
        .expect("the request is well formed");
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("the router must always produce a response");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .expect("the body");
    Answer {
        status,
        headers,
        body: bytes.to_vec(),
    }
}

async fn get(app: &Router, repository: &str, path: &str) -> Answer {
    send(
        app,
        Method::GET,
        &format!("/repository/{repository}/{path}"),
        None,
    )
    .await
}

async fn provenance(app: &Router, repository: &str, path: &str) -> serde_json::Value {
    let answer = send(
        app,
        Method::GET,
        &format!("/api/v1/provenance?repository={repository}&path={path}"),
        None,
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK, "{:?}", answer.body);
    serde_json::from_slice(&answer.body).expect("json")
}

/// El rig completo: grafo, CAS y upstream compartidos entre la primera
/// vida del servicio y sus reinicios.
struct Rig {
    account: String,
    shared: Shared,
}

impl Rig {
    async fn mount(tag: &str) -> Option<Self> {
        let client = fresh_client().await?;
        let store = Arc::new(
            DgraphCatalogStore::open(client.clone(), Arc::new(AllowAllAuthorizer))
                .await
                .expect("the dgraph catalog store opens"),
        );
        let tmp = tempdir().expect("the shared dir");
        let account = account(tag);
        let base = format!("http://127.0.0.1:{}/", shared_lab::port());
        for descriptor in [
            descriptor(&account, "hos", RepositoryMode::Hosted, json!({})),
            descriptor(
                &account,
                "pxy",
                RepositoryMode::Proxy,
                upstream_of("lab", &base, &["/"]),
            ),
            descriptor(
                &account,
                "pxy-scoped",
                RepositoryMode::Proxy,
                upstream_of("lab", &base, &["ok/"]),
            ),
            descriptor(
                &account,
                "pxy-ghost",
                RepositoryMode::Proxy,
                upstream_of(
                    "ghost",
                    &format!("http://localhost:{}/", shared_lab::port()),
                    &["/"],
                ),
            ),
            descriptor(
                &account,
                "grp",
                RepositoryMode::Group,
                json!({ "group": { "members": ["hos", "pxy"] } }),
            ),
        ] {
            store
                .create_repository(&account, descriptor)
                .await
                .expect("the repository is created");
        }
        Some(Self {
            account,
            shared: Shared { tmp, client, store },
        })
    }

    async fn instance(&self, offline: bool) -> Instance {
        Instance::open(&self.shared, &self.account, offline).await
    }
}

// ---------------------------------------------------------------------------
// AC1 — miss / hit / offline y grupo virtual sobre Dgraph.
// ---------------------------------------------------------------------------

/// La primera lectura baja del upstream y deja UN recibo; la segunda se
/// sirve como propia sin salir; el reinicio en modo offline sirve lo
/// verificado y rechaza lo desconocido sin tocar la red. La egress y el
/// scope no cambian: el host no autorizado sigue siendo 502 y la ruta
/// fuera de scope sigue siendo 404 sin preguntar al upstream.
#[tokio::test]
async fn proxy_miss_then_hit_and_offline_over_dgraph() {
    let Some(rig) = Rig::mount("ac1").await else {
        return;
    };
    let instance = rig.instance(false).await;

    // Miss: baja del upstream, se verifica, se publica y queda su recibo.
    let before = shared_lab::total();
    let answer = get(&instance.app, "pxy", "pkg/lib.bin").await;
    assert_eq!(answer.status, StatusCode::OK, "{:?}", answer.body);
    assert_eq!(answer.body, BODY.to_vec());
    assert_eq!(answer.header("x-hodei-cache"), Some("miss"));
    assert_eq!(shared_lab::total(), before + 1);

    let found = provenance(&instance.app, "pxy", "pkg/lib.bin").await;
    let receipts = found["receipts"].as_array().expect("receipts");
    assert_eq!(receipts.len(), 1, "{found}");
    assert_eq!(receipts[0]["outcome"], "fetched");
    assert_eq!(receipts[0]["upstream"], "lab");
    assert_eq!(receipts[0]["status"], 200);
    assert_eq!(
        receipts[0]["digest"],
        format!("sha256:{}", ContentDigest::of_bytes(BODY).value)
    );

    // Hit: la segunda lectura no sale a la red.
    let before = shared_lab::total();
    let answer = get(&instance.app, "pxy", "pkg/lib.bin").await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.header("x-hodei-cache"), Some("hit"));
    assert_eq!(shared_lab::total(), before);

    // La egress sigue igual: `localhost` no está en la allow-list, aunque
    // apunte al mismo puerto; el upstream nunca ve esa petición.
    let before = shared_lab::total();
    let answer = get(&instance.app, "pxy-ghost", "pkg/lib.bin").await;
    assert_eq!(answer.status, StatusCode::BAD_GATEWAY, "{:?}", answer.body);
    assert_eq!(shared_lab::total(), before);

    // El scope sigue acotado: fuera de `ok/` es 404 sin preguntar.
    let before = shared_lab::total();
    let answer = get(&instance.app, "pxy-scoped", "other/nope.txt").await;
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
    assert_eq!(shared_lab::total(), before);
    let answer = get(&instance.app, "pxy-scoped", "ok/inside.txt").await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.header("x-hodei-cache"), Some("miss"));

    // Reinicio en modo offline: la cache de proceso es nueva, el estado
    // durable (grafo + CAS) no. Lo verificado se sirve; lo desconocido es
    // 503; y la red no se toca.
    drop(instance);
    let offline = rig.instance(true).await;
    let before = shared_lab::total();
    let answer = get(&offline.app, "pxy", "pkg/lib.bin").await;
    assert_eq!(answer.status, StatusCode::OK, "{:?}", answer.body);
    assert_eq!(answer.body, BODY.to_vec());
    assert_eq!(answer.header("x-hodei-cache"), Some("offline"));
    let answer = get(&offline.app, "pxy", "never/seen.bin").await;
    assert_eq!(answer.status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        String::from_utf8_lossy(&answer.body).contains("offline"),
        "{:?}",
        answer.body
    );
    assert_eq!(shared_lab::total(), before);

    // El recibo de la ruta escopetada también quedó en el libro.
    let found = provenance(&offline.app, "pxy-scoped", "ok/inside.txt").await;
    assert_eq!(found["receipts"].as_array().expect("receipts").len(), 1);
}

/// El grupo virtual recorre los miembros en orden: gana el hosted cuando
/// los dos tienen la ruta, y el proxy sirve lo que el hosted no tiene. La
/// resolución no se amplía: el grupo no da permisos propios.
#[tokio::test]
async fn the_virtual_group_resolves_by_precedence_over_dgraph() {
    let Some(rig) = Rig::mount("grp").await else {
        return;
    };
    let instance = rig.instance(false).await;

    // El hosted no tiene la ruta: el miembro proxy la baja del upstream.
    let before = shared_lab::total();
    let answer = get(&instance.app, "grp", "pkg/lib.bin").await;
    assert_eq!(answer.status, StatusCode::OK, "{:?}", answer.body);
    assert_eq!(answer.body, BODY.to_vec());
    assert_eq!(answer.header("x-hodei-resolved-from"), Some("pxy"));
    assert_eq!(shared_lab::total(), before + 1);

    // Se publica la misma ruta en el hosted (precisamente la que también
    // sirve el upstream): a partir de aquí gana el hosted y el upstream
    // deja de verse molestado por esa ruta.
    let published = send(
        &instance.app,
        Method::PUT,
        "/repository/hos/shared/file.bin",
        Some(SHARED.to_vec()),
    )
    .await;
    assert_eq!(
        published.status,
        StatusCode::CREATED,
        "{:?}",
        published.body
    );

    let before = shared_lab::total();
    let answer = get(&instance.app, "grp", "shared/file.bin").await;
    assert_eq!(answer.status, StatusCode::OK, "{:?}", answer.body);
    assert_eq!(answer.body, SHARED.to_vec());
    assert_eq!(answer.header("x-hodei-resolved-from"), Some("hos"));
    assert_eq!(shared_lab::total(), before);
}

// ---------------------------------------------------------------------------
// AC2 — el recibo y su digest sobreviven al reinicio; no hay duplicidad.
// ---------------------------------------------------------------------------

/// Cuántos nodos hay bajo una clave de recibo: el censo es solo por las
/// claves propias de la prueba, nunca global (el alpha se comparte).
async fn receipt_copies(client: &GraphClient, key: &str) -> usize {
    let query = format!(
        "{{ q(func: eq(hodei.proxy.receipt.key, {key})) {{ uid }} }}",
        key = hodei_graph_client::schema::dql_string(key),
    );
    let response = client.execute_dql(&query).await.expect("the count answers");
    response
        .json
        .get("q")
        .and_then(serde_json::Value::as_array)
        .map(|nodes| nodes.len())
        .unwrap_or(0)
}

#[tokio::test]
async fn the_receipt_and_digest_survive_a_restart_without_duplication() {
    let Some(rig) = Rig::mount("ac2").await else {
        return;
    };

    // Vida uno: un miss normal deja el recibo en el grafo.
    let first = rig.instance(false).await;
    let answer = get(&first.app, "pxy", "pkg/lib.bin").await;
    assert_eq!(answer.status, StatusCode::OK);
    let found = provenance(&first.app, "pxy", "pkg/lib.bin").await;
    let receipts = found["receipts"].as_array().expect("receipts").clone();
    assert_eq!(receipts.len(), 1);
    let id = receipts[0]["id"].as_str().expect("id").to_string();
    let digest = receipts[0]["digest"].as_str().expect("digest").to_string();

    // El reinicio: cliente, plano y CAS locales nuevos sobre el MISMO
    // grafo y el MISMO CAS compartido.
    drop(first);
    let second = rig.instance(false).await;
    let before = shared_lab::total();
    let answer = get(&second.app, "pxy", "pkg/lib.bin").await;
    assert_eq!(answer.status, StatusCode::OK, "{:?}", answer.body);
    assert_eq!(answer.body, BODY.to_vec());
    assert_eq!(answer.header("x-hodei-cache"), Some("hit"));
    assert_eq!(shared_lab::total(), before);

    let found = provenance(&second.app, "pxy", "pkg/lib.bin").await;
    let after = found["receipts"].as_array().expect("receipts");
    assert_eq!(after.len(), 1, "el reinicio no duplica ni pierde: {found}");
    assert_eq!(after[0]["id"], id.as_str());
    assert_eq!(after[0]["digest"], digest.as_str());

    // `record` repetido del mismo recibo es replay: Ok y una sola fila.
    let durable = second
        .ledger
        .receipts_of(&rig.account, "pxy", "pkg/lib.bin")
        .await
        .expect("the durable receipts");
    assert_eq!(durable.len(), 1);
    second
        .ledger
        .record(&durable[0])
        .await
        .expect("the replay of the same id is fine");
    let again = second
        .ledger
        .receipts_of(&rig.account, "pxy", "pkg/lib.bin")
        .await
        .expect("the durable receipts");
    assert_eq!(again.len(), 1);
    assert_eq!(
        receipt_copies(&rig.shared.client, &receipt_key(&rig.account, &id)).await,
        1
    );

    // Mismo id en otra cuenta: la clave lleva la cuenta, así que no hay
    // colisión ni duplicidad entre repos de tenants distintos.
    let other = {
        let mut receipt = durable[0].clone();
        receipt.account = format!("hsp196_peer_{}", &nonce()[4..]);
        receipt
    };
    second
        .ledger
        .record(&other)
        .await
        .expect("the peer row stands");
    assert_eq!(
        second
            .ledger
            .receipts_of(&other.account, "pxy", "pkg/lib.bin")
            .await
            .expect("the peer receipts")
            .len(),
        1
    );
    assert_eq!(
        second
            .ledger
            .receipts_of(&rig.account, "pxy", "pkg/lib.bin")
            .await
            .expect("own receipts after the peer write")
            .len(),
        1
    );
    assert_eq!(
        receipt_copies(
            &rig.shared.client,
            &receipt_key(&other.account, durable[0].id.as_str())
        )
        .await,
        1
    );

    // El baseline por tenant: el sujeto de la ruta sigue con UNA fila,
    // la del recibo original.
    let subject = format!("{}/{}/{}", rig.account, "pxy", "pkg/lib.bin");
    let query = format!(
        "{{ q(func: eq(hodei.proxy.receipt.subject, {s})) {{ uid }} }}",
        s = hodei_graph_client::schema::dql_string(&subject),
    );
    let response = rig
        .shared
        .client
        .execute_dql(&query)
        .await
        .expect("the census");
    assert_eq!(
        response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .map(|nodes| nodes.len())
            .unwrap_or(0),
        1
    );
}
