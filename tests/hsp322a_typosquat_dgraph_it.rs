//! HSP-322 tramo 1 (from HSP-166): the publish-time look-alike (typosquat)
//! gate of hosted npm repositories, through the real router.
//!
//! A hosted npm repository that declares `config_settings.typosquat` has every
//! NEW package name judged against the names the repository already holds and
//! the administrator's `protected` list, **before** a byte is written. This
//! suite drives the real router over the real Dgraph catalog, a real file CAS
//! and the real Cedar engine:
//!
//! - a legitimate new name is published;
//! - a look-alike (transposition, digit homoglyph, separator-only, one edit)
//!   of a protected or an already published name is refused (403, generic
//!   reason that never names the matched package) and NOTHING is published
//!   (no asset in the catalog, no packument), with a `typosquat.refused`
//!   audit event;
//! - a new version of an existing package is not judged;
//! - `mode = "audit"` publishes and leaves `typosquat.flagged`;
//! - an unreadable declaration and a registry listing that fails are both
//!   503 and publish nothing (fail closed);
//! - a repository WITHOUT the declaration behaves exactly as before and emits
//!   no typosquat event;
//! - declaring the policy is only accepted on hosted npm repositories and only
//!   if it can be read.
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
    AllowAllAuthorizer, AssetRecord, BlobReference, CatalogError, CatalogReader, CatalogWriter,
    DigestScope, MutableTag, PackageCoordinates, PackageVersion, ReferrerRecord, RepositoryConfig,
    RepositoryDescriptor, RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_catalog_storage::DgraphCatalogStore;
use hodei_evidence::ContentDigest;
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
    format!(
        r#"
permit(principal == Iam::User::"hrn:hodei:iam::{account}:User/alice",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset",
                  Artifacts::Action::"CreateRepository"],
       resource);
"#
    )
}

/// A catalog reader whose registry listing is broken: the detector cannot know
/// the names that exist.
struct BrokenListing(Arc<DgraphCatalogStore>);

#[async_trait]
impl CatalogReader for BrokenListing {
    async fn repository(&self, a: &str, r: &str) -> Result<RepositoryDescriptor, CatalogError> {
        self.0.repository(a, r).await
    }
    async fn assets_under(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> Result<Vec<AssetRecord>, CatalogError> {
        Err(CatalogError::Unavailable {
            detail: "the listing is down".to_string(),
        })
    }
    async fn asset(&self, a: &str, r: &str, p: &str) -> Result<AssetRecord, CatalogError> {
        self.0.asset(a, r, p).await
    }
    async fn version(
        &self,
        a: &str,
        r: &str,
        c: &PackageCoordinates,
    ) -> Result<PackageVersion, CatalogError> {
        self.0.version(a, r, c).await
    }
    async fn tag(&self, a: &str, r: &str, t: &str) -> Result<MutableTag, CatalogError> {
        self.0.tag(a, r, t).await
    }
    async fn reference(&self, a: &str, r: &str, p: &str) -> Result<BlobReference, CatalogError> {
        self.0.reference(a, r, p).await
    }
    async fn tags(&self, a: &str, r: &str) -> Result<Vec<MutableTag>, CatalogError> {
        self.0.tags(a, r).await
    }
    async fn referrers(
        &self,
        a: &str,
        r: &str,
        s: &ContentDigest,
    ) -> Result<Vec<ReferrerRecord>, CatalogError> {
        self.0.referrers(a, r, s).await
    }
}

struct Vertical {
    app: Router,
    store: Arc<DgraphCatalogStore>,
    audit: Arc<InMemoryAuditSink>,
    #[allow(dead_code)]
    tmp: TempDir,
}

async fn mount(account: &str) -> Option<Vertical> {
    mount_with(account, false).await
}

/// `broken_listing`: the registry listing the detector needs fails.
async fn mount_with(account: &str, broken_listing: bool) -> Option<Vertical> {
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
    let reader: Arc<dyn CatalogReader> = if broken_listing {
        Arc::new(BrokenListing(store.clone()))
    } else {
        store.clone()
    };
    let writer: Arc<dyn CatalogWriter> = store.clone();
    let content: Arc<dyn hodei_hub::blob::BlobStore> =
        Arc::new(FileBlobStore::new(tmp.path().join("cas")).expect("the CAS"));
    let audit = Arc::new(InMemoryAuditSink::new());
    let state = state
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
                format: RepositoryFormat::Npm,
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

/// The body of an `npm publish`: a version and its tarball in base64.
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

async fn publish(
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

/// The package names the catalog holds in the repository (packuments only).
async fn names_in(v: &Vertical, account: &str, repo: &str) -> Vec<String> {
    let mut names: Vec<String> = v
        .store
        .assets_under(account, repo, "")
        .await
        .expect("the listing")
        .into_iter()
        .filter_map(|asset| {
            asset
                .coordinates
                .package_name
                .strip_suffix("#packument")
                .map(str::to_string)
        })
        .collect();
    names.sort();
    names
}

/// How many audit events carry `action` over a resource that ends in `name`.
fn events(v: &Vertical, action: &str, name: &str) -> Vec<AuditResult> {
    v.audit
        .events()
        .into_iter()
        .filter(|event| event.action == action && event.resource.ends_with(&format!("/{name}")))
        .map(|event| event.result)
        .collect()
}

fn any_typosquat_event(v: &Vertical) -> bool {
    v.audit
        .events()
        .iter()
        .any(|event| event.action.starts_with("typosquat."))
}

const GENERIC: &str = "this repository does not accept that package name";

#[tokio::test]
async fn a_legitimate_name_is_published_and_a_lookalike_is_refused_and_leaves_nothing() {
    let account = format!("h{}a", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    seed(
        &v.store,
        &account,
        "npm-guarded",
        json!({"typosquat": {"mode": "block", "protected": ["react", "lodash", "react-dom"]}}),
    )
    .await;
    let alice = format!("alice@{account}");

    // A legitimate new name goes through.
    let (status, body) = publish(&v, "npm-guarded", &alice, "left-pad", "1.0.0").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    // ...and so does a name from the registry that later gets a typo'd neighbour.
    let (status, body) = publish(&v, "npm-guarded", &alice, "acme-utils", "1.0.0").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // Every family of look-alike: protected (transposition, digit homoglyph,
    // separator-only) and registry (one edit).
    for squat in ["raect", "l0dash", "reactdom", "acme-utlis", "left-pa"] {
        let (status, body) = publish(&v, "npm-guarded", &alice, squat, "1.0.0").await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{squat}: {body}");
        assert!(body.contains(GENERIC), "{squat}: {body}");
        // Generic: the matched package, the rule and the policy never leave.
        for leak in [
            "react",
            "lodash",
            "acme-utils",
            "left-pad",
            "edit",
            "confusable",
            "typosquat",
        ] {
            assert!(
                !body.to_lowercase().replace(squat, "").contains(leak),
                "{squat} leaks {leak}: {body}"
            );
        }
        assert_eq!(
            events(&v, "typosquat.refused", squat),
            vec![AuditResult::Denied],
            "{squat}"
        );
        // The packument is not there either.
        let (status, _) = send(
            &v.app,
            Method::GET,
            &format!("/npm/npm-guarded/{squat}"),
            &alice,
            Vec::new(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{squat}");
    }
    assert_eq!(
        names_in(&v, &account, "npm-guarded").await,
        vec!["acme-utils".to_string(), "left-pad".to_string()],
        "a refused look-alike leaves no asset in the catalog"
    );

    // A new VERSION of an existing package is the package itself, not judged.
    let (status, body) = publish(&v, "npm-guarded", &alice, "acme-utils", "1.1.0").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(events(&v, "typosquat.refused", "acme-utils").is_empty());
}

#[tokio::test]
async fn audit_mode_publishes_and_flags_instead_of_refusing() {
    let account = format!("h{}b", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    seed(
        &v.store,
        &account,
        "npm-audit",
        json!({"typosquat": {"mode": "audit", "protected": ["react"]}}),
    )
    .await;
    let alice = format!("alice@{account}");
    let (status, body) = publish(&v, "npm-audit", &alice, "raect", "1.0.0").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        events(&v, "typosquat.flagged", "raect"),
        vec![AuditResult::Allowed]
    );
    assert!(events(&v, "typosquat.refused", "raect").is_empty());
    assert_eq!(
        names_in(&v, &account, "npm-audit").await,
        vec!["raect".to_string()]
    );
}

#[tokio::test]
async fn a_repository_without_the_declaration_is_untouched() {
    let account = format!("h{}c", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    seed(&v.store, &account, "npm-plain", json!({})).await;
    let alice = format!("alice@{account}");
    for name in ["react", "raect", "reactdom", "l0dash"] {
        let (status, body) = publish(&v, "npm-plain", &alice, name, "1.0.0").await;
        assert_eq!(status, StatusCode::CREATED, "{name}: {body}");
    }
    assert!(
        !any_typosquat_event(&v),
        "no declaration, no detector, no event"
    );
}

#[tokio::test]
async fn an_unreadable_declaration_fails_closed_and_publishes_nothing() {
    let account = format!("h{}d", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    let alice = format!("alice@{account}");
    // Seeded straight into the catalog: the creation API would refuse these.
    for (repo, settings) in [
        ("bad-mode", json!({"typosquat": {"mode": "off"}})),
        ("bad-shape", json!({"typosquat": "block"})),
        (
            "empty",
            json!({"typosquat": {"mode": "block", "scan_registry": false}}),
        ),
    ] {
        seed(&v.store, &account, repo, settings).await;
        let (status, body) = publish(&v, repo, &alice, "perfectly-fine", "1.0.0").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{repo}: {body}");
        assert!(names_in(&v, &account, repo).await.is_empty(), "{repo}");
    }
    assert_eq!(
        events(&v, "typosquat.unavailable", "perfectly-fine").len(),
        3
    );
}

#[tokio::test]
async fn a_registry_listing_that_fails_fails_closed_and_publishes_nothing() {
    let account = format!("h{}e", &nonce()[8..]);
    let Some(v) = mount_with(&account, true).await else {
        return;
    };
    seed(
        &v.store,
        &account,
        "npm-broken",
        json!({"typosquat": {"mode": "block", "protected": ["react"]}}),
    )
    .await;
    let alice = format!("alice@{account}");
    let (status, body) = publish(&v, "npm-broken", &alice, "perfectly-fine", "1.0.0").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(names_in(&v, &account, "npm-broken").await.is_empty());
    assert_eq!(
        events(&v, "typosquat.unavailable", "perfectly-fine"),
        vec![AuditResult::Failed]
    );
}

#[tokio::test]
async fn declaring_the_policy_is_only_accepted_on_hosted_npm_and_only_if_readable() {
    let account = format!("h{}f", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    let alice = format!("alice@{account}");
    let create = |name: &str, format: &str, mode: &str, settings: Value| {
        let app = v.app.clone();
        let alice = alice.clone();
        let body = json!({"name": name, "format": format, "mode": mode, "settings": settings});
        async move {
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
    let good = json!({"typosquat": {"mode": "block", "protected": ["react"]}});
    assert_eq!(
        create("n-ok", "npm", "hosted", good.clone()).await,
        StatusCode::CREATED
    );
    // Anywhere else it would look active and not be.
    assert_eq!(
        create("raw-no", "raw", "hosted", good.clone()).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        create("pypi-no", "pypi", "hosted", good.clone()).await,
        StatusCode::BAD_REQUEST
    );
    // Unreadable declarations are refused, never read generously.
    for bad in [
        json!({"typosquat": {"mode": "maybe"}}),
        json!({"typosquat": {}}),
        json!({"typosquat": {"mode": "block", "protected": [""]}}),
        json!({"typosquat": {"mode": "block", "scan_registry": false}}),
    ] {
        assert_eq!(
            create("n-bad", "npm", "hosted", bad.clone()).await,
            StatusCode::BAD_REQUEST,
            "{bad}"
        );
    }
}

#[tokio::test]
async fn a_new_version_of_an_existing_lookalike_pair_is_not_judged() {
    let account = format!("h{}g", &nonce()[8..]);
    let Some(v) = mount(&account).await else {
        return;
    };
    // Two names that look alike already coexist (published while the
    // repository only audited); then the repository starts to block.
    seed(
        &v.store,
        &account,
        "npm-tighten",
        json!({"typosquat": {"mode": "audit"}}),
    )
    .await;
    let alice = format!("alice@{account}");
    assert_eq!(
        publish(&v, "npm-tighten", &alice, "acme-utils", "1.0.0")
            .await
            .0,
        StatusCode::CREATED
    );
    assert_eq!(
        publish(&v, "npm-tighten", &alice, "acme-utlis", "1.0.0")
            .await
            .0,
        StatusCode::CREATED
    );
    let mut config = RepositoryConfig::new(DigestScope::Global);
    config.config_settings = json!({"typosquat": {"mode": "block"}});
    v.store
        .update_config(&account, "npm-tighten", config)
        .await
        .expect("the repository tightens");
    // The existing package keeps releasing: only NEW names are judged.
    let (status, body) = publish(&v, "npm-tighten", &alice, "acme-utlis", "1.1.0").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(events(&v, "typosquat.refused", "acme-utlis").is_empty());
    // A third, new look-alike is refused.
    let (status, _) = publish(&v, "npm-tighten", &alice, "acme-utilz", "1.0.0").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
