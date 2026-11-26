//! HSP-321 tramo h: ABAC by resource tags, wired into the real decision points.
//!
//! The trusted namespace and tags of a repository (`config_settings.tags`)
//! travel with every repository decision (`authorize_repository`), so a Cedar
//! policy can read `resource.tags`. This suite drives the real router (raw
//! publish and download) over the real Dgraph catalog with a real Cedar engine:
//!
//! - a principal whose policy asks for the tag is admitted (publish + download);
//! - the same principal on a repository without the tag is denied (403);
//! - an account that has a same-named repository without the tag does not
//!   inherit the tag of the other account's repository;
//! - an undecidable outcome (a policy that errors, a malformed stored tag)
//!   is a denial, never a permit;
//! - declaring tags needs `ManageRepositoryTags`; `CreateRepository` alone
//!   (the only repository-writing grant that exists) neither writes nor
//!   escalates through tags.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT`; serialize with `--test-threads=1`.

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
    AllowAllAuthorizer, CatalogError, CatalogReader, CatalogWriter, DigestScope, RepositoryConfig,
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

/// The bundle of the suite, for the accounts `a` and `b`.
fn policy(a: &str, b: &str) -> String {
    format!(
        r#"
// alice reads and publishes only what carries the `prod` tag.
permit(principal == Iam::User::"hrn:hodei:iam::{a}:User/alice",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset"],
       resource) when {{ resource.tags.contains("prod") }};
// mallory (another account) has the very same ABAC grant.
permit(principal == Iam::User::"hrn:hodei:iam::{b}:User/mallory",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset"],
       resource) when {{ resource.tags.contains("prod") }};
// erin's policy cannot be evaluated: the resource has no `owner` attribute.
permit(principal == Iam::User::"hrn:hodei:iam::{a}:User/erin",
       action == Artifacts::Action::"DownloadAsset",
       resource) when {{ resource.owner == "erin" }};
// frank is unconditionally permitted: only a broken scope can stop him.
permit(principal == Iam::User::"hrn:hodei:iam::{a}:User/frank",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset"],
       resource);
// creator may create repositories and nothing else; it holds NO tag grant.
permit(principal == Iam::User::"hrn:hodei:iam::{a}:User/creator",
       action == Artifacts::Action::"CreateRepository", resource);
// manager may create repositories AND manage their tags.
permit(principal == Iam::User::"hrn:hodei:iam::{a}:User/manager",
       action in [Artifacts::Action::"CreateRepository", Artifacts::Action::"ManageRepositoryTags"],
       resource);
"#
    )
}

struct Vertical {
    app: Router,
    store: Arc<DgraphCatalogStore>,
    #[allow(dead_code)]
    tmp: TempDir,
}

async fn mount(account_a: &str, account_b: &str) -> Option<Vertical> {
    let store = open_store().await?;
    let tmp = tempfile::tempdir().expect("temp dir");
    let mut config = AppConfig::default();
    config.security.raw_api_enabled = true;
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
    let content: Arc<dyn hodei_hub::blob::BlobStore> =
        Arc::new(FileBlobStore::new(tmp.path().join("cas")).expect("the CAS"));
    let state = state
        .with_catalog(reader, writer)
        .with_content_store(content)
        .with_authenticator(Arc::new(TokenAuthenticator))
        .with_authorization(cedar(&policy(account_a, account_b)));
    Some(Vertical {
        app: build_router(state, &config),
        store,
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

async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    token: &str,
    body: Vec<u8>,
) -> (StatusCode, Vec<u8>) {
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
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, bytes.to_vec())
}

async fn put(app: &Router, repo: &str, token: &str) -> StatusCode {
    send(
        app,
        Method::PUT,
        &format!("/repository/{repo}/file.txt"),
        token,
        b"payload".to_vec(),
    )
    .await
    .0
}

async fn get(app: &Router, repo: &str, token: &str) -> (StatusCode, Vec<u8>) {
    send(
        app,
        Method::GET,
        &format!("/repository/{repo}/file.txt"),
        token,
        Vec::new(),
    )
    .await
}

async fn create(app: &Router, token: &str, name: &str, settings: Option<Value>) -> StatusCode {
    let mut body = json!({"name": name, "format": "raw", "mode": "hosted"});
    if let Some(settings) = settings {
        body["settings"] = settings;
    }
    send(
        app,
        Method::POST,
        "/api/v1/catalog/repositories",
        token,
        serde_json::to_vec(&body).unwrap(),
    )
    .await
    .0
}

#[tokio::test]
async fn a_tag_in_the_policy_admits_publish_and_download_and_nothing_else() {
    let (a, b) = (
        format!("h{}a", &nonce()[8..]),
        format!("h{}b", &nonce()[8..]),
    );
    let Some(v) = mount(&a, &b).await else { return };
    seed(&v.store, &a, "tagged", json!({"tags": ["prod", "eu"]})).await;
    seed(&v.store, &a, "plain", json!({})).await;
    seed(&v.store, &b, "tagged", json!({})).await; // same key, no tag
    let alice = format!("alice@{a}");

    // Allowed tag: publish and download succeed end to end.
    assert!(put(&v.app, "tagged", &alice).await.is_success());
    let (status, body) = get(&v.app, "tagged", &alice).await;
    assert_eq!(
        (status, body.as_slice()),
        (StatusCode::OK, b"payload".as_slice())
    );

    // Same principal, repository without the tag: denied on both paths.
    assert_eq!(put(&v.app, "plain", &alice).await, StatusCode::FORBIDDEN);
    assert_eq!(get(&v.app, "plain", &alice).await.0, StatusCode::FORBIDDEN);

    // Another account with the identical grant does not inherit the tag of
    // the same-named repository of the first account.
    let mallory = format!("mallory@{b}");
    assert_eq!(put(&v.app, "tagged", &mallory).await, StatusCode::FORBIDDEN);
    assert_eq!(
        get(&v.app, "tagged", &mallory).await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn an_undecidable_outcome_is_a_denial_never_a_permit() {
    let (a, b) = (
        format!("h{}a", &nonce()[8..]),
        format!("h{}b", &nonce()[8..]),
    );
    let Some(v) = mount(&a, &b).await else { return };
    seed(&v.store, &a, "tagged", json!({"tags": ["prod"]})).await;
    // A malformed stored tag: the scope cannot be built.
    seed(&v.store, &a, "broken", json!({"tags": ["Not A Tag!"]})).await;
    seed(&v.store, &a, "notarray", json!({"tags": "prod"})).await;

    // A policy that errors while evaluating (Indeterminate) denies.
    let erin = format!("erin@{a}");
    let status = get(&v.app, "tagged", &erin).await.0;
    assert!(
        !status.is_success(),
        "indeterminate must not permit: {status}"
    );

    // frank is unconditionally permitted by policy: a scope that cannot be
    // built still blocks him (fail closed), on both paths.
    let frank = format!("frank@{a}");
    for repo in ["broken", "notarray"] {
        assert!(!put(&v.app, repo, &frank).await.is_success(), "{repo}");
        assert!(!get(&v.app, repo, &frank).await.0.is_success(), "{repo}");
    }
    // Control: the same frank on a well formed repository is admitted.
    assert!(put(&v.app, "tagged", &frank).await.is_success());
}

#[tokio::test]
async fn declaring_tags_needs_manage_repository_tags_and_create_alone_never_escalates() {
    let (a, b) = (
        format!("h{}a", &nonce()[8..]),
        format!("h{}b", &nonce()[8..]),
    );
    let Some(v) = mount(&a, &b).await else { return };
    let (creator, manager, alice) = (
        format!("creator@{a}"),
        format!("manager@{a}"),
        format!("alice@{a}"),
    );

    // CreateRepository without tags keeps working (no regression).
    assert_eq!(
        create(&v.app, &creator, "no-tags", Some(json!({}))).await,
        StatusCode::CREATED
    );

    // The creator tries to give itself the tag alice's policy needs: refused,
    // and the repository does not exist afterwards.
    let denied = create(&v.app, &creator, "sneaky", Some(json!({"tags": ["prod"]}))).await;
    assert_eq!(denied, StatusCode::FORBIDDEN);
    let absent = v.store.repository(&a, "sneaky").await;
    assert!(
        matches!(absent, Err(CatalogError::Missing { .. })),
        "{absent:?}"
    );
    assert_eq!(put(&v.app, "sneaky", &alice).await, StatusCode::NOT_FOUND);

    // Even an empty tag declaration is a tag write and needs the action.
    assert_eq!(
        create(&v.app, &creator, "empty-tags", Some(json!({"tags": []}))).await,
        StatusCode::FORBIDDEN
    );

    // The manager holds the explicit grant: the repository is created with
    // its tags and alice is admitted end to end by them.
    assert_eq!(
        create(&v.app, &manager, "blessed", Some(json!({"tags": ["prod"]}))).await,
        StatusCode::CREATED
    );
    assert!(put(&v.app, "blessed", &alice).await.is_success());
    assert_eq!(get(&v.app, "blessed", &alice).await.0, StatusCode::OK);

    // An invalid tag is a bad request, not a stored scope.
    assert_eq!(
        create(&v.app, &manager, "bad", Some(json!({"tags": ["NOT OK"]}))).await,
        StatusCode::BAD_REQUEST
    );
}
