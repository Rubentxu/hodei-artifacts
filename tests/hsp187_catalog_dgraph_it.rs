//! HSP-187: el catálogo durable sobre el motor Dgraph real — el slice Raw.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` como todos los IT del stack. Lo que
//! prueba, contra el motor de verdad (TLS + ACL, el servidor vivo h186):
//!
//! - **AC1** el flujo Raw real (publish → GET/HEAD/Range) sobre el
//!   catálogo Dgraph sirve el mismo digest y la misma cuenta; el commit
//!   del catálogo y el evento de auditoría son **una transacción** (la
//!   frontera de HSP-199): el evento está si y solo si el asset está, y
//!   una fila de evento ya dicha con otro hecho impide la publicación
//!   entera;
//! - **AC2** dos escritores dejan un asset y un evento únicos (la guarda
//!   de la ruta + el índice `@upsert` + el marcador de operación
//!   once-only deciden en el servidor); el reintento tras el commit no
//!   duplica nada; un blob del CAS sin metadata de catálogo queda
//!   **huérfano seguro**: detectable por censo, y el catálogo sigue
//!   sirviendo lo que sí tiene;
//! - la paridad de lectura con la referencia Surreal para el mismo estado.
//!
//! El slice es Raw: Repository, Asset, Version, BlobReference y outbox.
//! Los demás formatos no están migrados.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use cedar_policy::Entities;
use chrono::Utc;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_audit::{AuditContext, AuditEvent, AuditOutbox, AuditResult};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use hodei_catalog::{
    AllowAllAuthorizer, AssetRecord, BlobReference, CatalogReader, CatalogWriter, DigestScope,
    PackageCoordinates, PackageVersion, Publication, RepositoryConfig, RepositoryDescriptor,
    RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_catalog_storage::{DgraphAuditOutbox, SurrealCatalogStore};
use hodei_evidence::ContentDigest;
use hodei_graph_client::{GraphClient, GraphClientConfig};
use hodei_hub::blob::{BlobStore, FileBlobStore};
use kernel::application::ports::clock::{CountingIdGenerator, FixedClock, Identity};
use surrealdb::Surreal;
use surrealdb::engine::local::RocksDb;
use tempfile::TempDir;
use tower::ServiceExt;

/// La correlación de las publicaciones. El outbox vive en un servidor
/// compartido entre ejecuciones: cada ejecución lleva su correlación, y
/// el censo de eventos cuenta la suya.
fn correlation_of(account: &str) -> String {
    format!("corr-hsp187-{account}")
}

/// El actor que publica.
fn actor(account: &str) -> String {
    format!("hrn:hodei:iam:{account}:User/alice")
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

/// Una conexión nueva, autenticada y con el arranque versionado validado:
/// un reinicio de verdad re-entra al grafo por aquí.
async fn fresh_client() -> Option<Arc<GraphClient>> {
    let Some(config) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return None;
    };
    let mut client = GraphClient::connect(config).await.expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    Some(Arc::new(client))
}

/// La cola del reloj, que apenas se mueve entre ejecuciones: la cola es
/// la que separa dos ejecuciones de la misma prueba.
fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

/// Una cuenta por prueba y por ejecución: ninguna prueba se encuentra con
/// lo que otra dejó.
fn account(tag: &str) -> String {
    format!("it187{}{}", tag, &nonce()[8..])
}

// El almacén Dgraph del catálogo: la apertura condicionada que comparten
// las suites gated del stack (HSP-219). `DgraphCatalogStore::open` declara
// su DDL con dos `alter` y Dgraph rechaza los `alter` concurrentes con
// `errIndexingInProgress` durante la ventana de reindexado: el helper abre
// de uno en uno dentro del proceso y reintenta lo transitorio con backoff.
// **Carril recomendado**: este binario con los threads por defecto, pero
// solo —sin otra suite gated contra el mismo alpha a la vez—; si se
// lanzan dos juntas, el retry acotado absorbe la ventana pero el carril
// seguro sigue siendo de una en una (lección HSP-190).
include!("support/hsp219_concurrent_open.rs");

/// Un descriptor de repositorio raw hosted.
fn a_descriptor(account: &str, repo_key: &str) -> RepositoryDescriptor {
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", account, repo_key),
        account: account.to_string(),
        repo_key: repo_key.to_string(),
        format: RepositoryFormat::Raw,
        mode: RepositoryMode::Hosted,
        config: RepositoryConfig::new(DigestScope::Global),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

/// Una publicación, con las coordenadas que se le piden.
fn a_publication_at(
    account: &str,
    repository: &str,
    asset_path: &str,
    package_version: &str,
    content: &[u8],
) -> Publication {
    Publication {
        repository: repository.to_string(),
        asset_path: asset_path.to_string(),
        coordinates: PackageCoordinates {
            package_name: "pkg".to_string(),
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
    }
}

/// Una publicación, en la versión por defecto del fixture.
fn a_publication(account: &str, repository: &str, asset_path: &str, content: &[u8]) -> Publication {
    a_publication_at(account, repository, asset_path, "1.0.0", content)
}

/// Un hecho del outbox, dicho como lo graba el outbox atómico: los campos
/// del `AuditEvent` que las aserciones miran.
#[derive(Debug)]
struct OutboxFact {
    correlation_id: String,
    resource: String,
    action: String,
    result: String,
    tenant: String,
    digest: String,
}

/// Los hechos del outbox de ESTA cuenta, dichos por su clave de tenant.
///
/// El censo va POR TENANT (índice exacto), no por el `pending()` global:
/// el outbox del servidor compartido acumula los hechos de TODAS las
/// corridas —ninguna prueba los marca entregados— y su respuesta llegó a
/// desbordar el límite de mensaje gRPC (4 MiB; hallazgo de HSP-194,
/// drenado y documentado por HSP-220). La cuenta es única por corrida,
/// así que la raíz `eq(tenant)` trae por índice solo los hechos de ESTA
/// prueba, y el filtro fino (la correlación) va en el adaptador.
async fn the_events_of(client: &GraphClient, tenant: &str) -> Vec<OutboxFact> {
    let escaped = tenant.replace('\\', "\\\\").replace('"', "\\\"");
    let response = client
        .execute_dql(&format!(
            "{{ q(func: eq(hodei.catalog.outbox.tenant, \"{escaped}\")) \
             {{ hodei.catalog.outbox.correlation_id hodei.catalog.outbox.resource \
                hodei.catalog.outbox.action hodei.catalog.outbox.result \
                hodei.catalog.outbox.digest hodei.catalog.outbox.tenant }} }}"
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
        .map(|node| OutboxFact {
            correlation_id: the_fact_field(node, "hodei.catalog.outbox.correlation_id"),
            resource: the_fact_field(node, "hodei.catalog.outbox.resource"),
            action: the_fact_field(node, "hodei.catalog.outbox.action"),
            result: the_fact_field(node, "hodei.catalog.outbox.result"),
            tenant: the_fact_field(node, "hodei.catalog.outbox.tenant"),
            digest: the_fact_field(node, "hodei.catalog.outbox.digest"),
        })
        .collect()
}

/// Un campo del hecho dicho por el grafo, o vacío cuando no está.
fn the_fact_field(node: &serde_json::Value, field: &str) -> String {
    node.get(field)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Cuántos eventos pendientes hay de ESTA ejecución: el censo va por
/// tenant y la correlación propia filtra en el adaptador.
async fn the_event_count(client: &GraphClient, account: &str) -> usize {
    the_events_of(client, account)
        .await
        .into_iter()
        .filter(|fact| fact.correlation_id == correlation_of(account))
        .count()
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

/// El vertical montado: el router del hub con el catálogo Dgraph y el CAS
/// de ficheros de verdad.
struct Vertical {
    app: Router,
    reader: Arc<dyn CatalogReader>,
    content: Arc<FileBlobStore>,
    #[allow(dead_code)]
    tmp: TempDir,
}

impl Vertical {
    async fn mount(account: &str, repo_key: &str) -> Self {
        let client = fresh_client().await.expect("the dgraph client connects");
        let store = Arc::new(open_store(client.clone()).await);
        store
            .create_repository(account, a_descriptor(account, repo_key))
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
        let content = Arc::new(FileBlobStore::new(tmp.path().join("cas")).expect("the CAS"));

        let state = state
            .with_catalog(reader.clone(), writer.clone())
            .with_content_store(content.clone())
            .with_authenticator(Arc::new(ContractAuthenticator {
                account: account.to_string(),
            }))
            .with_authorization(cedar_for(account, repo_key));

        Self {
            app: build_router(state, &config),
            reader,
            content,
            tmp,
        }
    }
}

/// La respuesta de una petición.
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

async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Vec<u8>>,
    extra: &[(&str, &str)],
) -> Answer {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    for (name, value) in extra {
        builder = builder.header(*name, *value);
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

// ---------------------------------------------------------------------------
// AC1: el flujo Raw real sobre el catálogo Dgraph.
// ---------------------------------------------------------------------------

/// El flujo Raw real: publish → GET/HEAD/Range contra el router del hub,
/// con el catálogo Dgraph y el CAS de ficheros de verdad. El mismo digest
/// que se publica es el que se sirve, y la cuenta es la que se cuenta.
#[tokio::test]
async fn the_raw_flow_serves_the_same_digest_and_count_over_dgraph() {
    let Some(_) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    let account = account("flow");
    let vertical = Vertical::mount(&account, "releases").await;

    let content = b"the quick brown fox jumps over the lazy dog\n";
    let published = send(
        &vertical.app,
        Method::PUT,
        "/repository/releases/their/app.bin",
        Some("alice"),
        Some(content.to_vec()),
        &[],
    )
    .await;
    assert_eq!(
        published.status,
        StatusCode::CREATED,
        "{:?}",
        published.body
    );

    let digest = ContentDigest::of_bytes(content);

    // GET: los bytes que se publicaron son los que se sirven.
    let downloaded = send(
        &vertical.app,
        Method::GET,
        "/repository/releases/their/app.bin",
        Some("alice"),
        None,
        &[],
    )
    .await;
    assert_eq!(downloaded.status, StatusCode::OK);
    assert_eq!(downloaded.body, content);
    assert_eq!(
        downloaded.header(header::ETAG.as_str()),
        Some(format!("\"sha256:{}\"", digest.value).as_str()),
        "the served digest is the published one"
    );

    // HEAD: la cuenta que se sirve es la que se publicó.
    let described = send(
        &vertical.app,
        Method::HEAD,
        "/repository/releases/their/app.bin",
        Some("alice"),
        None,
        &[],
    )
    .await;
    assert_eq!(described.status, StatusCode::OK);
    assert_eq!(
        described.header(header::CONTENT_LENGTH.as_str()),
        Some(content.len().to_string().as_str())
    );

    // Range: un trozo del contenido, no todo.
    let ranged = send(
        &vertical.app,
        Method::GET,
        "/repository/releases/their/app.bin",
        Some("alice"),
        None,
        &[("range", "bytes=0-9")],
    )
    .await;
    assert_eq!(ranged.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(ranged.body, &content[0..10]);

    // Y el catálogo dice lo mismo que se sirvió.
    let asset = vertical
        .reader
        .asset(&account, "releases", "their/app.bin")
        .await
        .expect("the asset");
    assert_eq!(asset.digest, digest);
    assert_eq!(asset.size_bytes, content.len() as u64);
    let reference = vertical
        .reader
        .reference(&account, "releases", "their/app.bin")
        .await
        .expect("the reference");
    assert_eq!(reference.digest, digest);
    assert_eq!(reference.scope, DigestScope::Global);
}

/// El commit del catálogo y el evento de auditoría son UNA transacción:
/// el evento está si y solo si el asset está, y dice el hecho de ESTE
/// asset.
#[tokio::test]
async fn the_publish_commit_and_its_audit_event_are_one_transaction() {
    let Some(_) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    let account = account("atomic");
    let client = fresh_client().await.expect("the client connects");
    let store = open_store(client.clone()).await;
    store
        .create_repository(&account, a_descriptor(&account, "raw"))
        .await
        .expect("the repository");

    let content = b"one atomic fact";
    store
        .publish(
            &account,
            a_publication(&account, "raw", "app/one.bin", content),
        )
        .await
        .expect("the publication");

    // El asset está y el evento también, con el mismo digest. El censo va
    // por tenant (la cuenta es única por corrida) y la correlación propia
    // filtra en el adaptador.
    let asset = store
        .asset(&account, "raw", "app/one.bin")
        .await
        .expect("the asset");
    let own: Vec<_> = the_events_of(&client, &account)
        .await
        .into_iter()
        .filter(|fact| fact.correlation_id == correlation_of(&account))
        .collect();
    assert_eq!(own.len(), 1, "one publish, one event");
    let event = &own[0];
    assert_eq!(event.resource, "raw/app/one.bin");
    assert_eq!(event.action, "publish");
    assert_eq!(event.result, AuditResult::Allowed.to_string());
    assert_eq!(event.tenant, account);
    assert_eq!(
        event.digest,
        format!(
            "{}:{}",
            ContentDigest::of_bytes(content).algorithm,
            ContentDigest::of_bytes(content).value
        )
    );
    assert_eq!(asset.digest, ContentDigest::of_bytes(content));
}

/// Una fila de evento ya dicha con OTRO contenido impide la publicación
/// entera: el asset no se publica sin su evento, y la referencia al blob
/// tampoco queda.
#[tokio::test]
async fn an_event_id_already_said_with_another_fact_blocks_the_publication() {
    let Some(_) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    let account = account("contra");
    let client = fresh_client().await.expect("the client connects");
    // El generador cuenta, así que el identificador del evento se sabe
    // antes de publicar; el prefijo lleva la cuenta de la ejecución para
    // que ningún evento de otra ejecución choque con el suyo.
    let identity = Identity::new(
        Arc::new(FixedClock::at("2026-11-20T00:00:00Z").expect("the moment")),
        Arc::new(CountingIdGenerator::with_prefix(format!(
            "event-{account}-"
        ))),
    );
    let store = open_store(client.clone()).await.with_identity(identity);
    store
        .create_repository(&account, a_descriptor(&account, "raw"))
        .await
        .expect("the repository");
    let outbox = DgraphAuditOutbox::new(client);

    // La fila del evento que la publicación va a escribir ya está dicha
    // con otro digest: la escritura del evento va a chocar.
    let seeded = AuditEvent::new(
        format!("event-{account}-0"),
        AuditContext::new(actor(&account), account.clone(), correlation_of(&account)),
        Utc::now(),
        "publish",
        "raw/app/one.bin",
        Some("sha256:0000".to_string()),
        AuditResult::Allowed,
    );
    outbox
        .put_pending(&seeded)
        .await
        .expect("the event is said");

    let outcome = store
        .publish(
            &account,
            a_publication(&account, "raw", "app/one.bin", b"one"),
        )
        .await;
    assert!(
        outcome.is_err(),
        "the publication should not have succeeded"
    );
    let missing = store.asset(&account, "raw", "app/one.bin").await;
    assert!(
        missing.is_err(),
        "the asset should not be there without its event: {missing:?}"
    );
    // El evento sigue siendo el que se dijo antes: la publicación no
    // escribió una contradicción encima.
    let entry = outbox
        .entry(&format!("event-{account}-0"))
        .await
        .expect("the entry")
        .expect("the entry");
    assert_eq!(entry.event.digest.as_deref(), Some("sha256:0000"));
}

/// Republicar lo mismo no es un error ni una escritura: un solo asset, un
/// solo evento. Y una publicación que choca deja un evento de conflicto.
#[tokio::test]
async fn a_republication_writes_nothing_and_a_conflict_says_a_conflict_event() {
    let Some(_) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    let account = account("idem");
    let client = fresh_client().await.expect("the client connects");
    let store = open_store(client.clone()).await;
    store
        .create_repository(&account, a_descriptor(&account, "raw"))
        .await
        .expect("the repository");

    let first = store
        .publish(
            &account,
            a_publication(&account, "raw", "app/one.bin", b"one"),
        )
        .await
        .expect("the publication");
    assert!(first.created);

    let second = store
        .publish(
            &account,
            a_publication(&account, "raw", "app/one.bin", b"one"),
        )
        .await
        .expect("the republication");
    assert!(!second.created, "the republication did not write anything");
    assert_eq!(first.asset.digest, second.asset.digest);
    assert_eq!(
        the_event_count(&client, &account).await,
        1,
        "no new fact was said"
    );

    // La misma ruta con otro contenido: conflicto, y el conflicto se dice.
    let refused = store
        .publish(
            &account,
            a_publication(&account, "raw", "app/one.bin", b"another"),
        )
        .await;
    assert!(matches!(
        refused,
        Err(hodei_catalog::CatalogError::Conflict { .. })
    ));
    let correlation = correlation_of(&account);
    let own: Vec<_> = the_events_of(&client, &account)
        .await
        .into_iter()
        .filter(|fact| fact.correlation_id == correlation)
        .collect();
    assert!(
        own.iter()
            .any(|fact| fact.result == AuditResult::Conflict.to_string()),
        "the conflict is said"
    );
    assert_eq!(own.len(), 2, "one allowed, one conflict");
    let asset = store
        .asset(&account, "raw", "app/one.bin")
        .await
        .expect("the asset is intact");
    assert_eq!(asset.digest, ContentDigest::of_bytes(b"one"));
}

// ---------------------------------------------------------------------------
// AC2: dos escritores, el reintento y el huérfano seguro.
// ---------------------------------------------------------------------------

/// Dos escritores publicando LO MISMO a la vez dejan un asset y un evento:
/// la guarda de la ruta y el marcador once-only deciden en el servidor.
#[tokio::test]
async fn two_writers_publishing_the_same_thing_leave_one_asset_and_one_event() {
    let Some(_) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    let account = account("race");
    let client = fresh_client().await.expect("the client connects");
    let left = Arc::new(open_store(client.clone()).await);
    let right = Arc::new(open_store(client.clone()).await);
    left.create_repository(&account, a_descriptor(&account, "releases"))
        .await
        .expect("the repository");

    // Dos clientes, el MISMO contenido, la MISMA ruta: una creación y una
    // constatación, no dos assets.
    let same_left = left.publish(
        &account,
        a_publication(&account, "releases", "app.bin", b"the same content"),
    );
    let same_right = right.publish(
        &account,
        a_publication(&account, "releases", "app.bin", b"the same content"),
    );
    let (same_left, same_right) = tokio::join!(same_left, same_right);
    let outcomes = [same_left, same_right];
    let created = outcomes
        .iter()
        .filter(|result| matches!(result, Ok(outcome) if outcome.created))
        .count();
    let idempotent = outcomes
        .iter()
        .filter(|result| matches!(result, Ok(outcome) if !outcome.created))
        .count();
    assert_eq!(created, 1, "one client created the asset");
    assert_eq!(idempotent, 1, "the other one found it already there");
    assert_eq!(
        the_event_count(&client, &account).await,
        1,
        "one fact was said"
    );

    // Ahora dos clientes publican CONTENIDO DISTINTO en la misma ruta: el
    // que pierde recibe un conflicto, no una sobrescritura, y no deja su
    // versión a medias.
    // Coordenadas distintas a propósito: cada uno tiene su propia versión,
    // y el que pierde se encuentra con que la versión se puede escribir
    // pero el asset de la ruta ya es del otro.
    let conflict_left = left.publish(
        &account,
        a_publication_at(
            &account,
            "releases",
            "other.bin",
            "2.0.0",
            b"the left content",
        ),
    );
    let conflict_right = right.publish(
        &account,
        a_publication_at(
            &account,
            "releases",
            "other.bin",
            "3.0.0",
            b"the right content",
        ),
    );
    let (conflict_left, conflict_right) = tokio::join!(conflict_left, conflict_right);
    let results = [conflict_left, conflict_right];
    for result in &results {
        if let Err(error) = result {
            eprintln!("conflict race error: {error}");
        }
    }
    let winners = results.iter().filter(|result| result.is_ok()).count();
    let conflicts = results
        .iter()
        .filter(|result| matches!(result, Err(hodei_catalog::CatalogError::Conflict { .. })))
        .count();
    assert_eq!(winners, 1, "one client won the race");
    assert_eq!(conflicts, 1, "the other got a conflict, not an overwrite");

    // Lo que quedó es lo que el ganador publicó, y el perdedor no dejó su
    // versión detrás.
    let asset = left
        .asset(&account, "releases", "other.bin")
        .await
        .expect("the asset");
    let digest = asset.digest.clone();
    assert!(
        digest == ContentDigest::of_bytes(b"the left content")
            || digest == ContentDigest::of_bytes(b"the right content"),
        "the asset holds a digest nobody published"
    );
    let loser_version = if asset.coordinates.package_version == "2.0.0" {
        PackageCoordinates {
            package_name: "pkg".to_string(),
            package_version: "3.0.0".to_string(),
        }
    } else {
        PackageCoordinates {
            package_name: "pkg".to_string(),
            package_version: "2.0.0".to_string(),
        }
    };
    // La versión del perdedor no está: su transacción se fue entera.
    let loser_left_behind = left.version(&account, "releases", &loser_version).await;
    assert!(
        loser_left_behind.is_err(),
        "the loser of the race left a version behind: {loser_left_behind:?}"
    );
    // Y la versión a la que apunta el asset existe y dice lo mismo.
    let version = left
        .version(&account, "releases", &asset.coordinates)
        .await
        .expect("the version behind the asset");
    assert_eq!(version.digest, asset.digest);
}

/// El reintento tras el commit relee el resultado comprometido: un solo
/// asset, un solo evento, ni una fila de más.
#[tokio::test]
async fn a_retry_after_the_commit_keeps_one_asset_and_one_event() {
    let Some(_) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    let account = account("retry");
    let client = fresh_client().await.expect("the client connects");
    let store = open_store(client.clone()).await;
    store
        .create_repository(&account, a_descriptor(&account, "raw"))
        .await
        .expect("the repository");

    let publication = a_publication(&account, "raw", "app/one.bin", b"one");
    let first = store
        .publish(&account, publication.clone())
        .await
        .expect("the first");
    assert!(first.created);

    // El reintento: la misma operación, dicho otra vez. La respuesta es
    // la comprometida, y no se escribe nada.
    for _attempt in 0..3 {
        let retry = store
            .publish(&account, publication.clone())
            .await
            .expect("the retry");
        assert!(!retry.created, "the retry did not write anything");
        assert_eq!(retry.asset.digest, first.asset.digest);
        assert_eq!(retry.version.digest, first.version.digest);
        assert_eq!(retry.reference.digest, first.reference.digest);
    }
    assert_eq!(
        the_event_count(&client, &account).await,
        1,
        "one fact, said once"
    );
    let asset = store
        .asset(&account, "raw", "app/one.bin")
        .await
        .expect("the asset");
    assert_eq!(asset.digest, ContentDigest::of_bytes(b"one"));
}

/// Un blob del CAS sin metadata de catálogo queda **huérfano seguro**:
/// detectable por censo (las claves del CAS que ninguna referencia del
/// catálogo reclama), y el catálogo sigue sirviendo lo que sí tiene.
///
/// La política de este slice: **referencia de huérfanos por censo**, no
/// GC-aware. Un blob staged cuya publicación se cayó es reconciliable —
/// la referencia es lo que impide que se recoja, así que un blob sin
/// referencia no rompe nada; el censo lo nombra y la recolección (HSP-104)
/// decide su destino.
#[tokio::test]
async fn a_cas_blob_without_catalog_metadata_is_a_detectable_safe_orphan() {
    let Some(_) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    let account = account("orphan");
    let vertical = Vertical::mount(&account, "releases").await;
    let client = fresh_client().await.expect("the client connects");

    // Un blob que entra al CAS y cuya publicación NUNCA ocurre.
    let orphan_bytes = b"the bytes that nobody catalogued";
    let orphan_digest = vertical
        .content
        .put(orphan_bytes)
        .await
        .expect("the blob is staged");

    // El blob que SÍ se publica.
    let content = b"the published bytes";
    let published = send(
        &vertical.app,
        Method::PUT,
        "/repository/releases/app.bin",
        Some("alice"),
        Some(content.to_vec()),
        &[],
    )
    .await;
    assert_eq!(published.status, StatusCode::CREATED);

    // El censo: los digests que el catálogo reclama frente a los que el
    // CAS guarda. El huérfano se nombra, el publicado no.
    let digests_of_catalog = |account: &str| {
        let client = client.clone();
        let account = account.to_string();
        async move {
            let query = format!(
                "{{ q(func: eq(hodei.catalog.asset.account, {})) {{ hodei.catalog.asset.digest }} \
                 r(func: eq(hodei.catalog.reference.account, {})) {{ hodei.catalog.reference.digest }} }}",
                hodei_graph_client::dql_string(&account),
                hodei_graph_client::dql_string(&account),
            );
            let response = client.execute_dql(&query).await.expect("the census");
            let mut claimed = Vec::new();
            for root in ["q", "r"] {
                if let Some(nodes) = response
                    .json
                    .get(root)
                    .and_then(serde_json::Value::as_array)
                {
                    for node in nodes {
                        if let Some(digest) = node
                            .get("hodei.catalog.asset.digest")
                            .and_then(|v| v.as_str())
                        {
                            claimed.push(digest.to_string());
                        }
                        if let Some(digest) = node
                            .get("hodei.catalog.reference.digest")
                            .and_then(|v| v.as_str())
                        {
                            claimed.push(digest.to_string());
                        }
                    }
                }
            }
            claimed
        }
    };
    let claimed = digests_of_catalog(&account).await;
    assert!(
        claimed
            .iter()
            .any(|digest| digest == &ContentDigest::of_bytes(content).value),
        "the published blob is catalogued"
    );
    assert!(
        !claimed.iter().any(|digest| digest == &orphan_digest.value),
        "the orphan is not catalogued"
    );

    // Y el huérfano no rompe nada: el catálogo sigue sirviendo lo que
    // tiene, con el digest intacto.
    let served = send(
        &vertical.app,
        Method::GET,
        "/repository/releases/app.bin",
        Some("alice"),
        None,
        &[],
    )
    .await;
    assert_eq!(served.status, StatusCode::OK);
    assert_eq!(served.body, content);
    let missing = vertical
        .reader
        .asset(&account, "releases", "orphan.bin")
        .await;
    assert!(missing.is_err(), "the orphan has no catalog row to serve");
}

// ---------------------------------------------------------------------------
// La paridad de lectura con la referencia Surreal.
// ---------------------------------------------------------------------------

/// El mismo estado escrito en los dos almacenes se lee igual: la
/// referencia Surreal (HSP-96) y el port Dgraph dicen lo mismo de
/// repository, asset, versión y referencia.
#[tokio::test]
async fn the_dgraph_reads_match_the_surreal_reference_for_the_same_state() {
    let Some(_) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    let account = account("parity");
    let repo = "releases";
    let path = "parity/app.bin";
    let content = b"the same state in two engines";

    // El port Dgraph.
    let client = fresh_client().await.expect("the client connects");
    let dgraph = open_store(client.clone()).await;
    dgraph
        .create_repository(&account, a_descriptor(&account, repo))
        .await
        .expect("the dgraph repository");

    // La referencia Surreal, sobre su motor persistente.
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = Surreal::new::<RocksDb>(tmp.path().join("db"))
        .await
        .expect("rocksdb");
    db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
    let surreal = SurrealCatalogStore::with_authorizer(db, Arc::new(AllowAllAuthorizer));
    surreal.initialize().await.expect("the surreal catalog");
    surreal
        .create_repository(&account, a_descriptor(&account, repo))
        .await
        .expect("the surreal repository");

    let publication = a_publication(&account, repo, path, content);
    let dgraph_outcome = dgraph
        .publish(&account, publication.clone())
        .await
        .expect("dgraph publish");
    let surreal_outcome = surreal
        .publish(&account, publication)
        .await
        .expect("surreal publish");

    // El mismo contenido declarado en las dos.
    assert_eq!(dgraph_outcome.asset.digest, surreal_outcome.asset.digest);
    assert_eq!(
        dgraph_outcome.version.digest,
        surreal_outcome.version.digest
    );
    assert_eq!(
        dgraph_outcome.reference.digest,
        surreal_outcome.reference.digest
    );
    assert!(dgraph_outcome.created && surreal_outcome.created);

    // Y las lecturas dicen lo mismo.
    let dgraph_repo = dgraph
        .repository(&account, repo)
        .await
        .expect("dgraph repository");
    let surreal_repo = surreal
        .repository(&account, repo)
        .await
        .expect("surreal repository");
    assert_eq!(dgraph_repo.account, surreal_repo.account);
    assert_eq!(dgraph_repo.repo_key, surreal_repo.repo_key);
    assert_eq!(dgraph_repo.format, surreal_repo.format);
    assert_eq!(dgraph_repo.mode, surreal_repo.mode);
    assert_eq!(dgraph_repo.hrn, surreal_repo.hrn);
    assert_eq!(dgraph_repo.config.version, surreal_repo.config.version);
    assert_eq!(
        dgraph_repo.config.digest_scope,
        surreal_repo.config.digest_scope
    );

    let dgraph_asset: AssetRecord = dgraph
        .asset(&account, repo, path)
        .await
        .expect("dgraph asset");
    let surreal_asset: AssetRecord = surreal
        .asset(&account, repo, path)
        .await
        .expect("surreal asset");
    assert_eq!(dgraph_asset.account, surreal_asset.account);
    assert_eq!(dgraph_asset.repository, surreal_asset.repository);
    assert_eq!(dgraph_asset.asset_path, surreal_asset.asset_path);
    assert_eq!(dgraph_asset.coordinates, surreal_asset.coordinates);
    assert_eq!(dgraph_asset.digest, surreal_asset.digest);
    assert_eq!(dgraph_asset.size_bytes, surreal_asset.size_bytes);
    assert_eq!(dgraph_asset.subject, surreal_asset.subject);

    let dgraph_version: PackageVersion = dgraph
        .version(&account, repo, &dgraph_asset.coordinates)
        .await
        .expect("dgraph version");
    let surreal_version: PackageVersion = surreal
        .version(&account, repo, &surreal_asset.coordinates)
        .await
        .expect("surreal version");
    assert_eq!(dgraph_version.digest, surreal_version.digest);
    assert_eq!(dgraph_version.size_bytes, surreal_version.size_bytes);
    assert_eq!(dgraph_version.coordinates, surreal_version.coordinates);

    let dgraph_reference: BlobReference = dgraph
        .reference(&account, repo, path)
        .await
        .expect("dgraph reference");
    let surreal_reference: BlobReference = surreal
        .reference(&account, repo, path)
        .await
        .expect("surreal reference");
    assert_eq!(dgraph_reference.digest, surreal_reference.digest);
    assert_eq!(dgraph_reference.scope, surreal_reference.scope);
    assert_eq!(dgraph_reference.size_bytes, surreal_reference.size_bytes);
    assert_eq!(dgraph_reference.trust_state, surreal_reference.trust_state);
    assert_eq!(dgraph_reference.account, surreal_reference.account);
    assert_eq!(dgraph_reference.repository, surreal_reference.repository);
    assert_eq!(dgraph_reference.asset_path, surreal_reference.asset_path);

    // La etiqueta mutable se mueve en las dos, y las dos dicen lo mismo:
    // la misma etiqueta apunta a la misma versión en los dos motores.
    let dgraph_tag = dgraph
        .move_tag(&account, repo, "stable", dgraph_asset.coordinates.clone())
        .await
        .expect("dgraph tag");
    let surreal_tag = surreal
        .move_tag(&account, repo, "stable", surreal_asset.coordinates.clone())
        .await
        .expect("surreal tag");
    assert_eq!(dgraph_tag.account, surreal_tag.account);
    assert_eq!(dgraph_tag.repository, surreal_tag.repository);
    assert_eq!(dgraph_tag.tag_name, surreal_tag.tag_name);
    assert_eq!(dgraph_tag.coordinates, surreal_tag.coordinates);

    let dgraph_tag_read = dgraph
        .tag(&account, repo, "stable")
        .await
        .expect("dgraph tag read");
    let surreal_tag_read = surreal
        .tag(&account, repo, "stable")
        .await
        .expect("surreal tag read");
    assert_eq!(dgraph_tag_read.coordinates, surreal_tag_read.coordinates);

    let dgraph_tags = dgraph.tags(&account, repo).await.expect("dgraph tags");
    let surreal_tags = surreal.tags(&account, repo).await.expect("surreal tags");
    assert_eq!(dgraph_tags.len(), surreal_tags.len());
    assert_eq!(dgraph_tags.len(), 1);
    assert_eq!(dgraph_tags[0].coordinates, surreal_tags[0].coordinates);
}
