//! HSP-187: the durable catalog over the real Dgraph engine -- the Raw slice.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` like all the stack's ITs. What it
//! tests, against the real engine (TLS + ACL, the live h186 server):
//!
//! - **AC1** the real Raw flow (publish → GET/HEAD/Range) over the
//!   Dgraph catalog serves the same digest and the same count; the catalog
//!   commit and the audit event are **one transaction** (the
//!   HSP-199 boundary): the event is there if and only if the asset is, and
//!   an event row already stated with another fact blocks the entire
//!   publication;
//! - **AC2** two writers leave a single asset and a single event (the
//!   path guard + the `@upsert` index + the once-only operation marker
//!   decide on the server); a retry after the commit does not
//!   duplicate anything; a CAS blob without catalog metadata is left a
//!   **safe orphan**: detectable by census, and the catalog keeps
//!   serving what it does have;
//! - reads of the published state say what the catalog contract
//!   says (repository, asset, version, reference and tag).
//!
//! The slice is Raw: Repository, Asset, Version, BlobReference and outbox.
//! The other formats are not migrated.

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
    AssetRecord, BlobReference, CatalogReader, CatalogWriter, DigestScope, PackageCoordinates,
    PackageVersion, Publication, RepositoryConfig, RepositoryDescriptor, RepositoryFormat,
    RepositoryMode, TrustState, repository_hrn,
};
use hodei_catalog_storage::DgraphAuditOutbox;
use hodei_evidence::ContentDigest;
use hodei_graph_client::{GraphClient, GraphClientConfig};
use hodei_hub::blob::{BlobStore, FileBlobStore};
use kernel::application::ports::clock::{CountingIdGenerator, FixedClock, Identity};
use tempfile::TempDir;
use tower::ServiceExt;

/// The correlation of the publications. The outbox lives on a server
/// shared across runs: each run carries its own correlation, and
/// the event census counts its own.
fn correlation_of(account: &str) -> String {
    format!("corr-hsp187-{account}")
}

/// The actor that publishes.
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

/// A new connection, authenticated and with the versioned startup validated:
/// a real restart re-enters the graph through here.
async fn fresh_client() -> Option<Arc<GraphClient>> {
    let Some(config) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return None;
    };
    let mut client = GraphClient::connect(config).await.expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    Some(Arc::new(client))
}

/// The clock's tail, which barely moves between runs: the tail is
/// what separates two runs of the same test.
fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

/// One account per test and per run: no test runs into
/// what another left behind.
fn account(tag: &str) -> String {
    format!("it187{}{}", tag, &nonce()[8..])
}

// The catalog's Dgraph store: the conditioned opening shared by
// the stack's gated suites (HSP-219). `DgraphCatalogStore::open` declares
// its DDL with two `alter`s and Dgraph rejects concurrent `alter`s with
// `errIndexingInProgress` during the reindexing window: the helper opens
// one at a time within the process and retries the transient case with backoff.
// **Recommended lane**: this binary with the default threads, but
// alone -- no other gated suite against the same alpha at once --; if
// two are launched together, the bounded retry absorbs the window but the
// safe lane remains one at a time (HSP-190 lesson).
include!("support/hsp219_concurrent_open.rs");

/// A raw hosted repository descriptor.
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

/// A publication, with the coordinates requested.
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

/// A publication, at the fixture's default version.
fn a_publication(account: &str, repository: &str, asset_path: &str, content: &[u8]) -> Publication {
    a_publication_at(account, repository, asset_path, "1.0.0", content)
}

/// An outbox fact, stated the way the atomic outbox records it: the fields
/// of the `AuditEvent` that the assertions look at.
#[derive(Debug)]
struct OutboxFact {
    correlation_id: String,
    resource: String,
    action: String,
    result: String,
    tenant: String,
    digest: String,
}

/// The outbox facts of THIS account, stated by its tenant key.
///
/// The census goes PER TENANT (exact index), not through the global `pending()`:
/// the shared server's outbox accumulates the facts of ALL
/// runs -- no test marks them delivered -- and its response came to
/// overflow the gRPC message limit (4 MiB; finding of HSP-194,
/// drained and documented by HSP-220). The account is unique per run,
/// so the `eq(tenant)` root brings through the index only the facts of THIS
/// test, and the fine filter (the correlation) goes in the adapter.
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

/// A field of the fact stated by the graph, or empty when absent.
fn the_fact_field(node: &serde_json::Value, field: &str) -> String {
    node.get(field)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// How many pending events there are from THIS run: the census goes by
/// tenant and the own correlation filters in the adapter.
async fn the_event_count(client: &GraphClient, account: &str) -> usize {
    the_events_of(client, account)
        .await
        .into_iter()
        .filter(|fact| fact.correlation_id == correlation_of(account))
        .count()
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

/// The mounted vertical: the hub router with the Dgraph catalog and the real
/// file-based CAS.
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

/// The response to a request.
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
// AC1: the real Raw flow over the Dgraph catalog.
// ---------------------------------------------------------------------------

/// The real Raw flow: publish → GET/HEAD/Range against the hub router,
/// with the Dgraph catalog and the real file-based CAS. The same digest
/// that is published is the one that is served, and the count is the one counted.
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

    // GET: the bytes that were published are the ones served.
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

    // HEAD: the count that is served is the one that was published.
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

    // Range: a piece of the content, not all of it.
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

    // And the catalog says the same as what was served.
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

/// The catalog commit and the audit event are ONE transaction:
/// the event is there if and only if the asset is, and it states the fact of THIS
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

    // The asset is there and so is the event, with the same digest. The census goes
    // per tenant (the account is unique per run) and the own correlation
    // filters in the adapter.
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

/// An event row already stated with OTHER content blocks the entire
/// publication: the asset is not published without its event, and the blob
/// reference is not left behind either.
#[tokio::test]
async fn an_event_id_already_said_with_another_fact_blocks_the_publication() {
    let Some(_) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    let account = account("contra");
    let client = fresh_client().await.expect("the client connects");
    // The generator counts, so the event identifier is known
    // before publishing; the prefix carries the run's account so
    // that no event from another run collides with its own.
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

    // The event row that the publication is going to write is already stated
    // with another digest: the event write is going to collide.
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
    // The event is still the one stated before: the publication did not
    // write a contradiction on top.
    let entry = outbox
        .entry(&format!("event-{account}-0"))
        .await
        .expect("the entry")
        .expect("the entry");
    assert_eq!(entry.event.digest.as_deref(), Some("sha256:0000"));
}

/// Republishing the same thing is neither an error nor a write: a single asset, a
/// single event. And a publication that collides leaves a conflict event.
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

    // The same path with other content: conflict, and the conflict is stated.
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
// AC2: two writers, the retry and the safe orphan.
// ---------------------------------------------------------------------------

/// Two writers publishing THE SAME THING at once leave one asset and one event:
/// the path guard and the once-only marker decide on the server.
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

    // Two clients, the SAME content, the SAME path: one creation and one
    // acknowledgement, not two assets.
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

    // Now two clients publish DIFFERENT CONTENT at the same path: the
    // one that loses receives a conflict, not an overwrite, and does not leave its
    // version half-written.
    // Different coordinates on purpose: each has its own version,
    // and the loser finds that the version can be written
    // but the path's asset already belongs to the other.
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

    // What remained is what the winner published, and the loser did not leave its
    // version behind.
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
    // The loser's version is not there: its transaction went away whole.
    let loser_left_behind = left.version(&account, "releases", &loser_version).await;
    assert!(
        loser_left_behind.is_err(),
        "the loser of the race left a version behind: {loser_left_behind:?}"
    );
    // And the version the asset points to exists and says the same.
    let version = left
        .version(&account, "releases", &asset.coordinates)
        .await
        .expect("the version behind the asset");
    assert_eq!(version.digest, asset.digest);
}

/// The retry after the commit rereads the committed result: a single
/// asset, a single event, not one row extra.
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

    // The retry: the same operation, stated again. The response is
    // the committed one, and nothing is written.
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

/// A CAS blob without catalog metadata is left a **safe orphan**:
/// detectable by census (the CAS keys that no catalog
/// reference claims), and the catalog keeps serving what it does have.
///
/// The policy of this slice: **orphan reference by census**, not
/// GC-aware. A staged blob whose publication fell through is reconcilable --
/// the reference is what prevents it from being collected, so a blob without a
/// reference breaks nothing; the census names it and collection (HSP-104)
/// decides its fate.
#[tokio::test]
async fn a_cas_blob_without_catalog_metadata_is_a_detectable_safe_orphan() {
    let Some(_) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    let account = account("orphan");
    let vertical = Vertical::mount(&account, "releases").await;
    let client = fresh_client().await.expect("the client connects");

    // A blob that enters the CAS and whose publication NEVER happens.
    let orphan_bytes = b"the bytes that nobody catalogued";
    let orphan_digest = vertical
        .content
        .put(orphan_bytes)
        .await
        .expect("the blob is staged");

    // The blob that IS published.
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

    // The census: the digests the catalog claims versus the ones the
    // CAS holds. The orphan is named, the published one is not.
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

    // And the orphan breaks nothing: the catalog keeps serving what it
    // has, with the digest intact.
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
// What the contract says about reads of the published state.
// ---------------------------------------------------------------------------

/// The Dgraph catalog reads return exactly what the catalog
/// contract says about the published state: repository, asset, version,
/// reference and tag. (HSP-314 F4: they used to be compared against a second
/// engine; now each expected value is derived from the descriptor and the
/// publication, which is where the contract gets it from.)
#[tokio::test]
async fn the_dgraph_reads_match_the_contract_for_the_published_state() {
    let Some(_) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    let account = account("parity");
    let repo = "releases";
    let path = "parity/app.bin";
    let content = b"the same state in two engines";

    let client = fresh_client().await.expect("the client connects");
    let dgraph = open_store(client.clone()).await;
    let descriptor = a_descriptor(&account, repo);
    dgraph
        .create_repository(&account, descriptor.clone())
        .await
        .expect("the dgraph repository");

    let publication = a_publication(&account, repo, path, content);
    let expected_digest = ContentDigest::of_bytes(content);
    let outcome = dgraph
        .publish(&account, publication)
        .await
        .expect("dgraph publish");

    // The declared content is that of the publication.
    assert_eq!(outcome.asset.digest, expected_digest);
    assert_eq!(outcome.version.digest, expected_digest);
    assert_eq!(outcome.reference.digest, expected_digest);
    assert!(outcome.created, "the first publication creates the asset");

    // The repository is read as it was created.
    let repository = dgraph
        .repository(&account, repo)
        .await
        .expect("dgraph repository");
    assert_eq!(repository.account, account);
    assert_eq!(repository.repo_key, repo);
    assert_eq!(repository.format, RepositoryFormat::Raw);
    assert_eq!(repository.mode, RepositoryMode::Hosted);
    assert_eq!(repository.hrn, descriptor.hrn);
    assert_eq!(repository.config.version, 1);
    assert_eq!(repository.config.digest_scope, DigestScope::Global);

    // The asset: account, repository, path, coordinates, digest and size of
    // the publication; without a subject because the publication declares none.
    let asset: AssetRecord = dgraph
        .asset(&account, repo, path)
        .await
        .expect("dgraph asset");
    let coordinates = PackageCoordinates {
        package_name: "pkg".to_string(),
        package_version: "1.0.0".to_string(),
    };
    assert_eq!(asset.account, account);
    assert_eq!(asset.repository, repo);
    assert_eq!(asset.asset_path, path);
    assert_eq!(asset.coordinates, coordinates);
    assert_eq!(asset.digest, expected_digest);
    assert_eq!(asset.size_bytes, content.len() as u64);
    assert_eq!(asset.subject, None);

    let version: PackageVersion = dgraph
        .version(&account, repo, &asset.coordinates)
        .await
        .expect("dgraph version");
    assert_eq!(version.digest, expected_digest);
    assert_eq!(version.size_bytes, content.len() as u64);
    assert_eq!(version.coordinates, coordinates);

    // The reference: a newly admitted artifact is in quarantine and its
    // scope is the repository's.
    let reference: BlobReference = dgraph
        .reference(&account, repo, path)
        .await
        .expect("dgraph reference");
    assert_eq!(reference.digest, expected_digest);
    assert_eq!(reference.scope, DigestScope::Global);
    assert_eq!(reference.size_bytes, content.len() as u64);
    assert_eq!(reference.trust_state, TrustState::Quarantined);
    assert_eq!(reference.account, account);
    assert_eq!(reference.repository, repo);
    assert_eq!(reference.asset_path, path);

    // The mutable tag moves and points to the requested version.
    let moved = dgraph
        .move_tag(&account, repo, "stable", asset.coordinates.clone())
        .await
        .expect("dgraph tag");
    assert_eq!(moved.account, account);
    assert_eq!(moved.repository, repo);
    assert_eq!(moved.tag_name, "stable");
    assert_eq!(moved.coordinates, coordinates);

    let read = dgraph
        .tag(&account, repo, "stable")
        .await
        .expect("dgraph tag read");
    assert_eq!(read.coordinates, coordinates);

    let tags = dgraph.tags(&account, repo).await.expect("dgraph tags");
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].tag_name, "stable");
    assert_eq!(tags[0].coordinates, coordinates);
}
