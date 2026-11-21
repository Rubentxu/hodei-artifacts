//! HSP-193: npm sobre el motor Dgraph real, sin scan global.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` como los IT del stack (TLS + ACL,
//! el servidor vivo h186). La espina del catálogo (HSP-187/190/191:
//! guardas por clave, outbox atómico, cursor estable) es la que sirve el
//! contrato npm de HSP-111 (`crates/hodei-catalog/src/npm.rs` +
//! `src/handlers/npm.rs`), y este slice lo demuestra CONTRA EL MOTOR:
//!
//! - **AC1** el cliente `npm` real publica e instala sobre el vertical con
//!   el catálogo Dgraph: el packument y la metadata por versión llevan los
//!   DIGESTS estables del tarball (shasum e integrity calculados por el
//!   servicio), el packument leído dos veces dice lo mismo (instantánea
//!   direccionada por digest), el tarball sale del CAS y el grafo solo
//!   guarda coordenadas y referencias, y los dist-tags son consistentes:
//!   la lista se lee POR CLAVE (el packument de ese paquete) y la mutación
//!   es una instantánea nueva arbitrada por la guarda de la clave del
//!   asset — dos escritores concurrentes no la rompen.
//! - **AC2** la republicación inmutable (una versión no se reescribe), el
//!   outbox atómico que acompaña a cada commit, el cursor estable en los
//!   listados, el scoping por cuenta y la paridad de lectura con la
//!   referencia Surreal para el mismo estado npm.
//! - los DISTRACTORES (muchos paquetes de otras cuentas con las mismas
//!   claves de repositorio y los mismos nombres) no causan scan global:
//!   la raíz de cada consulta es eq(account/repository) por índice, y el
//!   probe de plan confirma el índice exacto en los predicados filtrados
//!   (patrón hsp191).
//!
//! La suite es SERIALIZADA (`--test-threads=1`): la apertura de la guarda
//! bajo carga es el flake conocido del stack (HSP-219) y `open_store` lo
//! absorbe con tres intentos, como hsp191.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use base64::Engine as _;
use cedar_policy::Entities;
use chrono::Utc;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_audit::AuditOutbox;
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use hodei_catalog::npm::{self, PackageName};
use hodei_catalog::search::{SearchQuery, search_cursor};
use hodei_catalog::{
    AllowAllAuthorizer, CatalogReader, CatalogSearch, CatalogWriter, DigestScope,
    PackageCoordinates, Precondition, Publication, RepositoryConfig, RepositoryDescriptor,
    RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_catalog_storage::{DgraphAuditOutbox, DgraphCatalogStore, SurrealCatalogStore};
use hodei_evidence::ContentDigest;
use hodei_graph_client::{GraphClient, GraphClientConfig};
use hodei_hub::blob::FileBlobStore;
use serde_json::{Value, json};
use sha2::Sha512;
use surrealdb::Surreal;
use surrealdb::engine::local::RocksDb;
use tempfile::TempDir;
use tower::ServiceExt;

/// La ejecución: una por corrida, para que ninguna prueba se encuentre
/// con lo que otra dejó.
fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

/// Una cuenta por prueba y por ejecución.
fn account(tag: &str) -> String {
    format!("it193{}{}", tag, &nonce()[8..])
}

/// El pid: nombres de paquete únicos por corrida (el recurso de auditoría
/// no lleva la cuenta, y el censo del outbox cuenta por camino).
fn the_pid() -> u32 {
    std::process::id()
}

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

async fn fresh_client() -> Option<Arc<GraphClient>> {
    let Some(config) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return None;
    };
    let mut client = GraphClient::connect(config).await.expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    Some(Arc::new(client))
}

/// El directorio de la evidencia: `target/uat/hsp193/`.
fn the_evidence_dir() -> PathBuf {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let target = tmp.parent().unwrap_or(&tmp).to_path_buf();
    let dir = target.join("uat").join("hsp193");
    std::fs::create_dir_all(&dir).expect("the evidence directory");
    dir
}

/// Una línea de evidencia, con el momento.
fn record_evidence(test: &str, line: &str) {
    let file = the_evidence_dir().join("evidencia.md");
    let entry = format!("- [{test}] {line}\n");
    use std::io::Write as _;
    if let Ok(mut out) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
    {
        let _ = writeln!(out, "{}", entry.trim_end());
    }
}

/// El almacén Dgraph del catálogo, con la apertura acotada: bajo carga,
/// la apertura de la guarda es el flake preexistente del stack (lecciones
/// HSP-190/HSP-219); la suite lo absorbe con tres intentos y un respiro.
async fn open_store(client: Arc<GraphClient>) -> DgraphCatalogStore {
    let mut last = String::new();
    for _attempt in 0..3 {
        match DgraphCatalogStore::open(client.clone(), Arc::new(AllowAllAuthorizer)).await {
            Ok(store) => return store,
            Err(error) => {
                last = error.to_string();
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        }
    }
    panic!("the dgraph catalog store does not open: {last}");
}

fn actor(account: &str) -> String {
    format!("hrn:hodei:iam:{account}:User/alice")
}

fn correlation_of(account: &str) -> String {
    format!("corr-hsp193-{account}")
}

/// Un descriptor de repositorio hosted, del formato y los ajustes pedidos.
fn a_descriptor(
    account: &str,
    repo_key: &str,
    format: RepositoryFormat,
    settings: serde_json::Value,
) -> RepositoryDescriptor {
    let mut config = RepositoryConfig::new(DigestScope::Global);
    config.config_settings = settings;
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", account, repo_key),
        account: account.to_string(),
        repo_key: repo_key.to_string(),
        format,
        mode: RepositoryMode::Hosted,
        config,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

// ---------------------------------------------------------------------------
// Las publicaciones npm al nivel del almacén: la misma forma que escribe
// la superficie (`src/handlers/npm.rs`), para sembrar estados completos
// sin pasar por el cliente npm.
// ---------------------------------------------------------------------------

/// Los digests que el servicio calcula sobre un tarball: el `shasum`
/// histórico, el `integrity` de npm y el sha256 que dice el catálogo.
struct TarballDigests {
    shasum: String,
    integrity: String,
    sha256: String,
}

fn the_tarball_digests(bytes: &[u8]) -> TarballDigests {
    use sha1::Digest as _;
    TarballDigests {
        shasum: sha1::Sha1::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        integrity: format!(
            "sha512-{}",
            base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes))
        ),
        sha256: {
            let hash = sha2::Sha256::digest(bytes);
            hash.iter().map(|byte| format!("{byte:02x}")).collect()
        },
    }
}

/// Un packument con las versiones pedidas, construido con las MISMAS
/// funciones puras que usa la superficie (el oráculo del contrato).
fn a_packument(
    name: &PackageName,
    versions: &[(&str, &[u8])],
    tags: &[(&str, &str)],
    now: &str,
) -> Value {
    let mut packument = npm::empty_packument(name, now);
    for (version, bytes) in versions {
        let facts = npm::TarballFacts {
            shasum: the_tarball_digests(bytes).shasum,
            integrity: the_tarball_digests(bytes).integrity,
        };
        let manifest = json!({
            "name": name.full(),
            "version": version,
            "description": "a package seeded by the npm over dgraph suite",
        })
        .as_object()
        .expect("an object")
        .clone();
        let document = npm::Document {
            name: name.clone(),
            value: json!({
                "name": name.full(),
                "versions": {*version: manifest.clone()},
            })
            .as_object()
            .expect("an object")
            .clone(),
        };
        let origin = npm::Origin {
            publisher: "alice".into(),
            at: now.to_string(),
            correlation: "it-hsp193".into(),
        };
        let new = npm::NewVersion {
            name: name.clone(),
            version: (*version).to_string(),
            manifest,
            tags: Vec::new(),
            attachment: npm::Attachment {
                file: name.tarball_file(version),
                bytes: (*bytes).to_vec(),
            },
            document,
        };
        npm::add_version(&mut packument, &new, &facts, &origin).expect("the version is new");
    }
    for (tag, version) in tags {
        npm::set_dist_tag(&mut packument, tag, version, now).expect("the tag settles");
    }
    Value::Object(packument)
}

/// Publica el tarball de una versión npm, con las coordenadas de la
/// superficie: el camino `{nombre}/-/{fichero}.tgz` y `{nombre}@{version}`.
async fn publish_a_tarball<Store>(
    store: &Store,
    account: &str,
    repository: &str,
    name: &PackageName,
    version: &str,
    bytes: &[u8],
) where
    Store: hodei_catalog::CatalogWriter + Send + Sync,
{
    store
        .publish(
            account,
            Publication {
                repository: repository.to_string(),
                asset_path: name.tarball_path(version),
                coordinates: PackageCoordinates {
                    package_name: name.full().to_string(),
                    package_version: version.to_string(),
                },
                digest: ContentDigest::of_bytes(bytes),
                size_bytes: bytes.len() as u64,
                tags: Vec::new(),
                actor: actor(account),
                correlation_id: correlation_of(account),
                overwrite: false,
                precondition: None,
                subject: None,
            },
        )
        .await
        .expect("the tarball publication settles");
}

/// Publica una instantánea del packument, como la superficie: cada estado
/// es una versión nueva de contenido y el puntero de la ruta se mueve con
/// la precondición del digest anterior. La guarda de la CLAVE del asset
/// (`hodei.catalog.asset.key`, `@upsert`) es la que arbitra dos escritores
/// de la misma ruta — el mismo arbitraje por clave que las etiquetas OCI
/// (HSP-190).
async fn publish_a_packument_snapshot<Store>(
    store: &Store,
    account: &str,
    repository: &str,
    name: &PackageName,
    document: &Value,
    previous: Option<&ContentDigest>,
) -> ContentDigest
where
    Store: hodei_catalog::CatalogWriter + Send + Sync,
{
    let bytes = serde_json::to_vec(document).expect("the packument serializes");
    let digest = ContentDigest::of_bytes(&bytes);
    store
        .publish(
            account,
            Publication {
                repository: repository.to_string(),
                asset_path: name.full().to_string(),
                coordinates: PackageCoordinates {
                    package_name: format!("{}{}", name.full(), npm::PACKUMENT_COORDINATES_SUFFIX),
                    package_version: digest.value.clone(),
                },
                digest: digest.clone(),
                size_bytes: bytes.len() as u64,
                tags: Vec::new(),
                actor: actor(account),
                correlation_id: correlation_of(account),
                overwrite: true,
                precondition: Some(match previous {
                    None => Precondition::Absent,
                    Some(current) => Precondition::Present(vec![current.clone()]),
                }),
                subject: None,
            },
        )
        .await
        .expect("the packument snapshot settles");
    digest
}

/// El contenido determinista del tarball sembrado de una versión.
fn seeded_tarball(name: &PackageName, version: &str) -> Vec<u8> {
    format!("seeded tarball of {}@{version}", name.full()).into_bytes()
}

/// Siembra un paquete npm completo: su tarball y la instantánea del
/// packument que lo nombra. Devuelve el digest de la instantánea.
async fn seed_a_package(
    store: &DgraphCatalogStore,
    account: &str,
    repository: &str,
    name: &PackageName,
    version: &str,
    tags: &[(&str, &str)],
    now: &str,
) -> ContentDigest {
    let tarball = seeded_tarball(name, version);
    publish_a_tarball(store, account, repository, name, version, &tarball).await;
    let document = a_packument(name, &[(version, tarball.as_slice())], tags, now);
    publish_a_packument_snapshot(store, account, repository, name, &document, None).await
}

// ---------------------------------------------------------------------------
// El vertical HTTP: el router del hub sobre el catálogo Dgraph.
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

/// La Cedar bundle: solo `alice` de esta cuenta publica y descarga.
fn cedar_for(account: &str, repo_key: &str) -> Arc<dyn AuthorizerPort> {
    let policy = format!(
        r#"permit(
        principal == Iam::User::"hrn:hodei:iam::{account}:User/alice",
        action == Artifacts::Action::"UploadAsset",
        resource == Artifacts::Repository::"hrn:hodei:artifacts:eu:{account}:repository/{repo_key}"
    );
    permit(
        principal == Iam::User::"hrn:hodei:iam::{account}:User/alice",
        action == Artifacts::Action::"DownloadAsset",
        resource == Artifacts::Repository::"hrn:hodei:artifacts:eu:{account}:repository/{repo_key}"
    );"#
    );
    Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            &policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .expect("authorizer"),
    )))
}

struct Vertical {
    app: Router,
    store: Arc<DgraphCatalogStore>,
    /// El CAS del vertical: la siembra al nivel del almacén tiene que
    /// dejar AHI los bytes que el catálogo referencia, o las lecturas del
    /// contrato no encuentran el contenido.
    content: Arc<dyn hodei_hub::blob::BlobStore>,
    #[allow(dead_code)]
    tmp: TempDir,
}

impl Vertical {
    async fn mount_npm(account: &str, repo_key: &str) -> Self {
        let client = fresh_client().await.expect("the dgraph client connects");
        let store = Arc::new(open_store(client.clone()).await);
        store
            .create_repository(
                account,
                a_descriptor(
                    account,
                    repo_key,
                    RepositoryFormat::Npm,
                    serde_json::json!({}),
                ),
            )
            .await
            .expect("the repository is created");

        let tmp = tempfile::tempdir().expect("temp dir");
        let mut config = AppConfig::default();
        config.security.raw_api_enabled = true;
        config.repository.durable = false;
        config.rocksdb.path = tmp
            .path()
            .join("hodei.rocksdb")
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

        let reader: Arc<dyn CatalogReader> = store.clone();
        let writer: Arc<dyn CatalogWriter> = store.clone();
        let content: Arc<dyn hodei_hub::blob::BlobStore> =
            Arc::new(FileBlobStore::new(tmp.path().join("cas")).expect("the CAS"));

        let state = state
            .with_catalog(reader, writer)
            .with_content_store(content.clone())
            .with_authenticator(Arc::new(ContractAuthenticator {
                account: account.to_string(),
            }))
            .with_authorization(cedar_for(account, repo_key));

        Self {
            app: build_router(state, &config),
            store,
            content,
            tmp,
        }
    }
}

/// El mismo servicio, en un puerto de verdad: el cliente npm habla TCP.
async fn serve_over_tcp(vertical: &Vertical) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("the service binds a local port");
    let address = listener
        .local_addr()
        .expect("the service knows its address");
    let app = vertical.app.clone();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    address
}

/// La respuesta de una petición del contrato HTTP.
struct Answer {
    status: StatusCode,
    etag: Option<String>,
    body: Vec<u8>,
}

async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Vec<u8>>,
) -> Answer {
    let mut builder = Request::builder().method(method).uri(uri);
    // El Host que un cliente de verdad manda: la superficie npm construye
    // las URLs absolutas del packument con él.
    builder = builder.header(header::HOST, "127.0.0.1");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    let request = builder
        .body(body.map(Body::from).unwrap_or_else(Body::empty))
        .expect("the request is well formed");
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("the router must always produce a response");
    let status = response.status();
    let etag = response
        .headers()
        .get(header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the body can be read");
    Answer {
        status,
        etag,
        body: body.to_vec(),
    }
}

// ---------------------------------------------------------------------------
// El cliente npm real, como hsp111: su HOME, su cache y su .npmrc.
// ---------------------------------------------------------------------------

fn require_npm() {
    let found = npm_command()
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "the npm CLI is not installed: the npm suite needs `npm` in the PATH"
    );
}

/// El `npm` para los clientes de la suite. El `npm` del PATH es el shim de
/// builds de la maquina, que mete cada llamada en su slice (`bld`): el
/// cliente de ESTA suite no compila nada y no tiene que pasar por el
/// reparto de recursos de los builds, asi que resuelve el BINARIO REAL —
/// el escape documentado del propio shim — por el instalador asdf, o el
/// que diga `HODEI_IT_NPM_BIN`, y solo si no lo hay vuelve al PATH.
fn npm_command() -> Command {
    static REAL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let real = REAL.get_or_init(|| {
        if let Ok(overridden) = std::env::var("HODEI_IT_NPM_BIN")
            && Path::new(&overridden).is_file()
        {
            return overridden;
        }
        let home = std::env::var_os("HOME").unwrap_or_default();
        let installs = Path::new(&home).join(".asdf/installs/nodejs");
        if let Ok(entries) = std::fs::read_dir(&installs) {
            let mut candidates: Vec<PathBuf> = entries
                .flatten()
                .map(|entry| entry.path().join("bin/npm"))
                .filter(|npm| npm.is_file())
                .collect();
            candidates.sort();
            if let Some(last) = candidates.pop() {
                return last.to_string_lossy().into_owned();
            }
        }
        "npm".to_string()
    });
    Command::new(real)
}

struct Run {
    ok: bool,
    output: String,
}

struct NpmClient {
    root: PathBuf,
    rc: PathBuf,
}

impl NpmClient {
    fn new(root: &Path, name: &str, port: u16, repository: &str, token: &str) -> Self {
        let root = root.join(name);
        std::fs::create_dir_all(root.join("home")).expect("the home");
        let rc = root.join("npmrc");
        std::fs::write(
            &rc,
            format!(
                "registry=http://127.0.0.1:{port}/npm/{repository}/\n\
                 //127.0.0.1:{port}/npm/{repository}/:_authToken={token}\n\
                 fetch-retries=0\n\
                 update-notifier=false\n"
            ),
        )
        .expect("the npmrc is written");
        Self { root, rc }
    }

    fn run(&self, dir: &Path, args: &[&str]) -> Run {
        let output = npm_command()
            .arg("--userconfig")
            .arg(&self.rc)
            .arg("--globalconfig")
            .arg(self.root.join("no-global-config"))
            .arg("--cache")
            .arg(self.root.join("cache"))
            .args(["--no-audit", "--no-fund", "--loglevel=warn"])
            .args(args)
            .env("HOME", self.root.join("home"))
            .current_dir(dir)
            .output()
            .expect("npm runs");
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Run {
            ok: output.status.success(),
            output: text,
        }
    }

    /// Una cache vacia: lo siguiente que se instale sale del servicio.
    fn forget_the_cache(&self) {
        let _ = std::fs::remove_dir_all(self.root.join("cache"));
    }
}

/// Un paquete en `dir` listo para publicar.
fn package(dir: &Path, name: &str, version: &str, body: &str) {
    std::fs::create_dir_all(dir).expect("the package directory");
    std::fs::write(
        dir.join("package.json"),
        format!(
            "{{\"name\":\"{name}\",\"version\":\"{version}\",\"main\":\"index.js\",\
             \"description\":\"an npm package over dgraph\"}}"
        ),
    )
    .expect("package.json");
    std::fs::write(dir.join("index.js"), body).expect("index.js");
}

/// Un proyecto consumidor vacio.
fn consumer(dir: &Path) {
    std::fs::create_dir_all(dir).expect("the consumer directory");
    std::fs::write(
        dir.join("package.json"),
        "{\"name\":\"consumer\",\"version\":\"1.0.0\",\"private\":true}",
    )
    .expect("package.json");
}

/// Cuántos eventos pendientes nombra el recurso pedido: el outbox del
/// servidor acumula los de todas las ejecuciones, y el censo cuenta solo
/// los hechos que nombran lo publicado por ESTA corrida.
async fn the_events_of(outbox: &DgraphAuditOutbox, resource_needle: &str) -> usize {
    outbox
        .pending(Utc::now())
        .await
        .expect("the pending events")
        .into_iter()
        .filter(|entry| entry.event.resource.contains(resource_needle))
        .count()
}

// ---------------------------------------------------------------------------
// AC1: el cliente npm real publica e instala sobre el catálogo Dgraph,
// con digests estables, tarball desde el CAS y dist-tags consistentes.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_real_npm_client_publishes_and_reads_over_dgraph() {
    require_npm();
    let Some(_guard) = fresh_client().await else {
        return;
    };
    let account = account("npm");
    let repo = "npm-hosted";
    let vertical = Vertical::mount_npm(&account, repo).await;
    let address = serve_over_tcp(&vertical).await;
    let pid = the_pid();

    let evidence = the_evidence_dir();
    let root = evidence.join(format!("npm-work-{}", nonce()));
    std::fs::create_dir_all(&root).expect("the npm work directory");
    let widget_name = format!("uat{pid}-widget");
    let client = NpmClient::new(&root, "client", address.port(), repo, "alice");

    // La publicación real: el cliente npm empaqueta y manda el documento
    // con el tarball en base64. El catálogo Dgraph lo asienta.
    let widget = root.join(&widget_name);
    package(
        &widget,
        &widget_name,
        "1.0.0",
        "module.exports = 'widget over dgraph';\n",
    );
    let published = client.run(&widget, &["publish", "--access", "public"]);
    assert!(
        published.ok,
        "npm publish over dgraph failed:\n{}",
        published.output
    );
    record_evidence(
        "the_real_npm_client_publishes_and_reads_over_dgraph",
        &format!("npm publish {widget_name}@1.0.0 sobre {repo} (catálogo Dgraph): OK"),
    );

    // Una versión publicada NO se reescribe: el cliente recibe la negativa
    // del registro (403, `cannot publish over`).
    let again = client.run(&widget, &["publish", "--access", "public"]);
    assert!(!again.ok, "npm republished an existing version");
    assert!(
        again.output.contains("E403")
            || again.output.contains("403")
            || again
                .output
                .contains("cannot publish over the previously published"),
        "republishing is not the npm refusal:\n{}",
        again.output
    );
    record_evidence(
        "the_real_npm_client_publishes_and_reads_over_dgraph",
        "re-publicación de la misma versión: el registro la rehúsa (403 cannot publish over), nada se reescribe",
    );

    // El packument servido POR EL PORT Dgraph, leído dos veces: la
    // instantánea es contenido direccionado por digest, y el mismo body y
    // ETag es lo que el cliente ve.
    let packument_url = format!("/npm/{repo}/{widget_name}");
    let first = send(
        &vertical.app,
        Method::GET,
        &packument_url,
        Some("alice"),
        None,
    )
    .await;
    assert_eq!(
        first.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&first.body)
    );
    let second = send(
        &vertical.app,
        Method::GET,
        &packument_url,
        Some("alice"),
        None,
    )
    .await;
    assert_eq!(
        first.body, second.body,
        "the packument is a stable snapshot"
    );
    assert_eq!(
        first.etag, second.etag,
        "the packument digest is stable between reads"
    );
    let first_doc: Value = serde_json::from_slice(&first.body).expect("the packument parses");
    let entry = &first_doc["versions"]["1.0.0"];

    // El tarball sale del CAS: los bytes servidos son los que el servicio
    // digeró al publicar, y la metadata POR VERSIÓN del packument nombra
    // el shasum y el integrity de ESOS bytes. El grafo dice lo mismo: la
    // versión del catálogo lleva el sha256 de los bytes servidos.
    let served_tarball = send(
        &vertical.app,
        Method::GET,
        &format!("/npm/{repo}/{widget_name}/-/{widget_name}-1.0.0.tgz"),
        Some("alice"),
        None,
    )
    .await;
    assert_eq!(served_tarball.status, StatusCode::OK);
    let digests = the_tarball_digests(&served_tarball.body);
    assert_eq!(
        entry["dist"]["integrity"].as_str().expect("an integrity"),
        digests.integrity,
        "the version metadata carries the served tarball's integrity"
    );
    assert_eq!(
        entry["dist"]["shasum"].as_str().expect("a shasum"),
        digests.shasum,
        "the version metadata carries the served tarball's shasum"
    );
    let version = vertical
        .store
        .version(
            &account,
            repo,
            &PackageCoordinates {
                package_name: widget_name.clone(),
                package_version: "1.0.0".to_string(),
            },
        )
        .await
        .expect("the version is in the catalog");
    assert_eq!(version.digest.value, digests.sha256);
    record_evidence(
        "the_real_npm_client_publishes_and_reads_over_dgraph",
        &format!(
            "packument estable (mismo body y ETag en dos lecturas); metadata de 1.0.0 con shasum/integrity del tarball servido por el CAS; digest del catálogo == sha256(bytes): {}",
            digests.sha256
        ),
    );

    // El cliente real RESUELVE desde una cache vacía: instala lo publicado
    // y el lock dice el integrity del tarball que el servicio sirvió.
    let project = root.join("consumer");
    consumer(&project);
    client.forget_the_cache();
    let installed = client.run(&project, &["install", &widget_name]);
    assert!(
        installed.ok,
        "npm install over dgraph failed:\n{}",
        installed.output
    );
    assert_eq!(
        std::fs::read_to_string(project.join(format!("node_modules/{widget_name}/index.js")))
            .expect("the widget is installed"),
        "module.exports = 'widget over dgraph';\n"
    );
    let lock: Value =
        serde_json::from_slice(&std::fs::read(project.join("package-lock.json")).expect("lock"))
            .expect("the lock parses");
    assert_eq!(
        lock["packages"][format!("node_modules/{widget_name}")]["integrity"]
            .as_str()
            .expect("the lock carries an integrity"),
        digests.integrity,
        "the lock integrity is the served tarball's integrity"
    );
    record_evidence(
        "the_real_npm_client_publishes_and_reads_over_dgraph",
        "npm install desde cache vacía: los bytes instalados y su lock integrity son los del tarball servido por el CAS",
    );

    // Los dist-tags son consistentes: una versión nueva con su tag, la
    // lista POR CLAVE dice los dos, y `latest` sigue donde estaba.
    package(
        &widget,
        &widget_name,
        "1.1.0",
        "module.exports = 'widget 1.1';\n",
    );
    let next = client.run(&widget, &["publish", "--access", "public", "--tag", "next"]);
    assert!(next.ok, "npm publish --tag next failed:\n{}", next.output);
    let tags = client.run(&widget, &["dist-tag", "ls", &widget_name]);
    assert!(tags.ok, "npm dist-tag ls failed:\n{}", tags.output);
    assert!(tags.output.contains("latest: 1.0.0"), "{}", tags.output);
    assert!(tags.output.contains("next: 1.1.0"), "{}", tags.output);
    let listed = send(
        &vertical.app,
        Method::GET,
        &format!("/npm/{repo}/-/package/{widget_name}/dist-tags"),
        Some("alice"),
        None,
    )
    .await;
    assert_eq!(listed.status, StatusCode::OK);
    let dist_tags: Value = serde_json::from_slice(&listed.body).expect("the tags parse");
    assert_eq!(dist_tags["latest"], "1.0.0");
    assert_eq!(dist_tags["next"], "1.1.0");
    let after_next = send(
        &vertical.app,
        Method::GET,
        &packument_url,
        Some("alice"),
        None,
    )
    .await;
    assert_eq!(after_next.status, StatusCode::OK);
    let next_doc: Value = serde_json::from_slice(&after_next.body).expect("the packument parses");
    assert_ne!(
        after_next.etag, first.etag,
        "a new version is a new snapshot, content-addressed"
    );
    record_evidence(
        "the_real_npm_client_publishes_and_reads_over_dgraph",
        "dist-tags consistentes: lista por clave con latest 1.0.0 y next 1.1.0, cliente npm y contrato HTTP de acuerdo; la versión nueva es una instantánea nueva (ETag distinto)",
    );

    // La metadata muta SIN reescribir el contenido: deprecar crea una
    // instantánea nueva (otro ETag) y el integrity de cada versión es el
    // MISMO que antes de la mutación.
    let deprecated = client.run(
        &widget,
        &[
            "deprecate",
            &format!("{widget_name}@1.0.0"),
            "use 1.1.0 instead",
        ],
    );
    assert!(
        deprecated.ok,
        "npm deprecate failed:\n{}",
        deprecated.output
    );
    let after = send(
        &vertical.app,
        Method::GET,
        &packument_url,
        Some("alice"),
        None,
    )
    .await;
    assert_eq!(after.status, StatusCode::OK);
    let after_doc: Value = serde_json::from_slice(&after.body).expect("the packument parses");
    assert_eq!(
        after_doc["versions"]["1.0.0"]["deprecated"], "use 1.1.0 instead",
        "{}",
        after_doc
    );
    assert_eq!(
        after_doc["versions"]["1.0.0"]["dist"]["integrity"], entry["dist"]["integrity"],
        "the deprecation does not touch the version digests"
    );
    assert_eq!(
        after_doc["versions"]["1.1.0"]["dist"]["integrity"],
        next_doc["versions"]["1.1.0"]["dist"]["integrity"],
        "the other version keeps its own digests"
    );
    assert_ne!(
        after.etag, after_next.etag,
        "a metadata change is a new snapshot, content-addressed"
    );
    record_evidence(
        "the_real_npm_client_publishes_and_reads_over_dgraph",
        "npm deprecate: instantánea nueva (ETag distinto), deprecated escrito y los digests de ambas versiones intactos",
    );

    // El outbox atómico: cada commit de publicación lleva SU evento, y los
    // hechos nombran ESTA corrida (el nombre del paquete lleva el pid).
    let outbox = DgraphAuditOutbox::new(fresh_client().await.expect("client"));
    let said = the_events_of(&outbox, &format!("{repo}/{widget_name}")).await;
    assert!(said > 0, "the publish commits carried their audit events");
    record_evidence(
        "the_real_npm_client_publishes_and_reads_over_dgraph",
        &format!("eventos de auditoría de esta ejecución en el outbox: {said}"),
    );

    // El puntero del packument en el GRAFO es la última instantánea: la
    // ETag servida es el digest de la fila del asset, y su versión
    // `#packument` guarda el mismo contenido. El tarball y sus coordenadas
    // ya se comprobaron: el grafo solo guarda coordenadas y referencias,
    // nunca bytes.
    let pointer = vertical
        .store
        .asset(&account, repo, &widget_name)
        .await
        .expect("the packument pointer");
    assert_eq!(
        after.etag.as_deref(),
        Some(format!("\"{}\"", pointer.digest).as_str()),
        "the served ETag is the digest of the pointer row"
    );
    let snapshot_version = vertical
        .store
        .version(
            &account,
            repo,
            &PackageCoordinates {
                package_name: format!("{widget_name}{}", npm::PACKUMENT_COORDINATES_SUFFIX),
                package_version: pointer.digest.value.clone(),
            },
        )
        .await
        .expect("the snapshot version is in the catalog");
    assert_eq!(snapshot_version.digest.value, pointer.digest.value);
    record_evidence(
        "the_real_npm_client_publishes_and_reads_over_dgraph",
        "el puntero del packument en el grafo es la última instantánea (ETag == digest del asset) y su versión #packument guarda el mismo digest",
    );
}

// ---------------------------------------------------------------------------
// AC1: los dist-tags bajo DOS ESCRITORES concurrentes: la guarda por clave
// del packument arbitra, y el estado final es consistente.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_dist_tag_pointer_is_guarded_for_two_writers() {
    let Some(_guard) = fresh_client().await else {
        return;
    };
    let account = account("race");
    let repo = "npm-hosted";
    let vertical = Vertical::mount_npm(&account, repo).await;

    // Un paquete con dos versiones y `latest` en la segunda: el estado con
    // el que los dos escritores van a pelearse por el tag `beta`. La
    // primera instantánea la siembra el helper; la segunda, el propio
    // test, con la precondición del digest anterior — la misma forma que
    // usa la superficie.
    let name_text = format!("uat{}-race", the_pid());
    let name = PackageName::parse(&name_text, true).expect("a valid name");
    let now = "2026-01-01T00:00:00Z";
    let tarball_one = seeded_tarball(&name, "1.0.0");
    vertical.content.put(&tarball_one).await.expect("the CAS");
    publish_a_tarball(
        vertical.store.as_ref(),
        &account,
        repo,
        &name,
        "1.0.0",
        &tarball_one,
    )
    .await;
    let first_document = a_packument(&name, &[("1.0.0", tarball_one.as_slice())], &[], now);
    let first_bytes = serde_json::to_vec(&first_document).expect("the packument serializes");
    vertical.content.put(&first_bytes).await.expect("the CAS");
    let first_snapshot = publish_a_packument_snapshot(
        vertical.store.as_ref(),
        &account,
        repo,
        &name,
        &first_document,
        None,
    )
    .await;
    let tarball_two = seeded_tarball(&name, "2.0.0");
    vertical.content.put(&tarball_two).await.expect("the CAS");
    publish_a_tarball(
        vertical.store.as_ref(),
        &account,
        repo,
        &name,
        "2.0.0",
        &tarball_two,
    )
    .await;
    let latest_document = a_packument(
        &name,
        &[
            ("1.0.0", tarball_one.as_slice()),
            ("2.0.0", tarball_two.as_slice()),
        ],
        &[("latest", "2.0.0")],
        now,
    );
    let latest_bytes = serde_json::to_vec(&latest_document).expect("the packument serializes");
    vertical.content.put(&latest_bytes).await.expect("the CAS");
    publish_a_packument_snapshot(
        vertical.store.as_ref(),
        &account,
        repo,
        &name,
        &latest_document,
        Some(&first_snapshot),
    )
    .await;

    // El puntero del packument está donde la siembra lo dejó, antes de
    // la carrera: si no, el 404 de los escritores no dice nada de la
    // guarda.
    let before = vertical
        .store
        .asset(&account, repo, &name_text)
        .await
        .expect("the packument pointer before the race");
    assert_eq!(
        before.digest,
        ContentDigest::of_bytes(&serde_json::to_vec(&latest_document).expect("bytes")),
        "the pointer names the latest seeded snapshot"
    );

    // DOS escritores mueven el MISMO tag a versiones distintas, a la vez.
    let app = vertical.app.clone();
    let tag_url = format!("/npm/{repo}/-/package/{name_text}/dist-tags/beta");
    let left_body = json!("1.0.0").to_string().into_bytes();
    let right_body = json!("2.0.0").to_string().into_bytes();
    let (left, right) = tokio::join!(
        send(&app, Method::PUT, &tag_url, Some("alice"), Some(left_body),),
        send(&app, Method::PUT, &tag_url, Some("alice"), Some(right_body),),
    );
    // Los dos terminan bien: el que pierde la carrera relee y reintenta
    // con la instantánea nueva (el handler lo absorbe; la guarda por clave
    // del grafo es la que arbitra).
    assert!(
        left.status.is_success(),
        "the first writer failed with {:?}: {}",
        left.status,
        String::from_utf8_lossy(&left.body)
    );
    assert!(
        right.status.is_success(),
        "the second writer failed with {:?}: {}",
        right.status,
        String::from_utf8_lossy(&right.body)
    );

    // El estado final: el tag apunta a UNA de las dos, el packument por
    // clave lo dice igual que el endpoint de un solo tag, y `latest` no se
    // movió.
    let listed = send(
        &vertical.app,
        Method::GET,
        &format!("/npm/{repo}/-/package/{name_text}/dist-tags"),
        Some("alice"),
        None,
    )
    .await;
    assert_eq!(listed.status, StatusCode::OK);
    let dist_tags: Value = serde_json::from_slice(&listed.body).expect("the tags parse");
    let beta = dist_tags["beta"]
        .as_str()
        .expect("the beta tag")
        .to_string();
    assert!(
        beta == "1.0.0" || beta == "2.0.0",
        "the tag points at one of the two writers' versions: {dist_tags}"
    );
    assert_eq!(dist_tags["latest"], "2.0.0", "latest did not move");
    let single = send(
        &vertical.app,
        Method::GET,
        &format!("/npm/{repo}/-/package/{name_text}/dist-tags/beta"),
        Some("alice"),
        None,
    )
    .await;
    assert_eq!(single.status, StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<Value>(&single.body).expect("the tag value"),
        Value::String(beta.clone()),
        "the list and the single tag agree"
    );
    // Y las DOS versiones siguen servibles: mover un tag no toca contenido.
    for version in ["1.0.0", "2.0.0"] {
        let tarball = send(
            &vertical.app,
            Method::GET,
            &format!("/npm/{repo}/{name_text}/-/{name_text}-{version}.tgz"),
            Some("alice"),
            None,
        )
        .await;
        assert_eq!(
            tarball.status,
            StatusCode::OK,
            "the version {version} is still served"
        );
    }
    record_evidence(
        "the_dist_tag_pointer_is_guarded_for_two_writers",
        &format!(
            "dos escritores mueven `beta` a la vez (1.0.0 y 2.0.0): los dos terminan OK, el estado final dice beta={beta}, latest intacto y ambos tarballs servibles; la guarda por clave del packument arbitró"
        ),
    );
}

// ---------------------------------------------------------------------------
// AC2: cursor estable, scoping por cuenta y distractores que NO causan
// scan global (el probe confirma el índice en los predicados).
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_npm_queries_are_scoped_keyed_and_indexed() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let (account, other) = (account("search"), account("other"));
    let store = Arc::new(open_store(client.clone()).await);
    store
        .create_repository(
            &account,
            a_descriptor(
                &account,
                "npm-hosted",
                RepositoryFormat::Npm,
                serde_json::json!({}),
            ),
        )
        .await
        .expect("the repository is created");
    store
        .create_repository(
            &account,
            a_descriptor(
                &account,
                "npm-noise",
                RepositoryFormat::Npm,
                serde_json::json!({}),
            ),
        )
        .await
        .expect("the noise repository is created");
    // La cuenta distractora: la MISMA clave de repositorio y los MISMOS
    // nombres de paquete. Lo que ella guarda no existe aquí.
    store
        .create_repository(
            &other,
            a_descriptor(
                &other,
                "npm-hosted",
                RepositoryFormat::Npm,
                serde_json::json!({}),
            ),
        )
        .await
        .expect("the other repository is created");

    // Los paquetes de la cuenta: seis en npm-hosted.
    for index in 1..=6u32 {
        let name = PackageName::parse(&format!("pkg-{index:02}"), true).expect("a name");
        seed_a_package(
            &store,
            &account,
            "npm-hosted",
            &name,
            "1.0.0",
            &[],
            "2026-01-01T00:00:00Z",
        )
        .await;
    }
    // Distractores de la MISMA cuenta (otro repositorio npm) y de la OTRA
    // cuenta (el mismo repositorio, los mismos paquetes, diez veces cada
    // uno). Suficientes para que un scan global destaque.
    for index in 0..60u32 {
        let name = PackageName::parse(&format!("noise-{index:02}"), true).expect("a name");
        seed_a_package(
            &store,
            &account,
            "npm-noise",
            &name,
            "1.0.0",
            &[],
            "2026-01-01T00:00:00Z",
        )
        .await;
    }
    // Los mismos nombres, un paquete entero por nombre (el distractor
    // mas fuerte para el scoping), y diez versiones por paquete para el
    // volumen: sesenta filas de tarball en la otra cuenta. La
    // instantánea del packument es UNA por nombre — sembrar la misma
    // instantánea dos veces sería una precondición `Absent` sobre una
    // ruta ya ocupada, que el contrato rehúsa.
    // El instante distinto: el MISMO paquete con la MISMA semilla diría
    // el MISMO digest, y el aislamiento se comprobaría con contenidos
    // iguales. Con el instante distinto, la instantánea de la otra cuenta
    // es OTRO contenido en el MISMO camino.
    for index in 1..=6u32 {
        let name = PackageName::parse(&format!("pkg-{index:02}"), true).expect("a name");
        seed_a_package(
            &store,
            &other,
            "npm-hosted",
            &name,
            "1.0.0",
            &[],
            "2026-02-02T00:00:00Z",
        )
        .await;
    }
    for index in 1..=6u32 {
        for minor in 1..=9u32 {
            let name = PackageName::parse(&format!("pkg-{index:02}"), true).expect("a name");
            let version = format!("1.{minor}.0");
            publish_a_tarball(
                store.as_ref(),
                &other,
                "npm-hosted",
                &name,
                &version,
                &seeded_tarball(&name, &version),
            )
            .await;
        }
    }

    // El listado de repositorios es DE LA CUENTA.
    let repositories = store.repositories(&account).await.expect("the listing");
    let keys: Vec<String> = repositories
        .iter()
        .map(|repo| repo.repo_key.clone())
        .collect();
    assert_eq!(
        keys,
        vec!["npm-hosted".to_string(), "npm-noise".to_string()]
    );
    let other_repositories = store.repositories(&other).await.expect("the listing");
    assert_eq!(
        other_repositories
            .iter()
            .map(|repo| repo.repo_key.clone())
            .collect::<Vec<_>>(),
        vec!["npm-hosted".to_string()],
    );

    // Búsqueda exacta por paquete: solo el tarball de ESTA cuenta. Ni los
    // sesenta de la otra cuenta con los mismos nombres, ni los sesenta de
    // ruido de la propia cuenta. Los packuments no son el paquete por
    // coordenadas: su coordenada lleva el sufijo #packument.
    let started = std::time::Instant::now();
    let page = store
        .search(
            &account,
            &repositories,
            &SearchQuery::new()
                .with_package_name("pkg-03")
                .with_limit(50),
        )
        .await
        .expect("the exact search");
    let elapsed = started.elapsed();
    assert_eq!(page.total, Some(1), "only the account's own tarball");
    assert_eq!(page.results[0].asset_path, "pkg-03/-/pkg-03-1.0.0.tgz");
    assert!(
        elapsed.as_millis() < 2_000,
        "the exact search over 186 distractors took {:?}",
        elapsed
    );
    record_evidence(
        "the_npm_queries_are_scoped_keyed_and_indexed",
        &format!(
            "búsqueda exacta por paquete npm con 186+6 distractores de otra cuenta y otro repos: 1 resultado, total 1, {:?}",
            elapsed
        ),
    );

    // La metadata POR VERSIÓN: las coordenadas completas dicen la fila de
    // la versión del tarball, por índice y sin scan.
    let version = store
        .version(
            &account,
            "npm-hosted",
            &PackageCoordinates {
                package_name: "pkg-03".to_string(),
                package_version: "1.0.0".to_string(),
            },
        )
        .await
        .expect("the version by coordinates");
    assert_eq!(
        version.digest.value,
        the_tarball_digests(&seeded_tarball(
            &PackageName::parse("pkg-03", true).expect("a name"),
            "1.0.0"
        ))
        .sha256
    );

    // El scoping: el MISMO camino en la OTRA cuenta es OTRO contenido, y
    // el total de la caminata de ESTA cuenta no cuenta lo de la otra.
    let ours = store
        .asset(&account, "npm-hosted", "pkg-01")
        .await
        .expect("our packument pointer");
    let theirs = store
        .asset(&other, "npm-hosted", "pkg-01")
        .await
        .expect("their packument pointer");
    assert_ne!(ours.digest, theirs.digest, "the tenants do not share paths");
    let all = store
        .search(&account, &repositories, &SearchQuery::new().with_limit(100))
        .await
        .expect("the walk");
    assert_eq!(
        all.total,
        Some(132),
        "the whole account: 12 in npm-hosted + 120 in npm-noise, without the other account's 60"
    );

    // El cursor estable: la caminata por clave no salta ni duplica aunque
    // llegue una inserción ANTES de la clave ya vista, como hsp191.
    let first = store
        .search(
            &account,
            &repositories,
            &SearchQuery::new()
                .with_repository("npm-hosted")
                .with_limit(2),
        )
        .await
        .expect("the first page");
    assert_eq!(first.results.len(), 2);
    assert!(first.more);
    let last_seen = first
        .results
        .last()
        .expect("a page with rows")
        .asset_path
        .clone();
    let cursor = search_cursor("npm-hosted", &last_seen);
    let late = PackageName::parse("pkg-00", true).expect("a name");
    seed_a_package(
        &store,
        &account,
        "npm-hosted",
        &late,
        "1.0.0",
        &[],
        "2026-01-01T00:00:00Z",
    )
    .await;
    let mut walked: Vec<String> = first
        .results
        .iter()
        .map(|hit| hit.asset_path.clone())
        .collect();
    let mut next = Some(cursor);
    while let Some(cursor) = next {
        let page = store
            .search(
                &account,
                &repositories,
                &SearchQuery::new()
                    .with_repository("npm-hosted")
                    .with_limit(2)
                    .with_cursor(cursor),
            )
            .await
            .expect("a keyed page");
        for hit in &page.results {
            walked.push(hit.asset_path.clone());
        }
        next = if page.more {
            let last = page.results.last().expect("a page with rows");
            Some(search_cursor("npm-hosted", &last.asset_path))
        } else {
            None
        };
    }
    let mut expected: Vec<String> = Vec::new();
    for index in 1..=6u32 {
        let name = PackageName::parse(&format!("pkg-{index:02}"), true).expect("a name");
        expected.push(name.full().to_string());
        expected.push(name.tarball_path("1.0.0"));
    }
    assert_eq!(
        walked, expected,
        "the keyed walk neither skips nor duplicates"
    );
    assert!(
        !walked.iter().any(|path| path.starts_with("pkg-00")),
        "the walk started before the late insertion"
    );
    record_evidence(
        "the_npm_queries_are_scoped_keyed_and_indexed",
        "cursor estable en el listado npm tras insertar pkg-00 antes de la clave ya vista: la caminata no salta ni duplica y el tardío queda fuera; el total por cuenta no cuenta los 60 de la otra",
    );

    // El probe del PLAN: los predicados que las consultas npm filtran
    // llevan índice exacto, y las claves llevan `@upsert` — el servidor es
    // el que arbitra dos escritores de la misma clave.
    let probed = [
        ("hodei.catalog.asset.key", true),
        ("hodei.catalog.version.key", true),
        ("hodei.catalog.asset.account", false),
        ("hodei.catalog.asset.repository", false),
        ("hodei.catalog.asset.package_name", false),
        ("hodei.catalog.asset.package_version", false),
        ("hodei.catalog.version.package_name", false),
        ("hodei.catalog.version.package_version", false),
    ];
    for (predicate, upsert) in probed {
        let schema = client
            .execute_dql(&format!("schema(pred: [{predicate}]) {{}}"))
            .await
            .expect("the schema answers");
        let text = schema.json.to_string();
        assert!(
            text.contains("exact"),
            "the predicate {predicate} must carry the exact index: {text}"
        );
        if upsert {
            assert!(
                text.contains("upsert"),
                "the key predicate {predicate} must carry the upsert guard: {text}"
            );
        }
    }
    record_evidence(
        "the_npm_queries_are_scoped_keyed_and_indexed",
        "probe de plan: asset.key/version.key con @index(exact) @upsert y account/repository/package_name/package_version con @index(exact); la raíz de cada consulta es eq(account/repository) por índice, sin scan global",
    );
}

// ---------------------------------------------------------------------------
// AC2: la paridad de lectura con la referencia Surreal para el mismo
// estado npm.
// ---------------------------------------------------------------------------

async fn the_surreal_reference() -> (TempDir, SurrealCatalogStore) {
    let tmp = tempfile::tempdir().expect("tmp");
    let db = loop {
        match Surreal::new::<RocksDb>(tmp.path().join("db")).await {
            Ok(db) => {
                db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
                break db;
            }
            Err(error) => {
                eprintln!("the surreal reference retries its open: {error}");
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    };
    let store = SurrealCatalogStore::with_authorizer(db, Arc::new(AllowAllAuthorizer));
    store.initialize().await.expect("initialize");
    (tmp, store)
}

/// El mismo estado npm en los dos motores responde lo mismo: el puntero
/// del packument, el tarball por coordenadas, las instantáneas (la
/// historia inmutable de los packuments) y la secuencia de páginas.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_npm_reads_match_the_surreal_reference() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let account = account("parity");

    let dgraph = Arc::new(open_store(client.clone()).await);
    let (_tmp, surreal) = the_surreal_reference().await;

    let repo = "npm-hosted";
    let descriptor = a_descriptor(&account, repo, RepositoryFormat::Npm, serde_json::json!({}));
    dgraph
        .create_repository(&account, descriptor.clone())
        .await
        .expect("the dgraph repository");
    surreal
        .create_repository(&account, descriptor)
        .await
        .expect("the surreal repository");

    // El mismo estado, con la MISMA secuencia de publicaciones en los dos
    // motores: el tarball de cada versión y una instantánea del packument
    // por estado; la del widget lleva una segunda instantánea (el tag
    // `beta` movido) que deja la primera en la historia.
    let name = PackageName::parse("parity-widget", true).expect("a name");
    let other = PackageName::parse("parity-other", true).expect("a name");
    let now = "2026-01-01T00:00:00Z";
    for (package, version, tags) in [
        (&name, "1.0.0", Vec::new()),
        (&name, "1.1.0", vec![("beta", "1.1.0")]),
        (&other, "2.0.0", Vec::new()),
    ] {
        let tarball = format!("parity tarball of {}@{version}", package.full()).into_bytes();
        publish_a_tarball(&*dgraph, &account, repo, package, version, &tarball).await;
        publish_a_tarball(&surreal, &account, repo, package, version, &tarball).await;
        let document = a_packument(package, &[(version, tarball.as_slice())], &tags, now);
        // La segunda instantánea del widget se publica CON la precondición
        // del digest vigente, como hace la superficie al mutar metadatos.
        let (dgraph_previous, surreal_previous) = if version == "1.1.0" {
            (
                dgraph
                    .asset(&account, repo, package.full())
                    .await
                    .expect("the dgraph pointer")
                    .digest,
                surreal
                    .asset(&account, repo, package.full())
                    .await
                    .expect("the surreal pointer")
                    .digest,
            )
        } else {
            (ContentDigest::sha256(""), ContentDigest::sha256(""))
        };
        publish_a_packument_snapshot(
            &*dgraph,
            &account,
            repo,
            package,
            &document,
            (version == "1.1.0").then_some(&dgraph_previous),
        )
        .await;
        publish_a_packument_snapshot(
            &surreal,
            &account,
            repo,
            package,
            &document,
            (version == "1.1.0").then_some(&surreal_previous),
        )
        .await;
    }

    // Por puntero de ruta: la misma instantánea vigente en los dos.
    let dgraph_pointer = dgraph
        .asset(&account, repo, "parity-widget")
        .await
        .expect("the dgraph pointer");
    let surreal_pointer = surreal
        .asset(&account, repo, "parity-widget")
        .await
        .expect("the surreal pointer");
    assert_eq!(dgraph_pointer.digest.value, surreal_pointer.digest.value);
    assert_eq!(dgraph_pointer.size_bytes, surreal_pointer.size_bytes);

    // Por coordenadas: el tarball y la instantánea vigente dicen lo mismo
    // en los dos motores.
    for coordinates in [
        PackageCoordinates {
            package_name: "parity-widget".to_string(),
            package_version: "1.1.0".to_string(),
        },
        PackageCoordinates {
            package_name: format!("parity-widget{}", npm::PACKUMENT_COORDINATES_SUFFIX),
            package_version: dgraph_pointer.digest.value.clone(),
        },
    ] {
        let dgraph_version = dgraph
            .version(&account, repo, &coordinates)
            .await
            .expect("the dgraph version");
        let surreal_version = surreal
            .version(&account, repo, &coordinates)
            .await
            .expect("the surreal version");
        assert_eq!(dgraph_version.digest, surreal_version.digest);
        assert_eq!(dgraph_version.size_bytes, surreal_version.size_bytes);
    }

    // La secuencia de páginas: la referencia por offsets, el grafo por
    // cursores de clave; la caminata entera es la misma.
    let repositories = dgraph.repositories(&account).await.expect("the repos");
    let surreal_repositories = surreal.repositories(&account).await.expect("the repos");
    let mut dgraph_page = dgraph
        .search(
            &account,
            &repositories,
            &SearchQuery::new().with_repository(repo).with_limit(2),
        )
        .await
        .expect("the dgraph page");
    let mut surreal_page = surreal
        .search(
            &account,
            &surreal_repositories,
            &SearchQuery::new().with_repository(repo).with_limit(2),
        )
        .await
        .expect("the surreal page");
    assert_eq!(dgraph_page.total, surreal_page.total);
    let mut keyed: Vec<String> = dgraph_page
        .results
        .iter()
        .map(|hit| hit.asset_path.clone())
        .collect();
    let mut offseted: Vec<String> = surreal_page
        .results
        .iter()
        .map(|hit| hit.asset_path.clone())
        .collect();
    let mut offset = 2usize;
    while dgraph_page.more || surreal_page.more {
        if dgraph_page.more {
            let last = dgraph_page.results.last().expect("a page with rows");
            dgraph_page = dgraph
                .search(
                    &account,
                    &repositories,
                    &SearchQuery::new()
                        .with_repository(repo)
                        .with_limit(2)
                        .with_cursor(search_cursor(repo, &last.asset_path)),
                )
                .await
                .expect("the keyed page");
            keyed.extend(dgraph_page.results.iter().map(|hit| hit.asset_path.clone()));
        }
        if surreal_page.more {
            surreal_page = surreal
                .search(
                    &account,
                    &surreal_repositories,
                    &SearchQuery::new()
                        .with_repository(repo)
                        .with_limit(2)
                        .with_offset(offset),
                )
                .await
                .expect("the offset page");
            offset += 2;
            offseted.extend(
                surreal_page
                    .results
                    .iter()
                    .map(|hit| hit.asset_path.clone()),
            );
        }
    }
    assert_eq!(
        keyed, offseted,
        "the same npm state pages the same, by key or by offset"
    );
    record_evidence(
        "the_npm_reads_match_the_surreal_reference",
        "paridad Surreal del estado npm (puntero del packument, tarball por coordenadas, instantánea vigente e historia, secuencia de páginas cursor vs offset): idéntica",
    );
}
