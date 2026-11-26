//! HSP-321 tramo i: publish-time admission (HSP-153) through the real router.
//!
//! A hosted raw repository that declares `config_settings.admission` has every
//! publication decided by the pipeline **before** the catalog is written:
//! integrity + an isolated, versioned provider's scan, measured by the existing
//! Trust Gate. This suite drives the real router over the real Dgraph catalog,
//! a real file CAS, the real Cedar engine and the real egress path, against an
//! in-process provider that the test controls (and counts):
//!
//! - an honest, passing artifact is admitted and served;
//! - a failing scan is refused (403 quarantine) and leaves nothing published;
//! - a provider that is down, partial, hung, out of schema (it tries to choose
//!   the outcome), speaking of another digest, outside the egress allow-list,
//!   or a requirement the pipeline cannot measure, all fail closed (503 +
//!   Retry-After) and publish nothing; the attempts are bounded by the budget;
//! - a repository without the declaration keeps the normal path and the
//!   provider is never called;
//! - declaring admission is only accepted on hosted raw repositories and only
//!   if it can be read.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT`; serialize with `--test-threads=1`.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use axum::extract::{Path, State};
use hodei_artifacts_api::proxy::egress::AllowedHost;

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
    AllowAllAuthorizer, CatalogReader, CatalogWriter, DigestScope, RepositoryConfig,
    RepositoryDescriptor, RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_catalog_storage::DgraphCatalogStore;
use hodei_graph_client::{GraphClient, GraphClientConfig};
use hodei_hub::blob::FileBlobStore;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

fn config_from_env() -> Option<GraphClientConfig> {
    let endpoint = std::env::var("HODEI_GRAPH_IT_ENDPOINT").ok()?;
    let user = std::env::var("HODEI_GRAPH_IT_USER").unwrap_or_default();
    let password = std::env::var("HODEI_GRAPH_IT_PASSWORD").unwrap_or_default();
    let mut text =
        format!("endpoint = \"{endpoint}\"\n\n[tls]\nenabled = false\nca_cert_path = \"\"\n");
    if !user.is_empty() {
        text.push_str(&format!(
            "\n[credentials]\nuserid = \"{user}\"\npassword = \"{password}\"\n"
        ));
    }
    Some(GraphClientConfig::from_toml_str(&text).expect("the IT configuration validates"))
}

async fn open_store() -> Option<Arc<DgraphCatalogStore>> {
    let Some(config) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return None;
    };
    let mut client = GraphClient::connect(config).await.expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    let client = Arc::new(client);
    let mut last = String::new();
    for _ in 0..3 {
        match DgraphCatalogStore::open(client.clone(), Arc::new(AllowAllAuthorizer)).await {
            Ok(store) => return Some(Arc::new(store)),
            Err(error) => {
                last = error.to_string();
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        }
    }
    panic!("the dgraph catalog store does not open: {last}");
}

/// Tokens are `<user>@<account>`: the authenticator trusts them like a test
/// double, the service still derives the account from the verified principal.
struct TokenAuthenticator;

#[async_trait]
impl PrincipalAuthenticator for TokenAuthenticator {
    async fn authenticate(
        &self,
        credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        let Credentials::Bearer(token) = credentials else {
            return Err(AuthenticationError::InvalidCredentials);
        };
        let (user, account) = token
            .split_once('@')
            .ok_or(AuthenticationError::InvalidCredentials)?;
        Ok(PrincipalClaims::new(
            PrincipalId::new(format!("hrn:hodei:iam::{account}:User/{user}")).unwrap(),
            AccountId::new(account).unwrap(),
        ))
    }
}

fn cedar(policy: &str) -> Arc<dyn AuthorizerPort> {
    Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .expect("the policy parses"),
    )))
}

const BODY: &[u8] = b"admitted payload";

/// The provider the test controls: `POST /report/<mode>`. Each mode is a way
/// of answering, and every path has a hit counter.
#[derive(Clone, Default)]
struct Provider {
    hits: Arc<Mutex<HashMap<String, usize>>>,
}

impl Provider {
    fn hits(&self, mode: &str) -> usize {
        *self.hits.lock().unwrap().get(mode).unwrap_or(&0)
    }
}

async fn report(
    State(provider): State<Provider>,
    Path(mode): Path<String>,
    axum::Json(request): axum::Json<Value>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    *provider
        .hits
        .lock()
        .unwrap()
        .entry(mode.clone())
        .or_insert(0) += 1;
    let digest = request["digest"].as_str().unwrap_or_default().to_string();
    let ok = |scan: &str, coverage: &str| {
        axum::Json(json!({"digest": digest, "coverage": coverage, "scan": scan})).into_response()
    };
    match mode.as_str() {
        "pass" => ok("pass", "complete"),
        "fail" => ok("fail", "complete"),
        "partial" => ok("pass", "partial"),
        "unknown" => ok("maybe", "complete"),
        "down" => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        "hung" => {
            tokio::time::sleep(Duration::from_secs(10)).await;
            ok("pass", "complete")
        }
        "adversary" => axum::Json(json!({
            "digest": digest, "coverage": "complete", "scan": "fail", "outcome": "allow"
        }))
        .into_response(),
        "otherdigest" => axum::Json(json!({
            "digest": "sha256:0000", "coverage": "complete", "scan": "pass"
        }))
        .into_response(),
        "huge" => axum::Json(json!({
            "digest": digest, "coverage": "complete", "scan": "pass",
            "pad": "x".repeat(4096)
        }))
        .into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn start_provider() -> (Provider, u16) {
    let provider = Provider::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = axum::Router::new()
        .route("/report/{mode}", axum::routing::post(report))
        .with_state(provider.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.ok() });
    (provider, port)
}

fn policy(account: &str) -> String {
    format!(
        r#"
permit(principal == Iam::User::"hrn:hodei:iam::{account}:User/alice",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset",
                  Artifacts::Action::"CreateRepository"],
       resource);
"#
    )
}

struct Vertical {
    app: Router,
    store: Arc<DgraphCatalogStore>,
    provider: Provider,
    port: u16,
    #[allow(dead_code)]
    tmp: TempDir,
}

async fn mount(account: &str) -> Option<Vertical> {
    let store = open_store().await?;
    let (provider, port) = start_provider().await;
    let tmp = tempfile::tempdir().expect("temp dir");
    let mut config = AppConfig::default();
    config.security.raw_api_enabled = true;
    config.graph = config_from_env();
    config.content.cas_path = tmp
        .path()
        .join("bootstrap-cas")
        .to_string_lossy()
        .into_owned();
    // The administrator's egress allow-list: only this provider, on loopback.
    config.proxy.allowed_hosts = vec![AllowedHost {
        host: "127.0.0.1".to_string(),
        port: Some(port),
        allow_private: true,
        allow_http: true,
    }];
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
        .with_content_store(content)
        .with_authenticator(Arc::new(TokenAuthenticator))
        .with_authorization(cedar(&policy(account)));
    Some(Vertical {
        app: build_router(state, &config),
        store,
        provider,
        port,
        tmp,
    })
}

async fn seed(store: &DgraphCatalogStore, account: &str, key: &str, settings: Value) {
    let mut config = RepositoryConfig::new(DigestScope::Global);
    config.config_settings = settings;
    store
        .create_repository(
            account,
            RepositoryDescriptor {
                hrn: repository_hrn("hodei", "artifacts", "eu", account, key),
                account: account.to_string(),
                repo_key: key.to_string(),
                format: RepositoryFormat::Raw,
                mode: RepositoryMode::Hosted,
                config,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        )
        .await
        .expect("the repository is seeded");
}

/// A hosted raw repository that declares admission against `mode`.
async fn seed_gated(v: &Vertical, account: &str, key: &str, mode: &str, required: Value) {
    seed(
        &v.store,
        account,
        key,
        json!({"admission": {
            "required": required,
            "policy_version": "1",
            "provider": {
                "url": format!("http://127.0.0.1:{}/report/{mode}", v.port),
                "id": "scanner", "version": "7"
            },
            "budget": {"timeout_ms": 400, "max_attempts": 2, "max_response_bytes": 512}
        }}),
    )
    .await;
}

async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    token: &str,
    body: Vec<u8>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "127.0.0.1")
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    if !body.is_empty() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::from(body)).unwrap())
        .await
        .expect("a response");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, headers, bytes.to_vec())
}

async fn put(v: &Vertical, repo: &str, token: &str) -> (StatusCode, axum::http::HeaderMap, String) {
    let (status, headers, body) = send(
        &v.app,
        Method::PUT,
        &format!("/repository/{repo}/file.txt"),
        token,
        BODY.to_vec(),
    )
    .await;
    (status, headers, String::from_utf8_lossy(&body).into_owned())
}

async fn get(v: &Vertical, repo: &str, token: &str) -> (StatusCode, Vec<u8>) {
    let (status, _, body) = send(
        &v.app,
        Method::GET,
        &format!("/repository/{repo}/file.txt"),
        token,
        Vec::new(),
    )
    .await;
    (status, body)
}

#[tokio::test]
async fn an_honest_artifact_is_admitted_and_served_and_the_provider_is_asked_once() {
    let account = format!("h{}a", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    seed_gated(&v, &account, "gated", "pass", json!(["integrity", "scan"])).await;
    let alice = format!("alice@{account}");

    let (status, _, body) = put(&v, "gated", &alice).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        get(&v, "gated", &alice).await,
        (StatusCode::OK, BODY.to_vec())
    );
    assert_eq!(v.provider.hits("pass"), 1);
}

#[tokio::test]
async fn a_failing_scan_is_refused_and_leaves_nothing_published() {
    let account = format!("h{}a", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    seed_gated(&v, &account, "gated", "fail", json!(["integrity", "scan"])).await;
    let alice = format!("alice@{account}");

    let (status, headers, body) = put(&v, "gated", &alice).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(body.contains("admission quarantine"), "{body}");
    assert!(
        headers.get(header::RETRY_AFTER).is_none(),
        "a verdict is not retryable"
    );
    // No false 404 on the publisher, and no asset behind it.
    assert_eq!(get(&v, "gated", &alice).await.0, StatusCode::NOT_FOUND);
    assert_eq!(v.provider.hits("fail"), 1, "a verdict needs no retry");
}

#[tokio::test]
async fn whatever_cannot_be_known_fails_closed_publishes_nothing_and_is_bounded() {
    let account = format!("h{}a", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    let alice = format!("alice@{account}");
    let cases: [(&str, &str, usize); 7] = [
        ("down", "not give a report", 2),
        ("partial", "partial", 1),
        ("unknown", "UNKNOWN", 1),
        ("hung", "not give a report", 2),
        ("adversary", "out of schema", 1),
        ("otherdigest", "does not speak about this digest", 1),
        ("huge", "not usable", 1),
    ];
    for (mode, reason, calls) in cases {
        let repo = format!("r-{mode}");
        seed_gated(&v, &account, &repo, mode, json!(["integrity", "scan"])).await;
        let started = std::time::Instant::now();
        let (status, headers, body) = put(&v, &repo, &alice).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{mode}: {body}");
        assert!(body.contains("admission pending"), "{mode}: {body}");
        assert!(body.contains(reason), "{mode}: {body}");
        assert!(headers.get(header::RETRY_AFTER).is_some(), "{mode}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{mode}: the budget bounds the wait"
        );
        assert_eq!(
            v.provider.hits(mode),
            calls,
            "{mode}: attempts are bounded by the budget"
        );
        assert_eq!(
            get(&v, &repo, &alice).await.0,
            StatusCode::NOT_FOUND,
            "{mode}"
        );
    }
}

#[tokio::test]
async fn a_provider_outside_the_egress_allow_list_is_never_contacted() {
    let account = format!("h{}a", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    let alice = format!("alice@{account}");
    // Same host, a port the administrator did not allow.
    let (other, other_port) = start_provider().await;
    seed(
        &v.store,
        &account,
        "elsewhere",
        json!({"admission": {"required": ["scan"], "provider": {
            "url": format!("http://127.0.0.1:{other_port}/report/pass"),
            "id": "scanner", "version": "7"}}}),
    )
    .await;
    let (status, _, body) = put(&v, "elsewhere", &alice).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(body.contains("egress denied"), "{body}");
    assert_eq!(other.hits("pass"), 0, "the destination was never visited");
    assert_eq!(get(&v, "elsewhere", &alice).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_requirement_the_pipeline_cannot_measure_fails_closed() {
    let account = format!("h{}a", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    let alice = format!("alice@{account}");
    seed_gated(
        &v,
        &account,
        "signed",
        "pass",
        json!(["integrity", "signature"]),
    )
    .await;
    let (status, _, body) = put(&v, "signed", &alice).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(body.contains("unsupported:signature"), "{body}");
    assert_eq!(get(&v, "signed", &alice).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_repository_without_the_declaration_keeps_the_normal_path() {
    let account = format!("h{}a", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    let alice = format!("alice@{account}");
    seed(&v.store, &account, "plain", json!({})).await;
    assert_eq!(put(&v, "plain", &alice).await.0, StatusCode::CREATED);
    assert_eq!(get(&v, "plain", &alice).await.0, StatusCode::OK);
    for mode in ["pass", "fail", "down"] {
        assert_eq!(v.provider.hits(mode), 0, "the provider is not consulted");
    }
}

#[tokio::test]
async fn declaring_admission_is_only_accepted_where_it_gates_and_when_it_can_be_read() {
    let account = format!("h{}a", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    let alice = format!("alice@{account}");
    let create =
        |name: &'static str, format: &'static str, mode: &'static str, admission: Value| {
            let app = v.app.clone();
            let alice = alice.clone();
            async move {
                let body = json!({"name": name, "format": format, "mode": mode,
                "settings": {"admission": admission}});
                send(
                    &app,
                    Method::POST,
                    "/api/v1/catalog/repositories",
                    &alice,
                    serde_json::to_vec(&body).unwrap(),
                )
                .await
                .0
            }
        };
    let valid = json!({"required": ["integrity"]});
    assert_eq!(
        create("ok-raw", "raw", "hosted", valid.clone()).await,
        StatusCode::CREATED
    );
    assert_eq!(
        create("npm-gated", "npm", "hosted", valid.clone()).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        create("broken", "raw", "hosted", json!({"required": "scan"})).await,
        StatusCode::BAD_REQUEST
    );
}
