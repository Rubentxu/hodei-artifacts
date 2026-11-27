//! HSP-322 tramo 2 (from HSP-161): the publication freeze of hosted npm and
//! raw repositories, through the real router.
//!
//! A repository that declares `config_settings.freeze {scope, reason, until?}`
//! refuses publication (409, generic reason) before a byte is written, with a
//! `freeze.refused` audit event. This suite drives the real router over the
//! real Dgraph catalog, a real file CAS and the real Cedar engine:
//!
//! - an active `scope = "all"` freeze refuses new versions, new packages and
//!   dist-tag changes (npm) and new/replaced paths (raw), leaves nothing in the
//!   catalog, never leaks the declared reason, and reads keep working;
//! - `scope = "existing"` still admits NEW names/paths;
//! - an expired freeze publishes (including one that expires while running);
//! - an unreadable declaration is 503 and publishes nothing (fail closed);
//! - a repository WITHOUT the declaration behaves as before, no freeze event;
//! - declaring a freeze needs the dedicated Cedar action: a principal that can
//!   publish and create repositories cannot declare one, and cannot lift one;
//! - the freeze is declared and released through
//!   `PUT|DELETE /api/v1/catalog/repositories/{name}/freeze` under that same
//!   action only (403 audited, uniform 404 across accounts, idempotent release,
//!   unreadable declaration is a 400 that changes nothing, persisted state);
//! - a frozen raw repository refuses `DELETE` of its assets too.
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
use base64::Engine as _;
use cedar_policy::Entities;
use chrono::Utc;
use hodei_artifacts_api::audit::{AuditResult, InMemoryAuditSink};
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

fn policy(account: &str) -> String {
    // alice publishes and creates repositories; only carol may ALSO declare a
    // freeze (`ManageRepositoryFreeze` is its own action).
    format!(
        r#"
permit(principal == Iam::User::"hrn:hodei:iam::{account}:User/alice",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset",
                  Artifacts::Action::"CreateRepository", Artifacts::Action::"DeleteAsset"],
       resource);
permit(principal == Iam::User::"hrn:hodei:iam::{account}:User/carol",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset",
                  Artifacts::Action::"CreateRepository", Artifacts::Action::"DeleteAsset",
                  Artifacts::Action::"ManageRepositoryFreeze"],
       resource);
"#
    )
}
struct Vertical {
    app: Router,
    store: Arc<DgraphCatalogStore>,
    audit: Arc<InMemoryAuditSink>,
    #[allow(dead_code)]
    tmp: TempDir,
}

async fn mount(account: &str) -> Option<Vertical> {
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
    let audit = Arc::new(InMemoryAuditSink::new());
    let ledger: Arc<dyn hodei_catalog::ContentLedger> = store.clone();
    let gc = Arc::new(
        hodei_artifacts_api::gc::GcService::new(
            ledger,
            Arc::new(FileBlobStore::new(tmp.path().join("cas")).expect("the CAS")),
            std::time::Duration::ZERO,
        )
        .expect("the GC service"),
    );
    let state = state
        .with_gc(gc)
        .with_catalog(reader, writer)
        .with_content_store(content)
        .with_authenticator(Arc::new(TokenAuthenticator))
        .with_authorization(cedar(&policy(account)))
        .with_audit_sink(audit.clone());
    Some(Vertical {
        app: build_router(state, &config),
        store,
        audit,
        tmp,
    })
}

async fn seed(
    store: &DgraphCatalogStore,
    account: &str,
    key: &str,
    format: RepositoryFormat,
    settings: Value,
) {
    let mut config = RepositoryConfig::new(DigestScope::Global);
    config.config_settings = settings;
    store
        .create_repository(
            account,
            RepositoryDescriptor {
                hrn: repository_hrn("hodei", "artifacts", "eu", account, key),
                account: account.to_string(),
                repo_key: key.to_string(),
                format,
                mode: RepositoryMode::Hosted,
                config,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        )
        .await
        .expect("the repository is seeded");
}

/// Declares (or rewrites) the settings of an existing repository the way an
/// operator with store access would: there is no API for it.
async fn reconfigure(v: &Vertical, account: &str, key: &str, settings: Value) {
    let mut config = RepositoryConfig::new(DigestScope::Global);
    config.config_settings = settings;
    v.store
        .update_config(account, key, config)
        .await
        .expect("the repository is reconfigured");
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

fn publication(name: &str, version: &str) -> Vec<u8> {
    let tarball = format!("tarball of {name}@{version}");
    let file = format!("{name}-{version}.tgz");
    serde_json::to_vec(&json!({
        "_id": name,
        "name": name,
        "dist-tags": { "latest": version },
        "versions": { version: { "name": name, "version": version, "_id": format!("{name}@{version}") } },
        "_attachments": { file: {
            "content_type": "application/octet-stream",
            "data": base64::engine::general_purpose::STANDARD.encode(tarball.as_bytes()),
            "length": tarball.len(),
        } },
    }))
    .unwrap()
}

async fn npm_publish(
    v: &Vertical,
    repo: &str,
    token: &str,
    name: &str,
    version: &str,
) -> (StatusCode, String) {
    let (status, body) = send(
        &v.app,
        Method::PUT,
        &format!("/npm/{repo}/{name}"),
        token,
        publication(name, version),
    )
    .await;
    (status, String::from_utf8_lossy(&body).into_owned())
}

async fn npm_tag(
    v: &Vertical,
    repo: &str,
    token: &str,
    name: &str,
    tag: &str,
    version: &str,
) -> StatusCode {
    send(
        &v.app,
        Method::PUT,
        &format!("/npm/{repo}/-/package/{name}/dist-tags/{tag}"),
        token,
        serde_json::to_vec(&version).unwrap(),
    )
    .await
    .0
}

async fn raw_put(
    v: &Vertical,
    repo: &str,
    token: &str,
    path: &str,
    body: &str,
) -> (StatusCode, String) {
    let (status, body) = send(
        &v.app,
        Method::PUT,
        &format!("/repository/{repo}/{path}"),
        token,
        body.as_bytes().to_vec(),
    )
    .await;
    (status, String::from_utf8_lossy(&body).into_owned())
}

async fn raw_get(v: &Vertical, repo: &str, token: &str, path: &str) -> (StatusCode, String) {
    let (status, body) = send(
        &v.app,
        Method::GET,
        &format!("/repository/{repo}/{path}"),
        token,
        Vec::new(),
    )
    .await;
    (status, String::from_utf8_lossy(&body).into_owned())
}

/// The versions the packument of `name` lists (empty if there is none).
async fn npm_versions(v: &Vertical, repo: &str, token: &str, name: &str) -> Vec<String> {
    let (status, body) = send(
        &v.app,
        Method::GET,
        &format!("/npm/{repo}/{name}"),
        token,
        Vec::new(),
    )
    .await;
    if status != StatusCode::OK {
        return Vec::new();
    }
    let document: Value = serde_json::from_slice(&body).unwrap();
    let mut versions: Vec<String> = document["versions"]
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default();
    versions.sort();
    versions
}

fn events(v: &Vertical, action: &str) -> Vec<(AuditResult, String)> {
    v.audit
        .events()
        .into_iter()
        .filter(|event| event.action == action)
        .map(|event| (event.result, event.resource))
        .collect()
}

fn any_freeze_event(v: &Vertical) -> bool {
    v.audit
        .events()
        .iter()
        .any(|event| event.action.starts_with("freeze."))
}

const GENERIC: &str = "this repository does not accept publications at this time";
const REASON: &str = "release-4-2-audit-window";

fn frozen(scope: &str) -> Value {
    json!({"freeze": {"scope": scope, "reason": REASON}})
}

fn assert_generic(body: &str) {
    assert!(body.contains(GENERIC), "{body}");
    for leak in [REASON, "freeze", "frozen", "scope"] {
        assert!(!body.to_lowercase().contains(leak), "leaks {leak}: {body}");
    }
}

#[tokio::test]
async fn an_active_freeze_refuses_every_npm_write_and_leaves_nothing() {
    let account = format!("h{}a", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    seed(
        &v.store,
        &account,
        "npm-fz",
        RepositoryFormat::Npm,
        json!({}),
    )
    .await;
    let alice = format!("alice@{account}");
    assert_eq!(
        npm_publish(&v, "npm-fz", &alice, "left-pad", "1.0.0")
            .await
            .0,
        StatusCode::CREATED
    );
    assert!(
        !any_freeze_event(&v),
        "an unfrozen repository emits nothing"
    );

    reconfigure(&v, &account, "npm-fz", frozen("all")).await;
    // A new version, a new package and a dist-tag move are all refused.
    for (name, version) in [("left-pad", "1.1.0"), ("brand-new", "1.0.0")] {
        let (status, body) = npm_publish(&v, "npm-fz", &alice, name, version).await;
        assert_eq!(status, StatusCode::CONFLICT, "{name}: {body}");
        assert_generic(&body);
    }
    assert_eq!(
        npm_tag(&v, "npm-fz", &alice, "left-pad", "beta", "1.0.0").await,
        StatusCode::CONFLICT
    );
    // Nothing was published: the frozen set is exactly what it was.
    assert_eq!(
        npm_versions(&v, "npm-fz", &alice, "left-pad").await,
        vec!["1.0.0".to_string()]
    );
    assert!(
        npm_versions(&v, "npm-fz", &alice, "brand-new")
            .await
            .is_empty()
    );
    // Reads keep serving the fixed set.
    assert_eq!(
        send(
            &v.app,
            Method::GET,
            "/npm/npm-fz/left-pad",
            &alice,
            Vec::new()
        )
        .await
        .0,
        StatusCode::OK
    );
    let refused = events(&v, "freeze.refused");
    assert_eq!(refused.len(), 3, "{refused:?}");
    assert!(
        refused
            .iter()
            .all(|(result, _)| *result == AuditResult::Denied)
    );
    assert!(
        refused
            .iter()
            .any(|(_, resource)| resource == "npm-fz/brand-new")
    );
}

#[tokio::test]
async fn scope_existing_admits_new_names_and_refuses_changes_to_published_ones() {
    let account = format!("h{}b", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    seed(
        &v.store,
        &account,
        "npm-ex",
        RepositoryFormat::Npm,
        json!({}),
    )
    .await;
    seed(
        &v.store,
        &account,
        "raw-ex",
        RepositoryFormat::Raw,
        json!({}),
    )
    .await;
    let alice = format!("alice@{account}");
    assert_eq!(
        npm_publish(&v, "npm-ex", &alice, "left-pad", "1.0.0")
            .await
            .0,
        StatusCode::CREATED
    );
    assert_eq!(
        raw_put(&v, "raw-ex", &alice, "a.txt", "one").await.0,
        StatusCode::CREATED
    );
    reconfigure(&v, &account, "npm-ex", frozen("existing")).await;
    reconfigure(&v, &account, "raw-ex", frozen("existing")).await;

    let (status, body) = npm_publish(&v, "npm-ex", &alice, "left-pad", "1.1.0").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_generic(&body);
    assert_eq!(
        npm_publish(&v, "npm-ex", &alice, "another", "1.0.0")
            .await
            .0,
        StatusCode::CREATED
    );

    let (status, body) = raw_put(&v, "raw-ex", &alice, "a.txt", "two").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_generic(&body);
    assert_eq!(
        raw_get(&v, "raw-ex", &alice, "a.txt").await,
        (StatusCode::OK, "one".to_string())
    );
    assert_eq!(
        raw_put(&v, "raw-ex", &alice, "b.txt", "new").await.0,
        StatusCode::CREATED
    );
}

#[tokio::test]
async fn an_active_freeze_refuses_raw_publication_and_leaves_nothing() {
    let account = format!("h{}c", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    seed(
        &v.store,
        &account,
        "raw-fz",
        RepositoryFormat::Raw,
        json!({}),
    )
    .await;
    let alice = format!("alice@{account}");
    assert_eq!(
        raw_put(&v, "raw-fz", &alice, "a.txt", "one").await.0,
        StatusCode::CREATED
    );
    reconfigure(&v, &account, "raw-fz", frozen("all")).await;
    for (path, body) in [("a.txt", "two"), ("new.txt", "fresh")] {
        let (status, text) = raw_put(&v, "raw-fz", &alice, path, body).await;
        assert_eq!(status, StatusCode::CONFLICT, "{path}: {text}");
        assert_generic(&text);
    }
    assert_eq!(
        raw_get(&v, "raw-fz", &alice, "a.txt").await,
        (StatusCode::OK, "one".to_string())
    );
    assert_eq!(
        raw_get(&v, "raw-fz", &alice, "new.txt").await.0,
        StatusCode::NOT_FOUND
    );
    let refused = events(&v, "freeze.refused");
    assert_eq!(refused.len(), 2, "{refused:?}");
    assert!(
        refused
            .iter()
            .all(|(result, _)| *result == AuditResult::Denied)
    );
}

#[tokio::test]
async fn an_expired_freeze_publishes_and_one_that_expires_lets_go() {
    let account = format!("h{}d", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    let alice = format!("alice@{account}");
    let at = |seconds: i64| (Utc::now() + chrono::Duration::seconds(seconds)).to_rfc3339();

    seed(
        &v.store,
        &account,
        "npm-past",
        RepositoryFormat::Npm,
        json!({"freeze": {"scope": "all", "reason": REASON, "until": at(-3600)}}),
    )
    .await;
    assert_eq!(
        npm_publish(&v, "npm-past", &alice, "left-pad", "1.0.0")
            .await
            .0,
        StatusCode::CREATED
    );
    assert!(
        !any_freeze_event(&v),
        "an expired freeze blocks nothing and says nothing"
    );

    seed(
        &v.store,
        &account,
        "npm-soon",
        RepositoryFormat::Npm,
        json!({"freeze": {"scope": "all", "reason": REASON, "until": at(4)}}),
    )
    .await;
    assert_eq!(
        npm_publish(&v, "npm-soon", &alice, "left-pad", "1.0.0")
            .await
            .0,
        StatusCode::CONFLICT
    );
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    assert_eq!(
        npm_publish(&v, "npm-soon", &alice, "left-pad", "1.0.0")
            .await
            .0,
        StatusCode::CREATED
    );

    seed(
        &v.store,
        &account,
        "raw-future",
        RepositoryFormat::Raw,
        json!({"freeze": {"scope": "all", "reason": REASON, "until": at(3600)}}),
    )
    .await;
    assert_eq!(
        raw_put(&v, "raw-future", &alice, "a.txt", "x").await.0,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn an_unreadable_declaration_fails_closed_and_publishes_nothing() {
    let account = format!("h{}e", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    let alice = format!("alice@{account}");
    // Seeded straight into the catalog: the creation API would refuse these.
    for (repo, settings) in [
        ("bad-shape", json!({"freeze": "yes"})),
        (
            "bad-scope",
            json!({"freeze": {"scope": "none", "reason": "x"}}),
        ),
        (
            "bad-until",
            json!({"freeze": {"scope": "all", "reason": "x", "until": "soon"}}),
        ),
        ("no-reason", json!({"freeze": {"scope": "all"}})),
    ] {
        seed(
            &v.store,
            &account,
            &format!("n-{repo}"),
            RepositoryFormat::Npm,
            settings.clone(),
        )
        .await;
        let (status, body) =
            npm_publish(&v, &format!("n-{repo}"), &alice, "left-pad", "1.0.0").await;
        assert_eq!(
            status,
            StatusCode::SERVICE_UNAVAILABLE,
            "npm {repo}: {body}"
        );
        assert!(
            npm_versions(&v, &format!("n-{repo}"), &alice, "left-pad")
                .await
                .is_empty()
        );
        seed(
            &v.store,
            &account,
            &format!("r-{repo}"),
            RepositoryFormat::Raw,
            settings,
        )
        .await;
        let (status, body) = raw_put(&v, &format!("r-{repo}"), &alice, "a.txt", "x").await;
        assert_eq!(
            status,
            StatusCode::SERVICE_UNAVAILABLE,
            "raw {repo}: {body}"
        );
        assert_eq!(
            raw_get(&v, &format!("r-{repo}"), &alice, "a.txt").await.0,
            StatusCode::NOT_FOUND
        );
    }
    let unavailable = events(&v, "freeze.unavailable");
    assert_eq!(unavailable.len(), 8);
    assert!(
        unavailable
            .iter()
            .all(|(result, _)| *result == AuditResult::Failed)
    );
}

#[tokio::test]
async fn a_repository_without_the_declaration_is_untouched() {
    let account = format!("h{}f", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    let alice = format!("alice@{account}");
    seed(
        &v.store,
        &account,
        "npm-plain",
        RepositoryFormat::Npm,
        json!({}),
    )
    .await;
    seed(
        &v.store,
        &account,
        "raw-plain",
        RepositoryFormat::Raw,
        json!({}),
    )
    .await;
    for version in ["1.0.0", "1.1.0"] {
        assert_eq!(
            npm_publish(&v, "npm-plain", &alice, "left-pad", version)
                .await
                .0,
            StatusCode::CREATED
        );
    }
    assert_eq!(
        npm_tag(&v, "npm-plain", &alice, "left-pad", "beta", "1.0.0").await,
        StatusCode::OK
    );
    assert_eq!(
        raw_put(&v, "raw-plain", &alice, "a.txt", "x").await.0,
        StatusCode::CREATED
    );
    assert!(!any_freeze_event(&v), "no declaration, no gate, no event");
}

#[tokio::test]
async fn declaring_a_freeze_needs_its_own_cedar_action_and_nobody_publishing_can_lift_it() {
    let account = format!("h{}g", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    let alice = format!("alice@{account}");
    let carol = format!("carol@{account}");
    let create = |token: String, name: String, format: &'static str, settings: Value| {
        let app = v.app.clone();
        let body = json!({"name": name, "format": format, "mode": "hosted", "settings": settings});
        async move {
            send(
                &app,
                Method::POST,
                "/api/v1/catalog/repositories",
                &token,
                serde_json::to_vec(&body).unwrap(),
            )
            .await
            .0
        }
    };
    let good = frozen("all");

    // CreateRepository + UploadAsset are not enough: 403 and nothing is created.
    assert_eq!(
        create(alice.clone(), "fz-no".into(), "npm", good.clone()).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        npm_publish(&v, "fz-no", &alice, "left-pad", "1.0.0")
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    // The dedicated action is.
    assert_eq!(
        create(carol.clone(), "fz-npm".into(), "npm", good.clone()).await,
        StatusCode::CREATED
    );
    assert_eq!(
        create(carol.clone(), "fz-raw".into(), "raw", good.clone()).await,
        StatusCode::CREATED
    );
    // A repository without the declaration stays within plain CreateRepository.
    assert_eq!(
        create(alice.clone(), "plain-ok".into(), "npm", json!({})).await,
        StatusCode::CREATED
    );

    // The freeze is active from the first request, for everybody who publishes.
    for token in [&alice, &carol] {
        assert_eq!(
            npm_publish(&v, "fz-npm", token, "left-pad", "1.0.0")
                .await
                .0,
            StatusCode::CONFLICT
        );
        assert_eq!(
            raw_put(&v, "fz-raw", token, "a.txt", "x").await.0,
            StatusCode::CONFLICT
        );
    }

    // Alice cannot lift it: re-creating the repository does not replace the
    // declaration, and there is no update route.
    assert_ne!(
        create(alice.clone(), "fz-npm".into(), "npm", json!({})).await,
        StatusCode::CREATED
    );
    for method in [Method::PUT, Method::PATCH, Method::DELETE, Method::POST] {
        let (status, _) = send(
            &v.app,
            method.clone(),
            "/api/v1/catalog/repositories/fz-npm",
            &alice,
            serde_json::to_vec(&json!({"settings": {}})).unwrap(),
        )
        .await;
        assert!(
            !status.is_success(),
            "{method} lifted the freeze ({status})"
        );
    }
    assert_eq!(
        npm_publish(&v, "fz-npm", &alice, "left-pad", "1.0.0")
            .await
            .0,
        StatusCode::CONFLICT
    );

    // Only readable declarations on the surfaces that honour them.
    assert_eq!(
        create(carol.clone(), "pypi-no".into(), "pypi", good.clone()).await,
        StatusCode::BAD_REQUEST
    );
    for bad in [
        json!({"freeze": {"scope": "maybe", "reason": "x"}}),
        json!({"freeze": {"scope": "all"}}),
        json!({"freeze": {"scope": "all", "reason": "x", "until": "later"}}),
        json!({"freeze": 1}),
    ] {
        assert_eq!(
            create(carol.clone(), "bad".into(), "npm", bad.clone()).await,
            StatusCode::BAD_REQUEST,
            "{bad}"
        );
    }
}

async fn raw_delete(v: &Vertical, repo: &str, token: &str, path: &str) -> (StatusCode, String) {
    let (status, body) = send(
        &v.app,
        Method::DELETE,
        &format!("/repository/{repo}/{path}"),
        token,
        Vec::new(),
    )
    .await;
    (status, String::from_utf8_lossy(&body).into_owned())
}

async fn freeze_call(
    v: &Vertical,
    method: Method,
    repo: &str,
    token: &str,
    body: &Value,
) -> (StatusCode, Value) {
    let (status, bytes) = send(
        &v.app,
        method,
        &format!("/api/v1/catalog/repositories/{repo}/freeze"),
        token,
        serde_json::to_vec(body).unwrap(),
    )
    .await;
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn a_frozen_raw_repository_refuses_deleting_its_assets() {
    let account = format!("h{}h", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    let alice = format!("alice@{account}");
    for scope in ["all", "existing"] {
        let repo = format!("raw-del-{scope}");
        seed(&v.store, &account, &repo, RepositoryFormat::Raw, json!({})).await;
        assert_eq!(
            raw_put(&v, &repo, &alice, "a.txt", "one").await.0,
            StatusCode::CREATED
        );
        reconfigure(&v, &account, &repo, frozen(scope)).await;
        let (status, body) = raw_delete(&v, &repo, &alice, "a.txt").await;
        assert_eq!(status, StatusCode::CONFLICT, "{scope}: {body}");
        assert_generic(&body);
        assert_eq!(
            raw_get(&v, &repo, &alice, "a.txt").await,
            (StatusCode::OK, "one".to_string()),
            "{scope}: the frozen asset is still served"
        );
    }
    let refused = events(&v, "freeze.refused");
    assert_eq!(refused.len(), 2, "{refused:?}");
    // Released, the same repository deletes again.
    reconfigure(&v, &account, "raw-del-all", json!({})).await;
    let (status, body) = raw_delete(&v, "raw-del-all", &alice, "a.txt").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        raw_get(&v, "raw-del-all", &alice, "a.txt").await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn the_freeze_is_declared_and_released_through_its_own_action() {
    let account = format!("h{}i", &nonce()[8..]);
    let other = format!("o{}i", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    let alice = format!("alice@{account}");
    let carol = format!("carol@{account}");
    let mallory = format!("carol@{other}");
    seed(
        &v.store,
        &account,
        "npm-lc",
        RepositoryFormat::Npm,
        json!({"npm": {}}),
    )
    .await;
    seed(
        &v.store,
        &account,
        "raw-lc",
        RepositoryFormat::Raw,
        json!({}),
    )
    .await;
    assert_eq!(
        npm_publish(&v, "npm-lc", &alice, "left-pad", "1.0.0")
            .await
            .0,
        StatusCode::CREATED
    );
    let decl = json!({"scope": "all", "reason": REASON});

    // alice (can publish, delete and create) cannot freeze nor release: 403.
    for method in [Method::PUT, Method::DELETE] {
        let (status, _) = freeze_call(&v, method.clone(), "npm-lc", &alice, &decl).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method}");
    }
    assert_eq!(
        npm_publish(&v, "npm-lc", &alice, "left-pad", "1.1.0")
            .await
            .0,
        StatusCode::CREATED,
        "a denied request froze nothing"
    );
    let denied: Vec<AuditResult> = v
        .audit
        .events()
        .into_iter()
        .filter(|event| event.action.contains("ManageRepositoryFreeze"))
        .map(|event| event.result)
        .collect();
    assert!(
        denied
            .iter()
            .filter(|result| **result == AuditResult::Denied)
            .count()
            >= 2,
        "{denied:?}"
    );

    // Another account: the repository does not exist for it, same 404 as a
    // repository that never existed.
    let (existing, _) = freeze_call(&v, Method::PUT, "npm-lc", &mallory, &decl).await;
    let (missing, _) = freeze_call(&v, Method::PUT, "nope", &mallory, &decl).await;
    assert_eq!(
        (existing, missing),
        (StatusCode::NOT_FOUND, StatusCode::NOT_FOUND)
    );
    let (existing, _) = freeze_call(&v, Method::DELETE, "npm-lc", &mallory, &decl).await;
    assert_eq!(existing, StatusCode::NOT_FOUND);
    assert_eq!(
        npm_publish(&v, "npm-lc", &alice, "left-pad", "1.2.0")
            .await
            .0,
        StatusCode::CREATED,
        "another account froze nothing"
    );

    // An unreadable declaration is a 400 and changes nothing.
    for bad in [
        json!({"scope": "maybe", "reason": "x"}),
        json!({"scope": "all"}),
        json!({"scope": "all", "reason": "x", "until": "later"}),
        json!("all"),
        json!({"scope": "all", "reason": "x", "extra": 1}),
    ] {
        let (status, _) = freeze_call(&v, Method::PUT, "npm-lc", &carol, &bad).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
    assert_eq!(
        npm_publish(&v, "npm-lc", &alice, "left-pad", "1.3.0")
            .await
            .0,
        StatusCode::CREATED
    );
    // Releasing what is not frozen is a no-op that says so.
    let (status, view) = freeze_call(&v, Method::DELETE, "npm-lc", &carol, &json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (view["frozen"].clone(), view["changed"].clone()),
        (json!(false), json!(false))
    );

    // carol freezes: publication stops at once, other settings survive.
    let (status, view) = freeze_call(&v, Method::PUT, "npm-lc", &carol, &decl).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["frozen"], json!(true));
    let (status, body) = npm_publish(&v, "npm-lc", &alice, "left-pad", "1.4.0").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_generic(&body);
    let stored = v.store.repository(&account, "npm-lc").await.unwrap();
    assert_eq!(
        stored.config.config_settings["freeze"]["scope"],
        json!("all")
    );
    assert_eq!(
        stored.config.config_settings["npm"],
        json!({}),
        "other settings kept"
    );

    // Replacing the declaration takes effect (existing admits new names).
    let (status, _) = freeze_call(
        &v,
        Method::PUT,
        "npm-lc",
        &carol,
        &json!({"scope": "existing", "reason": REASON}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        npm_publish(&v, "npm-lc", &alice, "fresh-name", "1.0.0")
            .await
            .0,
        StatusCode::CREATED
    );
    assert_eq!(
        npm_publish(&v, "npm-lc", &alice, "left-pad", "1.5.0")
            .await
            .0,
        StatusCode::CONFLICT
    );

    // alice cannot release it; carol can, and it is idempotent.
    let (status, _) = freeze_call(&v, Method::DELETE, "npm-lc", &alice, &json!({})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        npm_publish(&v, "npm-lc", &alice, "left-pad", "1.5.0")
            .await
            .0,
        StatusCode::CONFLICT
    );
    let (status, view) = freeze_call(&v, Method::DELETE, "npm-lc", &carol, &json!({})).await;
    assert_eq!(
        (status, view["changed"].clone()),
        (StatusCode::OK, json!(true))
    );
    let (status, view) = freeze_call(&v, Method::DELETE, "npm-lc", &carol, &json!({})).await;
    assert_eq!(
        (status, view["changed"].clone()),
        (StatusCode::OK, json!(false))
    );
    assert_eq!(
        npm_publish(&v, "npm-lc", &alice, "left-pad", "1.5.0")
            .await
            .0,
        StatusCode::CREATED,
        "released: publication is back"
    );
    let stored = v.store.repository(&account, "npm-lc").await.unwrap();
    assert!(stored.config.config_settings.get("freeze").is_none());

    // The same on raw, with deletion; and the lifecycle is audited.
    assert_eq!(
        raw_put(&v, "raw-lc", &alice, "a.txt", "one").await.0,
        StatusCode::CREATED
    );
    assert_eq!(
        freeze_call(&v, Method::PUT, "raw-lc", &carol, &decl)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        raw_put(&v, "raw-lc", &alice, "a.txt", "two").await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        raw_delete(&v, "raw-lc", &alice, "a.txt").await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        freeze_call(&v, Method::DELETE, "raw-lc", &carol, &json!({}))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        raw_delete(&v, "raw-lc", &alice, "a.txt").await.0,
        StatusCode::OK
    );
    let trail: Vec<String> = v
        .audit
        .events()
        .into_iter()
        .filter(|e| e.resource.contains("freeze=") && e.result == AuditResult::Allowed)
        .map(|e| e.resource)
        .collect();
    assert!(trail.iter().any(|r| r.ends_with("freeze=set")), "{trail:?}");
    assert!(
        trail.iter().any(|r| r.ends_with("freeze=released")),
        "{trail:?}"
    );

    // Only hosted raw/npm: a pypi repository answers 400 and is not changed.
    seed(
        &v.store,
        &account,
        "py-lc",
        RepositoryFormat::PyPi,
        json!({}),
    )
    .await;
    assert_eq!(
        freeze_call(&v, Method::PUT, "py-lc", &carol, &decl).await.0,
        StatusCode::BAD_REQUEST
    );
    // No credential, no answer.
    let (status, _) = freeze_call(&v, Method::PUT, "npm-lc", "x", &decl).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
