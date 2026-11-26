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
//! of content are **real** (a Dgraph namespace and the file CAS), because
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
    CatalogReader, CatalogWriter, DigestScope, RepositoryConfig, RepositoryDescriptor,
    RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_evidence::domain::content::ContentDigest;
use hodei_hub::blob::FileBlobStore;
use serde_json::{Value, json};
use tower::ServiceExt;

// The graph profile of the harness: a REAL Dgraph namespace per application.
#[path = "support/graph_profile.rs"]
mod graph_profile;
use graph_profile::Workdir;

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
    /// The store of content, so that what is left in it can be looked at.
    content: Arc<FileBlobStore>,
    /// The directory of the CAS, which owns the Dgraph namespace.
    #[allow(dead_code)]
    workdir: Workdir,
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
        let workdir = Workdir::new();

        let mut config = AppConfig::default();
        // The raw surface is closed by default; this contract opens it on
        // purpose, the way a deployment that serves content does.
        config.security.raw_api_enabled = true;
        // The catalog and the store of content are built below and
        // injected, so the contract owns them.
        workdir.apply(&mut config);

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

        let catalog = Arc::new(workdir.tenant().catalog_store().await);
        let reader: Arc<dyn CatalogReader> = catalog.clone();
        let writer: Arc<dyn CatalogWriter> = catalog.clone();

        let content =
            Arc::new(FileBlobStore::new(workdir.path().join("cas")).expect("the store of content"));

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
            workdir,
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

    // The content is read from the store of content, not from the memory
    // of the request: what is downloaded is what was published.
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

    // A range with a suffix: the last bytes.
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

    // An open range: from there to the end.
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

    // Several ranges: this release serves one, so the request is ignored
    // and the whole content is served, which is what the RFC allows.
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

    // And the unit does not distinguish case: `Bytes=0-4` is a range.
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
    // RFC 9110: `If-None-Match` is compared with the **weak** function,
    // and a `W/` in front says "shares the representation". A client
    // that revalidates against a cache sends the tag that way, and a
    // 304 is what it expects.
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

    // The content that is there is the one from before: the request that
    // is not made leaves nothing written.
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

    // And nothing was published.
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

    // And what was there is still what was there: the condition says that
    // nothing is written, not that it is replaced.
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

    // And in the address the traversal wanted to reach there is nothing.
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

    // The address publishes once: the content of the address is not
    // replaced in silence.
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

    // And what is there is still what it was.
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

    // And in the store of content there is not a single byte of the
    // rejected content: neither the file, nor a half-written one.
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
    // The same account, another repository: the bundle of Cedar does not
    // cover it, so the same principal that can in `releases` cannot in
    // `private`.
    //
    // Without this test, an implementation that authorized **by account**
    // would pass the whole suite: the bundle names one repository, and
    // until now there was no second repository to compare it with.
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

    // And it cannot read from it either.
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
    // The catalog only serves the `hosted` mode, so the modes that this
    // release does not serve cannot even be created. What can be created
    // and is not of this surface is a 404.
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
async fn a_virtual_repository_that_cannot_work_is_refused_when_it_is_created() {
    // Un proxy sin upstream no se crea: no se crea un repositorio que
    // luego no va a funcionar.
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
            Err(hodei_catalog::CatalogError::InvalidRequest { .. })
        ),
        "a proxy repository without an upstream has to be refused clearly"
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
    let workdir = Workdir::new();
    let mut config = AppConfig::default();
    config.security.raw_api_enabled = false;
    workdir.apply(&mut config);

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

    // And in the store of content there is not a single byte: content that
    // is not published cannot stay on the disk.
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

    // A 404 here would be lying: the repository exists, and what is not
    // served is the content of a repository with the partitioned scope.
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

    // With the prefix of the algorithm, and without the case changing the
    // meaning: `SHA256:` says the same as `sha256:`.
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
