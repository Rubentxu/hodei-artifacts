//! The OCI catalog on the real Dgraph engine: manifest, tags and referrers
//! consistent.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` like all the stack's ITs. What it
//! proves, against the real engine (TLS + ACL) and the real registry:
//!
//! - **AC1** the real OCI flow (blobs → manifest by tag → pull →
//!   referrers) over the Dgraph catalog serves the same digest that was
//!   published: the manifest digest is that of ITS bytes, always. Two
//!   writers moving the same tag are serialized by the guard
//!   of the tag key: the commit that the graph arbitrates last
//!   wins, and the row is one, not two;
//! - **AC2** redelivery of the same push duplicates neither objects nor
//!   tags, and a referrer does not cross the account: another
//!   account's referrer does not appear in this one's list, and another account's
//!   manifest validates nothing here;
//! - the blob availability gate **before serving**: a manifest
//!   whose bytes are no longer in the CAS is not served, and a manifest that
//!   names a layer the repository does not have is not published;
//! - the reads of the published state say what the contract says
//!   (manifest, tags list, referrers list).

use std::path::PathBuf;
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
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use hodei_catalog::{
    CatalogReader, CatalogWriter, DigestScope, PackageCoordinates, Publication, ReferrerRecord,
    RepositoryConfig, RepositoryDescriptor, RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_evidence::ContentDigest;
use hodei_graph_client::{GraphClient, GraphClientConfig};
use hodei_hub::blob::FileBlobStore;
use tempfile::TempDir;
use tower::ServiceExt;

/// The run: one per execution, so that no test finds
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
    format!("it190{}{}", tag, &nonce()[8..])
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

// The Dgraph catalog store: the conditional open shared by
// the stack's gated suites (HSP-219). The flake proved lives here -- this
// binary and HSP-187's failed at `open` when running both in
// parallel with default threads (2/8 observed; 5/8 replicated 21-11)--:
// concurrent DDL `alter`s fall in Dgraph's reindexing window
// and return `errIndexingInProgress`. The helper serializes the process's
// opens and retries the transient. **Recommended lane**: suites
// that alter the same type, one at a time; default threads within
// each binary.
include!("support/hsp219_concurrent_open.rs");

/// The evidence directory: `target/uat/hsp190/`.
fn the_evidence_dir() -> PathBuf {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let target = tmp.parent().unwrap_or(&tmp).to_path_buf();
    let dir = target.join("uat").join("hsp190");
    std::fs::create_dir_all(&dir).expect("the evidence directory");
    dir
}

/// A line of evidence, with the timestamp.
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

// ---------------------------------------------------------------------------
// The HTTP vertical: the real OCI registry over the Dgraph catalog.
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

/// A hosted OCI repository descriptor.
fn an_oci_descriptor(account: &str, repo_key: &str) -> RepositoryDescriptor {
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", account, repo_key),
        account: account.to_string(),
        repo_key: repo_key.to_string(),
        format: RepositoryFormat::Oci,
        mode: RepositoryMode::Hosted,
        config: RepositoryConfig::new(DigestScope::Global),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

/// The mounted vertical: the OCI registry with the Dgraph catalog and the real
/// file CAS.
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
        let store = Arc::new(open_store(client).await);
        store
            .create_repository(account, an_oci_descriptor(account, repo_key))
            .await
            .expect("the repository is created");

        let tmp = tempfile::tempdir().expect("temp dir");
        let mut config = AppConfig::default();
        config.security.oci_api_enabled = true;
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
    content_type: Option<&str>,
) -> Answer {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(kind) = content_type {
        builder = builder.header(header::CONTENT_TYPE, kind);
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
// Deterministic OCI content: the same inputs, the same digests.
// ---------------------------------------------------------------------------

const OCI_MANIFEST_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";

/// The digest of some bytes.
fn the_digest(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    format!("sha256:{:x}", Sha256::digest(bytes))
}

/// A deterministic layer blob.
fn a_layer(seed: u8) -> Vec<u8> {
    vec![seed; 64]
}

/// A deterministic manifest: config + layer, without clock or randomness. The same
/// set of bytes ALWAYS produces the same manifest digest.
fn a_manifest_bytes(
    config_digest: &str,
    config_size: u64,
    layer_digest: &str,
    layer_size: u64,
) -> Vec<u8> {
    let manifest = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": OCI_MANIFEST_TYPE,
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": config_digest,
            "size": config_size,
        },
        "layers": [
            {
                "mediaType": "application/vnd.oci.image.layer.v1.tar",
                "digest": layer_digest,
                "size": layer_size,
            }
        ],
    });
    serde_json::to_vec(&manifest).expect("the manifest serializes")
}

/// A referrer manifest: the same body with a declared `subject`.
fn a_referrer_bytes(subject_digest: &str, layer_digest: &str, layer_size: u64) -> Vec<u8> {
    let manifest = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": OCI_MANIFEST_TYPE,
        "artifactType": "application/vnd.hodei.sbom.v1",
        "config": {
            "mediaType": "application/vnd.oci.empty.v1+json",
            "digest": the_digest(b"{}"),
            "size": 2u64,
        },
        "layers": [
            {
                "mediaType": "application/vnd.oci.image.layer.v1.tar",
                "digest": layer_digest,
                "size": layer_size,
            }
        ],
        "subject": {
            "mediaType": OCI_MANIFEST_TYPE,
            "digest": subject_digest,
            "size": 0u64,
        },
    });
    serde_json::to_vec(&manifest).expect("the referrer serializes")
}

/// Publishes a blob by the monolithic route: `POST .../blobs/uploads/?digest=`.
async fn push_blob(vertical: &Vertical, repo: &str, bytes: &[u8]) -> Answer {
    let digest = the_digest(bytes);
    send(
        &vertical.app,
        Method::POST,
        &format!("/v2/{repo}/blobs/uploads/?digest={digest}"),
        Some("alice"),
        Some(bytes.to_vec()),
        Some("application/octet-stream"),
    )
    .await
}

// ---------------------------------------------------------------------------
// AC1: push/pull/tag/referrers with a reproducible digest over Dgraph.
// ---------------------------------------------------------------------------

/// The real OCI flow over the Dgraph catalog: the blobs enter the CAS, the
/// manifest enters by its tag, and what is served is the digest of the
/// bytes that were published -- by tag and by digest. The referrer hangs
/// from the subject and the list discovers it.
#[tokio::test]
async fn the_oci_flow_serves_the_same_digest_tag_and_referrers_over_dgraph() {
    let Some(_) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    let account = account("flow");
    let repo = "images";
    let vertical = Vertical::mount(&account, repo).await;

    // The blobs first: config and layer.
    let config_bytes = b"the config of the image";
    let layer = a_layer(1);
    for blob in [config_bytes.as_slice(), layer.as_slice()] {
        let answer = push_blob(&vertical, repo, blob).await;
        assert_eq!(answer.status, StatusCode::CREATED, "{:?}", answer.body);
    }
    let config_digest = the_digest(config_bytes);
    let layer_digest = the_digest(&layer);

    // The manifest: the EXPECTED digest is that of the bytes that go
    // on the wire. The registry invents none.
    let manifest_bytes = a_manifest_bytes(
        &config_digest,
        config_bytes.len() as u64,
        &layer_digest,
        layer.len() as u64,
    );
    let expected_digest = the_digest(&manifest_bytes);

    let pushed = send(
        &vertical.app,
        Method::PUT,
        &format!("/v2/{repo}/manifests/v1"),
        Some("alice"),
        Some(manifest_bytes.clone()),
        Some(OCI_MANIFEST_TYPE),
    )
    .await;
    assert_eq!(pushed.status, StatusCode::CREATED, "{:?}", pushed.body);
    assert_eq!(
        pushed.header("docker-content-digest"),
        Some(expected_digest.as_str()),
        "the digest of the manifest is the digest of its bytes"
    );

    // Pull by tag: the same bytes, the same digest.
    let pulled_by_tag = send(
        &vertical.app,
        Method::GET,
        &format!("/v2/{repo}/manifests/v1"),
        Some("alice"),
        None,
        None,
    )
    .await;
    assert_eq!(pulled_by_tag.status, StatusCode::OK);
    assert_eq!(pulled_by_tag.body, manifest_bytes);
    assert_eq!(
        pulled_by_tag.header("docker-content-digest"),
        Some(expected_digest.as_str())
    );

    // Pull by digest: the same.
    let pulled_by_digest = send(
        &vertical.app,
        Method::GET,
        &format!("/v2/{repo}/manifests/{expected_digest}"),
        Some("alice"),
        None,
        None,
    )
    .await;
    assert_eq!(pulled_by_digest.status, StatusCode::OK);
    assert_eq!(pulled_by_digest.body, manifest_bytes);

    // The tag list: the tag that was born with the push.
    let tags = send(
        &vertical.app,
        Method::GET,
        &format!("/v2/{repo}/tags/list"),
        Some("alice"),
        None,
        None,
    )
    .await;
    assert_eq!(tags.status, StatusCode::OK);
    let listed: serde_json::Value = serde_json::from_slice(&tags.body).expect("the tag list");
    let names = listed
        .get("tags")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(
        names.iter().any(|tag| tag.as_str() == Some("v1")),
        "the tag list does not name v1: {listed}"
    );

    // The referrer: a manifest whose subject is the manifest.
    let referrer_bytes = a_referrer_bytes(&expected_digest, &layer_digest, layer.len() as u64);
    let empty_config = b"{}";
    let referrer_push = {
        push_blob(&vertical, repo, empty_config).await;
        send(
            &vertical.app,
            Method::PUT,
            &format!("/v2/{repo}/manifests/{}", the_digest(&referrer_bytes)),
            Some("alice"),
            Some(referrer_bytes.clone()),
            Some(OCI_MANIFEST_TYPE),
        )
        .await
    };
    assert_eq!(
        referrer_push.status,
        StatusCode::CREATED,
        "{:?}",
        referrer_push.body
    );

    let referrers = send(
        &vertical.app,
        Method::GET,
        &format!("/v2/{repo}/referrers/{expected_digest}"),
        Some("alice"),
        None,
        None,
    )
    .await;
    assert_eq!(referrers.status, StatusCode::OK, "{:?}", referrers.body);
    let discovered: serde_json::Value =
        serde_json::from_slice(&referrers.body).expect("the referrers list");
    let manifests = discovered
        .get("manifests")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        manifests.len(),
        1,
        "the referrers list has exactly the one referrer: {discovered}"
    );
    assert_eq!(
        manifests[0]
            .get("digest")
            .and_then(serde_json::Value::as_str),
        Some(the_digest(&referrer_bytes).as_str()),
    );

    record_evidence(
        "AC1",
        &format!(
            "push/pull (tag y digest) sobre Dgraph: manifest digest {expected_digest} reproducible; \
             tags/list dice v1; referrers del subject dice 1 entrada con el digest del referrer"
        ),
    );
}

// ---------------------------------------------------------------------------
// AC1: the concurrent tag with CAS.
// ---------------------------------------------------------------------------

/// Two writers that move the SAME tag: the tag-key guard
/// serializes them, the commit that the graph arbitrates last
/// wins, and the referrer history is not corrupted: the row is one, and the
/// referrers of each manifest remain where they were.
#[tokio::test]
async fn two_writers_moving_the_same_tag_keep_one_row_and_the_referrers() {
    let Some(_) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    let account = account("race");
    let repo = "images";
    let client = fresh_client().await.expect("the client connects");
    let store = Arc::new(open_store(client).await);
    store
        .create_repository(&account, an_oci_descriptor(&account, repo))
        .await
        .expect("the repository");

    // Two published manifests: two versions the tag can
    // point to. And a referrer hanging from the first, which has to stay
    // there after the fight.
    let content_a = b"manifest-a";
    let content_b = b"manifest-b";
    let subject = ContentDigest::of_bytes(b"the-subject");
    for (path, content) in [
        ("manifests/digest-a", content_a.as_slice()),
        ("manifests/digest-b", content_b.as_slice()),
    ] {
        let publication = Publication {
            repository: repo.to_string(),
            asset_path: path.to_string(),
            coordinates: PackageCoordinates {
                package_name: path.to_string(),
                package_version: ContentDigest::of_bytes(content).value.clone(),
            },
            digest: ContentDigest::of_bytes(content),
            size_bytes: content.len() as u64,
            tags: Vec::new(),
            actor: "it".to_string(),
            correlation_id: format!("corr-{account}"),
            overwrite: true,
            precondition: None,
            subject: Some(subject.clone()),
        };
        store
            .publish(&account, publication)
            .await
            .expect("the publish");
    }
    let coords_a = PackageCoordinates {
        package_name: "manifests/digest-a".to_string(),
        package_version: ContentDigest::of_bytes(content_a).value,
    };
    let coords_b = PackageCoordinates {
        package_name: "manifests/digest-b".to_string(),
        package_version: ContentDigest::of_bytes(content_b).value,
    };

    // The fight: two writers, the same tag, different versions,
    // at the same time. Two rounds, so that the order in which the graph
    // arbitrates is tested more than once.
    for round in 0..2 {
        let (left, right) = tokio::join!(
            store.move_tag(&account, repo, "latest", coords_a.clone()),
            store.move_tag(&account, repo, "latest", coords_b.clone()),
        );
        assert!(
            left.is_ok() && right.is_ok(),
            "both writers settle: {left:?} / {right:?} (round {round})"
        );
    }

    // The row is ONE, and points to one of the two versions: the last
    // commit wins, and there is no invented third version.
    let tags = store.tags(&account, repo).await.expect("the tags");
    assert_eq!(tags.len(), 1, "one tag row, not one per writer: {tags:?}");
    assert_eq!(tags[0].tag_name, "latest");
    assert!(
        tags[0].coordinates == coords_a || tags[0].coordinates == coords_b,
        "the tag points at one of the two commits: {:?}",
        tags[0]
    );
    let read = store.tag(&account, repo, "latest").await.expect("the tag");
    assert_eq!(read.coordinates, tags[0].coordinates);

    // The referrer history did not move: both manifests are still
    // hanging from the subject.
    let referrers: Vec<ReferrerRecord> = store
        .referrers(&account, repo, &subject)
        .await
        .expect("the referrers");
    assert_eq!(referrers.len(), 2, "both manifests stay referrers");
    let paths: Vec<&str> = referrers
        .iter()
        .map(|record| record.asset_path.as_str())
        .collect();
    assert!(paths.contains(&"manifests/digest-a"));
    assert!(paths.contains(&"manifests/digest-b"));

    record_evidence(
        "AC1",
        &format!(
            "tag concurrente con CAS: dos escritores moviendo 'latest' dos rondas; la fila es 1, \
             apunta a {}@{}; referrers del subject intactos (2)",
            tags[0].coordinates.package_name, tags[0].coordinates.package_version
        ),
    );
}

// ---------------------------------------------------------------------------
// AC2: redelivery of the same push and the tenant crossing.
// ---------------------------------------------------------------------------

/// Redelivery of the same push (same bytes, same tag) duplicates
/// nothing: the same manifest twice is one object, the tag is one, and
/// the referrer hangs once.
#[tokio::test]
async fn a_replayed_push_keeps_one_manifest_one_tag_and_one_referrer() {
    let Some(_) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    let account = account("retry");
    let repo = "images";
    let vertical = Vertical::mount(&account, repo).await;

    let config_bytes = b"the config of the retry";
    let layer = a_layer(2);
    for blob in [config_bytes.as_slice(), layer.as_slice()] {
        let answer = push_blob(&vertical, repo, blob).await;
        assert_eq!(answer.status, StatusCode::CREATED);
    }
    let config_digest = the_digest(config_bytes);
    let layer_digest = the_digest(&layer);
    let manifest_bytes = a_manifest_bytes(
        &config_digest,
        config_bytes.len() as u64,
        &layer_digest,
        layer.len() as u64,
    );
    let manifest_digest = the_digest(&manifest_bytes);

    // The push, twice: redelivery after a timeout is the same
    // request with the same bytes.
    for attempt in 1..=2 {
        let answer = send(
            &vertical.app,
            Method::PUT,
            &format!("/v2/{repo}/manifests/v1"),
            Some("alice"),
            Some(manifest_bytes.clone()),
            Some(OCI_MANIFEST_TYPE),
        )
        .await;
        assert_eq!(
            answer.status,
            StatusCode::CREATED,
            "the attempt {attempt} settles: {:?}",
            answer.body
        );
        assert_eq!(
            answer.header("docker-content-digest"),
            Some(manifest_digest.as_str())
        );
    }

    // One object, one tag. And the read says so: the asset path
    // carries the hex of the digest, without the algorithm.
    let manifest_hex = manifest_digest
        .strip_prefix("sha256:")
        .expect("the digest names its algorithm");
    let asset = vertical
        .reader
        .asset(&account, repo, &format!("manifests/{manifest_hex}"))
        .await
        .expect("the manifest asset");
    assert_eq!(asset.digest.value, manifest_hex);

    let tags = send(
        &vertical.app,
        Method::GET,
        &format!("/v2/{repo}/tags/list"),
        Some("alice"),
        None,
        None,
    )
    .await;
    let listed: serde_json::Value = serde_json::from_slice(&tags.body).expect("the tag list");
    let names: Vec<String> = listed
        .get("tags")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        names.iter().filter(|tag| tag.as_str() == "v1").count(),
        1,
        "the tag is one, not one per delivery: {listed}"
    );

    // And the referrer published before the redelivery is still one.
    let referrer_bytes = a_referrer_bytes(&manifest_digest, &layer_digest, layer.len() as u64);
    let answer = send(
        &vertical.app,
        Method::PUT,
        &format!("/v2/{repo}/manifests/{}", the_digest(&referrer_bytes)),
        Some("alice"),
        Some(referrer_bytes.clone()),
        Some(OCI_MANIFEST_TYPE),
    )
    .await;
    assert_eq!(answer.status, StatusCode::CREATED, "{:?}", answer.body);
    let referrers: Vec<ReferrerRecord> = vertical
        .reader
        .referrers(&account, repo, &ContentDigest::of_bytes(&manifest_bytes))
        .await
        .expect("the referrers");
    assert_eq!(referrers.len(), 1, "one referrer, not one per delivery");

    record_evidence(
        "AC2",
        "reentrega del mismo push (2 entregas de los mismos bytes y la misma etiqueta): \
         1 asset de manifiesto, 1 entrada en tags/list, 1 referrer; sin objetos duplicados",
    );
}

/// A referrer does not cross the account: another account's manifest does not appear in
/// this one's list, nor does it validate anything in it. And the same subject in two
/// accounts carries two lists that do not get mixed.
#[tokio::test]
async fn a_referrer_of_another_account_does_not_cross_and_a_retry_writes_nothing_twice() {
    let Some(_) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    let account_a = account("tenant-a");
    let account_b = account("tenant-b");
    let repo = "images";
    let client = fresh_client().await.expect("the client connects");
    let store = Arc::new(open_store(client).await);
    store
        .create_repository(&account_a, an_oci_descriptor(&account_a, repo))
        .await
        .expect("the repository A");
    store
        .create_repository(&account_b, an_oci_descriptor(&account_b, repo))
        .await
        .expect("the repository B");

    // The subject lives in account A: a manifest, and its referrer.
    let manifest_a = b"the manifest of account A";
    let digest_a = ContentDigest::of_bytes(manifest_a);
    let subject = ContentDigest::of_bytes(b"the-shared-subject");
    let publication_a = Publication {
        repository: repo.to_string(),
        asset_path: format!("manifests/{}", digest_a.value),
        coordinates: PackageCoordinates {
            package_name: format!("manifests/{}", digest_a.value),
            package_version: digest_a.value.clone(),
        },
        digest: digest_a.clone(),
        size_bytes: manifest_a.len() as u64,
        tags: Vec::new(),
        actor: "it".to_string(),
        correlation_id: format!("corr-{account_a}"),
        overwrite: true,
        precondition: None,
        subject: Some(subject.clone()),
    };
    store
        .publish(&account_a, publication_a.clone())
        .await
        .expect("the publish in A");

    // Account B publishes ITS manifest with the same subject: the protocol
    // accepts a referrer whose subject is not there yet, and B does not see A.
    let manifest_b = b"the manifest of account B";
    let digest_b = ContentDigest::of_bytes(manifest_b);
    let publication_b = Publication {
        repository: repo.to_string(),
        asset_path: format!("manifests/{}", digest_b.value),
        coordinates: PackageCoordinates {
            package_name: format!("manifests/{}", digest_b.value),
            package_version: digest_b.value.clone(),
        },
        digest: digest_b.clone(),
        size_bytes: manifest_b.len() as u64,
        tags: Vec::new(),
        actor: "it".to_string(),
        correlation_id: format!("corr-{account_b}"),
        overwrite: true,
        precondition: None,
        subject: Some(subject.clone()),
    };
    store
        .publish(&account_b, publication_b.clone())
        .await
        .expect("the publish in B");

    // The two lists: each its own. A's does not name B.
    let in_a: Vec<ReferrerRecord> = store
        .referrers(&account_a, repo, &subject)
        .await
        .expect("the referrers of A");
    assert_eq!(
        in_a.iter()
            .map(|record| record.digest.value.clone())
            .collect::<Vec<_>>(),
        vec![digest_a.value.clone()],
        "the list of A only says A"
    );
    let in_b: Vec<ReferrerRecord> = store
        .referrers(&account_b, repo, &subject)
        .await
        .expect("the referrers of B");
    assert_eq!(
        in_b.iter()
            .map(|record| record.digest.value.clone())
            .collect::<Vec<_>>(),
        vec![digest_b.value.clone()],
        "the list of B only says B"
    );

    // And A's manifest validates nothing in B: B's read of the path
    // of A's manifest says it is not there.
    let cross = store
        .asset(&account_b, repo, &format!("manifests/{}", digest_a.value))
        .await;
    assert!(cross.is_err(), "the manifest of A is not content of B");

    // Redelivery of the same publish in A (the same fact) does not add a
    // second row: A's list still says the same.
    store
        .publish(&account_a, publication_a)
        .await
        .expect("the replayed publish");
    let in_a_after: Vec<ReferrerRecord> = store
        .referrers(&account_a, repo, &subject)
        .await
        .expect("the referrers of A after the replay");
    assert_eq!(in_a_after.len(), 1, "the replay writes nothing twice");

    record_evidence(
        "AC2",
        "cruce de tenant: el mismo subject en dos cuentas lleva una lista cada una; \
         el manifest de A no es contenido de B; la reentrega del publish de A no duplica filas",
    );
}

// ---------------------------------------------------------------------------
// AC3: the blob gate before serving.
// ---------------------------------------------------------------------------

/// A manifest whose layer is not in the repository is not published, and a
/// manifest whose bytes are no longer in the CAS is not served: the gate goes
/// before serving, and the external CAS is the one that has the bytes.
#[tokio::test]
async fn a_manifest_without_its_blob_is_not_published_and_one_without_bytes_is_not_served() {
    let Some(_) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    let account = account("gate");
    let repo = "images";
    let vertical = Vertical::mount(&account, repo).await;

    // The layer is NOT published, and the manifest that names it falls: the
    // write gate checks the CAS for each required descriptor.
    let config_bytes = b"the config of the gate";
    let answer = push_blob(&vertical, repo, config_bytes).await;
    assert_eq!(answer.status, StatusCode::CREATED);
    let layer = a_layer(3);
    let layer_digest = the_digest(&layer);
    let manifest_bytes = a_manifest_bytes(
        &the_digest(config_bytes),
        config_bytes.len() as u64,
        &layer_digest,
        layer.len() as u64,
    );
    let refused = send(
        &vertical.app,
        Method::PUT,
        &format!("/v2/{repo}/manifests/v1"),
        Some("alice"),
        Some(manifest_bytes.clone()),
        Some(OCI_MANIFEST_TYPE),
    )
    .await;
    assert_eq!(
        refused.status,
        StatusCode::NOT_FOUND,
        "the manifest naming an unknown blob is not published: {:?}",
        refused.body
    );

    // The happy path: the layer is published, the manifest gets in.
    let pushed_layer = push_blob(&vertical, repo, &layer).await;
    assert_eq!(pushed_layer.status, StatusCode::CREATED);
    let published = send(
        &vertical.app,
        Method::PUT,
        &format!("/v2/{repo}/manifests/v1"),
        Some("alice"),
        Some(manifest_bytes.clone()),
        Some(OCI_MANIFEST_TYPE),
    )
    .await;
    assert_eq!(
        published.status,
        StatusCode::CREATED,
        "{:?}",
        published.body
    );

    // The manifest bytes disappear from the CAS: the graph still
    // says the row is there, but serving without bytes is lying. The gate
    // before serving rejects it as MANIFEST_UNKNOWN.
    let manifest_blob_digest = ContentDigest::of_bytes(&manifest_bytes);
    let path = vertical.content.the_path_of(&manifest_blob_digest);
    std::fs::remove_file(&path).expect("the manifest bytes are removed from the CAS");
    let served = send(
        &vertical.app,
        Method::GET,
        &format!("/v2/{repo}/manifests/v1"),
        Some("alice"),
        None,
        None,
    )
    .await;
    assert_eq!(
        served.status,
        StatusCode::NOT_FOUND,
        "the manifest without its bytes is not served: {:?}",
        served.body
    );

    // And the blob without bytes, the same: BLOB_UNKNOWN, no invented bytes.
    let blob = send(
        &vertical.app,
        Method::GET,
        &format!("/v2/{repo}/blobs/{layer_digest}"),
        Some("alice"),
        None,
        None,
    )
    .await;
    assert_eq!(blob.status, StatusCode::OK);
    let path = vertical
        .content
        .the_path_of(&ContentDigest::of_bytes(&layer));
    std::fs::remove_file(&path).expect("the layer bytes are removed from the CAS");
    let blob_after = send(
        &vertical.app,
        Method::GET,
        &format!("/v2/{repo}/blobs/{layer_digest}"),
        Some("alice"),
        None,
        None,
    )
    .await;
    assert_eq!(
        blob_after.status,
        StatusCode::NOT_FOUND,
        "the blob without bytes is not served: {:?}",
        blob_after.body
    );

    record_evidence(
        "AC3",
        &format!(
            "gate antes de servir: manifest con capa desconocida NO se publica ({}); \
             manifest sin bytes en CAS NO se sirve; blob sin bytes NO se sirve",
            refused.status
        ),
    );
}

// ---------------------------------------------------------------------------
// The native client: the real registry on a real port, spoken to by oras.
// ---------------------------------------------------------------------------

/// What a client command returns.
struct Outcome {
    status: i32,
    stdout: String,
    stderr: String,
}

impl Outcome {
    fn said(&self) -> String {
        let mut text = self.stdout.clone();
        if !self.stderr.trim().is_empty() {
            if !text.trim().is_empty() {
                text.push('\n');
            }
            text.push_str(&self.stderr);
        }
        text
    }
}

/// A client command, run in a temporary home.
fn run_in(home: &std::path::Path, program: &str, args: &[&str]) -> Outcome {
    let output = std::process::Command::new(program)
        .args(args)
        .env("HOME", home)
        .env("DOCKER_CONFIG", home.join("config"))
        .current_dir(home)
        .output()
        .expect("the client binary runs");
    Outcome {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }
}

/// The same registry, on a real port: the native client speaks TCP.
async fn serve_over_tcp(vertical: &Vertical) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("the registry binds a local port");
    let address = listener
        .local_addr()
        .expect("the registry knows its address");
    let app = vertical.app.clone();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    address
}

/// `oras`, against the registry with the Dgraph catalog: push, pull,
/// attach and discover. The digests that the client checks are those the
/// catalog stores.
///
/// The runtime is multi-threaded on purpose: the native client runs in a
/// separate process that blocks the caller, and the test's server
/// needs its thread to answer it. With a single-threaded runtime, the
/// two wait for each other forever.
#[tokio::test(flavor = "multi_thread")]
async fn the_oras_pushes_pulls_and_discovers_over_the_dgraph_catalog() {
    let Some(_) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    // The client is whichever the machine has: without it, the test says so and
    // does not pretend.
    let probe = run_in(&std::env::temp_dir(), "oras", &["version"]);
    if probe.status != 0 {
        record_evidence(
            "oras",
            "SKIP: the oras binary is not available on this host",
        );
        eprintln!("SKIP: oras is not available");
        return;
    }

    let account = account("oras");
    let repo = "images";
    let vertical = Vertical::mount(&account, repo).await;
    let address = serve_over_tcp(&vertical).await;
    let registry = format!("{address}/{repo}");

    let home = tempfile::tempdir().expect("the client home");
    let reference = format!("{registry}:v1");

    // The credential travels via standard input: the command line
    // is visible in the machine-wide ps, and standard input is not.
    let mut login = std::process::Command::new("oras")
        .args([
            "login",
            "--plain-http",
            "--username",
            "alice",
            "--password-stdin",
            &format!("{address}"),
        ])
        .env("HOME", home.path())
        .env("DOCKER_CONFIG", home.path().join("config"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("oras login starts");
    {
        use std::io::Write as _;
        login
            .stdin
            .take()
            .expect("the stdin of the login")
            .write_all(b"alice")
            .expect("the credential is written");
    }
    let logged = login.wait_with_output().expect("the login finishes");
    let logged = Outcome {
        status: logged.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&logged.stdout).to_string(),
        stderr: String::from_utf8_lossy(&logged.stderr).to_string(),
    };
    assert_eq!(logged.status, 0, "oras login:\n{}", logged.said());

    // Push: a file as a layer, with a declared artifact type.
    std::fs::write(home.path().join("layer.bin"), b"the oras artifact bytes")
        .expect("the layer is written");
    let pushed = run_in(
        home.path(),
        "oras",
        &[
            "push",
            "--plain-http",
            "--artifact-type",
            "application/vnd.hodei.artifact.v1",
            &reference,
            "layer.bin",
        ],
    );
    assert_eq!(pushed.status, 0, "oras push:\n{}", pushed.said());
    let pushed_digest = pushed
        .said()
        .lines()
        .find_map(|line| {
            line.strip_prefix("Digest: ")
                .map(|digest| digest.trim().to_string())
        })
        .unwrap_or_default();
    assert!(
        pushed_digest.starts_with("sha256:"),
        "oras push says a digest:\n{}",
        pushed.said()
    );

    // Pull: the same file, in the same state.
    let pulled = run_in(home.path(), "oras", &["pull", "--plain-http", &reference]);
    assert_eq!(pulled.status, 0, "oras pull:\n{}", pulled.said());
    let downloaded = std::fs::read(home.path().join("layer.bin")).expect("the pulled layer");
    assert_eq!(downloaded, b"the oras artifact bytes");

    // Attach: an SBOM hung from the artifact; discover finds it through the
    // referrers list that the Dgraph catalog serves.
    std::fs::write(home.path().join("sbom.json"), b"{\"components\":[]}")
        .expect("the sbom is written");
    let attached = run_in(
        home.path(),
        "oras",
        &[
            "attach",
            "--plain-http",
            "--artifact-type",
            "application/vnd.hodei.sbom.v1",
            &reference,
            "sbom.json",
        ],
    );
    assert_eq!(attached.status, 0, "oras attach:\n{}", attached.said());

    let discovered = run_in(
        home.path(),
        "oras",
        &["discover", "--plain-http", "--format", "json", &reference],
    );
    assert_eq!(
        discovered.status,
        0,
        "oras discover:\n{}",
        discovered.said()
    );
    // The referrers list names the SBOM manifest, with its artifact
    // type: it is what the catalog serves by the subject.
    let discovered_json: serde_json::Value =
        serde_json::from_str(discovered.stdout.trim()).expect("the discover output");
    let manifests = discovered_json
        .get("manifests")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert_eq!(manifests.len(), 1, "one referrer: {discovered_json}");
    assert_eq!(
        manifests[0]
            .get("artifactType")
            .and_then(serde_json::Value::as_str),
        Some("application/vnd.hodei.sbom.v1")
    );

    // The tag that the push created is the one the catalog reports.
    let tags = vertical
        .reader
        .tag(&account, repo, "v1")
        .await
        .expect("the tag the oras created");
    assert_eq!(tags.tag_name, "v1");

    std::fs::write(
        the_evidence_dir().join("oras-discover.json"),
        &discovered.stdout,
    )
    .expect("the oras evidence");
    record_evidence(
        "oras",
        &format!(
            "oras real sobre el catálogo Dgraph (puerto {address}): login, push ({pushed_digest}), \
             pull de los mismos bytes, attach del SBOM y discover que nombra su digest; \
             la etiqueta v1 resuelve en el catálogo"
        ),
    );
}

// ---------------------------------------------------------------------------
// AC4: the reads say what the catalog contract says.
// ---------------------------------------------------------------------------

/// The OCI state (manifest with subject, its referrer, its tag) published
/// in Dgraph is read as the contract says: asset (digest, subject, size),
/// referrers list (exactly the published referrer), tag and tags list.
/// (HSP-314 F4: it used to be compared against a second engine; the expected
/// values now come from what the test published.)
#[tokio::test]
async fn the_dgraph_oci_reads_match_the_contract_for_the_published_state() {
    let Some(_) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    let account = account("parity");
    let repo = "images";

    let client = fresh_client().await.expect("the client connects");
    let dgraph = open_store(client).await;
    dgraph
        .create_repository(&account, an_oci_descriptor(&account, repo))
        .await
        .expect("the dgraph repository");

    // The state: manifest, its referrer, the tag.
    let manifest = b"the same oci state in two engines";
    let manifest_digest = ContentDigest::of_bytes(manifest);
    let subject = ContentDigest::of_bytes(b"the parity subject");
    let referrer = b"the parity sbom";
    let referrer_digest = ContentDigest::of_bytes(referrer);
    let correlation = format!("corr-dg-{account}");
    let manifest_publication = Publication {
        repository: repo.to_string(),
        asset_path: format!("manifests/{}", manifest_digest.value),
        coordinates: PackageCoordinates {
            package_name: format!("manifests/{}", manifest_digest.value),
            package_version: manifest_digest.value.clone(),
        },
        digest: manifest_digest.clone(),
        size_bytes: manifest.len() as u64,
        tags: Vec::new(),
        actor: "it".to_string(),
        correlation_id: correlation.clone(),
        overwrite: true,
        precondition: None,
        subject: Some(subject.clone()),
    };
    let referrer_publication = Publication {
        repository: repo.to_string(),
        asset_path: format!("manifests/{}", referrer_digest.value),
        coordinates: PackageCoordinates {
            package_name: format!("manifests/{}", referrer_digest.value),
            package_version: referrer_digest.value.clone(),
        },
        digest: referrer_digest.clone(),
        size_bytes: referrer.len() as u64,
        tags: Vec::new(),
        actor: "it".to_string(),
        correlation_id: correlation,
        overwrite: true,
        precondition: None,
        subject: Some(manifest_digest.clone()),
    };
    dgraph
        .publish(&account, manifest_publication)
        .await
        .expect("dgraph manifest");
    dgraph
        .publish(&account, referrer_publication)
        .await
        .expect("dgraph referrer");

    // The manifest: the digest, the subject and the bytes that were published.
    let dgraph_asset = dgraph
        .asset(
            &account,
            repo,
            &format!("manifests/{}", manifest_digest.value),
        )
        .await
        .expect("dgraph asset");
    assert_eq!(dgraph_asset.digest, manifest_digest);
    assert_eq!(dgraph_asset.subject, Some(subject));
    assert_eq!(dgraph_asset.size_bytes, manifest.len() as u64);

    // The manifest's referrers: exactly the published referrer.
    let dgraph_referrers: Vec<ReferrerRecord> = dgraph
        .referrers(&account, repo, &manifest_digest)
        .await
        .expect("dgraph referrers");
    let dgraph_digests: Vec<String> = dgraph_referrers
        .iter()
        .map(|record| record.digest.value.clone())
        .collect();
    assert_eq!(dgraph_digests, vec![referrer_digest.value.clone()]);

    // The tag: moved to the manifest, and the tags list names it.
    let coords = PackageCoordinates {
        package_name: format!("manifests/{}", manifest_digest.value),
        package_version: manifest_digest.value.clone(),
    };
    let dgraph_tag = dgraph
        .move_tag(&account, repo, "stable", coords.clone())
        .await
        .expect("dgraph tag");
    assert_eq!(dgraph_tag.coordinates, coords);

    let dgraph_tags = dgraph.tags(&account, repo).await.expect("dgraph tags");
    assert_eq!(dgraph_tags.len(), 1);
    assert_eq!(dgraph_tags[0].tag_name, "stable");
    assert_eq!(dgraph_tags[0].coordinates, coords);

    record_evidence(
        "AC4",
        "lecturas OCI segun el contrato: asset (digest+subject+tamano), \
         referrers list (1 entrada, el digest publicado), tag y tags list",
    );
}
