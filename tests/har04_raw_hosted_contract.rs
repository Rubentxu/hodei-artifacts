//! The raw hosted content surface (HAR-04).
//!
//! # What this file asserts
//!
//! The surface serves **bytes at a path**: `PUT` publishes, `GET`
//! downloads, `HEAD` describes. The five acceptance criteria of HSP-98
//! are asserted one by one, and each criterion has at least one
//! **discriminating negative**: a test that only proves the happy path
//! also passes when the guarantee is not there.
//!
//! - **AC1** A `PUT` publishes an asset at a path under a raw hosted
//!   repository; `GET` and `HEAD` return the content intact, with the
//!   `ETag` of the digest and the right size.
//! - **AC2** `Range` is RFC 9110: `206` for a range that fits, `416` for
//!   one that does not; `If-None-Match` and `If-Match` behave as the
//!   versioned contract says.
//! - **AC3** Valid paths are preserved; traversal, double encoding,
//!   ambiguous paths and control characters are refused; a replacement
//!   outside the policy of the repository is refused too.
//! - **AC4** `401`, `403`/`404` and `409` are coherent; Cedar authorizes
//!   the upload and the download without a way around it, and the
//!   existence of a repository of another account is not exposed.
//! - **AC5** is not here: it needs a real process and a real `curl`, and
//!   it lives in `tests/raw_hosted_restart_contract.rs`.
//!
//! The router is driven in process, through `tower::ServiceExt::oneshot`:
//! no port is bound and no process is spawned. The catalog and the store
//! of content are **real** (SurrealDB on disk and the file CAS), because
//! a contract asserted against a mock proves that the mock behaves.

use std::sync::Arc;

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
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use hodei_catalog::{
    AllowAllAuthorizer, CatalogReader, CatalogWriter, DigestScope, RepositoryConfig,
    RepositoryDescriptor, RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_catalog_storage::SurrealCatalogStore;
use hodei_evidence::domain::content::ContentDigest;
use hodei_hub::blob::FileBlobStore;
use serde_json::{Value, json};
use surrealdb::Surreal;
use surrealdb::engine::local::RocksDb;
use tempfile::TempDir;
use tower::ServiceExt;

/// The account of the principal that is allowed to publish and download.
const ACCOUNT: &str = "acme";

/// The principal that is allowed to publish and download.
const ALLOWED: &str = "alice";

/// The principal of the same account that is **not** allowed.
const NOT_ALLOWED: &str = "bob";

/// The principal of another account.
const OUTSIDER: &str = "mallory";

/// The repository the contract serves.
const REPOSITORY: &str = "releases";

/// The content of the first publication.
const CONTENT: &[u8] = b"the quick brown fox jumps over the lazy dog\n";

/// The limit of the body, in bytes, in the contracts that do not change it.
const DEFAULT_BODY_LIMIT: usize = 10 * 1024 * 1024;

/// A second content, for the paths that are already occupied.
const OTHER_CONTENT: &[u8] = b"a completely different content\n";

struct ContractAuthenticator;

#[async_trait]
impl PrincipalAuthenticator for ContractAuthenticator {
    async fn authenticate(
        &self,
        credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        let token = match credentials {
            Credentials::Bearer(token) => token.as_str(),
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        let (account, name) = match token {
            ALLOWED => (ACCOUNT, ALLOWED),
            NOT_ALLOWED => (ACCOUNT, NOT_ALLOWED),
            OUTSIDER => ("other", OUTSIDER),
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        Ok(PrincipalClaims::new(
            PrincipalId::new(format!("hrn:hodei:iam::{account}:User/{name}")).unwrap(),
            AccountId::new(account).unwrap(),
        ))
    }
}

/// The Cedar bundle: only `alice` may publish or download.
fn cedar() -> Arc<dyn AuthorizerPort> {
    let policy = r#"permit(
        principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
        action == Artifacts::Action::"UploadAsset",
        resource == Artifacts::Repository::"hrn:hodei:artifacts:eu:acme:repository/releases"
    );
    permit(
        principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
        action == Artifacts::Action::"DownloadAsset",
        resource == Artifacts::Repository::"hrn:hodei:artifacts:eu:acme:repository/releases"
    );"#;
    Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .expect("authorizer"),
    )))
}

/// The catalog of the contract, and the store of content.
///
/// They are built here and injected, so the contract can create the
/// repository it serves and can look at what was written.
struct Contract {
    app: Router,
    writer: Arc<dyn CatalogWriter>,
    /// El almacén de contenido, para poder mirar lo que queda en el.
    content: Arc<FileBlobStore>,
    #[allow(dead_code)]
    tmp: TempDir,
}

impl Contract {
    /// A contract with a repository that is raw, hosted and publishes once.
    async fn new() -> Self {
        let contract = Self::without_repository().await;
        contract
            .create_repository(a_descriptor(
                ACCOUNT,
                REPOSITORY,
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                json!({}),
            ))
            .await;
        contract
    }

    /// A contract with no repository at all.
    async fn without_repository() -> Self {
        Self::with_body_limit(DEFAULT_BODY_LIMIT).await
    }

    /// A contract that admits at most `limit` bytes of content.
    async fn with_body_limit(limit: usize) -> Self {
        let tmp = tempfile::tempdir().expect("temp dir");

        let mut config = AppConfig::default();
        // The raw surface is closed by default; this contract opens it on
        // purpose, the way a deployment that serves content does.
        config.security.raw_api_enabled = true;
        // The catalog and the store of content are built below and
        // injected, so the contract owns them.
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
        .expect("bootstrap must succeed for the raw contract");

        let db = Surreal::new::<RocksDb>(tmp.path().join("catalog.rocksdb"))
            .await
            .expect("rocksdb");
        db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
        let catalog = Arc::new(SurrealCatalogStore::with_authorizer(
            db,
            Arc::new(AllowAllAuthorizer),
        ));
        catalog.initialize().await.expect("catalog initialize");
        let reader: Arc<dyn CatalogReader> = catalog.clone();
        let writer: Arc<dyn CatalogWriter> = catalog.clone();

        let content =
            Arc::new(FileBlobStore::new(tmp.path().join("cas")).expect("the store of content"));

        let state = state
            .with_catalog(reader.clone(), writer.clone())
            .with_content_store(content.clone())
            .with_body_limit(limit)
            .with_authenticator(Arc::new(ContractAuthenticator))
            .with_authorization(cedar());

        Self {
            app: build_router(state, &config),
            writer,
            content,
            tmp,
        }
    }

    /// Creates a repository in the catalog.
    async fn create_repository(&self, descriptor: RepositoryDescriptor) {
        let account = descriptor.account.clone();
        self.writer
            .create_repository(account.as_str(), descriptor)
            .await
            .expect("the repository is created");
    }
}

/// A repository descriptor.
fn a_descriptor(
    account: &str,
    repo_key: &str,
    format: RepositoryFormat,
    mode: RepositoryMode,
    settings: Value,
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

/// The answer of a request: the status, the headers and the bytes.
#[derive(Debug)]
struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl Answer {
    /// The value of a header.
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }

    /// The body, parsed as JSON.
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

/// Sends one request to the router.
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

/// Publishes the content of the fixture and returns the answer.
async fn publish(app: &Router, path: &str, content: &[u8]) -> Answer {
    send(
        app,
        Method::PUT,
        &format!("/repository/{REPOSITORY}/{path}"),
        Some(ALLOWED),
        Some(content.to_vec()),
        &[],
    )
    .await
}

/// The digest of a content, in hexadecimal.
fn digest_of(content: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content);
    let bytes = hasher.finalize();
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

// ---------------------------------------------------------------------------
// AC1 — publish, download and describe.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_publication_is_downloaded_intact_with_its_digest_and_its_size() {
    let contract = Contract::new().await;

    let published = publish(&contract.app, "path/to/file.bin", CONTENT).await;
    assert_eq!(published.status, StatusCode::CREATED);
    let body = published.json();
    assert_eq!(body["digest"], format!("sha256:{}", digest_of(CONTENT)));
    assert_eq!(body["size_bytes"], CONTENT.len() as u64);

    let downloaded = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/path/to/file.bin"),
        Some(ALLOWED),
        None,
        &[],
    )
    .await;
    assert_eq!(downloaded.status, StatusCode::OK);
    assert_eq!(downloaded.body, CONTENT);
    assert_eq!(
        downloaded.header(header::ETAG.as_str()),
        Some(format!("\"sha256:{}\"", digest_of(CONTENT)).as_str())
    );
    assert_eq!(
        downloaded.header(header::CONTENT_TYPE.as_str()),
        Some("application/octet-stream")
    );
    assert_eq!(
        downloaded.header(header::ACCEPT_RANGES.as_str()),
        Some("bytes")
    );
}

#[tokio::test]
async fn the_head_describes_the_content_without_serving_it() {
    let contract = Contract::new().await;
    publish(&contract.app, "file.bin", CONTENT).await;

    let described = send(
        &contract.app,
        Method::HEAD,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[],
    )
    .await;
    assert_eq!(described.status, StatusCode::OK);
    // El HEAD dice cuanto tiene el contenido, y no trae ni un byte.
    assert_eq!(
        described.header(header::CONTENT_LENGTH.as_str()),
        Some(CONTENT.len().to_string().as_str())
    );
    assert!(described.body.is_empty());
    assert_eq!(
        described.header(header::ETAG.as_str()),
        Some(format!("\"sha256:{}\"", digest_of(CONTENT)).as_str())
    );
}

#[tokio::test]
async fn the_head_of_a_range_says_the_length_of_the_range() {
    let contract = Contract::new().await;
    publish(&contract.app, "file.bin", CONTENT).await;

    // Un HEAD con un tramo dice lo que diria el GET con el mismo tramo:
    // la longitud del tramo, y no la del contenido entero. Un HEAD que
    // dice cien y un GET que devuelve diez son dos servidores que no se
    // conocen.
    let described = send(
        &contract.app,
        Method::HEAD,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[("range", "bytes=0-4")],
    )
    .await;
    assert_eq!(described.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(described.header(header::CONTENT_LENGTH.as_str()), Some("5"));
    assert_eq!(
        described.header(header::CONTENT_RANGE.as_str()),
        Some(format!("bytes 0-4/{}", CONTENT.len()).as_str())
    );
    assert!(described.body.is_empty());
}

#[tokio::test]
async fn a_content_that_is_not_there_is_a_not_found() {
    let contract = Contract::new().await;

    let answer = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/nothing.bin"),
        Some(ALLOWED),
        None,
        &[],
    )
    .await;
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_content_survives_the_round_trip_through_the_store() {
    let contract = Contract::new().await;
    publish(&contract.app, "file.bin", CONTENT).await;

    // El contenido se lee del almacén de contenido, no de la memoria de
    // la petición: lo que se descarga es lo que se publico.
    let downloaded = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[],
    )
    .await;
    assert_eq!(downloaded.body, CONTENT);
    assert_eq!(downloaded.body.len(), CONTENT.len());
}

// ---------------------------------------------------------------------------
// AC2 — Range and the conditional headers.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_range_that_fits_is_served_as_partial_content() {
    let contract = Contract::new().await;
    publish(&contract.app, "file.bin", CONTENT).await;

    let answer = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[("range", "bytes=0-4")],
    )
    .await;
    assert_eq!(answer.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(answer.body, &CONTENT[0..5]);
    assert_eq!(
        answer.header(header::CONTENT_RANGE.as_str()),
        Some(format!("bytes 0-4/{}", CONTENT.len()).as_str())
    );
}

#[tokio::test]
async fn the_forms_of_a_range_are_served_as_they_are_written() {
    let contract = Contract::new().await;
    publish(&contract.app, "file.bin", CONTENT).await;

    // Un tramo con sufijo: los últimos bytes.
    let suffix = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[("range", "bytes=-5")],
    )
    .await;
    assert_eq!(suffix.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(suffix.body, &CONTENT[CONTENT.len() - 5..]);

    // Un tramo abierto: desde ahí hasta el final.
    let open = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[("range", &format!("bytes={}-", CONTENT.len() - 3))],
    )
    .await;
    assert_eq!(open.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(open.body, &CONTENT[CONTENT.len() - 3..]);

    // Varios tramos: esta release sirve uno, así que se ignora la
    // petición y se sirve el contenido entero, que es lo que permite el
    // RFC.
    let several = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[("range", "bytes=0-4,10-14")],
    )
    .await;
    assert_eq!(several.status, StatusCode::OK);
    assert_eq!(several.body, CONTENT);

    // Otra unidad: tampoco se entiende, y lo que no se entiende se
    // ignora.
    let other = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[("range", "items=0-4")],
    )
    .await;
    assert_eq!(other.status, StatusCode::OK);
    assert_eq!(other.body, CONTENT);

    // Y la unidad no distingue mayúsculas: `Bytes=0-4` es un tramo.
    let capital = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[("range", "Bytes=0-4")],
    )
    .await;
    assert_eq!(capital.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(capital.body, &CONTENT[0..5]);
}

#[tokio::test]
async fn a_range_that_does_not_fit_is_not_satisfiable() {
    let contract = Contract::new().await;
    publish(&contract.app, "file.bin", CONTENT).await;

    let answer = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[(
            "range",
            &format!("bytes={}-{}", CONTENT.len() + 10, CONTENT.len() + 20),
        )],
    )
    .await;
    assert_eq!(answer.status, StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(
        answer.header(header::CONTENT_RANGE.as_str()),
        Some(format!("bytes */{}", CONTENT.len()).as_str())
    );
}

#[tokio::test]
async fn a_range_that_is_not_a_range_is_not_satisfiable() {
    let contract = Contract::new().await;
    publish(&contract.app, "file.bin", CONTENT).await;

    let answer = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[("range", "bytes=abc-def")],
    )
    .await;
    assert_eq!(answer.status, StatusCode::RANGE_NOT_SATISFIABLE);
}

#[tokio::test]
async fn a_conditional_read_that_holds_is_not_modified() {
    let contract = Contract::new().await;
    publish(&contract.app, "file.bin", CONTENT).await;
    let etag = format!("\"sha256:{}\"", digest_of(CONTENT));

    let answer = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[("if-none-match", &etag)],
    )
    .await;
    assert_eq!(answer.status, StatusCode::NOT_MODIFIED);
    assert!(answer.body.is_empty());
    assert_eq!(answer.header(header::ETAG.as_str()), Some(etag.as_str()));
}

#[tokio::test]
async fn a_weak_conditional_read_is_not_modified_too() {
    // RFC 9110: `If-None-Match` se compara con la función **débil**, y
    // un `W/` delante dice "comparte la representación". Un cliente que
    // revalida contra una caché manda la etiqueta así, y un 304 es lo
    // que espera.
    let contract = Contract::new().await;
    publish(&contract.app, "file.bin", CONTENT).await;
    let etag = format!("W/\"sha256:{}\"", digest_of(CONTENT));

    let answer = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[("if-none-match", &etag)],
    )
    .await;
    assert_eq!(answer.status, StatusCode::NOT_MODIFIED);
    assert!(answer.body.is_empty());
}

#[tokio::test]
async fn a_conditional_read_that_does_not_hold_is_a_precondition_failure() {
    let contract = Contract::new().await;
    publish(&contract.app, "file.bin", CONTENT).await;

    let answer = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[(
            "if-match",
            "\"sha256:0000000000000000000000000000000000000000000000000000000000000000\"",
        )],
    )
    .await;
    assert_eq!(answer.status, StatusCode::PRECONDITION_FAILED);
}

#[tokio::test]
async fn a_conditional_write_that_does_not_hold_leaves_the_content_alone() {
    let contract = Contract::new().await;
    publish(&contract.app, "file.bin", CONTENT).await;

    let answer = send(
        &contract.app,
        Method::PUT,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        Some(OTHER_CONTENT.to_vec()),
        &[(
            "if-match",
            "\"sha256:0000000000000000000000000000000000000000000000000000000000000000\"",
        )],
    )
    .await;
    assert_eq!(answer.status, StatusCode::PRECONDITION_FAILED);

    // El contenido que hay es el de antes: la petición que no se hace no
    // deja nada escrito.
    let downloaded = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[],
    )
    .await;
    assert_eq!(downloaded.body, CONTENT);
}

#[tokio::test]
async fn a_conditional_write_that_holds_goes_on() {
    let contract = Contract::new().await;
    publish(&contract.app, "file.bin", CONTENT).await;
    let etag = format!("\"sha256:{}\"", digest_of(CONTENT));

    let answer = send(
        &contract.app,
        Method::PUT,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        Some(CONTENT.to_vec()),
        &[("if-match", &etag)],
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
}

#[tokio::test]
async fn a_conditional_write_with_a_star_on_an_empty_path_fails() {
    // `*` quiere decir "el recurso que hay", y si no hay recurso no hay
    // nada que comparar: la escritura no se hace.
    let contract = Contract::new().await;

    let answer = send(
        &contract.app,
        Method::PUT,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        Some(CONTENT.to_vec()),
        &[("if-match", "*")],
    )
    .await;
    assert_eq!(answer.status, StatusCode::PRECONDITION_FAILED);

    // Y no se publicó nada.
    let downloaded = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[],
    )
    .await;
    assert_eq!(downloaded.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_conditional_write_with_a_star_on_an_occupied_path_goes_on() {
    let contract = Contract::new().await;
    publish(&contract.app, "file.bin", CONTENT).await;

    let answer = send(
        &contract.app,
        Method::PUT,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        Some(CONTENT.to_vec()),
        &[("if-match", "*")],
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
}

#[tokio::test]
async fn a_conditional_write_with_an_if_none_match_star_on_a_free_path_goes_on() {
    let contract = Contract::new().await;

    let answer = send(
        &contract.app,
        Method::PUT,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        Some(CONTENT.to_vec()),
        &[("if-none-match", "*")],
    )
    .await;
    assert_eq!(answer.status, StatusCode::CREATED);
}

#[tokio::test]
async fn a_conditional_write_with_an_if_none_match_star_on_an_occupied_path_fails() {
    let contract = Contract::new().await;
    publish(&contract.app, "file.bin", CONTENT).await;

    let answer = send(
        &contract.app,
        Method::PUT,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        Some(OTHER_CONTENT.to_vec()),
        &[("if-none-match", "*")],
    )
    .await;
    assert_eq!(answer.status, StatusCode::PRECONDITION_FAILED);

    // Y lo que había sigue siendo lo que había: la condición dice que no
    // se escriba, no que se sustituya.
    let downloaded = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[],
    )
    .await;
    assert_eq!(downloaded.status, StatusCode::OK);
    assert_eq!(downloaded.body, CONTENT);
}

// ---------------------------------------------------------------------------
// AC3 — the rules of the path, and the policy of the replacement.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_valid_path_is_preserved_as_it_was_written() {
    let contract = Contract::new().await;
    let published = publish(&contract.app, "a/b/c/file.bin", CONTENT).await;
    assert_eq!(published.status, StatusCode::CREATED);
    assert_eq!(published.json()["path"], "a/b/c/file.bin");

    let downloaded = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/a/b/c/file.bin"),
        Some(ALLOWED),
        None,
        &[],
    )
    .await;
    assert_eq!(downloaded.status, StatusCode::OK);
    assert_eq!(downloaded.body, CONTENT);
}

#[tokio::test]
async fn an_encoded_path_is_the_name_that_it_encodes() {
    let contract = Contract::new().await;

    // `%41` es `A`: el nombre que se guarda es el literal, y no hay dos
    // nombres para lo mismo.
    let published = publish(&contract.app, "informe%41final", CONTENT).await;
    assert_eq!(published.status, StatusCode::CREATED);
    assert_eq!(published.json()["path"], "informeAfinal");

    // Y el contenido se sirve por cualquiera de las dos formas, porque
    // son el mismo nombre.
    for path in ["informe%41final", "informeAfinal"] {
        let downloaded = send(
            &contract.app,
            Method::GET,
            &format!("/repository/{REPOSITORY}/{path}"),
            Some(ALLOWED),
            None,
            &[],
        )
        .await;
        assert_eq!(downloaded.status, StatusCode::OK, "the path {path}");
        assert_eq!(downloaded.body, CONTENT, "the path {path}");
    }
}

#[tokio::test]
async fn the_traversal_is_refused() {
    let contract = Contract::new().await;

    for path in [
        "../etc/passwd",
        "a/../../b",
        "./a",
        "%2e%2e/etc/passwd",
        "%252e%252e%252fetc",
        "a%2fb",
        "a%5cb",
    ] {
        let answer = send(
            &contract.app,
            Method::PUT,
            &format!("/repository/{REPOSITORY}/{path}"),
            Some(ALLOWED),
            Some(CONTENT.to_vec()),
            &[],
        )
        .await;
        assert_eq!(
            answer.status,
            StatusCode::BAD_REQUEST,
            "the path {path} has to be refused"
        );
    }
}

#[tokio::test]
async fn the_control_characters_and_the_ambiguous_paths_are_refused() {
    let contract = Contract::new().await;

    for path in ["a%00b", "a%09b", "a%20b", "caf%C3%A9", "a%zzb"] {
        let answer = send(
            &contract.app,
            Method::PUT,
            &format!("/repository/{REPOSITORY}/{path}"),
            Some(ALLOWED),
            Some(CONTENT.to_vec()),
            &[],
        )
        .await;
        assert_eq!(
            answer.status,
            StatusCode::BAD_REQUEST,
            "the path {path} has to be refused"
        );
    }
}

#[tokio::test]
async fn a_refused_path_leaves_nothing_behind() {
    let contract = Contract::new().await;

    let answer = send(
        &contract.app,
        Method::PUT,
        &format!("/repository/{REPOSITORY}/../escape.bin"),
        Some(ALLOWED),
        Some(CONTENT.to_vec()),
        &[],
    )
    .await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);

    // Y en la dirección que el traversal queria alcanzar no hay nada.
    let escaped = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/escape.bin"),
        Some(ALLOWED),
        None,
        &[],
    )
    .await;
    assert_eq!(escaped.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_path_that_is_occupied_by_another_content_is_a_conflict() {
    let contract = Contract::new().await;
    publish(&contract.app, "file.bin", CONTENT).await;

    // La dirección publica una vez: el contenido de la dirección no se
    // sustituye en silencio.
    let answer = send(
        &contract.app,
        Method::PUT,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        Some(OTHER_CONTENT.to_vec()),
        &[],
    )
    .await;
    assert_eq!(answer.status, StatusCode::CONFLICT);

    // Y lo que hay sigue siendo lo de antes.
    let downloaded = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[],
    )
    .await;
    assert_eq!(downloaded.body, CONTENT);

    // Y en el almacén de contenido no queda ni un byte del contenido
    // rechazado: ni el fichero, ni una escritura a medias.
    let rejected = ContentDigest::of_bytes(OTHER_CONTENT);
    assert!(!contract.content.the_path_of(&rejected).exists());
    assert!(
        contract
            .content
            .staging_entries()
            .expect("the staging is readable")
            .is_empty(),
        "a refused publication left a file in the staging"
    );
}

#[tokio::test]
async fn publishing_the_same_content_again_is_idempotent() {
    let contract = Contract::new().await;
    let first = publish(&contract.app, "file.bin", CONTENT).await;
    assert_eq!(first.status, StatusCode::CREATED);

    let second = publish(&contract.app, "file.bin", CONTENT).await;
    assert_eq!(second.status, StatusCode::OK);
    assert_eq!(second.json()["created"], false);
    assert_eq!(second.json()["digest"], first.json()["digest"]);
}

#[tokio::test]
async fn a_repository_that_allows_the_replacement_replaces_the_content() {
    let contract = Contract::without_repository().await;
    contract
        .create_repository(a_descriptor(
            ACCOUNT,
            REPOSITORY,
            RepositoryFormat::Raw,
            RepositoryMode::Hosted,
            json!({ "raw": { "replace": "allow" } }),
        ))
        .await;
    publish(&contract.app, "file.bin", CONTENT).await;

    let answer = send(
        &contract.app,
        Method::PUT,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        Some(OTHER_CONTENT.to_vec()),
        &[],
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);

    let downloaded = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[],
    )
    .await;
    assert_eq!(downloaded.body, OTHER_CONTENT);
}

// ---------------------------------------------------------------------------
// AC4 — the status codes, and the boundary of the account.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn without_a_credential_it_is_unauthorized() {
    let contract = Contract::new().await;

    let answer = send(
        &contract.app,
        Method::PUT,
        &format!("/repository/{REPOSITORY}/file.bin"),
        None,
        Some(CONTENT.to_vec()),
        &[],
    )
    .await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        answer.header(header::WWW_AUTHENTICATE.as_str()),
        Some("Bearer")
    );
}

#[tokio::test]
async fn with_a_credential_that_is_not_valid_it_is_unauthorized() {
    let contract = Contract::new().await;

    let answer = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some("whoever"),
        None,
        &[],
    )
    .await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn without_the_permission_it_is_forbidden() {
    let contract = Contract::new().await;
    publish(&contract.app, "file.bin", CONTENT).await;

    // El principal es de la cuenta que tiene el repositorio, pero Cedar
    // no le da permiso: la respuesta es 403.
    let answer = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(NOT_ALLOWED),
        None,
        &[],
    )
    .await;
    assert_eq!(answer.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_publication_also_needs_the_permission() {
    let contract = Contract::new().await;

    let answer = send(
        &contract.app,
        Method::PUT,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(NOT_ALLOWED),
        Some(CONTENT.to_vec()),
        &[],
    )
    .await;
    assert_eq!(answer.status, StatusCode::FORBIDDEN);

    // Y no se publico nada.
    let downloaded = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[],
    )
    .await;
    assert_eq!(downloaded.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_permission_is_bound_to_the_repository_and_not_to_the_account() {
    // La misma cuenta, otro repositorio: el bundle de Cedar no lo cubre,
    // así que el mismo principal que puede en `releases` no puede en
    // `private`.
    //
    // Sin este test, una implementación que autorizara **por cuenta**
    // pasaría toda la suite: el bundle solo nombra un repositorio, y
    // hasta ahora no había un segundo repositorio con el que
    // compararlo.
    let contract = Contract::new().await;
    contract
        .create_repository(a_descriptor(
            ACCOUNT,
            "private",
            RepositoryFormat::Raw,
            RepositoryMode::Hosted,
            json!({}),
        ))
        .await;

    let published = send(
        &contract.app,
        Method::PUT,
        "/repository/private/file.bin",
        Some(ALLOWED),
        Some(CONTENT.to_vec()),
        &[],
    )
    .await;
    assert_eq!(published.status, StatusCode::FORBIDDEN);

    // Y tampoco puede leer de él.
    let downloaded = send(
        &contract.app,
        Method::GET,
        "/repository/private/file.bin",
        Some(ALLOWED),
        None,
        &[],
    )
    .await;
    assert_eq!(downloaded.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_repository_of_another_account_does_not_exist_for_this_one() {
    let contract = Contract::new().await;
    publish(&contract.app, "file.bin", CONTENT).await;

    // La cuenta ajena recibe un 404, no un 403: un 403 diria que el
    // repositorio existe en algun sitio, y esa es la existencia que no
    // se puede exponer.
    let answer = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(OUTSIDER),
        None,
        &[],
    )
    .await;
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_repository_of_another_format_is_not_found() {
    // El catalogo solo sirve el modo `hosted`, así que los modos que están
    // release no sirve ni siquiera se pueden crear. Lo que se puede crear
    // y no es de esta superficie es un 404.
    let contract = Contract::without_repository().await;
    contract
        .create_repository(a_descriptor(
            ACCOUNT,
            REPOSITORY,
            RepositoryFormat::Oci,
            RepositoryMode::Hosted,
            json!({}),
        ))
        .await;

    let answer = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[],
    )
    .await;
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_mode_that_this_release_does_not_serve_cannot_be_created() {
    // Un proxy no se crea: no se crea un repositorio que luego no va a
    // funcionar. Y la superficie de contenido crudo no lo serviria.
    let contract = Contract::without_repository().await;
    let outcome = contract
        .writer
        .create_repository(
            ACCOUNT,
            a_descriptor(
                ACCOUNT,
                REPOSITORY,
                RepositoryFormat::Raw,
                RepositoryMode::Proxy,
                json!({}),
            ),
        )
        .await;
    assert!(
        matches!(
            outcome,
            Err(hodei_catalog::CatalogError::ModeNotImplemented { .. })
        ),
        "a proxy repository has to be refused clearly"
    );
}

#[tokio::test]
async fn a_repository_that_does_not_exist_is_not_found() {
    let contract = Contract::without_repository().await;

    let answer = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[],
    )
    .await;
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_surface_is_closed_when_the_deployment_does_not_open_it() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let mut config = AppConfig::default();
    config.security.raw_api_enabled = false;
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
    .expect("bootstrap");
    let app = build_router(state, &config);

    let answer = send(
        &app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[],
    )
    .await;
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// The limits of the service, and the promises it does not keep yet.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_content_that_is_larger_than_the_limit_is_refused() {
    let contract = Contract::with_body_limit(16).await;
    contract
        .create_repository(a_descriptor(
            ACCOUNT,
            REPOSITORY,
            RepositoryFormat::Raw,
            RepositoryMode::Hosted,
            json!({}),
        ))
        .await;

    let answer = send(
        &contract.app,
        Method::PUT,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        Some(vec![b'x'; 64]),
        &[],
    )
    .await;
    assert_eq!(answer.status, StatusCode::PAYLOAD_TOO_LARGE);

    // Y no se publico nada.
    let downloaded = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[],
    )
    .await;
    assert_eq!(downloaded.status, StatusCode::NOT_FOUND);

    // Y en el almacén de contenido no queda ni un byte: un contenido que
    // no se publica no puede quedarse en el disco.
    let refused = ContentDigest::of_bytes(&[b'x'; 64]);
    assert!(!contract.content.the_path_of(&refused).exists());
    assert!(
        contract
            .content
            .staging_entries()
            .expect("the staging is readable")
            .is_empty(),
        "a refused publication left a file in the staging"
    );
}

#[tokio::test]
async fn a_content_that_is_not_the_declared_one_is_refused() {
    let contract = Contract::new().await;

    let answer = send(
        &contract.app,
        Method::PUT,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        Some(OTHER_CONTENT.to_vec()),
        &[("x-checksum-sha256", &digest_of(CONTENT))],
    )
    .await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);

    let downloaded = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[],
    )
    .await;
    assert_eq!(downloaded.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_repository_that_asks_for_a_partitioned_scope_is_not_served_yet() {
    let contract = Contract::without_repository().await;
    let mut descriptor = a_descriptor(
        ACCOUNT,
        REPOSITORY,
        RepositoryFormat::Raw,
        RepositoryMode::Hosted,
        json!({}),
    );
    descriptor.config.digest_scope = DigestScope::AccountPartitioned;
    contract.create_repository(descriptor).await;

    // Un 404 aquí sería mentir: el repositorio existe, y lo que no se
    // sirve es el contenido de un repositorio con el ambito particionado.
    let answer = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[],
    )
    .await;
    assert_eq!(answer.status, StatusCode::NOT_IMPLEMENTED);
}

#[tokio::test]
async fn a_declared_digest_that_is_not_a_digest_is_refused() {
    let contract = Contract::new().await;

    let answer = send(
        &contract.app,
        Method::PUT,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        Some(CONTENT.to_vec()),
        &[("x-checksum-sha256", "not-a-digest")],
    )
    .await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);

    let downloaded = send(
        &contract.app,
        Method::GET,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        None,
        &[],
    )
    .await;
    assert_eq!(downloaded.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_declared_digest_that_is_the_right_one_goes_on() {
    let contract = Contract::new().await;

    let answer = send(
        &contract.app,
        Method::PUT,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        Some(CONTENT.to_vec()),
        &[("x-checksum-sha256", &digest_of(CONTENT))],
    )
    .await;
    assert_eq!(answer.status, StatusCode::CREATED);
}

#[tokio::test]
async fn a_declared_digest_with_the_prefix_goes_on() {
    let contract = Contract::new().await;

    // Con el prefijo del algoritmo, y sin que las mayúsculas cambien el
    // sentido: `SHA256:` dice lo mismo que `sha256:`.
    let answer = send(
        &contract.app,
        Method::PUT,
        &format!("/repository/{REPOSITORY}/file.bin"),
        Some(ALLOWED),
        Some(CONTENT.to_vec()),
        &[(
            "x-checksum-sha256",
            &format!("sha256:{}", digest_of(CONTENT)),
        )],
    )
    .await;
    assert_eq!(answer.status, StatusCode::CREATED);
}
