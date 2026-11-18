//! Configuration module for Hodei Artifacts API
//!
//! This module handles loading and managing application configuration
//! from multiple sources with hierarchical precedence and validation.

use ::config::{Config, ConfigError, Environment, File};
use axum::http::HeaderValue;
use serde::{Deserialize, Serialize};
use std::env;

/// Main application configuration
///
/// This struct holds all configuration parameters for the Hodei Artifacts API.
/// Configuration is loaded from multiple sources with the following precedence:
/// 1. Environment variables (HODEI_ prefix)
/// 2. config/local.toml
/// 3. config/{RUN_MODE}.toml
/// 4. config/default.toml
/// 5. Hardcoded defaults
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepositoryConfig {
    /// Use the durable SurrealDB adapter instead of the in-memory slice.
    #[serde(default)]
    pub durable: bool,
}

/// The limits of the investigation surface.
///
/// An investigation is a read with a cost: it reads the evidence and the
/// graph of the subject. The limits are the **deployment's**, and a
/// request can only **narrow** them: a client that asks for less gets
/// less, and a client that asks for more gets the limit. What does not fit
/// is **omitted and declared**, never truncated in silence.
///
/// The limits are the same ones the domain uses for a retrieval, so the
/// budget of the surface and the budget of the investigator cannot
/// disagree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestigationConfig {
    /// How many evidence pieces one investigation may read.
    ///
    /// Default: 100.
    #[serde(default = "the_default_investigation_max_evidence")]
    pub max_evidence: usize,
    /// How many relations one investigation may read.
    ///
    /// Default: 400.
    #[serde(default = "the_default_investigation_max_relations")]
    pub max_relations: usize,
    /// How many bytes of evidence content one investigation may read.
    ///
    /// Default: 262144 (256 KiB).
    #[serde(default = "the_default_investigation_max_bytes")]
    pub max_bytes: u64,
    /// How long the question may be, in characters.
    ///
    /// The question is not answered by the tools: it is kept in the trace
    /// and declared as unanswered. It still has a size, because a size
    /// that does not exist is a size nobody bounds.
    ///
    /// Default: 2000.
    #[serde(default = "the_default_investigation_max_question_length")]
    pub max_question_length: usize,
    /// How the tenant of the graph is named.
    ///
    /// The graph keeps its rows under the tenant that owns them, and the
    /// name of that tenant is a convention of **whoever writes the
    /// evidence**, not of this service: the producer that delivers the
    /// evidence declares the tenant, and the rows are stored under it.
    ///
    /// The template says how to build that name from the account of the
    /// subject, and `{account}` and `{region}` are the two things that
    /// change from one account to another. A deployment whose producer
    /// names the tenant differently says so here; a deployment that does
    /// not say anything gets the default, and a graph written under
    /// another name simply has no relations to show.
    ///
    /// Default: `hrn:hodei:artifacts:{region}:{account}:artifact/{account}`.
    #[serde(default = "the_default_investigation_graph_tenant")]
    pub graph_tenant_template: String,
}

impl Default for InvestigationConfig {
    fn default() -> Self {
        Self {
            max_evidence: the_default_investigation_max_evidence(),
            max_relations: the_default_investigation_max_relations(),
            max_bytes: the_default_investigation_max_bytes(),
            max_question_length: the_default_investigation_max_question_length(),
            graph_tenant_template: the_default_investigation_graph_tenant(),
        }
    }
}

impl InvestigationConfig {
    /// Check that the template of the tenant names an HRN.
    ///
    /// The check is done with a placeholder account and a placeholder
    /// region, because those are the two things the template has to be
    /// able to carry: a template that does not name an HRN with them
    /// would fail on the first request instead of at the start.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let probe = self
            .graph_tenant_template
            .replace("{account}", "probe-account")
            .replace("{region}", "probe-region");
        if kernel::Hrn::from_string(&probe).is_none() {
            return Err(ConfigError::Message(format!(
                "Invalid investigation configuration: the graph tenant template {} does not name an HRN",
                self.graph_tenant_template
            )));
        }
        Ok(())
    }

    /// The tenant of the graph for a subject.
    ///
    /// The account of the subject is the account of the tenant, and the
    /// region of the subject is the region of the tenant: the tenant is
    /// where the account lives, not a thing that lives next to it.
    pub fn the_graph_tenant(&self, subject: &kernel::Hrn) -> Option<kernel::Hrn> {
        let name = self
            .graph_tenant_template
            .replace("{account}", subject.account_id())
            .replace("{region}", subject.region());
        kernel::Hrn::from_string(&name)
    }
}

/// How many evidence pieces one investigation may read.
fn the_default_investigation_max_evidence() -> usize {
    100
}

/// How many relations one investigation may read.
fn the_default_investigation_max_relations() -> usize {
    400
}

/// How many bytes of evidence content one investigation may read.
fn the_default_investigation_max_bytes() -> u64 {
    256 * 1024
}

/// How long the question may be.
fn the_default_investigation_max_question_length() -> usize {
    2000
}

/// How the tenant of the graph is named.
fn the_default_investigation_graph_tenant() -> String {
    "hrn:hodei:artifacts:{region}:{account}:artifact/{account}".to_string()
}

/// Where the content of the artifacts lives.
///
/// The content is stored **by digest**, not by path: the same content is
/// the same file, wherever it is. That is why the directory is not a
/// decision of the account, but of the deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentConfig {
    /// The directory of the store of content.
    ///
    /// Default: `./data/hodei.cas`.
    #[serde(default = "the_default_cas_path")]
    pub cas_path: String,
    /// How old an interrupted write may be before the store cleans it up.
    ///
    /// A process that dies in the middle of an upload leaves its staging
    /// file on disk. That file is never published -- the catalog write
    /// never happened -- but it is a file, and files pile up.
    ///
    /// The age is a margin of prudence, **not** the criterion. While a
    /// write is in progress its file is locked by the process that is
    /// writing it, and a locked file is not removed: a write that stops
    /// writing for longer than the age is still a live write, and
    /// taking its file away from under it breaks it.
    ///
    /// Minimum: 60 seconds. Below that the age stops being a margin.
    ///
    /// Default: 3600 seconds.
    #[serde(default = "the_default_staging_max_age_secs")]
    pub staging_max_age_secs: u64,
    /// How long an unreferenced blob is left alone before the garbage
    /// collection may reclaim it.
    ///
    /// The grace must outlast the time between the moment the bytes reach
    /// the store and the moment the catalog row that references them is
    /// written: a blob uploaded a moment ago has no row yet and is not
    /// garbage. The collection also refuses to delete anything while it
    /// cannot tell what is alive.
    ///
    /// Minimum: 1 second (tests); a production value is hours.
    ///
    /// Default: 86400 seconds (one day).
    #[serde(default = "the_default_gc_grace_secs")]
    pub gc_grace_secs: u64,
    /// Where the content actually lives: the local directory named by
    /// `cas_path`, or an S3-compatible object store.
    ///
    /// There is **no fallback** between them. A deployment that chooses
    /// S3 never writes a byte to `cas_path`: if the object store is not
    /// reachable, the service refuses to boot rather than quietly keep
    /// its content somewhere else. Choosing one backend and silently
    /// using the other would split the content in two places, and a blob
    /// that lives in the wrong one is a blob the GC will delete as
    /// garbage and the catalog will report as healthy.
    ///
    /// Default: `filesystem`.
    #[serde(default)]
    pub backend: ContentBackend,
    /// How to reach the object store when the backend is `s3`.
    ///
    /// Ignored -- and it should be absent -- when the backend is
    /// `filesystem`. Credentials are **never** here: they come from the
    /// SDK's own provider chain (environment, shared config, IAM role),
    /// so no key can end up in a TOML file, a log line or an audit
    /// record.
    #[serde(default)]
    pub s3: Option<S3ContentConfig>,
}

/// Which store of content the deployment uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentBackend {
    /// A directory on the local filesystem, named by `cas_path`.
    #[default]
    Filesystem,
    /// An S3-compatible object store, named by the `s3` table.
    S3,
}

/// The object store the content lives in when the backend is `s3`.
///
/// No credential field, on purpose (see [`ContentConfig::s3`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct S3ContentConfig {
    /// The bucket that holds the blobs. Required.
    pub bucket: String,
    /// A custom endpoint, for S3-compatible stores that are not AWS S3.
    ///
    /// Default: the AWS endpoint for the region.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// The region of the bucket.
    ///
    /// Default: the SDK resolves it from the environment or the shared
    /// config.
    #[serde(default)]
    pub region: Option<String>,
    /// Whether to talk to the endpoint over TLS.
    ///
    /// Default: true. Turning it off is for a local store behind no
    /// proxy; against a real object store it would send the credentials
    /// the SDK chain provides in the clear.
    #[serde(default = "the_default_s3_tls")]
    pub tls: bool,
}

fn the_default_s3_tls() -> bool {
    true
}

impl ContentConfig {
    /// Check that the values of the content configuration make sense.
    ///
    /// # Returns
    ///
    /// Ok(()) if the configuration is valid, ConfigError with clear message if invalid
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.staging_max_age_secs < the_minimum_staging_max_age_secs() {
            return Err(ConfigError::Message(format!(
                "Invalid content configuration: staging_max_age_secs must be at least {} seconds, and {} was given",
                the_minimum_staging_max_age_secs(),
                self.staging_max_age_secs
            )));
        }
        if self.gc_grace_secs < the_minimum_gc_grace_secs() {
            return Err(ConfigError::Message(format!(
                "Invalid content configuration: gc_grace_secs must be at least {} second, and {} was given",
                the_minimum_gc_grace_secs(),
                self.gc_grace_secs
            )));
        }
        // A backend chosen is a backend that has to be configured. The
        // service does not fall back to the directory when the table is
        // missing: content split across two stores is content lost, and
        // the choice must be made on purpose, in the configuration.
        match (self.backend, self.s3.as_ref()) {
            (ContentBackend::S3, None) => Err(ConfigError::Message(
                "Invalid content configuration: backend = \"s3\" requires the [content.s3] table with a bucket; there is no fallback to the filesystem".to_string(),
            )),
            (ContentBackend::S3, Some(table)) if table.bucket.trim().is_empty() => {
                Err(ConfigError::Message(
                    "Invalid content configuration: [content.s3].bucket cannot be empty".to_string(),
                ))
            }
            _ => Ok(()),
        }
    }
}

impl Default for ContentConfig {
    fn default() -> Self {
        Self {
            cas_path: the_default_cas_path(),
            staging_max_age_secs: the_default_staging_max_age_secs(),
            gc_grace_secs: the_default_gc_grace_secs(),
            backend: ContentBackend::default(),
            s3: None,
        }
    }
}

/// The trust plane: which policy decides what may be promoted.
///
/// Promotion to the trusted channel is evaluated against a trust policy.
/// Without a policy file the service has nothing to evaluate with, and
/// every promotion is refused with a recorded decision: a missing policy
/// is never a permit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustConfig {
    /// A JSON file with the trust policy (`TrustPolicy`).
    ///
    /// Default: none.
    #[serde(default)]
    pub policy_file: Option<String>,
    /// A JSON object file mapping advisory HRN to severity
    /// (`"critical"` or `"other"`): the minimum catalog that lets the
    /// risk facts say which of the advisories reaching a subject block
    /// it. Without it no severity is known, no risk is established, and
    /// a policy that demands the risk grants nothing -- UNKNOWN is not
    /// zero.
    ///
    /// Default: none.
    #[serde(default)]
    pub risk_catalog_file: Option<String>,
    /// The identifier under which the policy is activated and requested.
    ///
    /// Default: `default`.
    #[serde(default = "the_default_trust_policy_id")]
    pub policy_id: String,
    /// Ceiling, in seconds, of the validity of a newly issued trust
    /// grant. The grant lives until the earliest expiry the decision
    /// demanded (an applied exception), but never past this ceiling:
    /// past it the bytes have to pass a new decision before the trusted
    /// channel keeps serving them. A deployment that wants slower
    /// revalidation raises it; nothing runs the channel unbounded.
    ///
    /// Default: 86400 (one day). Minimum: 1.
    #[serde(default = "the_default_grant_max_validity_secs")]
    pub grant_max_validity_secs: u64,
    /// Intervalo, en segundos, del barrido que reevalúa las concesiones
    /// cuya vigencia se agotó o cuya política ya no es la activa, y del
    /// consumo de deltas que materializa la vista de impacto (HSP-137).
    ///
    /// El camino de servicio ya rechaza la concesión vencida
    /// (HSP-135); el barrido es lo que hace que la verdad se reevalúe
    /// y se escriba, no solo que se note al leer. `0` desactiva el
    /// bucle: el ciclo sigue vivo por la entrada de advisories y el
    /// `resume` de arranque.
    ///
    /// Default: 60.
    #[serde(default = "the_default_trust_sweep_interval_secs")]
    pub sweep_interval_secs: u64,
}

impl Default for TrustConfig {
    fn default() -> Self {
        Self {
            policy_file: None,
            risk_catalog_file: None,
            policy_id: the_default_trust_policy_id(),
            grant_max_validity_secs: the_default_grant_max_validity_secs(),
            sweep_interval_secs: the_default_trust_sweep_interval_secs(),
        }
    }
}

fn the_default_trust_policy_id() -> String {
    "default".to_string()
}

fn the_default_grant_max_validity_secs() -> u64 {
    86_400
}

fn the_default_trust_sweep_interval_secs() -> u64 {
    60
}

/// The per-account quotas.
///
/// Not saying is not saying zero: every limit here is an `Option`, and
/// a deployment that writes nothing runs exactly as it ran before
/// quotas existed. A limit that is not written is a limit that is not
/// enforced, and that decision belongs to the operator, in the open.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaConfig {
    /// The most bytes one account may have stored.
    ///
    /// Default: none (unlimited).
    #[serde(default)]
    pub max_stored_bytes_per_account: Option<u64>,
    /// The most uploads of one account in flight at the same time.
    ///
    /// Default: none (unlimited).
    #[serde(default)]
    pub max_concurrent_uploads_per_account: Option<usize>,
    /// The seconds `Retry-After` carries when the concurrency gate
    /// refuses. A `0` here would be a `Retry-After` that says "now",
    /// which is not a hint, so the minimum is one.
    ///
    /// Default: 1.
    #[serde(default = "the_default_quota_retry_after_secs")]
    pub retry_after_secs: u64,
}

fn the_default_quota_retry_after_secs() -> u64 {
    1
}

impl Default for QuotaConfig {
    fn default() -> Self {
        Self {
            max_stored_bytes_per_account: None,
            max_concurrent_uploads_per_account: None,
            retry_after_secs: the_default_quota_retry_after_secs(),
        }
    }
}

impl QuotaConfig {
    /// Check that the values of the quota configuration make sense.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.max_stored_bytes_per_account == Some(0) {
            return Err(ConfigError::Message(
                "Invalid quota configuration: max_stored_bytes_per_account of zero is not a quota, it is a shutdown; say nothing instead".to_string(),
            ));
        }
        if self.retry_after_secs == 0 {
            return Err(ConfigError::Message(
                "Invalid quota configuration: retry_after_secs must be at least one second, and 0 was given".to_string(),
            ));
        }
        Ok(())
    }

    /// The same decision in the language of the ledger.
    pub fn the_limits(&self) -> crate::quota::QuotaLimits {
        crate::quota::QuotaLimits {
            max_stored_bytes_per_account: self.max_stored_bytes_per_account,
            max_concurrent_uploads_per_account: self.max_concurrent_uploads_per_account,
            retry_after_secs: self.retry_after_secs,
        }
    }
}

/// The egress of the remote proxy: where the service may fetch from.
///
/// A proxy repository names an upstream, but the repository configuration
/// is not what decides that the service may reach it: this list does. A
/// host that is not in `allowed_hosts` is never contacted, whatever the
/// repository says. The default list is empty, so a service with no
/// `[proxy]` section makes no outbound request at all.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// The hosts the service may fetch from.
    ///
    /// Default: none.
    #[serde(default)]
    pub allowed_hosts: Vec<crate::proxy::egress::AllowedHost>,
    /// The names of the environment variables a repository may use as the
    /// credential of its upstream (`credential_ref = "env:NAME"`).
    ///
    /// A repository only names a variable; this list is what lets the
    /// service read it. Default: none, so no credential is ever sent.
    #[serde(default)]
    pub allowed_credentials: Vec<String>,
    /// Seconds to wait for a connection and for a name to resolve.
    ///
    /// Default: 5.
    #[serde(default = "the_default_proxy_connect_timeout_secs")]
    pub connect_timeout_secs: u64,
    /// Seconds a whole resolution may last, redirects and body included.
    ///
    /// Default: 300.
    #[serde(default = "the_default_proxy_total_timeout_secs")]
    pub total_timeout_secs: u64,
    /// The most bytes a single fetch may bring. A repository may lower it,
    /// never raise it.
    ///
    /// Default: 1 GiB.
    #[serde(default = "the_default_proxy_max_bytes")]
    pub max_bytes: u64,
    /// The most fetches in flight at the same time.
    ///
    /// Default: 8.
    #[serde(default = "the_default_proxy_max_concurrent")]
    pub max_concurrent: usize,
    /// Whether the service refuses to reach any upstream.
    ///
    /// In offline mode a proxy repository serves only the content it
    /// already holds with a receipt that proves where it came from; any
    /// other path fails with an explicit error and no request leaves the
    /// service. A repository may also go offline on its own
    /// (`proxy.cache.offline`); this switch is the operator's and wins.
    ///
    /// Default: false.
    #[serde(default)]
    pub offline: bool,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            allowed_hosts: Vec::new(),
            allowed_credentials: Vec::new(),
            connect_timeout_secs: the_default_proxy_connect_timeout_secs(),
            total_timeout_secs: the_default_proxy_total_timeout_secs(),
            max_bytes: the_default_proxy_max_bytes(),
            max_concurrent: the_default_proxy_max_concurrent(),
            offline: false,
        }
    }
}

fn the_default_proxy_connect_timeout_secs() -> u64 {
    5
}

fn the_default_proxy_total_timeout_secs() -> u64 {
    300
}

fn the_default_proxy_max_bytes() -> u64 {
    1 << 30
}

fn the_default_proxy_max_concurrent() -> usize {
    8
}

/// The default directory of the content.
fn the_default_cas_path() -> String {
    "./data/hodei.cas".to_string()
}

/// The default age of an interrupted write that the store cleans up.
fn the_default_staging_max_age_secs() -> u64 {
    3600
}

/// The default grace of an unreferenced blob before it is reclaimable.
fn the_default_gc_grace_secs() -> u64 {
    86_400
}

/// The smallest grace the collection accepts: zero would make a blob that
/// is being published reclaimable at the instant it arrives.
fn the_minimum_gc_grace_secs() -> u64 {
    1
}

/// The default life of an upload session that nobody finishes.
fn default_oci_upload_ttl_secs() -> u64 {
    3600
}

/// The default number of upload sessions alive at the same time.
fn default_oci_max_concurrent_uploads() -> usize {
    64
}

/// The smallest age that still is a margin of prudence.
///
/// Below a minute, the age of a staging file says nothing: the file is
/// either being written right now or it was left behind a moment ago, and
/// the two are told apart by the lock, not by the date.
fn the_minimum_staging_max_age_secs() -> u64 {
    60
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    /// Server configuration
    pub server: ServerConfig,

    /// Security configuration
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub repository: RepositoryConfig,

    /// Where the content of the artifacts lives.
    #[serde(default)]
    pub content: ContentConfig,

    /// The limits of the investigation surface.
    #[serde(default)]
    pub investigation: InvestigationConfig,

    /// The trust policy that decides promotions to the trusted channel.
    #[serde(default)]
    pub trust: TrustConfig,

    /// The egress of the remote proxy.
    #[serde(default)]
    pub proxy: ProxyConfig,

    /// The per-account quotas: what one account may store and how much
    /// it may do at once.
    #[serde(default)]
    pub quota: QuotaConfig,

    /// Database configuration
    pub database: DatabaseConfig,

    /// Schema configuration
    pub schema: SchemaConfig,

    /// Logging configuration
    pub logging: LoggingConfig,

    /// RocksDB specific configuration
    pub rocksdb: RocksDbConfig,
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Host to bind to (default: 0.0.0.0)
    pub host: String,

    /// Port to bind to (default: 3000)
    pub port: u16,

    /// Request timeout in seconds (default: 30)
    pub request_timeout_secs: u64,

    /// Maximum request body size in bytes (default: 10MB)
    pub max_body_size: usize,
}

/// Security configuration
///
/// The administrative API is closed by default. `admin_api_enabled` only
/// opens it when the application state has both an authenticator and an
/// authorizer; every request then needs a bearer credential and a Cedar
/// permit.
#[derive(Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Whether the `/api/v1` administrative surface is reachable.
    ///
    /// Default: `false`. When false every administrative route is answered
    /// with `403 Forbidden` before any handler runs. The repository routes have
    /// their own credential gate. CORS preflight is answered by the outer CORS
    /// layer and never reaches a handler; unknown routes remain `404`.
    #[serde(default)]
    pub admin_api_enabled: bool,

    /// Whether the raw content surface is reachable.
    ///
    /// Default: `false`. The surface is closed by default for the same
    /// reason the administrative one is: the Cedar bundle has to declare
    /// the `UploadAsset` and `DownloadAsset` actions, and a bundle that
    /// does not declare them cannot grant them. Turning this on without a
    /// bundle that declares them is a deployment that says "yes" to
    /// nothing.
    #[serde(default)]
    pub raw_api_enabled: bool,
    /// Whether the OCI distribution pull surface (`/v2`) is served.
    ///
    /// Same shape as `raw_api_enabled`: the surface has its own credential
    /// and authorization gate, and the default denies before any handler
    /// runs.
    #[serde(default)]
    pub oci_api_enabled: bool,

    /// Whether the investigation surface (`/api/v1/investigations`) is served.
    ///
    /// Same shape as `raw_api_enabled`: the surface asks for its own Cedar
    /// action, and a bundle that does not declare it cannot grant it. The
    /// default denies before any handler runs, and turning this on without
    /// a bundle that declares the action is a deployment that says "yes" to
    /// nothing.
    #[serde(default)]
    pub investigation_api_enabled: bool,

    /// How long an upload session lives without being touched, in seconds.
    ///
    /// The protocol of push is by chunks, and a session that nobody
    /// finishes is a session that holds a staging file on disk. The
    /// session is *forgettable*: after this time it is not there, and the
    /// client gets an explicit error instead of a upload that never ends.
    #[serde(default = "default_oci_upload_ttl_secs")]
    pub oci_upload_ttl_secs: u64,

    /// How many upload sessions can be alive at the same time.
    ///
    /// A cap, and not a queue: a client that opens sessions and abandons
    /// them cannot fill the registry.
    #[serde(default = "default_oci_max_concurrent_uploads")]
    pub oci_max_concurrent_uploads: usize,

    /// Browser origins allowed by CORS.
    ///
    /// Default: empty, which means no cross-origin access. Wildcards are
    /// rejected so the permissive policy cannot be reintroduced by
    /// configuration.
    #[serde(default)]
    pub cors_allowed_origins: Vec<String>,

    /// HS256 secret or a JWKS file for production JWTs (exactly one).
    #[serde(default, skip_serializing)]
    pub auth_jwt_secret: Option<String>,
    /// Local JWKS used for RS256/ES256 tokens. Never logged or serialized.
    #[serde(default, skip_serializing)]
    pub auth_jwks_path: Option<String>,
    /// Expected JWT issuer.
    #[serde(default)]
    pub auth_issuer: Option<String>,
    /// Expected JWT audience.
    #[serde(default)]
    pub auth_audience: Option<String>,
    /// Cedar policy bundle for production authorization.
    #[serde(default)]
    pub authorization_policy_path: Option<String>,
    /// Cedar schema used to validate the policy bundle.
    #[serde(default)]
    pub authorization_schema_path: Option<String>,
}

/// El valor por defecto de la configuracion de seguridad.
///
/// Todo cerrado y sin credenciales: un despliegue que no dice nada no
/// dice que si a nada. Y los limites de la superficie OCI tienen sus
/// valores, porque un limite que vale cero no es un limite: es una
/// superficie que no funciona.
impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            admin_api_enabled: false,
            raw_api_enabled: false,
            oci_api_enabled: false,
            investigation_api_enabled: false,
            oci_upload_ttl_secs: default_oci_upload_ttl_secs(),
            oci_max_concurrent_uploads: default_oci_max_concurrent_uploads(),
            cors_allowed_origins: Vec::new(),
            auth_jwt_secret: None,
            auth_jwks_path: None,
            auth_issuer: None,
            auth_audience: None,
            authorization_policy_path: None,
            authorization_schema_path: None,
        }
    }
}

impl std::fmt::Debug for SecurityConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SecurityConfig")
            .field("admin_api_enabled", &self.admin_api_enabled)
            .field("cors_allowed_origins", &self.cors_allowed_origins)
            .field(
                "auth_jwt_secret",
                &self.auth_jwt_secret.as_ref().map(|_| "<redacted>"),
            )
            .field("auth_jwks_path", &self.auth_jwks_path)
            .field("auth_issuer", &self.auth_issuer)
            .field("auth_audience", &self.auth_audience)
            .field("authorization_policy_path", &self.authorization_policy_path)
            .field("authorization_schema_path", &self.authorization_schema_path)
            .finish()
    }
}

impl SecurityConfig {
    /// Validate only the production auth/authorization bundle.
    pub fn validate_production_auth(&self) -> Result<(), ConfigError> {
        let auth_any = self.auth_jwt_secret.is_some()
            || self.auth_jwks_path.is_some()
            || self.auth_issuer.is_some()
            || self.auth_audience.is_some()
            || self.authorization_policy_path.is_some()
            || self.authorization_schema_path.is_some();
        if auth_any {
            if self.auth_jwt_secret.is_some() == self.auth_jwks_path.is_some() {
                return Err(ConfigError::Message(
                    "Production security requires exactly one of auth_jwt_secret or auth_jwks_path"
                        .to_string(),
                ));
            }
            if self.auth_issuer.is_none()
                || self.auth_audience.is_none()
                || self.authorization_policy_path.is_none()
                || self.authorization_schema_path.is_none()
            {
                return Err(ConfigError::Message(
                    "Production security requires auth_issuer, auth_audience, authorization_policy_path and authorization_schema_path together".to_string(),
                ));
            }
        }
        if self.auth_issuer.as_deref().is_some_and(str::is_empty)
            || self.auth_audience.as_deref().is_some_and(str::is_empty)
            || self.auth_jwt_secret.as_deref().is_some_and(str::is_empty)
            || self.auth_jwks_path.as_deref().is_some_and(str::is_empty)
        {
            return Err(ConfigError::Message(
                "Production auth issuer, audience, JWT secret and JWKS path cannot be empty"
                    .to_string(),
            ));
        }
        Ok(())
    }

    /// Validate the full security configuration.
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.validate_production_auth()?;
        for origin in &self.cors_allowed_origins {
            if origin.trim().is_empty() {
                return Err(ConfigError::Message(
                    "CORS origin cannot be empty. Set HODEI_SECURITY__CORS_ALLOWED_ORIGINS or remove the entry".to_string(),
                ));
            }
            if origin.trim() == "*" {
                return Err(ConfigError::Message(
                    "CORS wildcard is not allowed. List explicit origins instead of '*'"
                        .to_string(),
                ));
            }
            if origin != &origin.to_ascii_lowercase() {
                return Err(ConfigError::Message(format!(
                    "Invalid CORS origin '{}'. Use lowercase scheme and host",
                    origin
                )));
            }
            if origin.contains('#') {
                return Err(ConfigError::Message(format!(
                    "Invalid CORS origin '{}'. Origins cannot contain a fragment",
                    origin
                )));
            }
            let uri: axum::http::Uri = origin.parse().map_err(|_| {
                ConfigError::Message(format!(
                    "Invalid CORS origin '{}'. Use a valid origin such as https://app.example",
                    origin
                ))
            })?;
            let default_port = match uri.scheme_str() {
                Some("http") => uri.port_u16() == Some(80),
                Some("https") => uri.port_u16() == Some(443),
                _ => false,
            };
            let non_canonical_port = uri.authority().is_some_and(|authority| {
                authority
                    .as_str()
                    .rsplit_once(':')
                    .is_some_and(|(_, port)| port.len() > 1 && port.starts_with('0'))
            });
            let valid_authority = uri
                .authority()
                .is_some_and(|authority| !authority.as_str().contains('@'))
                && uri.host().is_some_and(|host| !host.contains('*'))
                && matches!(uri.scheme_str(), Some("http" | "https"))
                && (uri.path().is_empty() || uri.path() == "/")
                && uri.query().is_none()
                && !default_port
                && !non_canonical_port;
            if !valid_authority {
                return Err(ConfigError::Message(format!(
                    "Invalid CORS origin '{}'. Use scheme and authority only, such as https://app.example",
                    origin
                )));
            }
            HeaderValue::from_str(origin).map_err(|_| {
                ConfigError::Message(format!(
                    "Invalid CORS origin '{}'. Use a valid origin such as https://app.example",
                    origin
                ))
            })?;
        }
        Ok(())
    }
}

/// Database configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Database type (only "rocksdb" supported)
    pub db_type: String,

    /// Database namespace
    pub namespace: Option<String>,

    /// Database name
    pub database: Option<String>,

    /// Connection pool size (default: 10)
    pub pool_size: u32,
}

/// Schema configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaConfig {
    /// Whether to register IAM schema on startup (default: false)
    pub register_iam_on_startup: bool,

    /// Specific schema version to use (optional)
    pub version: Option<String>,

    /// Whether to validate schemas after building (default: true)
    pub validate: bool,

    /// Schema storage type (default: "rocksdb")
    pub storage_type: String,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level (default: "info")
    /// Valid values: "trace", "debug", "info", "warn", "error"
    pub level: String,

    /// Log format (default: "pretty")
    /// Valid values: "pretty", "json", "compact"
    pub format: String,

    /// Whether to include timestamps (default: true)
    pub include_timestamps: bool,

    /// Whether to include file/line information (default: false)
    pub include_location: bool,
}

/// RocksDB specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RocksDbConfig {
    /// Path to RocksDB database file (default: "./data/hodei.rocksdb")
    pub path: String,

    /// Create database if it doesn't exist (default: true)
    pub create_if_missing: bool,

    /// Enable compression (default: true)
    pub compression: bool,

    /// Maximum number of open files (default: 1000)
    pub max_open_files: i32,

    /// Write buffer size in bytes (default: 64MB)
    pub write_buffer_size: usize,
}

// Default derived for AppConfig

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 3000,
            request_timeout_secs: 30,
            max_body_size: 10 * 1024 * 1024, // 10MB
        }
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            db_type: "rocksdb".to_string(),
            namespace: Some("hodei".to_string()),
            database: Some("artifacts".to_string()),
            pool_size: 10,
        }
    }
}

impl Default for SchemaConfig {
    fn default() -> Self {
        Self {
            register_iam_on_startup: false,
            version: None,
            validate: true,
            storage_type: "rocksdb".to_string(),
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: "pretty".to_string(),
            include_timestamps: true,
            include_location: false,
        }
    }
}

impl Default for RocksDbConfig {
    fn default() -> Self {
        Self {
            path: "./target/debug/data/hodei.rocksdb".to_string(),
            create_if_missing: true,
            compression: true,
            max_open_files: 1000,
            write_buffer_size: 64 * 1024 * 1024, // 64MB
        }
    }
}

impl AppConfig {
    /// Load configuration from multiple sources with hierarchical precedence
    ///
    /// Sources (in order of precedence, highest first):
    /// 1. Environment variables (HODEI_ prefix)
    /// 2. config/local.toml
    /// 3. config/{RUN_MODE}.toml
    /// 4. config/default.toml
    /// 5. Hardcoded defaults
    ///
    /// # Returns
    ///
    /// A validated AppConfig instance or a ConfigError with clear messages
    pub fn new() -> Result<Self, ConfigError> {
        build_config(environment_source())
    }

    /// Validate the entire configuration
    ///
    /// # Returns
    ///
    /// Ok(()) if configuration is valid, ConfigError with clear message if invalid
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.server.validate()?;
        self.security.validate()?;
        self.database.validate()?;
        self.rocksdb.validate()?;
        self.logging.validate()?;
        self.content.validate()?;
        self.investigation.validate()?;
        self.quota.validate()?;
        Ok(())
    }

    /// Check that the storage the configuration names can actually be used.
    ///
    /// This is the infrastructure preflight, and it is the part that touches
    /// the filesystem. It runs when the service starts, not when somebody
    /// validates a configuration, and its failures say that the problem is
    /// the storage and not the configuration.
    pub fn preflight_storage(&self) -> Result<(), ConfigError> {
        self.rocksdb.preflight_storage()
    }

    /// Get the server bind address
    ///
    /// Returns a string in the format "host:port"
    pub fn server_address(&self) -> String {
        format!("{}:{}", self.server.host, self.server.port)
    }
}

/// The environment source that overrides the configuration.
///
/// The prefix separator has to be stated explicitly. Without it, config-rs
/// falls back to the *key* separator when one is given, so the prefix would
/// be looked for as `HODEI__` instead of `HODEI_`, and every
/// `HODEI_<SECTION>__<KEY>` variable would be **silently discarded**: the
/// documented override would be inert and the service would always boot with
/// the values from the TOML file.
///
/// `try_parsing` makes the collected values be parsed as numbers and booleans
/// instead of staying strings, which is what the typed configuration needs.
fn environment_source() -> Environment {
    Environment::with_prefix("HODEI")
        .prefix_separator("_")
        .separator("__")
        .try_parsing(true)
        .list_separator(",")
        .with_list_parse_key("security.cors_allowed_origins")
        .with_list_parse_key("proxy.allowed_hosts")
        .with_list_parse_key("proxy.allowed_credentials")
}

/// Builds the configuration from the file sources and the environment source.
///
/// The environment source is a parameter so that the precedence rules can be
/// exercised without mutating the process environment, which is shared by
/// every test running in the same binary.
fn build_config(environment: Environment) -> Result<AppConfig, ConfigError> {
    let run_mode = env::var("RUN_MODE").unwrap_or_else(|_| "development".into());

    let s = Config::builder()
        // Default values (lowest precedence)
        .add_source(File::with_name("config/default").required(false))
        // Environment-specific configuration
        .add_source(File::with_name(&format!("config/{}", run_mode)).required(false))
        // Local overrides (higher precedence)
        .add_source(File::with_name("config/local").required(false))
        // Environment variables with HODEI_ prefix (highest precedence)
        .add_source(environment)
        .build()?;

    let app_config: AppConfig = s.try_deserialize()?;

    // Validate the configuration
    app_config.validate()?;

    Ok(app_config)
}

impl ServerConfig {
    /// Validate server configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.port == 0 {
            return Err(ConfigError::Message(
                "Server port cannot be 0. Please set HODEI_SERVER__PORT to a valid port number (1-65535)".to_string()
            ));
        }

        if self.host.is_empty() {
            return Err(ConfigError::Message(
                "Server host cannot be empty. Please set HODEI_SERVER__HOST".to_string(),
            ));
        }

        if self.request_timeout_secs == 0 {
            return Err(ConfigError::Message(
                "Request timeout cannot be 0. Please set HODEI_SERVER__REQUEST_TIMEOUT_SECS to a positive value".to_string()
            ));
        }

        if self.max_body_size == 0 {
            return Err(ConfigError::Message(
                "Max body size cannot be 0. Please set HODEI_SERVER__MAX_BODY_SIZE to a positive value".to_string()
            ));
        }

        Ok(())
    }
}

impl DatabaseConfig {
    /// Validate database configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.db_type != "rocksdb" {
            return Err(ConfigError::Message(format!(
                "Unsupported database type '{}'. Only 'rocksdb' is supported. Please set HODEI_DATABASE__DB_TYPE to 'rocksdb'",
                self.db_type
            )));
        }

        if self.namespace.is_none() || self.namespace.as_ref().unwrap().is_empty() {
            return Err(ConfigError::Message(
                "Database namespace cannot be empty. Please set HODEI_DATABASE__NAMESPACE"
                    .to_string(),
            ));
        }

        if self.database.is_none() || self.database.as_ref().unwrap().is_empty() {
            return Err(ConfigError::Message(
                "Database name cannot be empty. Please set HODEI_DATABASE__DATABASE".to_string(),
            ));
        }

        if self.pool_size == 0 {
            return Err(ConfigError::Message(
                "Database pool size cannot be 0. Please set HODEI_DATABASE__POOL_SIZE to a positive value".to_string()
            ));
        }

        Ok(())
    }
}

impl RocksDbConfig {
    /// Validate the RocksDB configuration.
    ///
    /// This check is pure: it looks at the configuration and nothing else. It
    /// does not touch the filesystem, so validating a configuration twice
    /// gives the same answer twice, and validating one never leaves a
    /// directory behind.
    ///
    /// Whether the path can actually be used is a question about the machine,
    /// not about the configuration, and it is answered by
    /// `preflight_storage`.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.path.is_empty() {
            return Err(ConfigError::Message(
                "RocksDB path cannot be empty. Please set HODEI_ROCKSDB__PATH to a valid file path"
                    .to_string(),
            ));
        }

        if self.max_open_files <= 0 {
            return Err(ConfigError::Message(
                "RocksDB max_open_files must be positive. Please set HODEI_ROCKSDB__MAX_OPEN_FILES to a value > 0".to_string()
            ));
        }

        if self.write_buffer_size == 0 {
            return Err(ConfigError::Message(
                "RocksDB write_buffer_size cannot be 0. Please set HODEI_ROCKSDB__WRITE_BUFFER_SIZE to a positive value".to_string()
            ));
        }

        Ok(())
    }

    /// Check that the storage the configuration names can actually be used.
    ///
    /// This is the infrastructure preflight: it touches the filesystem, it
    /// creates the directory when the configuration says it may, and it
    /// reports what is wrong with the machine -- not with the configuration.
    /// A service that cannot write where it was told to write says so here,
    /// before it starts accepting requests.
    pub fn preflight_storage(&self) -> Result<(), ConfigError> {
        // RocksDB path is actually a directory that contains the database files
        let path = std::path::Path::new(&self.path);

        if path.exists() {
            // Case 1: Path already exists
            // It must be a directory (RocksDB creates and uses directories)
            if !path.is_dir() {
                return Err(ConfigError::Message(format!(
                    "RocksDB path '{}' exists but is not a directory. RocksDB requires a directory path. Please remove this file or choose a different path.",
                    self.path
                )));
            }

            // Verify the directory is writable
            let test_file = path.join(".hodei_test_write");
            match std::fs::write(&test_file, "test") {
                Ok(_) => {
                    // Clean up test file
                    let _ = std::fs::remove_file(test_file);
                }
                Err(e) => {
                    return Err(ConfigError::Message(format!(
                        "RocksDB directory '{}' exists but is not writable: {}. Please check permissions.",
                        path.display(),
                        e
                    )));
                }
            }
        } else {
            // Case 2: Path doesn't exist yet (will be created by RocksDB)
            // Validate that we can create it by checking parent directory
            let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));

            if !parent.exists() {
                // Parent doesn't exist, try to create it
                if self.create_if_missing {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        return Err(ConfigError::Message(format!(
                            "Cannot create parent directory '{}' for RocksDB: {}. Please check permissions or set HODEI_ROCKSDB__PATH to a writable location.",
                            parent.display(),
                            e
                        )));
                    }
                } else {
                    return Err(ConfigError::Message(format!(
                        "Parent directory '{}' does not exist and create_if_missing is false. Please create the directory manually or set create_if_missing=true.",
                        parent.display()
                    )));
                }
            } else {
                // Parent exists, verify it's writable
                let test_file = parent.join(".hodei_test_write");
                match std::fs::write(&test_file, "test") {
                    Ok(_) => {
                        // Clean up test file
                        let _ = std::fs::remove_file(test_file);
                    }
                    Err(e) => {
                        return Err(ConfigError::Message(format!(
                            "Parent directory '{}' is not writable: {}. Please check permissions.",
                            parent.display(),
                            e
                        )));
                    }
                }
            }
        }

        Ok(())
    }
}

impl LoggingConfig {
    /// Validate logging configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        let valid_levels = ["trace", "debug", "info", "warn", "error"];
        if !valid_levels.contains(&self.level.as_str()) {
            return Err(ConfigError::Message(format!(
                "Invalid log level '{}'. Valid values: {}. Please set HODEI_LOGGING__LEVEL to one of these",
                self.level,
                valid_levels.join(", ")
            )));
        }

        let valid_formats = ["pretty", "json", "compact"];
        if !valid_formats.contains(&self.format.as_str()) {
            return Err(ConfigError::Message(format!(
                "Invalid log format '{}'. Valid values: {}. Please set HODEI_LOGGING__FORMAT to one of these",
                self.format,
                valid_formats.join(", ")
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The environment variables that must override the file configuration.
    ///
    /// The values are injected into the environment source instead of the
    /// process environment, so the test does not depend on the order in
    /// which the tests of this binary run.
    fn overrides() -> std::collections::HashMap<String, String> {
        [
            ("HODEI_SERVER__PORT", "41535"),
            ("HODEI_SERVER__HOST", "127.0.0.1"),
            ("HODEI_DATABASE__NAMESPACE", "hodei"),
            ("HODEI_DATABASE__DATABASE", "artifacts"),
            ("HODEI_ROCKSDB__PATH", "/tmp/hodei-config-override.rocksdb"),
            ("HODEI_SCHEMA__REGISTER_IAM_ON_STARTUP", "true"),
            ("HODEI_LOGGING__LEVEL", "warn"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    /// The documented environment override has to reach the configuration
    ///
    /// `HODEI_<SECTION>__<KEY>` is the documented way to configure the
    /// service per environment, and it is advertised by the startup error
    /// message. It used to be inert: config-rs looked for the prefix as
    /// `HODEI__` and discarded every variable, so the service always booted
    /// with the values from the TOML file.
    #[test]
    fn environment_variables_override_the_configuration() {
        let config = build_config(environment_source().source(Some(overrides())))
            .expect("the configuration must be loadable");

        assert_eq!(
            config.server.port, 41535,
            "HODEI_SERVER__PORT must override the configured port"
        );
        assert_eq!(
            config.server.host, "127.0.0.1",
            "HODEI_SERVER__HOST must override the configured host"
        );
        assert_eq!(
            config.rocksdb.path, "/tmp/hodei-config-override.rocksdb",
            "HODEI_ROCKSDB__PATH must override the configured database path"
        );
        assert_eq!(
            config.database.namespace.as_deref(),
            Some("hodei"),
            "HODEI_DATABASE__NAMESPACE must override the configured namespace"
        );
        assert!(
            config.schema.register_iam_on_startup,
            "HODEI_SCHEMA__REGISTER_IAM_ON_STARTUP must override the configured flag"
        );
        assert_eq!(
            config.logging.level, "warn",
            "HODEI_LOGGING__LEVEL must override the configured log level"
        );
    }

    /// An invalid environment override has to be rejected, not ignored
    ///
    /// An override that cannot be parsed must not be silently dropped: a
    /// service that boots with a configuration nobody asked for is worse
    /// than one that refuses to boot.
    #[test]
    fn an_invalid_environment_override_is_rejected() {
        let mut invalid = overrides();
        invalid.insert("HODEI_SERVER__PORT".to_string(), "not-a-port".to_string());

        let result = build_config(environment_source().source(Some(invalid)));

        assert!(
            result.is_err(),
            "an environment override that cannot be parsed must be rejected"
        );
    }

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.port, 3000);
        assert_eq!(config.database.db_type, "rocksdb");
        assert!(!config.schema.register_iam_on_startup);
        assert_eq!(config.logging.level, "info");
        assert_eq!(config.rocksdb.path, "./target/debug/data/hodei.rocksdb");
    }

    /// The staging of an interrupted upload is cleaned up, but not too soon.
    ///
    /// The default has to be generous: the staging of a **live** upload
    /// belongs to another process, and taking it away breaks an upload
    /// that is working.
    #[test]
    fn the_staging_of_an_interrupted_upload_expires_but_not_at_once() {
        let config = AppConfig::default();
        assert_eq!(config.content.staging_max_age_secs, 3600);
        assert!(config.content.staging_max_age_secs > 0);
    }

    /// The age of the staging has a floor.
    ///
    /// Below a minute the age stops being a margin of prudence and starts
    /// being a way of deleting what other processes are writing, so a
    /// configuration that asks for it is rejected instead of obeyed.
    #[test]
    fn the_age_of_the_staging_has_a_floor() {
        let mut config = AppConfig::default();
        config.content.staging_max_age_secs = 0;
        let error = config.validate().expect_err("an age of zero is refused");
        assert!(
            error.to_string().contains("staging_max_age_secs"),
            "the error says what is wrong: {error}"
        );

        let mut config = AppConfig::default();
        config.content.staging_max_age_secs = 59;
        assert!(
            config.validate().is_err(),
            "an age below the floor is refused"
        );

        let mut config = AppConfig::default();
        config.content.staging_max_age_secs = 60;
        assert!(config.validate().is_ok(), "the floor itself is allowed");
    }

    #[test]
    fn test_config_validation() {
        let config = AppConfig::default();
        assert!(config.validate().is_ok());

        let mut invalid_config = AppConfig::default();
        invalid_config.server.port = 0;
        assert!(invalid_config.validate().is_err());

        let mut invalid_config = AppConfig::default();
        invalid_config.database.db_type = "postgres".to_string();
        assert!(invalid_config.validate().is_err());

        let mut invalid_config = AppConfig::default();
        invalid_config.logging.level = "invalid".to_string();
        assert!(invalid_config.validate().is_err());
    }

    #[test]
    fn security_defaults_are_closed() {
        let config = AppConfig::default();
        assert!(
            !config.security.admin_api_enabled,
            "the administrative surface must be closed by default"
        );
        assert!(
            config.security.cors_allowed_origins.is_empty(),
            "no browser origin is allowed by default"
        );
    }

    #[test]
    fn security_rejects_permissive_cors_configuration() {
        let mut config = AppConfig::default();
        config.security.cors_allowed_origins = vec!["*".to_string()];
        assert!(config.validate().is_err());

        config.security.cors_allowed_origins = vec!["".to_string()];
        assert!(config.validate().is_err());

        config.security.cors_allowed_origins = vec!["not an origin".to_string()];
        assert!(config.validate().is_err());

        config.security.cors_allowed_origins = vec!["https://app.example/".to_string()];
        assert!(config.validate().is_ok());

        config.security.cors_allowed_origins = vec!["https://user@app.example".to_string()];
        assert!(config.validate().is_err());

        config.security.cors_allowed_origins = vec!["HTTPS://app.example".to_string()];
        assert!(config.validate().is_err());

        config.security.cors_allowed_origins = vec!["https://*.example.com".to_string()];
        assert!(config.validate().is_err());

        config.security.cors_allowed_origins = vec!["https://app.example:443".to_string()];
        assert!(config.validate().is_err());

        config.security.cors_allowed_origins = vec!["https://app.example#fragment".to_string()];
        assert!(config.validate().is_err());

        config.security.cors_allowed_origins = vec!["https://app.example:08443".to_string()];
        assert!(config.validate().is_err());

        config.security.cors_allowed_origins = vec!["https://app.example".to_string()];
        assert!(config.validate().is_ok());

        config.security.cors_allowed_origins = vec![
            "http://localhost:8080".to_string(),
            "https://[::1]:8443".to_string(),
            "https://app.example:8443".to_string(),
        ];
        assert!(config.validate().is_ok());
    }

    #[test]
    fn security_environment_overrides_are_honored() {
        let mut overrides = overrides();
        overrides.insert(
            "HODEI_SECURITY__ADMIN_API_ENABLED".to_string(),
            "true".to_string(),
        );
        overrides.insert(
            "HODEI_SECURITY__CORS_ALLOWED_ORIGINS".to_string(),
            "https://app.example,https://admin.example".to_string(),
        );

        let config = build_config(environment_source().source(Some(overrides)))
            .expect("security environment overrides must parse");

        assert!(config.security.admin_api_enabled);
        assert_eq!(
            config.security.cors_allowed_origins,
            vec![
                "https://app.example".to_string(),
                "https://admin.example".to_string()
            ]
        );
    }

    #[test]
    fn deployed_config_files_keep_the_admin_surface_closed() {
        for name in ["config/default", "config/development", "config/release"] {
            let config: AppConfig = Config::builder()
                .add_source(File::with_name(name))
                .build()
                .expect("config file must load")
                .try_deserialize()
                .expect("config file must deserialize");
            assert!(
                !config.security.admin_api_enabled,
                "{name} must keep the administrative surface closed"
            );
            assert!(
                config.security.cors_allowed_origins.is_empty(),
                "{name} must not allow browser origins by default"
            );
        }
    }

    #[test]
    fn test_server_address() {
        let config = AppConfig::default();
        assert_eq!(config.server_address(), "0.0.0.0:3000");

        let mut config = AppConfig::default();
        config.server.host = "127.0.0.1".to_string();
        config.server.port = 8080;
        assert_eq!(config.server_address(), "127.0.0.1:8080");
    }

    #[test]
    fn test_rocksdb_validation() {
        let config = RocksDbConfig::default();
        assert!(config.validate().is_ok());

        let invalid_config = RocksDbConfig {
            path: "".to_string(),
            ..Default::default()
        };
        assert!(invalid_config.validate().is_err());

        let invalid_config = RocksDbConfig {
            max_open_files: 0,
            ..Default::default()
        };
        assert!(invalid_config.validate().is_err());
    }

    /// The validation does not touch the filesystem.
    ///
    /// Validating a configuration is a question about the configuration. When
    /// the answer depended on the machine, two validations of the same
    /// configuration could disagree, and validating one left directories
    /// behind.
    #[test]
    fn the_validation_does_not_touch_the_filesystem() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("not-created");

        let config = RocksDbConfig {
            path: missing.to_string_lossy().into_owned(),
            ..Default::default()
        };

        // The path does not exist and the validation says nothing about it:
        // the shape of the configuration is valid.
        assert!(
            config.validate().is_ok(),
            "a path that does not exist yet is a valid configuration"
        );
        assert!(
            !missing.exists(),
            "the validation must not create the directory"
        );

        // And it leaves nothing behind in the parent either.
        let entries = std::fs::read_dir(tmp.path()).unwrap().count();
        assert_eq!(entries, 0, "the validation must not write anything");
    }

    /// The preflight is the part that talks to the machine.
    ///
    /// It creates the directory when the configuration allows it, and it says
    /// that the problem is the storage when the storage is the problem.
    #[test]
    fn the_preflight_creates_the_storage_and_reports_the_storage_problem() {
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().join("created-by-the-preflight");
        let database = parent.join("db");

        let config = RocksDbConfig {
            path: database.to_string_lossy().into_owned(),
            create_if_missing: true,
            ..Default::default()
        };

        config
            .preflight_storage()
            .expect("the preflight can create it");
        assert!(
            parent.exists(),
            "the preflight creates the directory the database will live in"
        );

        // A path that is a file, and not a directory, is a storage problem.
        let file = tmp.path().join("a-file");
        std::fs::write(&file, "not a directory").unwrap();
        let config = RocksDbConfig {
            path: file.to_string_lossy().into_owned(),
            ..Default::default()
        };

        let error = config
            .preflight_storage()
            .expect_err("a file is not a valid storage path");
        assert!(
            error.to_string().contains("not a directory"),
            "the error must say what is wrong with the storage: {error}"
        );
    }

    /// The content table read as the service would read it from a TOML.
    fn the_content_from(toml: &str) -> ContentConfig {
        let built = ::config::Config::builder()
            .add_source(::config::File::from_str(toml, ::config::FileFormat::Toml))
            .build()
            .expect("the table is valid TOML");
        built
            .get::<ContentConfig>("content")
            .expect("the table deserializes into the content configuration")
    }

    /// A deployment that says nothing keeps what it had: the directory.
    /// The backend is a decision of the deployment, and the default of
    /// the configuration is the backend of every deployment so far.
    #[test]
    fn the_backend_of_a_table_that_says_nothing_is_the_directory() {
        let content = the_content_from("[content]\ncas_path = \"/srv/cas\"\n");
        assert_eq!(content.backend, ContentBackend::Filesystem);
        assert_eq!(content.s3, None);
        content.validate().expect("the directory needs no table");
    }

    /// The chosen backend is read from the table, with its whole shape.
    #[test]
    fn the_s3_backend_is_read_with_its_bucket_and_its_reach() {
        let content = the_content_from(
            r#"
[content]
backend = "s3"
[content.s3]
bucket = "hodei-content"
endpoint = "s3.eu-west-1.amazonaws.com"
region = "eu-west-1"
"#,
        );
        assert_eq!(content.backend, ContentBackend::S3);
        let table = content.s3.as_ref().expect("the table was given");
        assert_eq!(table.bucket, "hodei-content");
        assert_eq!(
            table.endpoint.as_deref(),
            Some("s3.eu-west-1.amazonaws.com")
        );
        assert_eq!(table.region.as_deref(), Some("eu-west-1"));
        // TLS is on unless the deployment says otherwise: turning it
        // off is the dangerous direction, so the safe one is the default.
        assert!(table.tls, "tls defaults to on");
        content.validate().expect("a full table is a valid choice");
    }

    /// AC1 of the backend selection, from the configuration side:
    /// choosing S3 without saying which bucket is refused, and the
    /// refusal names the absence -- there is no fallback to hide behind.
    #[test]
    fn choosing_s3_without_its_table_is_refused_not_falled_back() {
        let content = the_content_from("[content]\nbackend = \"s3\"\n");
        assert_eq!(content.backend, ContentBackend::S3);
        let error = content
            .validate()
            .expect_err("s3 without a bucket cannot boot");
        let said = error.to_string();
        assert!(
            said.contains("no fallback"),
            "the error must say there is no fallback: {said}"
        );
    }

    /// The table can be present and still not name anything: an empty
    /// bucket is the same split of the content in two places, only
    /// noisier. It is refused with the same clarity.
    #[test]
    fn an_s3_table_without_a_bucket_is_refused() {
        let content = the_content_from(
            r#"
[content]
backend = "s3"
[content.s3]
bucket = "  "
"#,
        );
        let error = content
            .validate()
            .expect_err("a bucket of spaces is no bucket");
        assert!(
            error.to_string().contains("bucket"),
            "the error must name the field: {error}"
        );
    }

    /// An absent table under a directory backend is fine even if it
    /// would be nonsense: the deployment chose the directory, and the
    /// s3 table it did not choose stays ignored.
    #[test]
    fn the_s3_table_under_a_directory_backend_does_no_harm() {
        let content = the_content_from(
            r#"
[content]
backend = "filesystem"
[content.s3]
bucket = "hodei-content"
"#,
        );
        content
            .validate()
            .expect("the directory does not care about a table it will not use");
    }

    /// The documented env override reaches the new discriminator. The
    /// value is not a string the code has to special-case: it is the
    /// same lowercase name the TOML uses, parsed by the same serde.
    #[test]
    fn the_backend_can_be_chosen_by_the_environment() {
        let mut given = overrides();
        given.insert("HODEI_CONTENT__BACKEND".to_string(), "s3".to_string());
        // The full config validation runs inside `build_config`, and an
        // s3 backend without its table has to refuse the boot -- which
        // is the same proof, from the other side: the override was read.
        let refused = build_config(environment_source().source(Some(given.clone())))
            .expect_err("s3 chosen by env without a table must refuse the boot");
        assert!(
            refused.to_string().contains("no fallback"),
            "the refusal is the backend one: {refused}"
        );
        given.insert(
            "HODEI_CONTENT__S3__BUCKET".to_string(),
            "hodei-content".to_string(),
        );
        let config = build_config(environment_source().source(Some(given)))
            .expect("s3 with a bucket is a bootable choice");
        assert_eq!(
            config.content.backend,
            ContentBackend::S3,
            "HODEI_CONTENT__BACKEND must select the object store"
        );
        assert_eq!(
            config
                .content
                .s3
                .as_ref()
                .expect("the env table was built")
                .bucket,
            "hodei-content",
            "HODEI_CONTENT__S3__BUCKET must reach the table"
        );
    }
}
