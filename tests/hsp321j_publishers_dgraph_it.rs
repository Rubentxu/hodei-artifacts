//! HSP-321 tramo j (HSP-152): trusted publishers end to end, through the REAL
//! router over the real Dgraph catalog and registry, real Cedar, real egress
//! and an in-process OIDC issuer (own JWKS, signed tokens).
//!
//! What it demonstrates:
//! - an administrator (dedicated Cedar action `ManageTrustedPublishers`)
//!   registers a trusted publisher; a CI token exchanges for a short
//!   credential; the credential publishes into the bound repository and
//!   nothing else (no read, no other repository, no management, no other
//!   account);
//! - every denial of the exchange is generic to the client and publishes
//!   nothing; deleting the record revokes the credential at once;
//! - the Dgraph registry converges under concurrent writers and isolates
//!   repositories and accounts.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT`; serialize with `--test-threads=1`.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use cedar_policy::Entities;
use chrono::Utc;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::proxy::egress::AllowedHost;
use hodei_artifacts_api::publishers::dgraph::DgraphPublisherRegistry;
use hodei_artifacts_api::publishers::{PublisherRegistry, TrustedPublisher};
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
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

const SECRET: &str = "publisher-credential-secret-0123456789abcdef";
const SUBJECT: &str = "repo:acme/app:ref:refs/heads/main";
const BODY: &[u8] = b"published by a workload";

fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
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

async fn open_client() -> Option<Arc<GraphClient>> {
    let config = config_from_env().or_else(|| {
        // With HODEI_REQUIRE_IT=1 a missing harness is a failure, not a skip.
        assert!(
            std::env::var("HODEI_REQUIRE_IT").as_deref() != Ok("1"),
            "HODEI_REQUIRE_IT=1 but HODEI_GRAPH_IT_ENDPOINT is not set"
        );
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        None
    })?;
    let mut client = GraphClient::connect(config).await.expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    Some(Arc::new(client))
}

/// Tokens are `<user>@<account>`: a test double for the user authenticator.
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

/// `alice` uses repositories; `manager` manages trusted publishers (and only
/// that); `rogue` can create repositories and upload but not manage.
fn policy(accounts: &[&str]) -> String {
    let mut text = String::new();
    for account in accounts {
        text.push_str(&format!(
            r#"
permit(principal == Iam::User::"hrn:hodei:iam::{account}:User/alice",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset",
                  Artifacts::Action::"CreateRepository", Artifacts::Action::"ReadRepository"],
       resource);
permit(principal == Iam::User::"hrn:hodei:iam::{account}:User/manager",
       action in [Artifacts::Action::"ManageTrustedPublishers", Artifacts::Action::"ReadRepository"],
       resource);
"#
        ));
    }
    text
}

// ---- the fake OIDC issuer -----------------------------------------------

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/hsp152/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture")
}

fn pem_of(name: &str) -> String {
    let key: Value = serde_json::from_str(&fixture(name)).unwrap();
    key["pem_lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l.as_str().unwrap())
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Clone)]
struct Issuer {
    status: Arc<Mutex<u16>>,
    hits: Arc<AtomicUsize>,
    port: u16,
}

impl Issuer {
    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

async fn jwks(State(issuer): State<Issuer>) -> axum::response::Response {
    issuer.hits.fetch_add(1, Ordering::SeqCst);
    let status = *issuer.status.lock().unwrap();
    (
        StatusCode::from_u16(status).unwrap(),
        fixture("issuer-jwks-1.json"),
    )
        .into_response()
}

async fn start_issuer() -> Issuer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let issuer = Issuer {
        status: Arc::new(Mutex::new(200)),
        hits: Arc::new(AtomicUsize::new(0)),
        port,
    };
    let app = axum::Router::new()
        .route("/jwks", axum::routing::get(jwks))
        .with_state(issuer.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.ok() });
    issuer
}

static JTI: AtomicUsize = AtomicUsize::new(0);

fn oidc(issuer: &Issuer, overrides: Value) -> String {
    sign("issuer-key-1.json", issuer, overrides)
}

fn sign(key: &str, issuer: &Issuer, overrides: Value) -> String {
    let n = now();
    let mut claims = json!({
        "iss": issuer.url(), "aud": "hodei-artifacts", "sub": SUBJECT,
        "iat": n, "nbf": n, "exp": n + 300,
        "jti": format!("jti-{}-{}", nonce(), JTI.fetch_add(1, Ordering::SeqCst)),
        "repository": "acme/app", "ref": "refs/heads/main", "workflow": "release.yml"
    });
    for (name, value) in overrides.as_object().unwrap() {
        claims[name] = value.clone();
    }
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("issuer-key-1".into());
    encode(
        &header,
        &claims,
        &EncodingKey::from_rsa_pem(pem_of(key).as_bytes()).unwrap(),
    )
    .unwrap()
}

// ---- the vertical ---------------------------------------------------------

struct Vertical {
    app: Router,
    store: Arc<DgraphCatalogStore>,
    issuer: Issuer,
    #[allow(dead_code)]
    tmp: TempDir,
}

async fn mount(accounts: &[&str]) -> Option<Vertical> {
    mount_with(accounts, "").await
}

async fn mount_with(accounts: &[&str], extra_policy: &str) -> Option<Vertical> {
    let client = open_client().await?;
    let store = Arc::new(
        DgraphCatalogStore::open(client, Arc::new(AllowAllAuthorizer))
            .await
            .expect("the catalog opens"),
    );
    let issuer = start_issuer().await;
    let tmp = tempfile::tempdir().expect("temp dir");
    let mut config = AppConfig::default();
    config.security.raw_api_enabled = true;
    config.security.publisher_credential_secret = Some(SECRET.to_string());
    config.graph = config_from_env();
    config.content.cas_path = tmp
        .path()
        .join("bootstrap-cas")
        .to_string_lossy()
        .into_owned();
    // The administrator's egress allow-list: only this issuer, on loopback.
    config.proxy.allowed_hosts = vec![AllowedHost {
        host: "127.0.0.1".to_string(),
        port: Some(issuer.port),
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
    let file = FileBlobStore::new(tmp.path().join("cas")).expect("the CAS");
    let state = state
        .with_catalog(reader, writer)
        .with_content_store(Arc::new(file))
        .with_authenticator(Arc::new(TokenAuthenticator))
        .with_authorization(cedar(&format!("{}{extra_policy}", policy(accounts))));
    Some(Vertical {
        app: build_router(state, &config),
        store,
        issuer,
        tmp,
    })
}

async fn seed(store: &DgraphCatalogStore, account: &str, key: &str) {
    store
        .create_repository(
            account,
            RepositoryDescriptor {
                hrn: repository_hrn("hodei", "artifacts", "eu", account, key),
                account: account.to_string(),
                repo_key: key.to_string(),
                format: RepositoryFormat::Raw,
                mode: RepositoryMode::Hosted,
                config: RepositoryConfig::new(DigestScope::Global),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        )
        .await
        .expect("the repository is seeded");
}

async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Vec<u8>,
) -> (StatusCode, String) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "127.0.0.1");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if !body.is_empty() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::from(body)).unwrap())
        .await
        .expect("a response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

fn spec(issuer: &Issuer) -> Vec<u8> {
    json!({
        "issuer": issuer.url(),
        "audience": "hodei-artifacts",
        "jwks_url": format!("{}/jwks", issuer.url()),
        "subject": SUBJECT,
        "claims": {"repository": "acme/app", "ref": "refs/heads/main", "workflow": "release.yml"}
    })
    .to_string()
    .into_bytes()
}

async fn register(v: &Vertical, account: &str, repo: &str, id: &str) {
    let (status, body) = send(
        &v.app,
        Method::PUT,
        &format!("/api/v1/trusted-publishers/{repo}/{id}"),
        Some(&format!("manager@{account}")),
        spec(&v.issuer),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

async fn exchange(v: &Vertical, token: &str, account: &str, repo: &str) -> (StatusCode, String) {
    send(
        &v.app,
        Method::POST,
        "/api/v1/publishers/exchange",
        Some(token),
        json!({"account": account, "repository": repo})
            .to_string()
            .into_bytes(),
    )
    .await
}

async fn credential(v: &Vertical, account: &str, repo: &str) -> String {
    let (status, body) = exchange(v, &oidc(&v.issuer, json!({})), account, repo).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let answer: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(answer["scope"]["repository"], repo);
    assert_eq!(answer["scope"]["actions"], json!(["upload"]));
    answer["credential"].as_str().unwrap().to_string()
}

async fn publish(v: &Vertical, repo: &str, token: &str) -> (StatusCode, String) {
    send(
        &v.app,
        Method::PUT,
        &format!("/repository/{repo}/file.txt"),
        Some(token),
        BODY.to_vec(),
    )
    .await
}

async fn download(v: &Vertical, repo: &str, token: &str) -> (StatusCode, String) {
    send(
        &v.app,
        Method::GET,
        &format!("/repository/{repo}/file.txt"),
        Some(token),
        Vec::new(),
    )
    .await
}

fn account() -> String {
    format!("p{}a", &nonce()[8..])
}

// ---- tests -----------------------------------------------------------------

#[tokio::test]
async fn a_ci_token_publishes_into_the_bound_repository_end_to_end() {
    let a = account();
    let Some(v) = mount(&[&a]).await else { return };
    seed(&v.store, &a, "releases").await;
    register(&v, &a, "releases", "ci-main").await;

    // The management surface lists it.
    let (status, body) = send(
        &v.app,
        Method::GET,
        "/api/v1/trusted-publishers/releases",
        Some(&format!("manager@{a}")),
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("ci-main"));

    let credential = credential(&v, &a, "releases").await;
    let (status, body) = publish(&v, "releases", &credential).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    // The bytes are there for a user who may read them.
    assert_eq!(
        download(&v, "releases", &format!("alice@{a}")).await,
        (StatusCode::OK, String::from_utf8_lossy(BODY).into_owned())
    );
}

#[tokio::test]
async fn the_credential_publishes_and_nothing_else() {
    let a = account();
    let Some(v) = mount(&[&a]).await else { return };
    seed(&v.store, &a, "releases").await;
    seed(&v.store, &a, "other").await;
    register(&v, &a, "releases", "ci-main").await;
    let credential = credential(&v, &a, "releases").await;

    // Another repository of the same account: out of contract.
    assert_eq!(
        publish(&v, "other", &credential).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        download(&v, "other", &format!("alice@{a}")).await.0,
        StatusCode::NOT_FOUND,
        "nothing was published there"
    );
    // Reading, managing and every user surface reject it as a credential.
    assert_eq!(
        download(&v, "releases", &credential).await.0,
        StatusCode::UNAUTHORIZED
    );
    for uri in [
        "/api/v1/trusted-publishers/releases",
        "/api/v1/catalog/repositories",
        "/api/v1/account/usage",
    ] {
        let (status, _) = send(&v.app, Method::GET, uri, Some(&credential), Vec::new()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri}");
    }
    let (status, _) = send(
        &v.app,
        Method::PUT,
        "/api/v1/trusted-publishers/releases/mine",
        Some(&credential),
        spec(&v.issuer),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "it cannot register publishers"
    );
}

#[tokio::test]
async fn the_credential_never_crosses_accounts() {
    let (a, b) = (account(), account());
    let Some(v) = mount(&[&a, &b]).await else {
        return;
    };
    seed(&v.store, &a, "releases").await;
    seed(&v.store, &b, "releases").await;
    register(&v, &a, "releases", "ci-main").await;
    let credential = credential(&v, &a, "releases").await;
    assert_eq!(
        publish(&v, "releases", &credential).await.0,
        StatusCode::CREATED
    );
    // Same repository name in the other account: untouched.
    assert_eq!(
        download(&v, "releases", &format!("alice@{b}")).await.0,
        StatusCode::NOT_FOUND
    );
    // The token cannot ask for the other account's repository either.
    let (status, _) = exchange(&v, &oidc(&v.issuer, json!({})), &b, "releases").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn every_denial_of_the_exchange_is_generic_and_publishes_nothing() {
    let a = account();
    let Some(v) = mount(&[&a]).await else { return };
    seed(&v.store, &a, "releases").await;
    seed(&v.store, &a, "unbound").await;
    register(&v, &a, "releases", "ci-main").await;
    let honest = oidc(&v.issuer, json!({}));

    let bad: Vec<(&str, String, &str, &str)> = vec![
        (
            "bad signature",
            sign("issuer-key-2.json", &v.issuer, json!({})),
            "releases",
            "unused",
        ),
        (
            "wrong audience",
            oidc(&v.issuer, json!({"aud": "x"})),
            "releases",
            "",
        ),
        (
            "wrong issuer",
            oidc(&v.issuer, json!({"iss": "http://evil.example"})),
            "releases",
            "",
        ),
        (
            "expired",
            oidc(&v.issuer, json!({"exp": now() - 3600})),
            "releases",
            "",
        ),
        (
            "other subject",
            oidc(&v.issuer, json!({"sub": "repo:evil/app"})),
            "releases",
            "",
        ),
        (
            "other workflow",
            oidc(&v.issuer, json!({"workflow": "evil.yml"})),
            "releases",
            "",
        ),
        (
            "missing ref",
            oidc(&v.issuer, json!({"ref": null})),
            "releases",
            "",
        ),
        // The honest token for a repository no record binds it to.
        ("unbound repository", honest.clone(), "unbound", ""),
        ("unknown repository", honest.clone(), "nope", ""),
    ];
    for (label, token, repo, _) in bad {
        let (status, body) = exchange(&v, &token, &a, repo).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{label}: {body}");
        let answer: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(answer["error"], "exchange_denied", "{label}");
        // Generic: no reason, no claim, no key id, no token fragment.
        for leak in [
            "signature",
            "audience",
            "issuer",
            "expired",
            "subject",
            "workflow",
            "claim",
            "kid",
            "jwks",
            token.split('.').nth(1).unwrap_or("-"),
        ] {
            assert!(!body.contains(leak), "{label} leaks {leak:?}: {body}");
        }
    }
    // No token at all.
    let (status, _) = send(
        &v.app,
        Method::POST,
        "/api/v1/publishers/exchange",
        None,
        json!({"account": a, "repository": "releases"})
            .to_string()
            .into_bytes(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        download(&v, "releases", &format!("alice@{a}")).await.0,
        StatusCode::NOT_FOUND
    );

    // The honest token works once and only once (replay).
    let (status, _) = exchange(&v, &honest, &a, "releases").await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = exchange(&v, &honest, &a, "releases").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "replay");
}

#[tokio::test]
async fn an_issuer_that_cannot_be_reached_fails_closed() {
    let a = account();
    let Some(v) = mount(&[&a]).await else { return };
    seed(&v.store, &a, "releases").await;
    register(&v, &a, "releases", "ci-main").await;
    *v.issuer.status.lock().unwrap() = 503;
    let (status, body) = exchange(&v, &oidc(&v.issuer, json!({})), &a, "releases").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(!body.contains("127.0.0.1"), "{body}");
}

#[tokio::test]
async fn deleting_the_record_revokes_the_credential() {
    let a = account();
    let Some(v) = mount(&[&a]).await else { return };
    seed(&v.store, &a, "releases").await;
    register(&v, &a, "releases", "ci-main").await;
    let credential = credential(&v, &a, "releases").await;
    assert_eq!(
        publish(&v, "releases", &credential).await.0,
        StatusCode::CREATED
    );
    let (status, _) = send(
        &v.app,
        Method::DELETE,
        "/api/v1/trusted-publishers/releases/ci-main",
        Some(&format!("manager@{a}")),
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        publish(&v, "releases", &credential).await.0,
        StatusCode::UNAUTHORIZED
    );
    // And no new exchange either.
    let (status, _) = exchange(&v, &oidc(&v.issuer, json!({})), &a, "releases").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn only_the_dedicated_cedar_action_manages_trusted_publishers() {
    let a = account();
    let other = account();
    let Some(v) = mount(&[&a, &other]).await else {
        return;
    };
    seed(&v.store, &a, "releases").await;
    // alice can create repositories and upload, but the action is not implied.
    for (method, uri, body) in [
        (
            Method::PUT,
            "/api/v1/trusted-publishers/releases/mine",
            spec(&v.issuer),
        ),
        (
            Method::GET,
            "/api/v1/trusted-publishers/releases",
            Vec::new(),
        ),
        (
            Method::DELETE,
            "/api/v1/trusted-publishers/releases/mine",
            Vec::new(),
        ),
    ] {
        let (status, _) = send(&v.app, method, uri, Some(&format!("alice@{a}")), body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{uri}");
    }
    // Nobody authenticated, nothing.
    let (status, _) = send(
        &v.app,
        Method::GET,
        "/api/v1/trusted-publishers/releases",
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // A manager of another account does not even see the repository.
    let (status, _) = send(
        &v.app,
        Method::PUT,
        "/api/v1/trusted-publishers/releases/x",
        Some(&format!("manager@{other}")),
        spec(&v.issuer),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_record_is_validated_and_its_jwks_must_be_in_the_egress_allow_list() {
    let a = account();
    let Some(v) = mount(&[&a]).await else { return };
    seed(&v.store, &a, "releases").await;
    let manager = format!("manager@{a}");
    let put = |body: Value| {
        let v = &v;
        let manager = manager.clone();
        async move {
            send(
                &v.app,
                Method::PUT,
                "/api/v1/trusted-publishers/releases/x",
                Some(&manager),
                body.to_string().into_bytes(),
            )
            .await
        }
    };
    let base = || serde_json::from_slice::<Value>(&spec(&v.issuer)).unwrap();
    let mut outside = base();
    outside["jwks_url"] = json!("http://127.0.0.1:9/jwks");
    assert_eq!(put(outside).await.0, StatusCode::BAD_REQUEST);
    let mut metadata = base();
    metadata["jwks_url"] = json!("http://169.254.169.254/latest");
    assert_eq!(put(metadata).await.0, StatusCode::BAD_REQUEST);
    let mut wildcard = base();
    wildcard["subject"] = json!("repo:acme/*");
    assert_eq!(put(wildcard).await.0, StatusCode::BAD_REQUEST);
    let mut unknown = base();
    unknown["actions"] = json!(["delete"]);
    assert_eq!(
        put(unknown).await.0.as_u16() / 100,
        4,
        "no extra fields are accepted"
    );
    // None of them was stored.
    let (_, body) = send(
        &v.app,
        Method::GET,
        "/api/v1/trusted-publishers/releases",
        Some(&manager),
        Vec::new(),
    )
    .await;
    assert_eq!(body.trim(), "[]");
}

#[tokio::test]
async fn the_dgraph_registry_converges_and_isolates() {
    let Some(client) = open_client().await else {
        return;
    };
    let registry = Arc::new(DgraphPublisherRegistry::open(client).await.unwrap());
    let a = account();
    let record = |account: &str, repository: &str, subject: &str| TrustedPublisher {
        id: "ci".into(),
        account: account.into(),
        repository: repository.into(),
        issuer: "https://issuer.example".into(),
        audience: "hodei".into(),
        jwks_url: "https://issuer.example/jwks".into(),
        subject: subject.into(),
        claims: Default::default(),
        created_by: "hrn:hodei:iam::x:User/admin".into(),
        created_at: "2026-01-01T00:00:00Z".into(),
    };
    // Ten concurrent writers of the same record: ONE node.
    let mut tasks = Vec::new();
    for n in 0..10 {
        let registry = registry.clone();
        let record = record(&a, "r1", &format!("s{n}"));
        tasks.push(tokio::spawn(async move { registry.put(&record).await }));
    }
    for task in tasks {
        task.await.unwrap().expect("a writer wins or retries");
    }
    assert_eq!(registry.list(&a, "r1").await.unwrap().len(), 1);
    // Replacement keeps one record; scopes are isolated.
    registry.put(&record(&a, "r1", "final")).await.unwrap();
    registry.put(&record(&a, "r2", "elsewhere")).await.unwrap();
    let listed = registry.list(&a, "r1").await.unwrap();
    assert_eq!((listed.len(), listed[0].subject.as_str()), (1, "final"));
    assert_eq!(registry.list(&a, "r2").await.unwrap().len(), 1);
    assert!(
        registry
            .list("someone-else", "r1")
            .await
            .unwrap()
            .is_empty()
    );
    assert!(registry.delete(&a, "r1", "ci").await.unwrap());
    assert!(!registry.delete(&a, "r1", "ci").await.unwrap());
    assert!(registry.list(&a, "r1").await.unwrap().is_empty());
    assert_eq!(registry.list(&a, "r2").await.unwrap().len(), 1);
}

#[tokio::test]
async fn the_number_of_publishers_per_repository_is_capped() {
    let a = account();
    let Some(v) = mount(&[&a]).await else { return };
    seed(&v.store, &a, "releases").await;
    let manager = format!("manager@{a}");
    let put = |id: String| {
        let (v, manager) = (&v, manager.clone());
        async move {
            send(
                &v.app,
                Method::PUT,
                &format!("/api/v1/trusted-publishers/releases/{id}"),
                Some(&manager),
                spec(&v.issuer),
            )
            .await
            .0
        }
    };
    for n in 0..16 {
        assert_eq!(put(format!("p{n}")).await, StatusCode::OK, "{n}");
    }
    assert_eq!(put("p16".into()).await, StatusCode::CONFLICT);
    // Replacing one that exists is not growth.
    assert_eq!(put("p3".into()).await, StatusCode::OK);
    let (_, body) = send(
        &v.app,
        Method::GET,
        "/api/v1/trusted-publishers/releases",
        Some(&manager),
        Vec::new(),
    )
    .await;
    let listed: Vec<Value> = serde_json::from_str(&body).unwrap();
    assert_eq!(listed.len(), 16);
    let ids: Vec<&str> = listed.iter().map(|r| r["id"].as_str().unwrap()).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted, "the listing order is deterministic");
}

/// DOCUMENTED EXCEPTION (HSP-321 tramo j): the publisher credential is its own
/// grant (the registry record, written under `ManageTrustedPublishers`), and
/// Cedar does not decide `UploadAsset` for a workload. This test pins that
/// behavior: a blanket `forbid` on `UploadAsset` stops users and does NOT stop
/// a live publisher credential. The kill switch for a workload is deleting its
/// record (immediate revocation), not a Cedar forbid.
#[tokio::test]
async fn a_cedar_forbid_on_upload_does_not_apply_to_the_publisher_credential() {
    let a = account();
    let forbid = r#"forbid(principal, action == Artifacts::Action::"UploadAsset", resource);"#;
    let Some(v) = mount_with(&[&a], forbid).await else {
        return;
    };
    seed(&v.store, &a, "releases").await;
    register(&v, &a, "releases", "ci-main").await;
    let credential = credential(&v, &a, "releases").await;
    assert_eq!(
        publish(&v, "releases", &format!("alice@{a}")).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        publish(&v, "releases", &credential).await.0,
        StatusCode::CREATED
    );
}
