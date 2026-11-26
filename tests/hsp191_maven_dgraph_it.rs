//! HSP-191: Maven and indexed catalog query over the real Dgraph engine.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` like the stack ITs (TLS + ACL, the
//! live h186 server). What it proves, against the real engine:
//!
//! - **AC1** the REAL Maven deployment: the `mvn` client (deploy-file,
//!   dependency:get) publishes POM+jar by `group:artifact:version` coordinates
//!   with the standard repository layout over the Dgraph catalog; the
//!   `maven-metadata.xml` DERIVED from the catalog and their checksums are
//!   equivalent (stable between reads); the bytes go to the CAS and the graph
//!   only stores coordinates and references; the atomic outbox of HSP-187
//!   accompanies each publication commit; the release is immutable (409) and
//!   the redeployment of the same content is idempotent;
//! - **AC2** listing by STABLE CURSOR (re-paginating after inserting neither
//!   skips nor duplicates), EXACT search by coordinates and type with
//!   distractors that do NOT cause a global scan (the plan uses the index: the
//!   schema declares `@index(exact)` on the filtered predicates and the
//!   exact query returns only what is its own at a small relative cost),
//!   and per-account scoping (the assets of another account do not exist here);
//! - reads of the published state according to the contract: by coordinates,
//!   listing by prefix and pages by key cursor.

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

/// The run: one per execution, so that no test runs into
/// what another left behind.
fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

/// One account per test and per run.
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

/// The evidence directory: `target/uat/hsp191/`.
fn the_evidence_dir() -> PathBuf {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let target = tmp.parent().unwrap_or(&tmp).to_path_buf();
    let dir = target.join("uat").join("hsp191");
    std::fs::create_dir_all(&dir).expect("the evidence directory");
    dir
}

/// An evidence line, with the timestamp.
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

// The catalog's Dgraph store: the conditioned opening shared by
// the stack's gated suites (HSP-219). The recipe born in this suite (the
// bounded retry of `open`) is generalized in the included helper: an
// intra-process serialization gate (the binary's opens share the
// schema preparation one at a time) and up to eight attempts with
// increasing linear backoff on transient errors (`Unavailable`, which is
// the class that the alter rejected by `errIndexingInProgress` is
// reported with). **Recommended lane**: this binary with default threads,
// but only against the alpha; two gated suites at once remain the
// ground of the cross-process flake (lesson HSP-190).
include!("support/hsp219_concurrent_open.rs");

fn actor(account: &str) -> String {
    format!("hrn:hodei:iam:{account}:User/alice")
}

fn correlation_of(account: &str) -> String {
    format!("corr-hsp191-{account}")
}

/// A hosted repository descriptor, with the requested format and settings.
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

/// A direct publication into the store, with the requested content.
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
// The HTTP vertical: the hub router over the Dgraph catalog.
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

/// The Cedar bundle: only `alice` of this account publishes and downloads.
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

/// The same service, on a real port: the Maven client speaks TCP.
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

/// The response to a request.
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
// The real Maven client (mvn), like hsp109: the credential in settings.xml.
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

/// The test group in Maven's local repository, which is deleted at the
/// end so as not to leave anything in the user's cache.
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

/// The SHA-256 digest in hexadecimal, of what the catalog also states.
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The SHA-1 digest in hexadecimal, of what Maven uploads alongside the file.
fn sha1_hex(bytes: &[u8]) -> String {
    use sha1::Digest as _;
    sha1::Sha1::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// How many outbox events of THIS account name the requested resource.
///
/// The census goes PER TENANT (exact index), not through the global `pending()`:
/// the shared server's outbox accumulates the facts of ALL
/// runs (no test marks them delivered) and its response came to
/// overflow the gRPC message limit (4 MiB; finding of HSP-194,
/// drained and documented by HSP-220). The account is unique per run,
/// so the `eq(tenant)` root brings by index only the facts of THIS
/// test, and the resource filter goes in the adapter (the Maven group
/// carries the pid, so the path is unique per execution).
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
// AC1: the real Maven deployment over the Dgraph catalog.
// ---------------------------------------------------------------------------

/// The vertical stops serving if the IT is not gated: without a server there is no
/// real client.
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

    // The jar that Maven deploys. The POM is generated by the plugin itself.
    let jar = work.join("lib.jar");
    let jar_bytes = b"PK-hsp191-maven-over-dgraph-release-bytes".to_vec();
    std::fs::write(&jar, &jar_bytes).expect("the jar");

    // Real deployment: jar + checksums + POM, by coordinates and standard
    // layout, against the vertical with the Dgraph catalog.
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

    // The Maven repo layout: the `group:artifact:version` coordinates
    // live at `com/hodei/uatN/lib/1.0/lib-1.0.jar` and its POM next to it. The
    // group becomes directories; the version and the name carry NO slashes.
    let group_prefix = group.replace('.', "/");
    let jar_path = format!("{group_prefix}/lib/1.0/lib-1.0.jar");
    let pom_path = format!("{group_prefix}/lib/1.0/lib-1.0.pom");

    // The bytes the catalog states are those of the deployed file, and
    // go to the CAS: the GET serves the same as was uploaded. The catalog
    // coordinates for each file are those of the content
    // surface: the Maven layout path and ITS digest.
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

    // The DERIVED metadata from the catalog exists and is equivalent between
    // reads: the same XML twice, and its .sha1 checksum is that of ITS XML.
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

    // The POM that Maven generated is stored and its coordinate is that of the
    // path (the handler validates it before storing).
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

    // The atomic outbox: each publication commit carries ITS event. The
    // deployed files (jar, pom and their absorbed checksums) state the
    // facts of this execution, and no event from another run enters
    // the census.
    let client = fresh_client().await.expect("client");
    let needle = format!("{repo}/{group_prefix}/lib/1.0/");
    let said = the_events_of(&client, &account, &needle).await;
    assert!(said > 0, "the publish commits carried their audit events");
    record_evidence(
        "the_real_maven_client_deploys_and_resolves_over_dgraph",
        &format!("eventos de auditoría de esta ejecución en el outbox: {said}"),
    );

    // The real client RESOLVES: dependency:get downloads the jar by the
    // coordinate and the bytes are the same.
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

    // The release is IMMUTABLE: other content with the same version is a
    // 409 and leaves nothing written.
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

    // The same content stated TWICE is neither an error nor a new fact:
    // the idempotent republication finishes well again.
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
// AC2: the stable cursor, the exact search and the per-account scoping.
// ---------------------------------------------------------------------------

/// The next page starts AT THE KEY already seen: an insertion BEFORE
/// the key does not move it, and re-paginating neither skips nor duplicates a result.
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

    // First page by key: two results and its cursor.
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

    // Insertions arrive: one BEFORE the key already seen and another at the end.
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

    // Re-paginate from the SAME cursor: the page is still at p/03 and the
    // whole walk by cursors neither skips nor duplicates anything.
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

    // The total of a page BY KEY is what remains to be seen from the
    // key: after p/02 five remain (p/03..p/07).
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

/// The exact search by coordinates and type, authorized by account, with
/// distractors that do not cause a global scan: the plan queries BY INDEX.
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
    // The distractor account: another account with the SAME repository key
    // and the same paths. What it stores does not exist here.
    store
        .create_repository(
            &other,
            a_descriptor(&other, "libs", RepositoryFormat::Raw, serde_json::json!({})),
        )
        .await
        .expect("the other repository is created");

    // Those that the search must find.
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
    // Distractors from the SAME account (another package in the same
    // repository, another repository of the account) and from the OTHER account with
    // the SAME coordinates and paths. Enough for a global
    // scan to stand out.
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

    // The repository listing is the ACCOUNT's: the other account with the
    // same key does not appear, and neither do its assets in the search.
    let repositories = store.repositories(&account).await.expect("the listing");
    let keys: Vec<String> = repositories
        .iter()
        .map(|repo| repo.repo_key.clone())
        .collect();
    assert_eq!(keys, vec!["libs".to_string(), "oci".to_string()]);

    // Exact search by coordinates: only the six of this account. Neither
    // the 120 of the other account with the same coordinates, nor the 120
    // of the same repository with another package.
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

    // The full coordinate: name AND version.
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

    // The filter by TYPE (repository format), resolved with the
    // descriptor: the `oci` repository is Maven2, and the search that asks for
    // the raw ones leaves out its 60 assets. The whole walk of the account
    // is 186 (126 of libs + 60 of oci); that of the raw ones, 126.
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

    // The PLAN probe: the predicates that the exact query
    // filters by are declared with `@index(exact)` in the graph schema.
    // The engine contract: a ROOT equality is served BY THE INDEX
    // of the predicate, and the searches of this slice are root equalities
    // (account and repository) plus a filter over the same index.
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

    // The exact tag: what the tag names, and nothing more.
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
// The reads state what the catalog contract states.
// ---------------------------------------------------------------------------

/// The published state answers what the contract says: by coordinates
/// (digest and size of what was published), the listing by prefix (the paths
/// of the prefix, in ascending order) and the pages by key cursor (the
/// whole sequence and the total are those of what was published). HSP-314 F4: before,
/// it was compared with a second engine by offset; now the expected sequence
/// is that of the published paths sorted.
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

    // The state: the paths, coordinates, digests and sizes. The
    // coordinates are those the content surface publishes: the
    // file path and its digest, one per file.
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

    // By coordinates: the version is the digest and the size of what was published.
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

    // The listing by prefix: the paths published under the prefix, in
    // ascending order and without those outside the prefix.
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

    // The search pages by key cursor: the whole sequence is
    // that of the published paths sorted, and the total, that of those
    // published.
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
