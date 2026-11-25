//! HSP-194: PyPI sobre el motor Dgraph real, sin scan global.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` como los IT del stack (TLS + ACL,
//! el servidor vivo h186). La espina del catálogo (HSP-187/190/191/193:
//! guardas por clave, outbox atómico, cursor estable, búsqueda indexada)
//! es la que sirve el contrato PyPI de HSP-112/HSP-178
//! (`crates/hodei-catalog/src/pypi.rs` + `src/handlers/pypi.rs`), y este
//! slice lo demuestra CONTRA EL MOTOR:
//!
//! - **AC1** el cliente PyPI real (`twine` publica, `pip` instala) publica
//!   y descarga sobre el vertical con el catálogo Dgraph: el simple index
//!   por rango acotado (el listado de la raíz sale de `assets_under` con
//!   ventana acotada), la metadata POR PAQUETE con los digests estables del
//!   contenido servido (el `#sha256=` del índice es el digest de los bytes
//!   que salen del CAS), el wheel sale del CAS y el grafo solo guarda
//!   coordenadas y referencias, y el outbox atómico acompaña a cada commit
//!   de publicación (patrón 187/193).
//! - **AC2** versionado PEP 440 (la versión canonizada por la superficie),
//!   republicación inmutable de una distribución (otro contenido con el
//!   mismo nombre es 409 y no deja nada escrito), re-entrega idempotente
//!   (el mismo contenido dicho dos veces no es un error ni un hecho nuevo),
//!   scoping por cuenta, consulta acotada con FIXTURE DISTRACTORA (muchos
//!   paquetes de otros tenants con las mismas claves → sin scan global; el
//!   probe de plan confirma el índice exacto, patrón hsp191/hsp193), cursor
//!   estable en los listados y lecturas del estado PyPI según el
//!   contrato.
//! - los DOS fixes de hsp193 (la publicación settled y las líneas de la
//!   consulta de efectos) ya vienen en la base: aquí NO se reintroducen.
//!   El camino PyPI ejercita la frontera por OTRO lado: el documento del
//!   proyecto se mueve con precondición de digest vigente (la mutación de
//!   metadata) mientras las distribuciones son inmutables.
//!
//! La suite es SERIALIZADA (`--test-threads=1`): la apertura de la guarda
//! bajo carga es el flake conocido del stack (HSP-219) y `open_store` lo
//! absorbe con tres intentos, como hsp191/hsp193.
//!
//! Carril de cliente real: `twine` y `pip` del host (el mismo carril que
//! hsp112), con su `HOME`, su `netrc` y su cache aislados. El `pip`
//! instala el wheel desde una cache vacía; el sdist se verifica por su
//! contrato HTTP (bytes servidos == digest del índice == fila del
//! catálogo), sin pedir al host que construya fuentes.

use sha2::Digest as _;
use std::io::Write as _;
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
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use hodei_catalog::pypi::{self, FileEntry, ProjectDocument};
use hodei_catalog::search::{SearchQuery, search_cursor};
use hodei_catalog::{
    AllowAllAuthorizer, CatalogReader, CatalogSearch, CatalogWriter, DigestScope,
    PackageCoordinates, Precondition, Publication, RepositoryConfig, RepositoryDescriptor,
    RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_catalog_storage::DgraphCatalogStore;
use hodei_evidence::ContentDigest;
use hodei_graph_client::{GraphClient, GraphClientConfig};
use hodei_hub::blob::FileBlobStore;
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
    format!("it194{}{}", tag, &nonce()[8..])
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

/// El directorio de la evidencia: `target/uat/hsp194/`.
fn the_evidence_dir() -> PathBuf {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let target = tmp.parent().unwrap_or(&tmp).to_path_buf();
    let dir = target.join("uat").join("hsp194");
    std::fs::create_dir_all(&dir).expect("the evidence directory");
    dir
}

/// Una línea de evidencia, con el momento.
fn record_evidence(test: &str, line: &str) {
    let file = the_evidence_dir().join("evidencia.md");
    let entry = format!("- [{test}] {line}\n");
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
    format!("corr-hsp194-{account}")
}

/// Un descriptor de repositorio hosted, del formato y los ajustes pedidos.
fn a_descriptor(
    account: &str,
    repo_key: &str,
    format: RepositoryFormat,
    mode: RepositoryMode,
    settings: serde_json::Value,
) -> RepositoryDescriptor {
    let mut config = RepositoryConfig::new(DigestScope::Global);
    config.config_settings = settings;
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", account, repo_key),
        account: account.to_string(),
        repo_key: repo_key.to_string(),
        format,
        mode,
        config,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

// ---------------------------------------------------------------------------
// Los fixtures PyPI: un wheel y un sdist construidos aquí, sin depender de
// una herramienta de Python (la misma forma que `tests/support` usa para
// la superficie PyPI de hsp112).
// ---------------------------------------------------------------------------

/// La metadata del paquete, tal y como la lee `twine`.
fn the_metadata(name: &str, version: &str, requires_python: Option<&str>) -> String {
    let mut metadata = format!(
        "Metadata-Version: 2.1\n\
         Name: {name}\n\
         Version: {version}\n\
         Summary: a distribution for the PyPI over dgraph suite\n"
    );
    if let Some(requires_python) = requires_python {
        metadata.push_str(&format!("Requires-Python: {requires_python}\n"));
    }
    metadata.push('\n');
    metadata
}

/// El nombre que lleva el fichero de una distribución: PEP 427 escapa el
/// nombre del proyecto con `_` en el fichero.
fn the_escaped_name(name: &str) -> String {
    name.replace(['-', '.'], "_")
}

/// El `RECORD` de un wheel: los ficheros, su hash y su tamaño.
fn the_record(entries: &[(&str, &[u8])]) -> String {
    let mut record = String::new();
    for (path, body) in entries {
        let digest = sha2::Sha256::digest(body);
        record.push_str(&format!(
            "{path},sha256={},{}\n",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest),
            body.len()
        ));
    }
    record
}

/// Un wheel: un zip con los metadatos, el `RECORD` y el módulo, cuyo
/// contenido lleva la marca pedida (dos cuentas con el mismo camino tienen
/// que guardar CONTENIDO distinto, y la inmutabilidad se demuestra con
/// otro contenido bajo el mismo nombre).
fn a_wheel(name: &str, version: &str, requires_python: Option<&str>, value: u32) -> Vec<u8> {
    let escaped = the_escaped_name(name);
    let dist = format!("{escaped}-{version}.dist-info");
    let metadata = the_metadata(name, version, requires_python);
    let wheel = "Wheel-Version: 1.0\n\
                 Generator: hodei-tests\n\
                 Root-Is-Purelib: true\n\
                 Tag: py3-none-any\n";
    let module = format!("\"\"\"{name}.\"\"\"\n\nVALUE = {value}\n");
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let entries: Vec<(String, Vec<u8>)> = vec![
        (format!("{dist}/METADATA"), metadata.into_bytes()),
        (format!("{dist}/WHEEL"), wheel.as_bytes().to_vec()),
        (format!("{name}/__init__.py"), module.into_bytes()),
    ];
    for (path, body) in &entries {
        writer
            .start_file(path.clone(), options)
            .expect("the file of the wheel starts");
        writer
            .write_all(body)
            .expect("the body of the file is written");
    }
    let record = the_record(
        &entries
            .iter()
            .map(|(path, body)| (path.as_str(), body.as_slice()))
            .collect::<Vec<_>>(),
    );
    writer
        .start_file(format!("{dist}/RECORD"), options)
        .expect("the record of the wheel starts");
    writer
        .write_all(record.as_bytes())
        .expect("the record is written");
    writer.finish().expect("the wheel is closed").into_inner()
}

/// Un sdist: un tar con gzip con el `PKG-INFO` y el `pyproject.toml`.
fn an_sdist(name: &str, version: &str, requires_python: Option<&str>, value: u32) -> Vec<u8> {
    let root = format!("{name}-{version}");
    let metadata = the_metadata(name, version, requires_python);
    let pyproject = format!(
        "[build-system]\n\
         requires = [\"setuptools>=61\"]\n\
         build-backend = \"setuptools.build_meta\"\n\
         \n\
         [project]\n\
         name = \"{name}\"\n\
         version = \"{version}\"\n"
    );
    let module = format!("\"\"\"{name}.\"\"\"\n\nVALUE = {value}\n");
    let entries: Vec<(String, Vec<u8>)> = vec![
        (format!("{root}/PKG-INFO"), metadata.into_bytes()),
        (format!("{root}/pyproject.toml"), pyproject.into_bytes()),
        (format!("{root}/{name}/__init__.py"), module.into_bytes()),
    ];
    let mut tar: Vec<u8> = Vec::new();
    for (path, body) in &entries {
        tar.extend_from_slice(&the_tar_header(path, body.len() as u64));
        tar.extend_from_slice(body);
        let padding = (512 - body.len() % 512) % 512;
        tar.extend(std::iter::repeat_n(0u8, padding));
    }
    tar.extend(std::iter::repeat_n(0u8, 1024));
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&tar).expect("the tar is compressed");
    encoder.finish().expect("the sdist is closed")
}

/// La cabecera de un fichero de tar.
fn the_tar_header(path: &str, size: u64) -> [u8; 512] {
    let mut header = [0u8; 512];
    header[..path.len()].copy_from_slice(path.as_bytes());
    let write = |header: &mut [u8; 512], at: usize, text: &str| {
        header[at..at + text.len()].copy_from_slice(text.as_bytes());
    };
    write(&mut header, 100, "000644 ");
    write(&mut header, 108, "000000 ");
    write(&mut header, 116, "000000 ");
    write(&mut header, 124, &format!("{size:011o} "));
    write(&mut header, 136, &format!("{:011o} ", 0));
    header[148..156].fill(b' ');
    header[156] = b'0';
    write(&mut header, 257, "ustar");
    header[263] = 0;
    write(&mut header, 265, "00");
    let checksum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
    write(&mut header, 148, &format!("{checksum:06o} "));
    header[154] = 0;
    header
}

/// El hex SHA-256 de unos bytes, tal y como lo lleva el índice.
fn sha256_hex(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Los nombres de las distribuciones de un paquete de la suite.
fn the_wheel_file(name: &str, version: &str) -> String {
    format!("{}-{version}-py3-none-any.whl", the_escaped_name(name))
}

fn the_sdist_file(name: &str, version: &str) -> String {
    format!("{}-{version}.tar.gz", the_escaped_name(name))
}

// ---------------------------------------------------------------------------
// Las publicaciones PyPI al nivel del almacén: la misma forma que escribe
// la superficie (`src/handlers/pypi.rs`), para sembrar estados completos
// sin pasar por el cliente real.
// ---------------------------------------------------------------------------

/// La entrada de índice de una distribución publicada.
fn an_entry(filename: &str, bytes: &[u8], requires_python: Option<&str>) -> FileEntry {
    FileEntry {
        filename: filename.to_string(),
        sha256: sha256_hex(bytes),
        size: Some(bytes.len() as u64),
        requires_python: requires_python.map(str::to_string),
        yanked: None,
        source: None,
    }
}

/// Publica una distribución PyPI, con las coordenadas de la superficie: el
/// camino `{proyecto}/{fichero}` y las coordenadas (`{proyecto}/{fichero}`,
/// versión PEP 440). Una distribución es inmutable: el commit lleva la
/// guarda de la CLAVE del asset y `overwrite: false`.
async fn publish_a_distribution<Store>(
    store: &Store,
    account: &str,
    repository: &str,
    project: &str,
    filename: &str,
    version: &str,
    bytes: &[u8],
) where
    Store: hodei_catalog::CatalogWriter + Send + Sync,
{
    let asset_path = format!("{project}/{filename}");
    store
        .publish(
            account,
            Publication {
                repository: repository.to_string(),
                asset_path: asset_path.clone(),
                coordinates: PackageCoordinates {
                    package_name: asset_path.clone(),
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
        .expect("the distribution publication settles");
}

/// Publica una instantánea del documento del proyecto, como la superficie:
/// cada estado es una versión nueva de contenido direccionada por digest y
/// el puntero de la ruta se mueve con la precondición del digest anterior.
async fn publish_a_document_snapshot<Store>(
    store: &Store,
    account: &str,
    repository: &str,
    project: &str,
    entries: &[FileEntry],
    previous: Option<&ContentDigest>,
) -> ContentDigest
where
    Store: hodei_catalog::CatalogWriter + Send + Sync,
{
    let mut document = ProjectDocument::new(project.to_string());
    for entry in entries {
        document.add(entry.clone()).expect("the entry is new");
    }
    let files_base = format!("/pypi/{repository}/files");
    let bytes = serde_json::to_vec(&document.to_value(&files_base)).expect("the document");
    let digest = ContentDigest::of_bytes(&bytes);
    store
        .publish(
            account,
            Publication {
                repository: repository.to_string(),
                asset_path: project.to_string(),
                coordinates: PackageCoordinates {
                    package_name: format!("{project}{}", pypi::PROJECT_COORDINATES_SUFFIX),
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
        .expect("the document snapshot settles");
    digest
}

/// Siembra un paquete PyPI completo: su wheel, su sdist y el documento del
/// proyecto que los nombra. Devuelve el digest de la instantánea.
async fn seed_a_package<Store>(
    store: &Store,
    account: &str,
    repository: &str,
    project: &str,
    version: &str,
    value: u32,
) -> ContentDigest
where
    Store: hodei_catalog::CatalogWriter + Send + Sync,
{
    let wheel = a_wheel(project, version, Some(">=3.9"), value);
    let sdist = an_sdist(project, version, Some(">=3.9"), value);
    publish_a_distribution(
        store,
        account,
        repository,
        project,
        &the_wheel_file(project, version),
        version,
        &wheel,
    )
    .await;
    publish_a_distribution(
        store,
        account,
        repository,
        project,
        &the_sdist_file(project, version),
        version,
        &sdist,
    )
    .await;
    let entries = vec![
        an_entry(&the_wheel_file(project, version), &wheel, Some(">=3.9")),
        an_entry(&the_sdist_file(project, version), &sdist, Some(">=3.9")),
    ];
    publish_a_document_snapshot(store, account, repository, project, &entries, None).await
}

/// El contenido determinista de la distribución sembrada de un proyecto.
fn seeded_wheel(project: &str, version: &str, value: u32) -> Vec<u8> {
    a_wheel(project, version, Some(">=3.9"), value)
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
    #[allow(dead_code)]
    content: Arc<dyn hodei_hub::blob::BlobStore>,
    #[allow(dead_code)]
    tmp: TempDir,
}

impl Vertical {
    async fn mount_pypi(account: &str, repo_key: &str) -> Self {
        let client = fresh_client().await.expect("the dgraph client connects");
        let store = Arc::new(open_store(client.clone()).await);
        store
            .create_repository(
                account,
                a_descriptor(
                    account,
                    repo_key,
                    RepositoryFormat::PyPi,
                    RepositoryMode::Hosted,
                    serde_json::json!({}),
                ),
            )
            .await
            .expect("the repository is created");

        let tmp = tempfile::tempdir().expect("temp dir");
        let mut config = AppConfig::default();
        config.security.raw_api_enabled = true;
        // The graph profile is the only authority: the bootstrap connects to the
        // same alpha the test seeds and its catalog is replaced just below.
        config.graph = config_from_env();
        config.content.cas_path = tmp
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

/// El mismo servicio, en un puerto de verdad: el cliente PyPI habla TCP.
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
    accept: Option<&str>,
) -> Answer {
    let mut builder = Request::builder().method(method).uri(uri);
    builder = builder.header(header::HOST, "127.0.0.1");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(accept) = accept {
        builder = builder.header(header::ACCEPT, accept);
    }
    let request = builder
        .body(Body::empty())
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
// El cliente PyPI real, como hsp112: su HOME, su netrc y su cache.
// ---------------------------------------------------------------------------

fn require_twine() {
    let found = Command::new("twine")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "the twine CLI is not installed: the PyPI suite needs `twine` in the PATH"
    );
}

fn require_pip() {
    let found = Command::new("pip")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "the pip CLI is not installed: the PyPI suite needs `pip` in the PATH"
    );
}

/// El sitio donde el interprete tiene sus paquetes de usuario: el `twine`
/// instalado para el usuario real no se ve desde el HOME aislado sin este
/// camino (hsp112).
fn the_python_path() -> Option<String> {
    let output = Command::new("python3")
        .args(["-c", "import site; print(site.getusersitepackages())"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return None;
    }
    Some(path)
}

/// La salida de un proceso, por stdout y por stderr.
fn the_text(stdout: &[u8], stderr: &[u8]) -> String {
    let mut text = String::from_utf8_lossy(stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(stderr));
    text
}

/// Un cliente PyPI aislado: su `HOME`, su `netrc` y su cache. La
/// credencial viaja en la CONTRASEÑA de la autenticación básica, que es
/// como la llevan `pip` y `twine` (hsp112).
struct Client {
    root: PathBuf,
    netrc: PathBuf,
    port: u16,
    repository: String,
}

impl Client {
    fn new(root: &Path, name: &str, port: u16, repository: &str, token: &str) -> Self {
        let root = root.join(name);
        std::fs::create_dir_all(&root).expect("the client home");
        let netrc = root.join("netrc");
        std::fs::write(
            &netrc,
            format!("machine 127.0.0.1 login alice password {token}\n"),
        )
        .expect("the netrc is written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&netrc, std::fs::Permissions::from_mode(0o600))
                .expect("the netrc is private");
        }
        Self {
            root,
            netrc,
            port,
            repository: repository.to_string(),
        }
    }

    /// Publica una distribución con `twine`.
    fn publish(&self, file: &Path) -> Run {
        let mut command = Command::new("twine");
        command
            .arg("upload")
            .arg("--repository-url")
            .arg(format!(
                "http://127.0.0.1:{}/pypi/{}/",
                self.port, self.repository
            ))
            .arg("--non-interactive")
            .arg("--disable-progress-bar")
            .arg(file)
            // La credencial va en el entorno, nunca en la linea de ordenes.
            .env("TWINE_USERNAME", "alice")
            .env("TWINE_PASSWORD", "alice")
            .env("HOME", self.root.join("home"))
            .env("XDG_CONFIG_HOME", self.root.join("home"));
        if let Some(path) = the_python_path() {
            command.env("PYTHONPATH", path);
        }
        let output = command.output().expect("twine runs");
        Run {
            ok: output.status.success(),
            output: the_text(&output.stdout, &output.stderr),
        }
    }

    /// Instala con `pip` desde una cache vacía.
    fn install(&self, target: &Path, package: &str) -> Run {
        let output = Command::new("pip")
            .arg("install")
            .arg("--index-url")
            .arg(format!(
                "http://127.0.0.1:{}/pypi/{}/simple/",
                self.port, self.repository
            ))
            .arg("--target")
            .arg(target)
            .arg("--no-cache-dir")
            .arg("--no-deps")
            .arg("--disable-pip-version-check")
            .arg(package)
            .env("NETRC", &self.netrc)
            .env("HOME", self.root.join("home"))
            .env("PIP_CONFIG_FILE", "/dev/null")
            .output()
            .expect("pip runs");
        Run {
            ok: output.status.success(),
            output: the_text(&output.stdout, &output.stderr),
        }
    }
}

struct Run {
    ok: bool,
    output: String,
}

/// Cuántos eventos del outbox de ESTA cuenta nombra el recurso pedido.
///
/// El censo va POR TENANT (índice exacto), no por el `pending()` global:
/// el outbox del servidor compartido acumula los hechos pendientes de
/// TODAS las corridas —ninguna prueba los marca entregados— y su respuesta
/// ya desborda el límite de mensaje gRPC (4 MiB). La cuenta es única por
/// corrida, así que la raíz `eq(tenant)` trae por índice solo los hechos
/// de ESTA prueba, y el filtro del recurso va en el adaptador (frontera
/// hallada por HSP-194; ver la evidencia).
async fn the_events_of(client: &GraphClient, tenant: &str, resource_needle: &str) -> usize {
    let escaped = tenant.replace('\\', "\\\\").replace('"', "\\\"");
    let response = client
        .execute_dql(&format!(
            "{{ q(func: eq(hodei.catalog.outbox.tenant, \"{escaped}\")) \
             {{ hodei.catalog.outbox.resource }} }}"
        ))
        .await
        .expect("the outbox census answers");
    let nodes = response
        .json
        .get("q")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    nodes
        .iter()
        .filter(|node| {
            node.get("hodei.catalog.outbox.resource")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|resource| resource.contains(resource_needle))
        })
        .count()
}

// ---------------------------------------------------------------------------
// AC1: el cliente PyPI real publica y lee sobre el catálogo Dgraph, con
// digests estables, wheel desde el CAS y outbox atómico.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_real_pypi_client_publishes_and_reads_over_dgraph() {
    require_twine();
    require_pip();
    let Some(_guard) = fresh_client().await else {
        return;
    };
    let account = account("pypi");
    let repo = "pypi-hosted";
    let vertical = Vertical::mount_pypi(&account, repo).await;
    let address = serve_over_tcp(&vertical).await;
    let pid = the_pid();
    let project = format!("uat{pid}pkg");

    let evidence = the_evidence_dir();
    let root = evidence.join(format!("pypi-work-{}", nonce()));
    std::fs::create_dir_all(&root).expect("the pypi work directory");
    let dist = root.join("dist");
    std::fs::create_dir_all(&dist).expect("the dist directory");
    let wheel_file = the_wheel_file(&project, "1.0");
    let sdist_file = the_sdist_file(&project, "1.0");
    let wheel = dist.join(&wheel_file);
    std::fs::write(&wheel, a_wheel(&project, "1.0", Some(">=3.9"), 42))
        .expect("the wheel is written");
    let sdist = dist.join(&sdist_file);
    std::fs::write(&sdist, an_sdist(&project, "1.0", Some(">=3.9"), 42))
        .expect("the sdist is written");

    let client = Client::new(&root, "client", address.port(), repo, "alice");

    // La publicación real: el `twine` manda el protocolo de subida y el
    // catálogo Dgraph lo asienta (distribución inmutable + documento del
    // proyecto con su instantánea).
    for file in [&wheel, &sdist] {
        let published = client.publish(file);
        assert!(
            published.ok,
            "twine upload of {} over dgraph failed:\n{}",
            file.display(),
            published.output
        );
    }
    record_evidence(
        "the_real_pypi_client_publishes_and_reads_over_dgraph",
        &format!("twine upload de {wheel_file} y {sdist_file} sobre {repo} (catálogo Dgraph): OK"),
    );

    // El simple index POR RANGO: la raíz lista los proyectos del
    // repositorio sin escanear nada (el rango acotado de assets_under),
    // y el proyecto aparece.
    let root_index = send(
        &vertical.app,
        Method::GET,
        &format!("/pypi/{repo}/simple/"),
        Some("alice"),
        Some("application/vnd.pypi.simple.v1+json"),
    )
    .await;
    assert_eq!(
        root_index.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&root_index.body)
    );
    let root_doc: serde_json::Value =
        serde_json::from_slice(&root_index.body).expect("the root index is JSON");
    let projects: Vec<&str> = root_doc["projects"]
        .as_array()
        .expect("the projects")
        .iter()
        .filter_map(|project| project["name"].as_str())
        .collect();
    assert!(
        projects.contains(&project.as_str()),
        "the root index lists the project: {projects:?}"
    );
    record_evidence(
        "the_real_pypi_client_publishes_and_reads_over_dgraph",
        &format!(
            "simple index por rango acotado: la raíz lista {project:?} y nada más de la cuenta ({projects:?})"
        ),
    );

    // La metadata POR PAQUETE, leída dos veces: la página es una
    // instantánea estable (mismo body y mismo ETag), y cada fichero lleva
    // el hash del contenido que el servicio sirve.
    let project_url = format!("/pypi/{repo}/simple/{project}/");
    let first = send(
        &vertical.app,
        Method::GET,
        &project_url,
        Some("alice"),
        Some("application/vnd.pypi.simple.v1+json"),
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
        &project_url,
        Some("alice"),
        Some("application/vnd.pypi.simple.v1+json"),
    )
    .await;
    assert_eq!(
        first.body, second.body,
        "the index page is a stable snapshot"
    );
    assert_eq!(
        first.etag, second.etag,
        "the index page digest is stable between reads"
    );
    let document: serde_json::Value =
        serde_json::from_slice(&first.body).expect("the index is JSON");
    let files = document["files"].as_array().expect("the files").clone();
    let wheel_bytes = std::fs::read(&wheel).expect("the wheel bytes");
    let sdist_bytes = std::fs::read(&sdist).expect("the sdist bytes");
    for (filename, bytes) in [
        (&wheel_file, wheel_bytes.as_slice()),
        (&sdist_file, sdist_bytes.as_slice()),
    ] {
        let entry = files
            .iter()
            .find(|entry| entry["filename"].as_str() == Some(filename))
            .unwrap_or_else(|| panic!("the index names {filename}: {document}"));
        assert_eq!(
            entry["hashes"]["sha256"].as_str().expect("a sha256"),
            sha256_hex(bytes),
            "the index carries the digest of the served content for {filename}"
        );
    }
    record_evidence(
        "the_real_pypi_client_publishes_and_reads_over_dgraph",
        "metadata por paquete estable (mismo body y ETag en dos lecturas); wheel y sdist llevan en el índice el sha256 de SUS bytes",
    );

    // El wheel sale del CAS: los bytes servidos son los que el servicio
    // digeró al publicar, y el grafo dice lo mismo: la versión del
    // catálogo con las coordenadas de la superficie lleva el sha256 de
    // ESOS bytes.
    let served_wheel = send(
        &vertical.app,
        Method::GET,
        &format!("/pypi/{repo}/files/{project}/{wheel_file}"),
        Some("alice"),
        None,
    )
    .await;
    assert_eq!(
        served_wheel.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&served_wheel.body)
    );
    assert_eq!(
        served_wheel.body, wheel_bytes,
        "the served wheel is the uploaded wheel"
    );
    let version = vertical
        .store
        .version(
            &account,
            repo,
            &PackageCoordinates {
                package_name: format!("{project}/{wheel_file}"),
                package_version: "1.0".to_string(),
            },
        )
        .await
        .expect("the version is in the catalog");
    assert_eq!(
        version.digest.value,
        sha256_hex(&wheel_bytes),
        "the catalog row carries the served content's sha256"
    );
    let served_sdist = send(
        &vertical.app,
        Method::GET,
        &format!("/pypi/{repo}/files/{project}/{sdist_file}"),
        Some("alice"),
        None,
    )
    .await;
    assert_eq!(served_sdist.status, StatusCode::OK);
    assert_eq!(
        served_sdist.body, sdist_bytes,
        "the served sdist is the uploaded sdist"
    );
    record_evidence(
        "the_real_pypi_client_publishes_and_reads_over_dgraph",
        &format!(
            "wheel y sdist servidos por el CAS byte a byte; la fila del catálogo ({project}/{wheel_file}, 1.0) lleva el sha256 de los bytes servidos: {}",
            version.digest.value
        ),
    );

    // El cliente real RESUELVE desde una cache vacía: `pip install` baja
    // el wheel por el índice y el módulo instalado es el publicado.
    let target = root.join("installed-wheel");
    let installed = client.install(&target, &project);
    assert!(
        installed.ok,
        "pip install over dgraph failed:\n{}",
        installed.output
    );
    let module = target.join(&project).join("__init__.py");
    assert!(
        module.exists(),
        "the wheel did not install the module:\n{}",
        installed.output
    );
    assert!(
        std::fs::read_to_string(&module)
            .expect("the module")
            .contains("VALUE = 42"),
        "the installed module is not the one that was published"
    );
    record_evidence(
        "the_real_pypi_client_publishes_and_reads_over_dgraph",
        "pip install desde cache vacía contra el índice del port Dgraph: los bytes instalados son los del wheel servido por el CAS",
    );

    // La distribución es INMUTABLE: otro contenido con el mismo nombre de
    // fichero es un 409, y el catálogo no reescribe nada. El nombre de
    // fichero es EL MISMO (la distribución se nombra por su fichero): el
    // contenido que lo lleva es OTRO.
    let other = dist.join("other").join(&wheel_file);
    std::fs::create_dir_all(dist.join("other")).expect("the other directory");
    std::fs::write(&other, a_wheel(&project, "1.0", Some(">=3.9"), 43))
        .expect("the other wheel is written");
    let refused = client.publish(&other);
    assert!(!refused.ok, "twine published over an existing distribution");
    assert!(
        refused.output.contains("409") || refused.output.to_lowercase().contains("conflict"),
        "the refusal is not the immutability conflict:\n{}",
        refused.output
    );
    let unchanged = vertical
        .store
        .version(
            &account,
            repo,
            &PackageCoordinates {
                package_name: format!("{project}/{wheel_file}"),
                package_version: "1.0".to_string(),
            },
        )
        .await
        .expect("the version survives");
    assert_eq!(
        unchanged.digest.value,
        sha256_hex(&wheel_bytes),
        "the immutable version keeps its own content"
    );

    // La re-entrega IDEMPOTENTE: el mismo contenido dicho dos veces no es
    // un error NI un hecho nuevo (la publicación settled lee el resultado
    // comprometido; el documento no cambia porque el fichero ya estaba).
    let census = fresh_client().await.expect("client");
    let needle = format!("{repo}/{project}");
    let before = the_events_of(&census, &account, &needle).await;
    let again = client.publish(&wheel);
    assert!(
        again.ok,
        "the same content republished is idempotent:\n{}",
        again.output
    );
    let after = the_events_of(&census, &account, &needle).await;
    assert_eq!(
        before, after,
        "an idempotent republication says no new fact"
    );
    assert!(before > 0, "the publish commits carried their audit events");
    record_evidence(
        "the_real_pypi_client_publishes_and_reads_over_dgraph",
        &format!(
            "distribución inmutable (409 con otro contenido, la fila del catálogo intacta) y re-entrega del mismo contenido idempotente (sin hechos nuevos: {before} eventos)"
        ),
    );

    // El grafo solo guarda coordenadas y referencias: el puntero del
    // documento del proyecto es la última instantánea, y su versión
    // `#project` guarda el mismo digest. Las distribuciones nunca son el
    // documento.
    let pointer = vertical
        .store
        .asset(&account, repo, &project)
        .await
        .expect("the document pointer");
    let snapshot = vertical
        .store
        .version(
            &account,
            repo,
            &PackageCoordinates {
                package_name: format!("{project}{}", pypi::PROJECT_COORDINATES_SUFFIX),
                package_version: pointer.digest.value.clone(),
            },
        )
        .await
        .expect("the snapshot version is in the catalog");
    assert_eq!(snapshot.digest.value, pointer.digest.value);
    record_evidence(
        "the_real_pypi_client_publishes_and_reads_over_dgraph",
        "el puntero del documento en el grafo es la última instantánea y su versión #project guarda el mismo digest; el grafo solo guarda coordenadas y referencias",
    );
}

// ---------------------------------------------------------------------------
// AC2: cursor estable, scoping por cuenta y distractores que NO causan
// scan global (el probe confirma el índice en los predicados).
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_pypi_queries_are_scoped_keyed_and_indexed() {
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
                "pypi-hosted",
                RepositoryFormat::PyPi,
                RepositoryMode::Hosted,
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
                "pypi-noise",
                RepositoryFormat::PyPi,
                RepositoryMode::Hosted,
                serde_json::json!({}),
            ),
        )
        .await
        .expect("the noise repository is created");
    // La cuenta distractora: la MISMA clave de repositorio y los MISMOS
    // nombres de proyecto. Lo que ella guarda no existe aquí.
    store
        .create_repository(
            &other,
            a_descriptor(
                &other,
                "pypi-hosted",
                RepositoryFormat::PyPi,
                RepositoryMode::Hosted,
                serde_json::json!({}),
            ),
        )
        .await
        .expect("the other repository is created");

    // Los paquetes de la cuenta: seis proyectos en pypi-hosted (wheel +
    // sdist + documento cada uno). La marca 42: el contenido de ESTA
    // cuenta.
    for index in 1..=6u32 {
        let project = format!("pkg-{index:02}");
        seed_a_package(store.as_ref(), &account, "pypi-hosted", &project, "1.0", 42).await;
    }
    // Distractores de la MISMA cuenta (otro repositorio PyPI) y de la OTRA
    // cuenta (el mismo repositorio, los mismos proyectos, otro contenido).
    // Suficientes para que un scan global destaque.
    for index in 0..30u32 {
        let project = format!("noise-{index:02}");
        seed_a_package(store.as_ref(), &account, "pypi-noise", &project, "1.0", 42).await;
    }
    for index in 1..=6u32 {
        let project = format!("pkg-{index:02}");
        seed_a_package(store.as_ref(), &other, "pypi-hosted", &project, "1.0", 43).await;
    }

    // El listado de repositorios es DE LA CUENTA.
    let repositories = store.repositories(&account).await.expect("the listing");
    let keys: Vec<String> = repositories
        .iter()
        .map(|repo| repo.repo_key.clone())
        .collect();
    assert_eq!(
        keys,
        vec!["pypi-hosted".to_string(), "pypi-noise".to_string()]
    );
    let other_repositories = store.repositories(&other).await.expect("the listing");
    assert_eq!(
        other_repositories
            .iter()
            .map(|repo| repo.repo_key.clone())
            .collect::<Vec<_>>(),
        vec!["pypi-hosted".to_string()],
    );

    // Búsqueda exacta por paquete: solo la distribución de ESTA cuenta. Ni
    // las de la otra cuenta con los mismos caminos, ni las de ruido de la
    // propia cuenta.
    let wheel_path = format!("pkg-03/{}", the_wheel_file("pkg-03", "1.0"));
    let started = std::time::Instant::now();
    let page = store
        .search(
            &account,
            &repositories,
            &SearchQuery::new()
                .with_package_name(&wheel_path)
                .with_limit(50),
        )
        .await
        .expect("the exact search");
    let elapsed = started.elapsed();
    assert_eq!(page.total, Some(1), "only the account's own distribution");
    assert_eq!(page.results[0].asset_path, wheel_path);
    assert!(
        elapsed.as_millis() < 2_000,
        "the exact search over the distractors took {elapsed:?}"
    );
    record_evidence(
        "the_pypi_queries_are_scoped_keyed_and_indexed",
        &format!(
            "búsqueda exacta por paquete PyPI ({wheel_path}) con 90+18 distractores de otra cuenta y otro repos: 1 resultado, total 1, {elapsed:?}"
        ),
    );

    // La metadata POR PAQUETE: las coordenadas completas dicen la fila del
    // documento del proyecto (el puntero `#project`), por índice y sin
    // scan, y las de la distribución dicen el sha256 de su contenido.
    let version = store
        .version(
            &account,
            "pypi-hosted",
            &PackageCoordinates {
                package_name: wheel_path.clone(),
                package_version: "1.0".to_string(),
            },
        )
        .await
        .expect("the version by coordinates");
    assert_eq!(
        version.digest.value,
        sha256_hex(&seeded_wheel("pkg-03", "1.0", 42)),
        "the catalog row says the distribution's own content"
    );
    let pointer = store
        .asset(&account, "pypi-hosted", "pkg-01")
        .await
        .expect("our document pointer");
    let snapshot = store
        .version(
            &account,
            "pypi-hosted",
            &PackageCoordinates {
                package_name: format!("pkg-01{}", pypi::PROJECT_COORDINATES_SUFFIX),
                package_version: pointer.digest.value.clone(),
            },
        )
        .await
        .expect("the project snapshot by coordinates");
    assert_eq!(snapshot.digest.value, pointer.digest.value);

    // El scoping: el MISMO camino en la OTRA cuenta es OTRO contenido, y
    // el total de la caminata de ESTA cuenta no cuenta lo de la otra.
    let ours = store
        .asset(&account, "pypi-hosted", "pkg-01")
        .await
        .expect("our document pointer");
    let theirs = store
        .asset(&other, "pypi-hosted", "pkg-01")
        .await
        .expect("their document pointer");
    assert_ne!(ours.digest, theirs.digest, "the tenants do not share paths");
    let all = store
        .search(&account, &repositories, &SearchQuery::new().with_limit(100))
        .await
        .expect("the walk");
    // 18 filas en pypi-hosted (6 proyectos x [documento, wheel, sdist]) +
    // 90 filas en pypi-noise (30 x 3), sin las 18 de la otra cuenta.
    assert_eq!(
        all.total,
        Some(108),
        "the whole account: 18 in pypi-hosted + 90 in pypi-noise, without the other account's rows"
    );

    // El cursor estable: la caminata por clave no salta ni duplica aunque
    // llegue una inserción ANTES de la clave ya vista, como hsp191.
    let first = store
        .search(
            &account,
            &repositories,
            &SearchQuery::new()
                .with_repository("pypi-hosted")
                .with_limit(4),
        )
        .await
        .expect("the first page");
    assert_eq!(first.results.len(), 4);
    assert!(first.more);
    let last_seen = first
        .results
        .last()
        .expect("a page with rows")
        .asset_path
        .clone();
    let cursor = search_cursor("pypi-hosted", &last_seen);
    let late = "pkg-00";
    seed_a_package(store.as_ref(), &account, "pypi-hosted", late, "1.0", 42).await;
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
                    .with_repository("pypi-hosted")
                    .with_limit(4)
                    .with_cursor(cursor),
            )
            .await
            .expect("a keyed page");
        for hit in &page.results {
            walked.push(hit.asset_path.clone());
        }
        next = if page.more {
            let last = page.results.last().expect("a page with rows");
            Some(search_cursor("pypi-hosted", &last.asset_path))
        } else {
            None
        };
    }
    let mut expected: Vec<String> = Vec::new();
    for index in 1..=6u32 {
        let project = format!("pkg-{index:02}");
        expected.push(project.clone());
        expected.push(format!("{project}/{}", the_wheel_file(&project, "1.0")));
        expected.push(format!("{project}/{}", the_sdist_file(&project, "1.0")));
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
        "the_pypi_queries_are_scoped_keyed_and_indexed",
        "cursor estable en el listado PyPI tras insertar pkg-00 antes de la clave ya vista: la caminata no salta ni duplica y el tardío queda fuera; el total por cuenta no cuenta las 18 filas de la otra",
    );

    // El probe del PLAN: los predicados que las consultas PyPI filtran
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
        "the_pypi_queries_are_scoped_keyed_and_indexed",
        "probe de plan: asset.key/version.key con @index(exact) @upsert y account/repository/package_name/package_version con @index(exact); la raíz de cada consulta es eq(account/repository) por índice, sin scan global",
    );
}

// ---------------------------------------------------------------------------
// AC2: las lecturas del estado PyPI dicen lo que el contrato dice.
// ---------------------------------------------------------------------------

/// La secuencia de publicaciones PyPI contra el almacén pedido: las
/// distribuciones de cada versión y una instantánea del documento por
/// estado; el proyecto del medio lleva una segunda instantánea (el yank
/// simulado al nivel del documento) que deja la primera en la historia.
/// Devuelve, por proyecto, el digest de la instantánea vigente.
macro_rules! seed_the_pypi_state {
    ($store:expr, $account:expr, $repo:expr) => {{
        let mut pointers: Vec<(&'static str, ContentDigest)> = Vec::new();
        for (project, version, second_state) in [
            ("parity-one", "1.0", false),
            ("parity-two", "2.0", true),
            ("parity-three", "3.0", false),
        ] {
            let wheel = a_wheel(project, version, Some(">=3.9"), 42);
            let sdist = an_sdist(project, version, Some(">=3.9"), 42);
            publish_a_distribution(
                $store,
                $account,
                $repo,
                project,
                &the_wheel_file(project, version),
                version,
                &wheel,
            )
            .await;
            publish_a_distribution(
                $store,
                $account,
                $repo,
                project,
                &the_sdist_file(project, version),
                version,
                &sdist,
            )
            .await;
            let entries = vec![
                an_entry(&the_wheel_file(project, version), &wheel, Some(">=3.9")),
                an_entry(&the_sdist_file(project, version), &sdist, Some(">=3.9")),
            ];
            let mut current =
                publish_a_document_snapshot($store, $account, $repo, project, &entries, None).await;
            if second_state {
                // La mutación de metadata: la versión queda retirada
                // (PEP 592) y el documento se mueve CON la precondición
                // del digest vigente, como hace la superficie.
                let mut entries = vec![
                    an_entry(&the_wheel_file(project, version), &wheel, Some(">=3.9")),
                    an_entry(&the_sdist_file(project, version), &sdist, Some(">=3.9")),
                ];
                for entry in &mut entries {
                    entry.yanked = Some(String::new());
                }
                let previous = $store
                    .asset($account, $repo, project)
                    .await
                    .expect("the pointer")
                    .digest;
                current = publish_a_document_snapshot(
                    $store,
                    $account,
                    $repo,
                    project,
                    &entries,
                    Some(&previous),
                )
                .await;
            }
            pointers.push((project, current));
        }
        pointers
    }};
}

/// El estado PyPI publicado responde lo que el contrato dice: el listado
/// por rango (los nueve caminos, ordenados, con el digest de lo publicado),
/// el puntero del documento (la última instantánea), la distribución por
/// coordenadas y la búsqueda exacta. HSP-314 F4: antes se comparaba con un
/// segundo motor; ahora los valores esperados salen de lo publicado.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_pypi_reads_match_the_contract_for_the_published_state() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let account = account("parity");

    let dgraph = Arc::new(open_store(client.clone()).await);

    let repo = "pypi-hosted";
    let descriptor = a_descriptor(
        &account,
        repo,
        RepositoryFormat::PyPi,
        RepositoryMode::Hosted,
        serde_json::json!({}),
    );
    dgraph
        .create_repository(&account, descriptor)
        .await
        .expect("the dgraph repository");

    let pointers = seed_the_pypi_state!(dgraph.as_ref(), &account, repo);

    // El listado por rango: los caminos publicados, en orden, con el
    // digest de lo publicado. Los documentos llevan el de su instantánea
    // vigente; las distribuciones, el de sus bytes y su tamaño.
    let dgraph_assets = dgraph
        .assets_under(&account, repo, "")
        .await
        .expect("the dgraph listing");
    let dgraph_view: Vec<(String, String)> = dgraph_assets
        .iter()
        .map(|asset| (asset.asset_path.clone(), asset.digest.value.clone()))
        .collect();
    let mut expected_view: Vec<(String, String)> = Vec::new();
    let mut expected_sizes: Vec<(String, u64)> = Vec::new();
    for (project, version, _) in [
        ("parity-one", "1.0", ()),
        ("parity-two", "2.0", ()),
        ("parity-three", "3.0", ()),
    ] {
        let wheel = a_wheel(project, version, Some(">=3.9"), 42);
        let sdist = an_sdist(project, version, Some(">=3.9"), 42);
        let pointer = pointers
            .iter()
            .find(|(name, _)| *name == project)
            .expect("the project was seeded");
        expected_view.push((project.to_string(), pointer.1.value.clone()));
        for (file, bytes) in [
            (the_wheel_file(project, version), wheel),
            (the_sdist_file(project, version), sdist),
        ] {
            let path = format!("{project}/{file}");
            expected_view.push((path.clone(), ContentDigest::of_bytes(&bytes).value));
            expected_sizes.push((path, bytes.len() as u64));
        }
    }
    expected_view.sort();
    assert_eq!(
        dgraph_view, expected_view,
        "the ranged listing is every published path, in order, with its digest"
    );
    // Tres proyectos x (documento + wheel + sdist).
    assert_eq!(dgraph_view.len(), 9);
    for (path, size) in &expected_sizes {
        let asset = dgraph_assets
            .iter()
            .find(|asset| &asset.asset_path == path)
            .expect("the distribution is listed");
        assert_eq!(asset.size_bytes, *size, "the size of {path}");
    }

    // El puntero del documento y la distribución por coordenadas.
    for (project, version, _) in [
        ("parity-one", "1.0", ()),
        ("parity-two", "2.0", ()),
        ("parity-three", "3.0", ()),
    ] {
        let dgraph_pointer = dgraph
            .asset(&account, repo, project)
            .await
            .expect("the dgraph pointer");
        let expected_pointer = &pointers
            .iter()
            .find(|(name, _)| *name == project)
            .expect("the project was seeded")
            .1;
        assert_eq!(
            &dgraph_pointer.digest, expected_pointer,
            "the document pointer is the last published snapshot"
        );
        let coordinates = PackageCoordinates {
            package_name: format!("{project}/{}", the_wheel_file(project, version)),
            package_version: version.to_string(),
        };
        let dgraph_version = dgraph
            .version(&account, repo, &coordinates)
            .await
            .expect("the dgraph version");
        assert_eq!(
            dgraph_version.digest,
            ContentDigest::of_bytes(&a_wheel(project, version, Some(">=3.9"), 42)),
            "the distribution by coordinates is the published wheel"
        );
    }

    // La búsqueda exacta por paquete devuelve solo la distribución pedida.
    let repositories = dgraph.repositories(&account).await.expect("the listing");
    let query = SearchQuery::new()
        .with_package_name(format!(
            "parity-one/{}",
            the_wheel_file("parity-one", "1.0")
        ))
        .with_limit(50);
    let dgraph_page = dgraph
        .search(&account, &repositories, &query)
        .await
        .expect("the dgraph search");
    let dgraph_hits: Vec<String> = dgraph_page
        .results
        .iter()
        .map(|hit| hit.asset_path.clone())
        .collect();
    assert_eq!(
        dgraph_hits,
        vec![format!(
            "parity-one/{}",
            the_wheel_file("parity-one", "1.0")
        )]
    );
    assert_eq!(dgraph_page.total, Some(1));
    record_evidence(
        "the_pypi_reads_match_the_contract_for_the_published_state",
        "lecturas PyPI segun el contrato: listado por rango (9 filas, caminos y digests publicados), puntero del documento = ultima instantanea, distribucion por coordenadas y busqueda exacta; el yank del proyecto del medio es una segunda instantanea con precondicion del digest vigente",
    );
}
