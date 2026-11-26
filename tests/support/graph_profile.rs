//! The graph profile of the conformance harness (HSP-314 F4 S2a).
//!
//! A test deployment runs the production binary (or the production
//! `bootstrap`, in process) on `RUN_MODE=dgraph` against a REAL Dgraph alpha.
//! The alpha is persistent and shared, so nothing here assumes an empty
//! database: every deployment gets its **own ACL namespace** (Dgraph
//! multi-tenancy) with its own `groot`, and deletes it when it drops. Two
//! deployments, two suites or two runs never see each other's data, and the
//! fixed accounts of the suites (`acme`, `other`) keep working untouched.
//!
//! Environment (the same convention as the other graph ITs):
//! - `HODEI_GRAPH_IT_ENDPOINT` (`host:port` of the gRPC listener): without it
//!   the suite skips itself (exit 0) and `scripts/dgraph-it-gate.sh` declares
//!   NOT_RUN.
//! - `HODEI_GRAPH_IT_USER` / `HODEI_GRAPH_IT_PASSWORD`: the guardian of the
//!   galaxy namespace, needed to create namespaces (ACL is mandatory).
//! - `HODEI_GRAPH_IT_ADMIN` (`host:port` of the alpha HTTP port, default
//!   `<host of the endpoint>:8080`): where `/admin` answers.
//!
//! This file is independent of `support/mod.rs` (it is also included on its
//! own by the suites that boot the application in process).

#![allow(dead_code)]

use std::io::Write as _;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

/// The locator prefix that `Deployment::db` carries in the graph profile: the
/// seeder child process learns the tenant from it.
pub const LOCATOR_PREFIX: &str = "graph://";

/// The gRPC endpoint of the alpha, when the suite is asked to run.
pub fn endpoint() -> Option<String> {
    std::env::var("HODEI_GRAPH_IT_ENDPOINT")
        .ok()
        .filter(|value| !value.is_empty())
}

/// Skip the whole test binary (exit 0, the convention of the `*_it` targets)
/// when there is no alpha to run against. A skip proves nothing, and the gate
/// of the pipeline says so.
pub fn skip_unless_configured() -> String {
    match endpoint() {
        Some(endpoint) => endpoint,
        None => {
            eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set (the suite needs a real Dgraph)");
            std::process::exit(0);
        }
    }
}

fn required(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!("{name} is required: the graph harness creates one ACL namespace per deployment")
    })
}

fn admin_address(endpoint: &str) -> String {
    std::env::var("HODEI_GRAPH_IT_ADMIN").unwrap_or_else(|_| {
        let host = endpoint.rsplit_once(':').map_or(endpoint, |(host, _)| host);
        format!("{host}:8080")
    })
}

/// One `curl` POST of a GraphQL body to the alpha `/admin`. The request goes
/// through `curl -K -` so neither the token nor the password reach argv.
fn admin_call(address: &str, token: Option<&str>, query: &str) -> serde_json::Value {
    let escape = |text: &str| text.replace('\\', "\\\\").replace('"', "\\\"");
    let body = serde_json::json!({ "query": query }).to_string();
    let mut config = format!(
        "url = \"http://{address}/admin\"\nsilent\nmax-time = 120\nheader = \"Content-Type: application/json\"\ndata = \"{}\"\n",
        escape(&body)
    );
    if let Some(token) = token {
        config.push_str(&format!(
            "header = \"X-Dgraph-AccessToken: {}\"\n",
            escape(token)
        ));
    }
    let mut child = Command::new("curl")
        .args(["-K", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("curl runs");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(config.as_bytes())
        .expect("the request is written");
    let output = child.wait_with_output().expect("curl answers");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("the alpha admin answered: {:?}", output.stdout));
    if let Some(errors) = value.get("errors")
        && !errors.is_null()
    {
        panic!("the alpha admin rejected the request: {errors}");
    }
    value
}

fn galaxy_token(address: &str) -> String {
    let user = required("HODEI_GRAPH_IT_USER");
    let password = required("HODEI_GRAPH_IT_PASSWORD");
    let answer = admin_call(
        address,
        None,
        &format!(
            "mutation {{ login(userId: \"{user}\", password: \"{password}\", namespace: 0) {{ response {{ accessJWT }} }} }}"
        ),
    );
    answer["data"]["login"]["response"]["accessJWT"]
        .as_str()
        .expect("the galaxy login yields a token")
        .to_string()
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// One ACL namespace of the alpha, owned by one deployment.
pub struct GraphTenant {
    /// The numeric ACL namespace (also the tenant of the durable writers).
    pub namespace: u64,
    /// The password of the `groot` of this namespace (a throwaway).
    pub password: String,
    /// `host:port` of the gRPC listener.
    pub endpoint: String,
    admin: String,
    owned: bool,
}

impl GraphTenant {
    /// Create a fresh namespace. Skips the binary when no alpha is configured.
    pub fn create() -> Self {
        let endpoint = skip_unless_configured();
        let admin = admin_address(&endpoint);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let password = format!(
            "ns{:x}{:x}{:x}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
            nanos
        );
        let token = galaxy_token(&admin);
        let answer = admin_call(
            &admin,
            Some(&token),
            &format!(
                "mutation {{ addNamespace(input: {{ password: \"{password}\" }}) {{ namespaceId message }} }}"
            ),
        );
        let namespace = answer["data"]["addNamespace"]["namespaceId"]
            .as_u64()
            .expect("the namespace id");
        Self {
            namespace,
            password,
            endpoint,
            admin,
            owned: true,
        }
    }

    /// The tenant described by a locator (`graph://<namespace>/<password>`),
    /// as the seeder child process reads it. It does not own the namespace.
    pub fn from_locator(locator: &str) -> Option<Self> {
        let rest = locator.strip_prefix(LOCATOR_PREFIX)?;
        let (namespace, password) = rest.split_once('/')?;
        let endpoint = skip_unless_configured();
        Some(Self {
            namespace: namespace.parse().ok()?,
            password: password.to_string(),
            admin: admin_address(&endpoint),
            endpoint,
            owned: false,
        })
    }

    /// The locator that travels in `Deployment::db`.
    pub fn locator(&self) -> String {
        format!("{LOCATOR_PREFIX}{}/{}", self.namespace, self.password)
    }

    /// The `[graph]` section of this tenant, as TOML.
    pub fn graph_toml(&self) -> String {
        format!(
            "endpoint = \"{}\"\nnamespace = {}\n\n[tls]\nenabled = false\nca_cert_path = \"\"\n\n[credentials]\nuserid = \"groot\"\npassword = \"{}\"\n",
            self.endpoint, self.namespace, self.password
        )
    }

    /// The environment that boots the production binary on this tenant.
    pub fn service_environment(&self) -> Vec<(String, String)> {
        vec![
            ("RUN_MODE".into(), "dgraph".into()),
            // The harness reads the bound port from the pretty log.
            ("HODEI_LOGGING__FORMAT".into(), "pretty".into()),
            ("HODEI_GRAPH__ENDPOINT".into(), self.endpoint.clone()),
            ("HODEI_GRAPH__TLS__ENABLED".into(), "false".into()),
            ("HODEI_GRAPH__TLS__CA_CERT_PATH".into(), String::new()),
            ("HODEI_GRAPH__TLS__SERVER_NAME".into(), String::new()),
            ("HODEI_GRAPH__NAMESPACE".into(), self.namespace.to_string()),
            ("HODEI_GRAPH__CREDENTIALS__USERID".into(), "groot".into()),
            (
                "HODEI_GRAPH__CREDENTIALS__PASSWORD".into(),
                self.password.clone(),
            ),
            ("HODEI_CONTENT__BACKEND".into(), "filesystem".into()),
            // `config/dgraph.toml` keeps interrupted uploads a day; the suites
            // assume the default of the code (an hour), as on the embedded profile.
            ("HODEI_CONTENT__STAGING_MAX_AGE_SECS".into(), "3600".into()),
        ]
    }

    /// Point an in-process `AppConfig` at this tenant: the production
    /// `bootstrap` then opens the graph profile (the shape of
    /// `config/dgraph.toml` with the filesystem content backend).
    pub fn apply(&self, config: &mut hodei_artifacts_api::AppConfig, cas: &std::path::Path) {
        config.graph = Some(
            hodei_graph_client::GraphClientConfig::from_toml_str(&self.graph_toml())
                .expect("the graph section validates"),
        );
        config.database.db_type = "dgraph".to_string();
        config.schema.register_iam_on_startup = false;
        config.schema.storage_type = "dgraph".to_string();
        config.content.backend = hodei_artifacts_api::config::ContentBackend::Filesystem;
        config.content.cas_path = cas.to_string_lossy().into_owned();
    }

    /// A connected, logged-in client of this tenant, to seed or to measure.
    pub async fn client(&self) -> std::sync::Arc<hodei_graph_client::GraphClient> {
        let mut client = hodei_graph_client::GraphClient::connect(
            hodei_graph_client::GraphClientConfig::from_toml_str(&self.graph_toml())
                .expect("the graph section validates"),
        )
        .await
        .expect("Dgraph connects");
        client.login().await.expect("the tenant logs in");
        std::sync::Arc::new(client)
    }

    /// The catalog store of this tenant. `DgraphCatalogStore::open` alters
    /// the schema and Dgraph rejects an `alter` during its reindexing window
    /// (`Unavailable`, transient by contract): retried with a bounded backoff.
    pub async fn catalog_store(&self) -> hodei_catalog_storage::DgraphCatalogStore {
        let client = self.client().await;
        let mut last = String::new();
        for attempt in 0..8u64 {
            match hodei_catalog_storage::DgraphCatalogStore::open(
                client.clone(),
                std::sync::Arc::new(hodei_catalog::AllowAllAuthorizer),
            )
            .await
            {
                Ok(store) => return store,
                Err(error @ hodei_catalog::CatalogError::Unavailable { .. }) => {
                    last = error.to_string();
                    tokio::time::sleep(std::time::Duration::from_millis(200 * (attempt + 1))).await;
                }
                Err(error) => panic!("the dgraph catalog store does not open: {error}"),
            }
        }
        panic!("the dgraph catalog store does not open after 8 attempts: {last}");
    }
}

impl Drop for GraphTenant {
    /// The namespace goes away with its deployment (best effort: a leaked
    /// namespace is isolated garbage, not a hazard for other tests).
    fn drop(&mut self) {
        if !self.owned {
            return;
        }
        let admin = self.admin.clone();
        let namespace = self.namespace;
        let _ = std::panic::catch_unwind(move || {
            let token = galaxy_token(&admin);
            admin_call(
                &admin,
                Some(&token),
                &format!(
                    "mutation {{ deleteNamespace(input: {{ namespaceId: {namespace} }}) {{ namespaceId }} }}"
                ),
            );
        });
    }
}

// ---------------------------------------------------------------------------
// The seeders: the same entries `Deployment::seed_*` describe, written through
// the REAL `DgraphCatalogStore` of the tenant (no double of Dgraph).
// ---------------------------------------------------------------------------

use hodei_catalog::{
    CatalogReader as _, CatalogWriter as _, DigestScope, RepositoryConfig, RepositoryDescriptor,
    RepositoryFormat, RepositoryMode, repository_hrn,
};

const FIRST_ACCOUNT: &str = "acme";
const SECOND_ACCOUNT: &str = "other";

fn format_of(name: Option<&str>) -> RepositoryFormat {
    match name {
        Some("raw") => RepositoryFormat::Raw,
        Some("npm") => RepositoryFormat::Npm,
        Some("pypi") => RepositoryFormat::PyPi,
        Some("cargo") => RepositoryFormat::Cargo,
        Some("go") => RepositoryFormat::Go,
        Some("nuget") => RepositoryFormat::NuGet,
        Some("oci") => RepositoryFormat::Oci,
        Some("helm") => RepositoryFormat::Helm,
        Some("apt") => RepositoryFormat::Apt,
        Some("rpm") => RepositoryFormat::Rpm,
        Some("conan") => RepositoryFormat::Conan,
        Some("galaxy") => RepositoryFormat::Galaxy,
        Some("terraform") => RepositoryFormat::Terraform,
        Some("hf") => RepositoryFormat::Hf,
        _ => RepositoryFormat::Maven2,
    }
}

fn descriptor(
    account: &str,
    key: &str,
    format: RepositoryFormat,
    mode: RepositoryMode,
    config: RepositoryConfig,
) -> RepositoryDescriptor {
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", account, key),
        account: account.to_string(),
        repo_key: key.to_string(),
        format,
        mode,
        config,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

/// `HODEI_SEED_EXTRA` & co: one repository per account plus the asked ones.
pub async fn seed_catalog(
    tenant: &GraphTenant,
    extra: &[&str],
    trusted: &[&str],
    proxies: &serde_json::Map<String, serde_json::Value>,
    replace: &[&str],
) {
    let store = tenant.catalog_store().await;
    let seeded = [
        (FIRST_ACCOUNT, "releases"),
        (FIRST_ACCOUNT, "private"),
        (SECOND_ACCOUNT, "releases"),
    ]
    .into_iter()
    .chain(extra.iter().map(|key| (FIRST_ACCOUNT, *key)))
    .chain(proxies.keys().map(|key| (FIRST_ACCOUNT, key.as_str())));
    for (account, key) in seeded {
        let mut config = RepositoryConfig::new(DigestScope::Global);
        let mut settings = serde_json::Map::new();
        if account == FIRST_ACCOUNT && trusted.contains(&key) {
            settings.insert("trust".into(), serde_json::json!({ "serve": "trusted" }));
        }
        if account == FIRST_ACCOUNT && replace.contains(&key) {
            settings.insert("raw".into(), serde_json::json!({ "replace": "allow" }));
        }
        let mode = match proxies.get(key) {
            Some(upstream) if account == FIRST_ACCOUNT => {
                let proxy = if upstream.get("upstream").is_some() {
                    upstream.clone()
                } else {
                    serde_json::json!({ "upstream": upstream })
                };
                settings.insert("proxy".into(), proxy);
                RepositoryMode::Proxy
            }
            _ => RepositoryMode::Hosted,
        };
        config.config_settings = serde_json::Value::Object(settings);
        store
            .create_repository(
                account,
                descriptor(account, key, RepositoryFormat::Raw, mode, config),
            )
            .await
            .expect("the repository is created");
    }
}

/// `HODEI_SEED_MAVEN`: free-form hosted/proxy repositories of the first account.
pub async fn seed_repositories(tenant: &GraphTenant, repositories: &[serde_json::Value]) {
    let store = tenant.catalog_store().await;
    for repository in repositories {
        let key = repository["key"].as_str().expect("the key");
        let scope = if repository["scope"].as_str() == Some("partitioned") {
            DigestScope::AccountPartitioned
        } else {
            DigestScope::Global
        };
        let mut config = RepositoryConfig::new(scope);
        let mut settings = serde_json::Map::new();
        for name in ["maven", "terraform", "npm", "pypi", "namespaces"] {
            if !repository[name].is_null() {
                settings.insert(name.into(), repository[name].clone());
            }
        }
        if repository["trusted"].as_bool() == Some(true) {
            settings.insert("trust".into(), serde_json::json!({ "serve": "trusted" }));
        }
        let mode = if repository["proxy"].is_null() {
            RepositoryMode::Hosted
        } else {
            settings.insert("proxy".into(), repository["proxy"].clone());
            RepositoryMode::Proxy
        };
        config.config_settings = serde_json::Value::Object(settings);
        store
            .create_repository(
                FIRST_ACCOUNT,
                descriptor(
                    FIRST_ACCOUNT,
                    key,
                    format_of(repository["format"].as_str()),
                    mode,
                    config,
                ),
            )
            .await
            .expect("the repository is created");
    }
}

/// `HODEI_SEED_HF`: hosted hf repositories of the first account. Every model
/// file goes into the content store of the deployment and is published through
/// the real catalog store: the hf dialect declares no write path, so the
/// content enters by seeding, as a hub import would.
pub async fn seed_hf(tenant: &GraphTenant, cas: &str, repositories: &[serde_json::Value]) {
    use hodei_catalog::{PackageCoordinates, Publication};
    use hodei_hub::{BlobStore as _, FileBlobStore};
    let store = tenant.catalog_store().await;
    let blobs = FileBlobStore::new(cas).expect("the blob store");
    for repository in repositories {
        let key = repository["key"].as_str().expect("the key");
        let mut config = RepositoryConfig::new(DigestScope::Global);
        let mut settings = serde_json::Map::new();
        if repository["trusted"].as_bool() == Some(true) {
            settings.insert("trust".into(), serde_json::json!({ "serve": "trusted" }));
        }
        config.config_settings = serde_json::Value::Object(settings);
        store
            .create_repository(
                FIRST_ACCOUNT,
                descriptor(
                    FIRST_ACCOUNT,
                    key,
                    RepositoryFormat::Hf,
                    RepositoryMode::Hosted,
                    config,
                ),
            )
            .await
            .expect("the hf repository is created");
        for asset in repository["assets"].as_array().expect("the assets") {
            let asset_path = asset["asset"].as_str().expect("the asset path");
            let file = asset["file"].as_str().expect("the source file");
            let bytes = std::fs::read(file).expect("the model file");
            let digest = blobs.put(&bytes).await.expect("the blob is stored");
            // The repo id is what the asset path carries between `artifacts/`
            // and the last slash; every model file is its own package version.
            let rest = asset_path
                .strip_prefix("artifacts/")
                .expect("the asset of a model");
            let (repo_id, filename) = rest.rsplit_once('/').expect("the file of a model");
            store
                .publish(
                    FIRST_ACCOUNT,
                    Publication {
                        repository: key.to_string(),
                        asset_path: asset_path.to_string(),
                        coordinates: PackageCoordinates {
                            package_name: repo_id.to_string(),
                            package_version: filename.to_string(),
                        },
                        digest,
                        size_bytes: bytes.len() as u64,
                        tags: Vec::new(),
                        actor: "seeder".to_string(),
                        correlation_id: "seed-hf".to_string(),
                        overwrite: false,
                        precondition: None,
                        subject: None,
                    },
                )
                .await
                .expect("the model file is published");
        }
    }
}

/// `HODEI_SEED_GROUPS`: group repositories of the first account.
pub async fn seed_groups(tenant: &GraphTenant, groups: &[serde_json::Value]) {
    let store = tenant.catalog_store().await;
    for group in groups {
        let key = group["key"].as_str().expect("the key");
        let mut config = RepositoryConfig::new(DigestScope::Global);
        let mut settings = serde_json::json!({ "group": { "members": group["members"] } });
        if !group["namespaces"].is_null() {
            settings["namespaces"] = group["namespaces"].clone();
        }
        config.config_settings = settings;
        let format = match group["format"].as_str() {
            Some(name @ ("npm" | "maven2" | "pypi" | "go" | "nuget" | "oci")) => {
                format_of(Some(name))
            }
            _ => RepositoryFormat::Raw,
        };
        store
            .create_repository(
                FIRST_ACCOUNT,
                descriptor(FIRST_ACCOUNT, key, format, RepositoryMode::Group, config),
            )
            .await
            .expect("the group is created");
    }
}

/// Rewrite the settings of a seeded repository (the version of its
/// configuration goes up).
async fn reconfigure(
    tenant: &GraphTenant,
    key: &str,
    edit: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>),
) {
    let store = tenant.catalog_store().await;
    let current = store
        .repository(FIRST_ACCOUNT, key)
        .await
        .expect("the repository exists");
    let mut config = current.config;
    let mut settings = match config.config_settings {
        serde_json::Value::Object(settings) => settings,
        _ => serde_json::Map::new(),
    };
    edit(&mut settings);
    config.config_settings = serde_json::Value::Object(settings);
    store
        .update_config(FIRST_ACCOUNT, key, config)
        .await
        .expect("the configuration is updated");
}

/// Dispatch of the seeder child process: the same environment protocol the
/// embedded seeder reads (`HODEI_SEED_*`, `HODEI_RECONFIG_*`).
pub async fn seed_from_environment(tenant: &GraphTenant) {
    let var = |name: &str| std::env::var(name).ok();
    if let Some(repositories) = var("HODEI_SEED_MAVEN") {
        let repositories: Vec<serde_json::Value> =
            serde_json::from_str(&repositories).expect("the repositories parse");
        return seed_repositories(tenant, &repositories).await;
    }
    if let Some(hf) = var("HODEI_SEED_HF") {
        let cas = var("HODEI_SEED_CAS").expect("the cas of the hf seeding");
        let repositories: Vec<serde_json::Value> =
            serde_json::from_str(&hf).expect("the hf repositories parse");
        return seed_hf(tenant, &cas, &repositories).await;
    }
    if let Some(groups) = var("HODEI_SEED_GROUPS") {
        let groups: Vec<serde_json::Value> =
            serde_json::from_str(&groups).expect("the groups parse");
        return seed_groups(tenant, &groups).await;
    }
    if let Some(key) = var("HODEI_RECONFIG_GROUP") {
        let members: Vec<String> =
            serde_json::from_str(&var("HODEI_RECONFIG_MEMBERS").expect("the new members"))
                .expect("the members parse");
        return reconfigure(tenant, &key, |settings| {
            settings.clear();
            settings.insert("group".into(), serde_json::json!({ "members": members }));
        })
        .await;
    }
    if let Some(key) = var("HODEI_RECONFIG_KEY") {
        if let Some(namespaces) = var("HODEI_RECONFIG_NAMESPACES") {
            let namespaces: serde_json::Value =
                serde_json::from_str(&namespaces).expect("the new namespaces parse");
            return reconfigure(tenant, &key, |settings| {
                if namespaces.is_null() {
                    settings.remove("namespaces");
                } else {
                    settings.insert("namespaces".into(), namespaces);
                }
            })
            .await;
        }
        let proxy: serde_json::Value =
            serde_json::from_str(&var("HODEI_RECONFIG_PROXY").expect("the new proxy settings"))
                .expect("the new proxy settings parse");
        return reconfigure(tenant, &key, |settings| {
            settings.insert("proxy".into(), proxy);
        })
        .await;
    }
    let list = |name: &str| -> Vec<String> {
        var(name)
            .unwrap_or_default()
            .split(',')
            .filter(|one| !one.is_empty())
            .map(str::to_string)
            .collect()
    };
    let (extra, trusted, replace) = (
        list("HODEI_SEED_EXTRA"),
        list("HODEI_SEED_TRUSTED"),
        list("HODEI_SEED_REPLACE"),
    );
    let proxies = var("HODEI_SEED_PROXY")
        .and_then(|raw| {
            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&raw).ok()
        })
        .unwrap_or_default();
    fn refs(list: &[String]) -> Vec<&str> {
        list.iter().map(String::as_str).collect()
    }
    seed_catalog(
        tenant,
        &refs(&extra),
        &refs(&trusted),
        &proxies,
        &refs(&replace),
    )
    .await;
}

/// The working directory of an in-process application: a temporary directory
/// (policy files, CAS) that owns the Dgraph namespace of the application.
/// It stands where `tempfile::TempDir` stood in the embedded suites.
pub struct Workdir {
    dir: tempfile::TempDir,
    tenant: GraphTenant,
}

impl Workdir {
    /// A fresh directory and a fresh namespace.
    pub fn new() -> Self {
        Self {
            dir: tempfile::tempdir().expect("temp dir"),
            tenant: GraphTenant::create(),
        }
    }

    pub fn path(&self) -> &std::path::Path {
        self.dir.path()
    }

    pub fn tenant(&self) -> &GraphTenant {
        &self.tenant
    }

    /// Point the application config at the graph profile of this tenant.
    pub fn apply(&self, config: &mut hodei_artifacts_api::AppConfig) {
        self.tenant.apply(config, &self.dir.path().join("cas"));
    }
}
