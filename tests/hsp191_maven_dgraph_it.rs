//! HSP-191: Maven y consulta de catálogo indexada sobre el motor Dgraph real.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` como los IT del stack (TLS + ACL, el
//! servidor vivo h186). Lo que prueba, contra el motor de verdad:
//!
//! - **AC1** el despliegue Maven REAL: el cliente `mvn` (deploy-file,
//!   dependency:get) publica POM+jar por coordenadas `group:artifact:version`
//!   con el layout estándar de repositorio sobre el catálogo Dgraph; los
//!   `maven-metadata.xml` DERIVADOS del catálogo y sus checksums son
//!   equivalentes (estables entre lecturas); los bytes van al CAS y el grafo
//!   solo guarda coordenadas y referencias; el outbox atómico de HSP-187
//!   acompaña a cada commit de publicación; la release es inmutable (409) y
//!   el re-despliegue del mismo contenido es idempotente;
//! - **AC2** el listado por CURSOR ESTABLE (re-paginar tras insertar no
//!   salta ni duplica), la búsqueda EXACTA por coordenadas y tipo con
//!   distractores que NO causan scan global (el plan usa el índice: el
//!   esquema declara `@index(exact)` en los predicados filtrados y la
//!   consulta exacta devuelve solo lo suyo en un coste relativo pequeño),
//!   y el scoping por cuenta (los assets de otra cuenta no existen aquí);
//! - las lecturas del estado publicado según el contrato: por coordenadas,
//!   listado por prefijo y páginas por cursor de clave.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
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
use hodei_catalog::search::{SearchQuery, search_cursor};
use hodei_catalog::{
    CatalogReader, CatalogSearch, CatalogWriter, DigestScope, PackageCoordinates, Publication,
    RepositoryConfig, RepositoryDescriptor, RepositoryFormat, RepositoryMode, repository_hrn,
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
    format!("it191{}{}", tag, &nonce()[8..])
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

/// El directorio de la evidencia: `target/uat/hsp191/`.
fn the_evidence_dir() -> PathBuf {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let target = tmp.parent().unwrap_or(&tmp).to_path_buf();
    let dir = target.join("uat").join("hsp191");
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

// El almacén Dgraph del catálogo: la apertura condicionada que comparten
// las suites gated del stack (HSP-219). La receta nacida en esta suite —el
// retry acotado del `open`— se generaliza en el helper incluido: puerta de
// serialización intra-proceso (los opens del binario comparten la
// preparación del schema de uno en uno) y hasta ocho intentos con backoff
// lineal creciente sobre los errores transitorios (`Unavailable`, que es
// la clase con la que el alter rechazado por `errIndexingInProgress` se
// dice). **Carril recomendado**: este binario con threads por defecto,
// pero solo contra el alpha; dos suites gated a la vez siguen siendo el
// terreno del flake entre procesos (lección HSP-190).
include!("support/hsp219_concurrent_open.rs");

fn actor(account: &str) -> String {
    format!("hrn:hodei:iam:{account}:User/alice")
}

fn correlation_of(account: &str) -> String {
    format!("corr-hsp191-{account}")
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

/// Una publicación directa en el almacén, del contenido pedido.
async fn publish(
    store: &DgraphCatalogStore,
    account: &str,
    repository: &str,
    asset_path: &str,
    package_name: &str,
    package_version: &str,
    content: &[u8],
) {
    store
        .publish(
            account,
            Publication {
                repository: repository.to_string(),
                asset_path: asset_path.to_string(),
                coordinates: PackageCoordinates {
                    package_name: package_name.to_string(),
                    package_version: package_version.to_string(),
                },
                digest: ContentDigest::of_bytes(content),
                size_bytes: content.len() as u64,
                tags: Vec::new(),
                actor: actor(account),
                correlation_id: correlation_of(account),
                overwrite: false,
                precondition: None,
                subject: None,
            },
        )
        .await
        .expect("the publication settles");
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
    #[allow(dead_code)]
    tmp: TempDir,
}

impl Vertical {
    async fn mount_maven(account: &str, repo_key: &str) -> Self {
        let client = fresh_client().await.expect("the dgraph client connects");
        let store = Arc::new(open_store(client.clone()).await);
        store
            .create_repository(
                account,
                a_descriptor(
                    account,
                    repo_key,
                    RepositoryFormat::Maven2,
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
        let content = Arc::new(FileBlobStore::new(tmp.path().join("cas")).expect("the CAS"));

        let state = state
            .with_catalog(reader, writer)
            .with_content_store(content)
            .with_authenticator(Arc::new(ContractAuthenticator {
                account: account.to_string(),
            }))
            .with_authorization(cedar_for(account, repo_key));

        Self {
            app: build_router(state, &config),
            store,
            tmp,
        }
    }
}

/// El mismo servicio, en un puerto de verdad: el cliente Maven habla TCP.
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

/// La respuesta de una petición.
struct Answer {
    status: StatusCode,
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
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
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
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the body can be read");
    Answer {
        status,
        body: body.to_vec(),
    }
}

// ---------------------------------------------------------------------------
// El cliente Maven real (mvn), como hsp109: la credencial en el settings.xml.
// ---------------------------------------------------------------------------

const DEPLOY: &str = "org.apache.maven.plugins:maven-deploy-plugin:3.1.2:deploy-file";
const GET: &str = "org.apache.maven.plugins:maven-dependency-plugin:3.6.1:get";

fn require_maven() {
    let found = Command::new("mvn")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "the Maven CLI is not installed: the Maven suite needs `mvn` in the PATH"
    );
}

/// El grupo de la prueba en el repositorio local de Maven, que se borra al
/// acabar para no dejar nada en la caché del usuario.
struct LocalGroup(PathBuf);

impl Drop for LocalGroup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn the_local_repository() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME");
    PathBuf::from(home).join(".m2").join("repository")
}

struct Maven {
    dir: PathBuf,
    settings: PathBuf,
    group: String,
}

struct Run {
    ok: bool,
    output: String,
}

impl Maven {
    fn new(dir: &std::path::Path, token: &str, group: String) -> Self {
        let settings = dir.join("settings.xml");
        std::fs::write(
            &settings,
            format!(
                "<settings><servers><server><id>hodei</id><configuration><httpHeaders>\
                 <property><name>Authorization</name><value>Bearer {token}</value></property>\
                 </httpHeaders></configuration></server></servers></settings>"
            ),
        )
        .expect("the settings are written");
        Self {
            dir: dir.to_path_buf(),
            settings,
            group,
        }
    }

    fn run(&self, arguments: &[String]) -> Run {
        let output = Command::new("mvn")
            .arg("--batch-mode")
            .arg("--settings")
            .arg(&self.settings)
            .args(arguments)
            .current_dir(&self.dir)
            .output()
            .expect("mvn runs");
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Run {
            ok: output.status.success(),
            output: text,
        }
    }

    fn deploy(
        &self,
        address: &str,
        repository: &str,
        version: &str,
        file: &std::path::Path,
    ) -> Run {
        self.run(&[
            DEPLOY.to_string(),
            format!("-Dfile={}", file.display()),
            format!("-DgroupId={}", self.group),
            "-DartifactId=lib".to_string(),
            format!("-Dversion={version}"),
            "-Dpackaging=jar".to_string(),
            "-DgeneratePom=true".to_string(),
            "-DrepositoryId=hodei".to_string(),
            format!("-Durl=http://{address}/repository/{repository}"),
        ])
    }

    fn resolve(&self, address: &str, repository: &str, version: &str) -> Run {
        self.run(&[
            GET.to_string(),
            format!("-Dartifact={}:lib:{version}", self.group),
            "-Dtransitive=false".to_string(),
            format!(
                "-DremoteRepositories=hodei::default::http://{address}/repository/{repository}"
            ),
        ])
    }

    fn local(&self, version: &str, name: &str) -> PathBuf {
        the_local_repository()
            .join(self.group.replace('.', "/"))
            .join("lib")
            .join(version)
            .join(name)
    }

    fn forget(&self) {
        let _ = std::fs::remove_dir_all(the_local_repository().join(self.group.replace('.', "/")));
    }
}

/// El resumen SHA-256 en hexadecimal, de lo que el catálogo también dice.
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// El resumen SHA-1 en hexadecimal, de lo que Maven sube junto al fichero.
fn sha1_hex(bytes: &[u8]) -> String {
    use sha1::Digest as _;
    sha1::Sha1::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Cuántos eventos del outbox de ESTA cuenta nombra el recurso pedido.
///
/// El censo va POR TENANT (índice exacto), no por el `pending()` global:
/// el outbox del servidor compartido acumula los hechos de TODAS las
/// corridas —ninguna prueba los marca entregados— y su respuesta llegó a
/// desbordar el límite de mensaje gRPC (4 MiB; hallazgo de HSP-194,
/// drenado y documentado por HSP-220). La cuenta es única por corrida,
/// así que la raíz `eq(tenant)` trae por índice solo los hechos de ESTA
/// prueba, y el filtro del recurso va en el adaptador (el grupo Maven
/// lleva el pid, así que el camino es único por ejecución).
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
// AC1: el despliegue Maven real sobre el catálogo Dgraph.
// ---------------------------------------------------------------------------

/// El vertical deja de servir si el IT no está gated: sin servidor no hay
/// cliente real.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_real_maven_client_deploys_and_resolves_over_dgraph() {
    require_maven();
    let Some(_guard) = fresh_client().await else {
        return;
    };
    let account = account("mvn");
    let repo = "mvn-hosted";
    let vertical = Vertical::mount_maven(&account, repo).await;
    let address = serve_over_tcp(&vertical).await;
    let address = address.to_string();

    let evidence = the_evidence_dir();
    let work = evidence.join(format!("mvn-work-{}", nonce()));
    std::fs::create_dir_all(&work).expect("the maven work directory");
    let group = format!("com.hodei.uat{}", std::process::id());
    let local = LocalGroup(the_local_repository().join(group.replace('.', "/")));
    let maven = Maven::new(&work, "alice", group.clone());

    // El jar que Maven despliega. El POM lo genera el propio plugin.
    let jar = work.join("lib.jar");
    let jar_bytes = b"PK-hsp191-maven-over-dgraph-release-bytes".to_vec();
    std::fs::write(&jar, &jar_bytes).expect("the jar");

    // Despliegue real: jar + checksums + POM, por coordenadas y layout
    // estándar, contra el vertical con el catálogo Dgraph.
    let deployed = maven.deploy(&address, repo, "1.0", &jar);
    assert!(
        deployed.ok,
        "the release deploy must succeed: {}",
        deployed.output
    );
    record_evidence(
        "the_real_maven_client_deploys_and_resolves_over_dgraph",
        &format!("mvn deploy-file 1.0 sobre {repo} (catálogo Dgraph): OK"),
    );

    // El layout del repo Maven: las coordenadas `group:artifact:version`
    // viven en `com/hodei/uatN/lib/1.0/lib-1.0.jar` y su POM al lado. El
    // grupo pasa a directorios; la versión y el nombre NO llevan barras.
    let group_prefix = group.replace('.', "/");
    let jar_path = format!("{group_prefix}/lib/1.0/lib-1.0.jar");
    let pom_path = format!("{group_prefix}/lib/1.0/lib-1.0.pom");

    // Los bytes que el catálogo dice son los del fichero desplegado, y
    // van al CAS: el GET sirve lo mismo que se subió. Las coordenadas
    // del catálogo para cada fichero son las de la superficie de
    // contenido: el camino del layout Maven y SU digest.
    let coordinates = PackageCoordinates {
        package_name: jar_path.clone(),
        package_version: sha256_hex(&jar_bytes),
    };
    let version = vertical
        .store
        .version(&account, repo, &coordinates)
        .await
        .expect("the version is in the catalog");
    assert_eq!(
        version.digest.value,
        sha256_hex(&jar_bytes),
        "the digest of the catalog is the digest of the deployed bytes"
    );
    record_evidence(
        "the_real_maven_client_deploys_and_resolves_over_dgraph",
        &format!(
            "digest del catálogo == sha256(bytes): {}",
            version.digest.value
        ),
    );

    let served = send(
        &vertical.app,
        Method::GET,
        &format!("/repository/{repo}/{jar_path}"),
        Some("alice"),
        None,
    )
    .await;
    assert_eq!(served.status, StatusCode::OK);
    assert_eq!(served.body, jar_bytes, "the CAS serves the deployed bytes");

    // La metadata DERIVADA del catálogo existe y es equivalente entre
    // lecturas: el mismo XML dos veces, y su checksum .sha1 el de SU XML.
    let metadata_path = format!("{group_prefix}/lib/maven-metadata.xml");
    let first = send(
        &vertical.app,
        Method::GET,
        &format!("/repository/{repo}/{metadata_path}"),
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
        &format!("/repository/{repo}/{metadata_path}"),
        Some("alice"),
        None,
    )
    .await;
    assert_eq!(first.body, second.body, "the derived metadata is stable");
    let xml = String::from_utf8_lossy(&first.body).into_owned();
    assert!(xml.contains("<version>1.0</version>"), "{xml}");
    record_evidence(
        "the_real_maven_client_deploys_and_resolves_over_dgraph",
        "maven-metadata.xml derivado estable entre lecturas; dice la versión 1.0",
    );

    let checksum = send(
        &vertical.app,
        Method::GET,
        &format!("/repository/{repo}/{metadata_path}.sha1"),
        Some("alice"),
        None,
    )
    .await;
    assert_eq!(checksum.status, StatusCode::OK);
    assert_eq!(
        String::from_utf8_lossy(&checksum.body).trim(),
        sha1_hex(first.body.as_slice()),
        "the checksum of the metadata is the checksum of the served metadata"
    );

    // El POM que Maven generó está guardado y su coordenada es la de la
    // ruta (el handler lo valida antes de guardar).
    let pom = send(
        &vertical.app,
        Method::GET,
        &format!("/repository/{repo}/{pom_path}"),
        Some("alice"),
        None,
    )
    .await;
    assert_eq!(pom.status, StatusCode::OK);
    let pom_text = String::from_utf8_lossy(&pom.body);
    assert!(
        pom_text.contains("<artifactId>lib</artifactId>"),
        "{pom_text}"
    );

    // El outbox atómico: cada commit de publicación lleva SU evento. Los
    // ficheros desplegados (jar, pom y sus checksums absorbidos) dicen los
    // hechos de esta ejecución, y ningún evento de otra corrida entra en
    // el censo.
    let client = fresh_client().await.expect("client");
    let needle = format!("{repo}/{group_prefix}/lib/1.0/");
    let said = the_events_of(&client, &account, &needle).await;
    assert!(said > 0, "the publish commits carried their audit events");
    record_evidence(
        "the_real_maven_client_deploys_and_resolves_over_dgraph",
        &format!("eventos de auditoría de esta ejecución en el outbox: {said}"),
    );

    // El cliente real RESUELVE: dependency:get baja el jar por la
    // coordenada y los bytes son los mismos.
    maven.forget();
    let resolved = maven.resolve(&address, repo, "1.0");
    assert!(
        resolved.ok,
        "the resolution must succeed: {}",
        resolved.output
    );
    assert_eq!(
        std::fs::read(maven.local("1.0", "lib-1.0.jar")).expect("the jar was resolved"),
        jar_bytes,
        "the resolved bytes are the deployed bytes"
    );
    record_evidence(
        "the_real_maven_client_deploys_and_resolves_over_dgraph",
        "mvn dependency:get 1.0: los bytes resueltos son los desplegados",
    );

    // La release es INMUTABLE: otro contenido con la misma versión es un
    // 409 y no deja nada escrito.
    let other = work.join("other.jar");
    std::fs::write(&other, b"PK-another-content").expect("the other jar");
    let again = maven.deploy(&address, repo, "1.0", &other);
    assert!(!again.ok, "a release must not be replaced");
    assert!(
        again.output.contains("409") || again.output.to_lowercase().contains("conflict"),
        "{}",
        again.output
    );
    let unchanged = vertical
        .store
        .version(&account, repo, &coordinates)
        .await
        .expect("the version is still there");
    assert_eq!(unchanged.digest.value, sha256_hex(&jar_bytes));

    // El mismo contenido dicho DOS veces no es un error ni un hecho nuevo:
    // la republicación idempotente vuelve a terminar bien.
    let events_before = said;
    let redeployed = maven.deploy(&address, repo, "1.0", &jar);
    assert!(
        redeployed.ok,
        "the same content republished is idempotent: {}",
        redeployed.output
    );
    let events_after = the_events_of(&client, &account, &needle).await;
    assert_eq!(
        events_before, events_after,
        "an idempotent republication says no new fact"
    );
    record_evidence(
        "the_real_maven_client_deploys_and_resolves_over_dgraph",
        "release inmutable (409 con otro contenido) y republicación del mismo contenido idempotente (sin hechos nuevos)",
    );

    drop(local);
}

// ---------------------------------------------------------------------------
// AC2: el cursor estable, la búsqueda exacta y el scoping por cuenta.
// ---------------------------------------------------------------------------

/// La página que sigue empieza EN LA CLAVE ya vista: una inserción ANTES de
/// la clave no la mueve, y re-paginar no salta ni duplica resultado.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_cursor_pages_are_stable_under_insertions() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let account = account("cursor");
    let repo = "libs";
    let store = Arc::new(open_store(client.clone()).await);
    store
        .create_repository(
            &account,
            a_descriptor(&account, repo, RepositoryFormat::Raw, serde_json::json!({})),
        )
        .await
        .expect("the repository is created");

    for index in 1..=6u32 {
        let path = format!("p/{index:02}");
        publish(
            &store,
            &account,
            repo,
            &path,
            "pkg",
            &index.to_string(),
            format!("cursor-seed-{index}").as_bytes(),
        )
        .await;
    }

    // Primera página por clave: dos resultados y su cursor.
    let query = SearchQuery::new().with_repository(repo).with_limit(2);
    let first = store
        .search(
            &account,
            &store.repositories(&account).await.expect("repos"),
            &query,
        )
        .await
        .expect("the first page");
    assert_eq!(first.results.len(), 2);
    assert!(first.more);
    assert_eq!(first.total, Some(6));
    let (first_repository, last_seen) = {
        let last = first.results.last().expect("a page with rows");
        (last.repository.clone(), last.asset_path.clone())
    };
    let cursor = search_cursor(&first_repository, &last_seen);

    // Llegan inserciones: una ANTES de la clave ya vista y otra al final.
    publish(
        &store,
        &account,
        repo,
        "p/00",
        "pkg",
        "0",
        b"cursor-late-before",
    )
    .await;
    publish(
        &store,
        &account,
        repo,
        "p/07",
        "pkg",
        "7",
        b"cursor-late-after",
    )
    .await;

    // Re-paginar desde el MISMO cursor: la página sigue en p/03 y la
    // caminata entera por cursores no salta ni duplica nada.
    let mut walked: Vec<String> = first
        .results
        .iter()
        .map(|hit| hit.asset_path.clone())
        .collect();
    let mut next = Some(cursor);
    for _ in 0..4 {
        let Some(cursor) = next else { break };
        let page = store
            .search(
                &account,
                &store.repositories(&account).await.expect("repos"),
                &SearchQuery::new()
                    .with_repository(repo)
                    .with_limit(2)
                    .with_cursor(cursor),
            )
            .await
            .expect("a keyed page");
        for hit in &page.results {
            walked.push(hit.asset_path.clone());
        }
        if !page.more {
            next = None;
        } else {
            let last = page.results.last().expect("a page with rows");
            next = Some(search_cursor(&last.repository, &last.asset_path));
        }
    }
    let expected: Vec<String> = (1..=7u32).map(|index| format!("p/{index:02}")).collect();
    assert_eq!(
        walked, expected,
        "the keyed walk neither skips nor duplicates"
    );
    record_evidence(
        "the_cursor_pages_are_stable_under_insertions",
        "caminata por cursor (2 por página) tras insertar p/00 (antes) y p/07 (después): p/01..p/07 una vez cada uno, sin saltos ni duplicados; p/00 queda fuera de la caminata ya empezada",
    );

    // El total de una página POR CLAVE es lo que queda por ver desde la
    // clave: tras p/02 quedan cinco (p/03..p/07).
    let second = store
        .search(
            &account,
            &store.repositories(&account).await.expect("repos"),
            &SearchQuery::new()
                .with_repository(repo)
                .with_limit(2)
                .with_cursor(search_cursor(repo, "p/02")),
        )
        .await
        .expect("the second page");
    assert_eq!(second.total, Some(5), "what remains after the key");
    assert_eq!(
        second
            .results
            .iter()
            .map(|hit| hit.asset_path.as_str())
            .collect::<Vec<_>>(),
        vec!["p/03", "p/04"],
    );
}

/// La búsqueda exacta por coordenadas y tipo, autorizada por cuenta, con
/// distractores que no causan scan global: el plan consulta POR ÍNDICE.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_exact_search_is_scoped_indexed_and_cheap() {
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
                "libs",
                RepositoryFormat::Raw,
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
                "oci",
                RepositoryFormat::Maven2,
                serde_json::json!({}),
            ),
        )
        .await
        .expect("the repository is created");
    // La cuenta distractora: otra cuenta con la MISMA clave de repositorio
    // y los mismos caminos. Lo que ella guarda no existe aquí.
    store
        .create_repository(
            &other,
            a_descriptor(&other, "libs", RepositoryFormat::Raw, serde_json::json!({})),
        )
        .await
        .expect("the other repository is created");

    // Los que la búsqueda debe encontrar.
    for index in 0..6u32 {
        publish(
            &store,
            &account,
            "libs",
            &format!("target/{index}.bin"),
            "target-pkg",
            &index.to_string(),
            format!("target-{index}").as_bytes(),
        )
        .await;
    }
    // Distractores de la MISMA cuenta (otro paquete en el mismo
    // repositorio, otro repositorio de la cuenta) y de la OTRA cuenta con
    // las MISMAS coordenadas y caminos. Suficientes para que un scan
    // global destaque.
    for index in 0..120u32 {
        publish(
            &store,
            &account,
            "libs",
            &format!("noise/{index}.bin"),
            "noise-pkg",
            &index.to_string(),
            format!("noise-{index}").as_bytes(),
        )
        .await;
    }
    for index in 0..120u32 {
        publish(
            &store,
            &other,
            "libs",
            &format!("target/{index}.bin"),
            "target-pkg",
            &index.to_string(),
            format!("foreign-{index}").as_bytes(),
        )
        .await;
    }
    for index in 0..60u32 {
        publish(
            &store,
            &account,
            "oci",
            &format!("oci/{index}.tar"),
            "oci-pkg",
            &format!("oci-{index}"),
            format!("oci-{index}").as_bytes(),
        )
        .await;
    }

    // El listado de repositorios es de LA CUENTA: la otra cuenta con la
    // misma clave no aparece, y sus assets tampoco en la búsqueda.
    let repositories = store.repositories(&account).await.expect("the listing");
    let keys: Vec<String> = repositories
        .iter()
        .map(|repo| repo.repo_key.clone())
        .collect();
    assert_eq!(keys, vec!["libs".to_string(), "oci".to_string()]);

    // Búsqueda exacta por coordenadas: solo los seis de esta cuenta. Ni
    // los 120 de la otra cuenta con las mismas coordenadas, ni los 120
    // del mismo repositorio con otro paquete.
    let started = Instant::now();
    let page = store
        .search(
            &account,
            &repositories,
            &SearchQuery::new()
                .with_package_name("target-pkg")
                .with_limit(50),
        )
        .await
        .expect("the exact search");
    let elapsed = started.elapsed();
    assert_eq!(page.total, Some(6), "only the account's own matches");
    assert!(page.results.iter().all(|hit| hit.repository == "libs"));
    assert!(
        page.results
            .iter()
            .all(|hit| hit.package_name == "target-pkg")
    );
    assert!(
        elapsed.as_millis() < 2_000,
        "the exact search over 306 distractors took {:?}",
        elapsed
    );
    record_evidence(
        "the_exact_search_is_scoped_indexed_and_cheap",
        &format!(
            "búsqueda exacta por package_name con 240+66 distractores: 6 resultados, total 6, {:?}",
            elapsed
        ),
    );

    // La coordenada completa: nombre Y versión.
    let one = store
        .search(
            &account,
            &repositories,
            &SearchQuery::new()
                .with_package_name("target-pkg")
                .with_package_version("3")
                .with_limit(50),
        )
        .await
        .expect("the coordinate search");
    assert_eq!(one.total, Some(1));
    assert_eq!(one.results[0].asset_path, "target/3.bin");

    // El filtro por TIPO (formato del repositorio), resuelto con el
    // descriptor: el repositorio `oci` es Maven2, y la búsqueda que pide
    // los raw deja fuera sus 60 assets. La caminata entera de la cuenta
    // son 186 (126 de libs + 60 de oci); la de los raw, 126.
    let all = store
        .search(&account, &repositories, &SearchQuery::new().with_limit(100))
        .await
        .expect("the walk");
    assert_eq!(
        all.total,
        Some(186),
        "the whole account, without a type filter"
    );
    let only_raw = store
        .search(
            &account,
            &repositories,
            &SearchQuery::new().with_limit(100).with_attribute(
                hodei_catalog::search::RepositoryAttribute::Format(RepositoryFormat::Raw),
            ),
        )
        .await
        .expect("the typed walk");
    assert_eq!(only_raw.total, Some(126), "the raw repositories only");
    assert!(only_raw.results.iter().all(|hit| hit.repository == "libs"));
    record_evidence(
        "the_exact_search_is_scoped_indexed_and_cheap",
        "filtro por tipo (format=raw): el total baja de 186 a 126, el repositorio Maven2 queda fuera",
    );

    // El probe del PLAN: los predicados por los que la consulta exacta
    // filtra están declarados con `@index(exact)` en el esquema del grafo.
    // El contrato del motor: una igualdad de RAÍZ se sirve POR EL ÍNDICE
    // del predicado, y las búsquedas de este slice son igualdades de raíz
    // (cuenta y repositorio) más filtro sobre el mismo índice.
    let probed = [
        "hodei.catalog.asset.account",
        "hodei.catalog.asset.repository",
        "hodei.catalog.asset.package_name",
        "hodei.catalog.asset.package_version",
        "hodei.catalog.asset.digest",
    ];
    for predicate in probed {
        let schema = client
            .execute_dql(&format!("schema(pred: [{predicate}]) {{}}"))
            .await
            .expect("the schema answers");
        let text = schema.json.to_string();
        assert!(
            text.contains("exact"),
            "the predicate {predicate} must carry the exact index: {text}"
        );
    }
    record_evidence(
        "the_exact_search_is_scoped_indexed_and_cheap",
        "probe de plan: account/repository/package_name/package_version/digest declarados @index(exact); la raíz de la consulta es eq(account/repository), servida por el índice",
    );

    // La etiqueta exacta: lo que la etiqueta nombra, y nada más.
    store
        .move_tag(
            &account,
            "libs",
            "latest",
            PackageCoordinates {
                package_name: "target-pkg".to_string(),
                package_version: "2".to_string(),
            },
        )
        .await
        .expect("the tag moves");
    let tagged = store
        .search(
            &account,
            &repositories,
            &SearchQuery::new().with_tag("latest").with_limit(50),
        )
        .await
        .expect("the tag search");
    assert_eq!(tagged.total, Some(1));
    assert_eq!(tagged.results[0].asset_path, "target/2.bin");
    record_evidence(
        "the_exact_search_is_scoped_indexed_and_cheap",
        "búsqueda exacta por etiqueta: 1 resultado, la coordenada que la etiqueta nombra",
    );
}

// ---------------------------------------------------------------------------
// Las lecturas dicen lo que el contrato del catálogo dice.
// ---------------------------------------------------------------------------

/// El estado publicado responde lo que el contrato dice: por coordenadas
/// (digest y tamaño de lo publicado), el listado por prefijo (los caminos
/// del prefijo, en orden ascendente) y las páginas por cursor de clave (la
/// secuencia entera y el total son los de lo publicado). HSP-314 F4: antes
/// se comparaba con un segundo motor por offset; ahora la secuencia
/// esperada es la de los caminos publicados ordenados.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_reads_match_the_contract_for_the_published_state() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let account = account("parity");

    let dgraph = Arc::new(open_store(client.clone()).await);

    let repo = "parity";
    let descriptor = a_descriptor(
        &account,
        repo,
        RepositoryFormat::Maven2,
        serde_json::json!({}),
    );
    dgraph
        .create_repository(&account, descriptor)
        .await
        .expect("the dgraph repository");

    // El estado: los caminos, coordenadas, digests y tamaños. Las
    // coordenadas son las que la superficie de contenido publica: el
    // camino del fichero y su digest, uno por fichero.
    let assets = [
        "com/acme/lib/1.0/lib-1.0.pom",
        "com/acme/lib/1.0/lib-1.0.jar",
        "com/acme/lib/1.1/lib-1.1.jar",
        "com/acme/lib/2.0/lib-2.0.jar",
        "com/acme/other/1.0/other-1.0.jar",
    ];
    for path in assets {
        let content = format!("parity::{path}").into_bytes();
        let digest = ContentDigest::of_bytes(&content);
        let publication = Publication {
            repository: repo.to_string(),
            asset_path: path.to_string(),
            coordinates: PackageCoordinates {
                package_name: path.to_string(),
                package_version: digest.value.clone(),
            },
            digest,
            size_bytes: content.len() as u64,
            tags: Vec::new(),
            actor: actor(&account),
            correlation_id: correlation_of(&account),
            overwrite: false,
            precondition: None,
            subject: None,
        };
        dgraph
            .publish(&account, publication)
            .await
            .expect("the dgraph publication");
    }

    // Por coordenadas: la versión es el digest y el tamaño de lo publicado.
    let published = b"parity::com/acme/lib/1.1/lib-1.1.jar";
    let coordinates = PackageCoordinates {
        package_name: "com/acme/lib/1.1/lib-1.1.jar".to_string(),
        package_version: ContentDigest::of_bytes(published).value,
    };
    let dgraph_version = dgraph
        .version(&account, repo, &coordinates)
        .await
        .expect("the dgraph version");
    assert_eq!(dgraph_version.digest, ContentDigest::of_bytes(published));
    assert_eq!(dgraph_version.size_bytes, published.len() as u64);

    // El listado por prefijo: los caminos publicados bajo el prefijo, en
    // orden ascendente y sin los de fuera del prefijo.
    let dgraph_listing = dgraph
        .assets_under(&account, repo, "com/acme/lib/")
        .await
        .expect("the dgraph listing");
    let dgraph_paths: Vec<String> = dgraph_listing
        .iter()
        .map(|asset| asset.asset_path.clone())
        .collect();
    let mut expected_listing: Vec<String> = assets
        .iter()
        .filter(|path| path.starts_with("com/acme/lib/"))
        .map(|path| path.to_string())
        .collect();
    expected_listing.sort();
    assert_eq!(expected_listing.len(), 4);
    assert_eq!(dgraph_paths, expected_listing);

    // Las páginas de búsqueda por cursor de clave: la secuencia entera es
    // la de los caminos publicados ordenados, y el total, el de los
    // publicados.
    let repositories = dgraph.repositories(&account).await.expect("the repos");
    let query = SearchQuery::new().with_repository(repo).with_limit(2);
    let mut dgraph_page = dgraph
        .search(&account, &repositories, &query)
        .await
        .expect("the dgraph page");
    assert_eq!(dgraph_page.total, Some(assets.len()));

    let mut keyed: Vec<(String, String)> = Vec::new();
    for hit in &dgraph_page.results {
        keyed.push((hit.repository.clone(), hit.asset_path.clone()));
    }
    assert!(dgraph_page.results.len() <= 2, "a page respects its limit");
    while dgraph_page.more {
        let last = dgraph_page.results.last().expect("a page with rows");
        dgraph_page = dgraph
            .search(
                &account,
                &repositories,
                &SearchQuery::new()
                    .with_repository(repo)
                    .with_limit(2)
                    .with_cursor(search_cursor(&last.repository, &last.asset_path)),
            )
            .await
            .expect("the keyed page");
        assert!(dgraph_page.results.len() <= 2, "a page respects its limit");
        for hit in &dgraph_page.results {
            keyed.push((hit.repository.clone(), hit.asset_path.clone()));
        }
    }
    let mut expected_sequence: Vec<(String, String)> = assets
        .iter()
        .map(|path| (repo.to_string(), path.to_string()))
        .collect();
    expected_sequence.sort();
    assert_eq!(
        keyed, expected_sequence,
        "paging by key walks every published path once, in order"
    );
    record_evidence(
        "the_reads_match_the_contract_for_the_published_state",
        "lecturas segun el contrato: coordenadas (digest y tamano), listado por prefijo ordenado y secuencia de paginas por cursor igual a lo publicado",
    );
}
