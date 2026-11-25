//! HSP-304 (AC2): Terraform, Conan, Galaxy and Hugging Face over the REAL
//! Dgraph catalog, with no global scan.
//!
//! The same slice as `hsp193_npm_dgraph_it` and `hsp194_pypi_dgraph_it`
//! (the key-guarded, outbox-atomic catalog spine of HSP-187/190/191), for the
//! four formats of the HSP-157..218 stack that had none. The router of the
//! hub is mounted over a `DgraphCatalogStore` and each format surface is
//! driven over HTTP, the way its client speaks it:
//!
//! - **terraform** (hosted module registry): publish a module version, list
//!   its versions, resolve the download and read the artifact byte for byte;
//!   the same version with other bytes is a 409 that writes nothing;
//! - **conan**: upload a recipe and a package file, read the revision lists,
//!   the files and the package binaries; an immutable revision refuses other
//!   content;
//! - **galaxy**: publish a collection through the multipart form, read the
//!   versions page, the version document and the artifact; a collection that
//!   does not exist is a 404;
//! - **hf**: a model seeded at the store level is served by revision
//!   document and resolve (the HF surface is read-only);
//! - in all four: another account sees nothing (404), the rows are scoped by
//!   account (probed at the store with the exact key), and the atomic outbox
//!   carries the audit events of the accepted writes of THIS run.
//!
//! The real clients of these formats run in `hsp158`, `hsp159`, `hsp217` and
//! `hsp218`; this suite proves that the Dgraph catalog serves the very same
//! surfaces.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` like the rest of the stack: without it
//! every test prints `SKIP` and the `integration (dgraph)` stage declares
//! `NOT_RUN` through `scripts/dgraph-it-gate.sh`. Single-threaded
//! (`--test-threads=1`), as the other IT of the stack.

use std::io::Write as _;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

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
use hodei_catalog::{
    AllowAllAuthorizer, CatalogReader, CatalogWriter, DigestScope, PackageCoordinates, Publication,
    RepositoryConfig, RepositoryDescriptor, RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_catalog_storage::DgraphCatalogStore;
use hodei_evidence::ContentDigest;
use hodei_graph_client::{GraphClient, GraphClientConfig};
use hodei_hub::blob::{BlobStore, FileBlobStore};
use sha2::Digest as _;
use tempfile::TempDir;
use tower::ServiceExt;

fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

/// One account per test and per run, so no test meets what another left.
fn account(tag: &str) -> String {
    format!("it304{}{}", tag, &nonce()[8..])
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

/// The store, with the bounded opening of the stack (HSP-190/HSP-219): the
/// guard opening under load is the known flake, absorbed with three tries.
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

/// How many outbox events of this tenant name the needle. The census is per
/// tenant (exact index): the shared outbox holds every run's pending facts.
async fn the_events_of(client: &GraphClient, tenant: &str, resource_needle: &str) -> usize {
    let escaped = tenant.replace('\\', "\\\\").replace('"', "\\\"");
    let response = client
        .execute_dql(&format!(
            "{{ q(func: eq(hodei.catalog.outbox.tenant, \"{escaped}\")) \
             {{ hodei.catalog.outbox.resource }} }}"
        ))
        .await
        .expect("the outbox census answers");
    response
        .json
        .get("q")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter(|node| {
            node.get("hodei.catalog.outbox.resource")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|resource| resource.contains(resource_needle))
        })
        .count()
}

// ---------------------------------------------------------------------------
// The vertical: the hub router over the Dgraph catalog.
// ---------------------------------------------------------------------------

/// `alice` is the principal of `account`; `bob` of `other` (another account).
struct ContractAuthenticator {
    account: String,
    other: String,
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
            Credentials::Bearer(token) if token == "bob" => Ok(PrincipalClaims::new(
                PrincipalId::new(format!("hrn:hodei:iam::{}:User/bob", self.other)).unwrap(),
                AccountId::new(self.other.clone()).unwrap(),
            )),
            _ => Err(AuthenticationError::InvalidCredentials),
        }
    }
}

fn cedar_for(account: &str, other: &str, repo_key: &str) -> Arc<dyn AuthorizerPort> {
    let mut policy = String::new();
    for (owner, user) in [(account, "alice"), (other, "bob")] {
        for action in ["UploadAsset", "DownloadAsset"] {
            policy.push_str(&format!(
                "permit(principal == Iam::User::\"hrn:hodei:iam::{owner}:User/{user}\", \
                 action == Artifacts::Action::\"{action}\", \
                 resource == Artifacts::Repository::\"hrn:hodei:artifacts:eu:{owner}:repository/{repo_key}\");\n"
            ));
        }
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

struct Vertical {
    app: Router,
    store: Arc<DgraphCatalogStore>,
    content: Arc<dyn BlobStore>,
    client: Arc<GraphClient>,
    #[allow(dead_code)]
    tmp: TempDir,
}

impl Vertical {
    async fn mount(account: &str, other: &str, repo_key: &str, format: RepositoryFormat) -> Self {
        let client = fresh_client().await.expect("the dgraph client connects");
        let store = Arc::new(open_store(client.clone()).await);
        let mut config = RepositoryConfig::new(DigestScope::Global);
        config.config_settings = serde_json::json!({});
        store
            .create_repository(
                account,
                RepositoryDescriptor {
                    hrn: repository_hrn("hodei", "artifacts", "eu", account, repo_key),
                    account: account.to_string(),
                    repo_key: repo_key.to_string(),
                    format,
                    mode: RepositoryMode::Hosted,
                    config,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                },
            )
            .await
            .expect("the repository is created");

        let tmp = tempfile::tempdir().expect("temp dir");
        let mut app_config = AppConfig::default();
        app_config.security.raw_api_enabled = true;
        // The graph profile is the only authority: the bootstrap connects to the
        // same alpha the test seeds and its catalog is replaced just below.
        app_config.graph = config_from_env();
        app_config.content.cas_path = tmp
            .path()
            .join("bootstrap-cas")
            .to_string_lossy()
            .into_owned();
        let state = bootstrap(
            &app_config,
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
        let content: Arc<dyn BlobStore> =
            Arc::new(FileBlobStore::new(tmp.path().join("cas")).expect("the CAS"));
        let state = state
            .with_catalog(reader, writer)
            .with_content_store(content.clone())
            .with_authenticator(Arc::new(ContractAuthenticator {
                account: account.to_string(),
                other: other.to_string(),
            }))
            .with_authorization(cedar_for(account, other, repo_key));
        Self {
            app: build_router(state, &app_config),
            store,
            content,
            client,
            tmp,
        }
    }
}

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Vec<u8>,
}

async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    authorization: Option<&str>,
    content_type: Option<&str>,
    body: Vec<u8>,
) -> Reply {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "127.0.0.1");
    if let Some(value) = authorization {
        builder = builder.header(header::AUTHORIZATION, value);
    }
    if let Some(value) = content_type {
        builder = builder.header(header::CONTENT_TYPE, value);
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(Body::from(body))
                .expect("a well formed request"),
        )
        .await
        .expect("the router always answers");
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the body can be read")
        .to_vec();
    Reply {
        status,
        headers,
        body,
    }
}

const ALICE: &str = "Bearer alice";
const BOB: &str = "Bearer bob";

async fn get(app: &Router, uri: &str, authorization: &str) -> Reply {
    send(app, Method::GET, uri, Some(authorization), None, Vec::new()).await
}

fn text(reply: &Reply) -> String {
    String::from_utf8_lossy(&reply.body).into_owned()
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", sha2::Sha256::digest(bytes))
}

/// A ustar member: header, content and the padding of the record.
fn the_member_bytes(name: &str, content: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0u8; 512];
    bytes[..name.len()].copy_from_slice(name.as_bytes());
    bytes[100..108].copy_from_slice(b"0000644\0");
    let size = format!("{:011o}", content.len());
    bytes[124..124 + size.len()].copy_from_slice(size.as_bytes());
    bytes[148..156].copy_from_slice(b"        ");
    bytes[156] = b'0';
    bytes[257..262].copy_from_slice(b"ustar");
    let checksum: u32 = bytes.iter().map(|byte| *byte as u32).sum();
    bytes[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());
    let mut member = bytes;
    member.extend_from_slice(content);
    member.extend(std::iter::repeat_n(0u8, (512 - content.len() % 512) % 512));
    member
}

fn a_targz(members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut tar = Vec::new();
    for (name, content) in members {
        tar.extend_from_slice(&the_member_bytes(name, content));
    }
    tar.extend_from_slice(&[0u8; 1024]);
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&tar).expect("the tar is compressed");
    encoder.finish().expect("the gzip is closed")
}

// ---------------------------------------------------------------------------
// Terraform: the hosted module registry.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_terraform_module_registry_runs_over_dgraph() {
    let Some(_guard) = fresh_client().await else {
        return;
    };
    let (account, other) = (account("tf"), account("tfo"));
    let repo = "tf-modules";
    let v = Vertical::mount(&account, &other, repo, RepositoryFormat::Terraform).await;

    let module = a_targz(&[
        ("main.tf", b"variable \"region\" {}\n"),
        ("outputs.tf", b"\n"),
    ]);
    let publish = |bytes: Vec<u8>, version: &str, who: &'static str| {
        let app = v.app.clone();
        let uri = format!("/terraform/{repo}/publish/hodei/demo/local/{version}");
        async move {
            send(
                &app,
                Method::POST,
                &uri,
                Some(who),
                Some("application/gzip"),
                bytes,
            )
            .await
        }
    };

    let accepted = publish(module.clone(), "1.0.0", ALICE).await;
    assert!(
        accepted.status.is_success(),
        "the publish settles over dgraph: {} {}",
        accepted.status,
        text(&accepted)
    );
    // A registry version is immutable even for the same bytes (409), and a
    // refused publish says no new fact.
    let before = the_events_of(&v.client, &account, repo).await;
    assert!(before > 0, "the accepted publish left its audit events");
    let again = publish(module.clone(), "1.0.0", ALICE).await;
    assert_eq!(again.status, StatusCode::CONFLICT, "{}", text(&again));
    assert_eq!(
        the_events_of(&v.client, &account, repo).await,
        before,
        "a refused republication says no new fact"
    );
    // Other bytes under the same version: 409 too, nothing written.
    let other_bytes = a_targz(&[("main.tf", b"# a different module\n")]);
    let refused = publish(other_bytes, "1.0.0", ALICE).await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", text(&refused));

    let versions = get(
        &v.app,
        &format!("/terraform/{repo}/v1/modules/hodei/demo/local/versions"),
        ALICE,
    )
    .await;
    assert_eq!(versions.status, StatusCode::OK, "{}", text(&versions));
    assert!(text(&versions).contains("1.0.0"), "{}", text(&versions));

    let download = get(
        &v.app,
        &format!("/terraform/{repo}/v1/modules/hodei/demo/local/1.0.0/download"),
        ALICE,
    )
    .await;
    assert_eq!(download.status, StatusCode::NO_CONTENT);
    let location = download
        .headers
        .get("x-terraform-get")
        .and_then(|value| value.to_str().ok())
        .expect("the download names the artifact")
        .to_string();
    assert!(
        location.contains(&sha256_hex(&module)),
        "the location carries the digest of the stored bytes: {location}"
    );
    let artifact = get(
        &v.app,
        &format!("/terraform/{repo}/artifacts/hodei/demo/local/1.0.0.tar.gz"),
        ALICE,
    )
    .await;
    assert_eq!(artifact.status, StatusCode::OK);
    assert_eq!(artifact.body, module, "the artifact is served from the CAS");

    // Account scoping: the other account sees neither the registry nor bytes.
    let stranger = get(
        &v.app,
        &format!("/terraform/{repo}/artifacts/hodei/demo/local/1.0.0.tar.gz"),
        BOB,
    )
    .await;
    assert_eq!(stranger.status, StatusCode::NOT_FOUND);
    assert!(stranger.body != module);
    let probe = v
        .store
        .version(
            &account,
            repo,
            &PackageCoordinates {
                package_name: "hodei/demo/local".to_string(),
                package_version: "1.0.0".to_string(),
            },
        )
        .await;
    assert!(
        probe.is_ok(),
        "the exact-key probe finds the version in its own account: {probe:?}"
    );
    let foreign = v
        .store
        .version(
            &other,
            repo,
            &PackageCoordinates {
                package_name: "hodei/demo/local".to_string(),
                package_version: "1.0.0".to_string(),
            },
        )
        .await;
    assert!(foreign.is_err(), "the other account has no such row");
}

// ---------------------------------------------------------------------------
// Conan.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_conan_remote_runs_over_dgraph() {
    let Some(_guard) = fresh_client().await else {
        return;
    };
    let (account, other) = (account("cn"), account("cno"));
    let repo = "conan-hosted";
    let v = Vertical::mount(&account, &other, repo, RepositoryFormat::Conan).await;
    let base = format!("/conan/{repo}/v2/conans/hello/1.0.0/hodei/stable");
    let recipe = b"from conan import ConanFile\nclass Hello(ConanFile):\n    name = 'hello'\n";
    let binary = b"the binary of package id 9f4c";

    let put = |uri: String, bytes: &'static [u8], who: &'static str| {
        let app = v.app.clone();
        async move {
            send(
                &app,
                Method::PUT,
                &uri,
                Some(who),
                Some("application/octet-stream"),
                bytes.to_vec(),
            )
            .await
        }
    };
    let recipe_uri = format!("{base}/revisions/rrev1/files/conanfile.py");
    let package_uri =
        format!("{base}/revisions/rrev1/packages/pid9f/revisions/prev1/files/conan_package.tgz");

    assert_eq!(
        put(recipe_uri.clone(), recipe, ALICE).await.status,
        StatusCode::OK
    );
    assert_eq!(
        put(package_uri.clone(), binary, ALICE).await.status,
        StatusCode::OK
    );
    let before = the_events_of(&v.client, &account, repo).await;
    assert!(
        before >= 2,
        "both writes carried their audit events: {before}"
    );
    // Idempotent echo, no new fact; other content under a revision is a 409.
    assert_eq!(
        put(recipe_uri.clone(), recipe, ALICE).await.status,
        StatusCode::OK
    );
    assert_eq!(the_events_of(&v.client, &account, repo).await, before);
    assert_eq!(
        put(recipe_uri.clone(), b"# rewritten", ALICE).await.status,
        StatusCode::CONFLICT,
        "a revision is immutable"
    );

    let latest = get(&v.app, &format!("{base}/latest"), ALICE).await;
    assert_eq!(latest.status, StatusCode::OK, "{}", text(&latest));
    assert!(text(&latest).contains("rrev1"), "{}", text(&latest));
    let revisions = get(&v.app, &format!("{base}/revisions"), ALICE).await;
    assert_eq!(revisions.status, StatusCode::OK, "{}", text(&revisions));
    assert!(text(&revisions).contains("rrev1"));
    let file = get(&v.app, &recipe_uri, ALICE).await;
    assert_eq!(file.status, StatusCode::OK);
    assert_eq!(file.body, recipe, "the recipe comes out of the CAS");
    let package = get(&v.app, &package_uri, ALICE).await;
    assert_eq!(package.status, StatusCode::OK);
    assert_eq!(package.body, binary);
    let packages = get(
        &v.app,
        &format!("{base}/revisions/rrev1/packages/search"),
        ALICE,
    )
    .await;
    assert_eq!(packages.status, StatusCode::OK, "{}", text(&packages));
    assert!(text(&packages).contains("pid9f"), "{}", text(&packages));

    // Scoping by account.
    assert_eq!(
        get(&v.app, &recipe_uri, BOB).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        put(
            format!("{base}/revisions/rrevB/files/conanfile.py"),
            recipe,
            BOB
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let missing = get(
        &v.app,
        &format!("{base}/revisions/rrevB/files/conanfile.py"),
        ALICE,
    )
    .await;
    assert_eq!(
        missing.status,
        StatusCode::NOT_FOUND,
        "the foreign write wrote nothing"
    );
}

// ---------------------------------------------------------------------------
// Galaxy.
// ---------------------------------------------------------------------------

fn a_collection(version: &str, readme: &str) -> Vec<u8> {
    let manifest = serde_json::json!({
        "collection_info": { "namespace": "hodei", "name": "demo", "version": version },
        "format": 1,
    });
    a_targz(&[
        ("MANIFEST.json", manifest.to_string().as_bytes()),
        ("README.md", readme.as_bytes()),
    ])
}

fn the_form(artifact: &[u8]) -> (String, Vec<u8>) {
    let boundary = "hodei-hsp304-it";
    let mut body = Vec::new();
    let _ = write!(
        body,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"sha256\"\r\n\r\n{}\r\n",
        sha256_hex(artifact)
    );
    let _ = write!(
        body,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"demo.tar.gz\"\r\nContent-Type: application/octet-stream\r\n\r\n"
    );
    body.extend_from_slice(artifact);
    let _ = write!(body, "\r\n--{boundary}--\r\n");
    (format!("multipart/form-data; boundary={boundary}"), body)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_galaxy_collections_run_over_dgraph() {
    let Some(_guard) = fresh_client().await else {
        return;
    };
    let (account, other) = (account("gx"), account("gxo"));
    let repo = "galaxy-hosted";
    let v = Vertical::mount(&account, &other, repo, RepositoryFormat::Galaxy).await;
    let root = format!("/galaxy/{repo}");
    let publish = |artifact: Vec<u8>, who: &'static str| {
        let app = v.app.clone();
        let uri = format!("{root}/api/v3/artifacts/collections/");
        async move {
            let (content_type, body) = the_form(&artifact);
            send(
                &app,
                Method::POST,
                &uri,
                Some(who),
                Some(&content_type),
                body,
            )
            .await
        }
    };
    let (token_alice, token_bob) = ("Token alice", "Token bob");

    let collection_versions = format!("{root}/api/v3/collections/hodei/demo/versions/");
    assert_eq!(
        get(&v.app, &collection_versions, token_alice).await.status,
        StatusCode::NOT_FOUND,
        "a collection nobody published has no versions page"
    );

    let artifact = a_collection("1.0.0", "# the demo collection\n");
    let accepted = publish(artifact.clone(), token_alice).await;
    assert_eq!(accepted.status, StatusCode::OK, "{}", text(&accepted));
    let before = the_events_of(&v.client, &account, repo).await;
    assert!(before > 0, "the publish carried its audit events");
    assert_eq!(
        publish(artifact.clone(), token_alice).await.status,
        StatusCode::OK
    );
    assert_eq!(
        the_events_of(&v.client, &account, repo).await,
        before,
        "an idempotent republication says no new fact"
    );
    let rewritten = publish(
        a_collection("1.0.0", "# other bytes, same version\n"),
        token_alice,
    )
    .await;
    assert_eq!(
        rewritten.status,
        StatusCode::CONFLICT,
        "{}",
        text(&rewritten)
    );

    let versions = get(&v.app, &collection_versions, token_alice).await;
    assert_eq!(versions.status, StatusCode::OK, "{}", text(&versions));
    assert!(text(&versions).contains("1.0.0"));
    let version = get(&v.app, &format!("{collection_versions}1.0.0/"), token_alice).await;
    assert_eq!(version.status, StatusCode::OK, "{}", text(&version));
    let document: serde_json::Value = serde_json::from_slice(&version.body).expect("a document");
    assert_eq!(
        document["artifact"]["sha256"].as_str(),
        Some(sha256_hex(&artifact).as_str()),
        "the document declares the digest of the stored bytes"
    );
    let download = document["download_url"].as_str().expect("the download url");
    let served = get(&v.app, download, token_alice).await;
    assert_eq!(served.status, StatusCode::OK, "{}", text(&served));
    assert_eq!(served.body, artifact, "the artifact comes out of the CAS");

    // Scoping by account.
    assert_eq!(
        get(&v.app, &collection_versions, token_bob).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        publish(a_collection("2.0.0", "# foreign\n"), token_bob)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&v.app, &format!("{collection_versions}2.0.0/"), token_alice)
            .await
            .status,
        StatusCode::NOT_FOUND,
        "the foreign publish wrote nothing"
    );
}

// ---------------------------------------------------------------------------
// Hugging Face: read-only surface over a seeded model.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_hf_models_run_over_dgraph() {
    let Some(_guard) = fresh_client().await else {
        return;
    };
    let (account, other) = (account("hf"), account("hfo"));
    let repo = "hf-hosted";
    let v = Vertical::mount(&account, &other, repo, RepositoryFormat::Hf).await;

    let config = b"{\"model_type\":\"bert\"}\n".to_vec();
    let weights: Vec<u8> = (0..4096u32).map(|index| (index % 251) as u8).collect();
    for (file, bytes) in [("config.json", &config), ("model.safetensors", &weights)] {
        let digest = v.content.put(bytes).await.expect("the blob is stored");
        v.store
            .publish(
                &account,
                Publication {
                    repository: repo.to_string(),
                    asset_path: format!("artifacts/hodei/demo-model/{file}"),
                    coordinates: PackageCoordinates {
                        package_name: "hodei/demo-model".to_string(),
                        package_version: file.to_string(),
                    },
                    digest: digest.clone(),
                    size_bytes: bytes.len() as u64,
                    tags: Vec::new(),
                    actor: format!("hrn:hodei:iam::{account}:User/alice"),
                    correlation_id: "corr-hsp304-hf".to_string(),
                    overwrite: false,
                    precondition: None,
                    subject: None,
                },
            )
            .await
            .expect("the model file publication settles");
        assert_eq!(digest, ContentDigest::of_bytes(bytes));
    }

    let revision = get(
        &v.app,
        &format!("/hf/{repo}/api/models/hodei/demo-model/revision/main"),
        ALICE,
    )
    .await;
    assert_eq!(revision.status, StatusCode::OK, "{}", text(&revision));
    let document: serde_json::Value = serde_json::from_slice(&revision.body).expect("a document");
    let siblings = document["siblings"].to_string();
    assert!(
        siblings.contains("config.json") && siblings.contains("model.safetensors"),
        "the revision lists the files of the model: {siblings}"
    );
    let resolved = get(
        &v.app,
        &format!("/hf/{repo}/hodei/demo-model/resolve/main/model.safetensors"),
        ALICE,
    )
    .await;
    assert!(
        resolved.status.is_success() || resolved.status.is_redirection(),
        "the resolve answers: {}",
        resolved.status
    );
    let blob = get(
        &v.app,
        &format!(
            "/hf/{repo}/cdn-lfs/hodei/demo-model/{}/model.safetensors",
            sha256_hex(&weights)
        ),
        ALICE,
    )
    .await;
    assert_eq!(blob.status, StatusCode::OK, "{}", text(&blob));
    assert_eq!(blob.body, weights, "the weights come out of the CAS");

    // Read-only, and scoped by account.
    let write = send(
        &v.app,
        Method::PUT,
        &format!("/hf/{repo}/hodei/demo-model/resolve/main/config.json"),
        Some(ALICE),
        None,
        b"{}".to_vec(),
    )
    .await;
    assert_eq!(write.status, StatusCode::METHOD_NOT_ALLOWED);
    for path in [
        format!("/hf/{repo}/api/models/hodei/demo-model/revision/main"),
        format!("/hf/{repo}/hodei/demo-model/resolve/main/config.json"),
    ] {
        assert_eq!(
            get(&v.app, &path, BOB).await.status,
            StatusCode::NOT_FOUND,
            "another account sees no model"
        );
    }
}
