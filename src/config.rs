//! Configuration module for Hodei Artifacts API
//!
//! This module handles loading and managing application configuration
//! from multiple sources with hierarchical precedence and validation.

use ::config::{Config, ConfigError, Environment, File};
use axum::http::HeaderValue;
use serde::{Deserialize, Serialize};
use std::env;

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
    /// The boot never applies a margin shorter than `oci_upload_ttl_secs`
    /// plus a minute: a resumable OCI session (HSP-201) keeps its staging
    /// for its whole TTL, and cleaning it earlier would break a session
    /// the journal still serves.
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
                "Invalid content configuration: backend = \"s3\" requires the [content.s3] table with a bucket; there is no fallback to the filesystem. Set HODEI_CONTENT__S3__BUCKET (and HODEI_CONTENT__S3__ENDPOINT for a non-AWS store) or set HODEI_CONTENT__BACKEND=filesystem".to_string(),
            )),
            (ContentBackend::S3, Some(table)) if table.bucket.trim().is_empty() => {
                Err(ConfigError::Message(
                    "Invalid content configuration: [content.s3].bucket cannot be empty. Set HODEI_CONTENT__S3__BUCKET".to_string(),
                ))
            }
            _ => Ok(()),
        }
    }
}

impl ContentConfig {
    /// Check that the content directory can be written when the backend is
    /// the filesystem.
    ///
    /// The S3 backend has its own preflight at bootstrap (it needs the
    /// network); here only the local machine is asked. The message names
    /// the path, the cause and the corrective action.
    pub fn preflight_storage(&self) -> Result<(), ConfigError> {
        if self.backend != ContentBackend::Filesystem {
            return Ok(());
        }
        let path = std::path::Path::new(&self.cas_path);
        if let Err(error) = std::fs::create_dir_all(path) {
            return Err(ConfigError::Message(format!(
                "Content directory '{}' cannot be created: {error}. \
                 Create it and give the service user write permission, or set HODEI_CONTENT__CAS_PATH to a writable directory.",
                path.display()
            )));
        }
        let probe = path.join(".hodei_test_write");
        match std::fs::write(&probe, b"probe") {
            Ok(()) => {
                let _ = std::fs::remove_file(probe);
                Ok(())
            }
            Err(error) => Err(ConfigError::Message(format!(
                "Content directory '{}' is not writable: {error}. \
                 Fix its ownership or permissions (for example chown/chmod for the service user), or set HODEI_CONTENT__CAS_PATH to a writable directory.",
                path.display()
            ))),
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
    /// Interval, in seconds, of the sweep that re-evaluates grants
    /// whose validity has run out or whose policy is no longer the active one, and of the
    /// consumption of deltas that materializes the impact view (HSP-137).
    ///
    /// The serving path already rejects an expired grant
    /// (HSP-135); the sweep is what makes the truth get re-evaluated
    /// and written, not merely noticed on read. `0` disables the
    /// loop: the cycle stays alive through advisory intake and the
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

/// The public address of the deployment.
///
/// It is what the registry tells its clients: the absolute URL of a
/// package, of a feed and of the token endpoint. A deployment that
/// declares nothing says that its address is the authority of the
/// request, and nothing more: the forwarded headers of a proxy are heard
/// only when the deployment says that proxy is trusted, and trust is
/// never the default.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PublicUrlConfig {
    /// The public base URL of the deployment.
    ///
    /// Default: none. Without it the public address is the authority of
    /// the request, when that authority is a valid one.
    #[serde(default)]
    pub public_base_url: Option<String>,
    /// The reverse proxies whose forwarded headers are believed.
    ///
    /// An entry is an address (`10.0.0.1`) or a network of addresses
    /// (`10.0.0.0/8`).
    ///
    /// Default: none, and a deployment that trusts no proxy ignores every
    /// forwarded header it receives.
    #[serde(default)]
    pub trusted_proxies: Vec<String>,
}

impl PublicUrlConfig {
    /// The public address, resolved.
    ///
    /// # Errors
    ///
    /// A message saying what is wrong when the base URL is not a usable
    /// public address, or when a trusted proxy is not an address or a
    /// network. A configuration that cannot be understood does not boot.
    pub fn the_public_url(&self) -> Result<crate::public_url::PublicUrl, String> {
        crate::public_url::PublicUrl::of(self.public_base_url.as_deref(), &self.trusted_proxies)
    }

    /// Check that the public address of the deployment can be used.
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.the_public_url().map(|_| ()).map_err(|error| {
            ConfigError::Message(format!("Invalid public URL configuration: {error}"))
        })
    }
}

/// The signing of short-lived URLs.
///
/// Some clients download an archive without any credential (the Terraform
/// CLI does so for the archive of a provider mirror). The hub answers an
/// authenticated read with a URL for that archive alone, signed and valid
/// for a few minutes. The signature needs a secret that **only the
/// operator provides**: there is no default key, and nothing is derived from
/// the JWT or JWKS material. Without a key the mirror does not issue
/// archive URLs and says so.
///
/// The keys form a ring: the FIRST key signs, ALL of them verify, so a key
/// can be rotated without cutting the URLs in flight. Every replica must be
/// given the same ring.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigningConfig {
    /// The keys of the ring. The secret of a key is read from a file or from
    /// an environment variable, never written in the configuration.
    ///
    /// In the environment the ring is written with indexes:
    /// `HODEI_SIGNING__KEYS__0__ID`, `HODEI_SIGNING__KEYS__0__SECRET_ENV`.
    ///
    /// Default: empty, which disables the archive URLs of the mirror.
    #[serde(default, deserialize_with = "the_signing_keys")]
    pub keys: Vec<SigningKeyConfig>,
    /// The lifetime of an issued URL, in seconds.
    ///
    /// Default: 300. Minimum: 1. Maximum: 3600.
    #[serde(default = "the_default_signing_ttl_secs")]
    pub ttl_secs: u64,
}

impl Default for SigningConfig {
    fn default() -> Self {
        Self {
            keys: Vec::new(),
            ttl_secs: the_default_signing_ttl_secs(),
        }
    }
}

fn the_default_signing_ttl_secs() -> u64 {
    crate::signed_url::DEFAULT_TTL_SECS
}

/// The most keys a ring may hold: a ring is a rotation window, not a
/// registry.
const THE_MAXIMUM_SIGNING_KEYS: usize = 8;

/// One key of the ring: where to read its secret, not the secret.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigningKeyConfig {
    /// The id of the key: 1 to 32 letters, digits, `-` or `_`. It travels
    /// in the URLs, so it names the key and reveals nothing about it.
    pub id: String,
    /// A file with the secret (at least 32 bytes; surrounding whitespace is
    /// ignored). Exactly one of `secret_file` and `secret_env`.
    #[serde(default, skip_serializing)]
    pub secret_file: Option<String>,
    /// The environment variable that holds the secret. Exactly one of
    /// `secret_file` and `secret_env`.
    #[serde(default, skip_serializing)]
    pub secret_env: Option<String>,
}

impl std::fmt::Debug for SigningKeyConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SigningKeyConfig")
            .field("id", &self.id)
            .field(
                "secret_file",
                &self.secret_file.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "secret_env",
                &self.secret_env.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

/// The keys as a list, or as a table with numeric indexes (the shape that
/// environment variables can express).
fn the_signing_keys<'de, D>(deserializer: D) -> Result<Vec<SigningKeyConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Shape {
        List(Vec<SigningKeyConfig>),
        Indexed(std::collections::BTreeMap<String, SigningKeyConfig>),
    }
    match Shape::deserialize(deserializer)? {
        Shape::List(keys) => Ok(keys),
        Shape::Indexed(table) => {
            let mut indexed = Vec::with_capacity(table.len());
            for (index, key) in table {
                let index: usize = index.parse().map_err(|_| {
                    serde::de::Error::custom(format!(
                        "signing key index `{index}` is not a number: write HODEI_SIGNING__KEYS__0__ID"
                    ))
                })?;
                indexed.push((index, key));
            }
            indexed.sort_by_key(|(index, _)| *index);
            Ok(indexed.into_iter().map(|(_, key)| key).collect())
        }
    }
}

impl SigningConfig {
    /// Check the shape of the ring, without reading any secret.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let invalid = |reason: String| {
            ConfigError::Message(format!("Invalid signing configuration: {reason}"))
        };
        if !(1..=crate::signed_url::MAXIMUM_TTL_SECS).contains(&self.ttl_secs) {
            return Err(invalid(format!(
                "ttl_secs must be between 1 and {}, and {} was given",
                crate::signed_url::MAXIMUM_TTL_SECS,
                self.ttl_secs
            )));
        }
        if self.keys.len() > THE_MAXIMUM_SIGNING_KEYS {
            return Err(invalid(format!(
                "a ring holds at most {THE_MAXIMUM_SIGNING_KEYS} keys, and {} were given",
                self.keys.len()
            )));
        }
        for (index, key) in self.keys.iter().enumerate() {
            if !crate::signed_url::is_valid_key_id(&key.id) {
                return Err(invalid(format!(
                    "the key id `{}` is not valid: use 1 to 32 characters among letters, digits, `-` and `_`",
                    key.id
                )));
            }
            if self.keys[..index].iter().any(|other| other.id == key.id) {
                return Err(invalid(format!("the key id `{}` is repeated", key.id)));
            }
            match (&key.secret_file, &key.secret_env) {
                (Some(_), Some(_)) | (None, None) => {
                    return Err(invalid(format!(
                        "the key `{}` needs exactly one of secret_file and secret_env",
                        key.id
                    )));
                }
                (Some(file), None) if file.trim().is_empty() => {
                    return Err(invalid(format!(
                        "the key `{}` has an empty secret_file",
                        key.id
                    )));
                }
                (None, Some(name)) if name.trim().is_empty() => {
                    return Err(invalid(format!(
                        "the key `{}` has an empty secret_env",
                        key.id
                    )));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Read the secrets and build the ring.
    ///
    /// This touches the filesystem and the environment, so it runs when the
    /// service starts and not when a configuration is validated. A key that
    /// is named and cannot be read, or whose secret is too short, stops the
    /// boot: a half-working ring would sign with some keys and not verify
    /// with others. An empty ring is valid and means "no archive URLs".
    ///
    /// # Errors
    ///
    /// A message naming the key and what is wrong with it; it never
    /// contains a secret.
    pub fn the_ring(&self) -> Result<crate::signed_url::SignedUrls, String> {
        self.the_ring_from(|name| env::var(name).ok())
    }

    /// [`Self::the_ring`] with the environment given as a lookup, so that a
    /// test never has to change the environment of the process.
    fn the_ring_from(
        &self,
        environment: impl Fn(&str) -> Option<String>,
    ) -> Result<crate::signed_url::SignedUrls, String> {
        let mut keys = Vec::with_capacity(self.keys.len());
        for key in &self.keys {
            let secret = match (&key.secret_file, &key.secret_env) {
                (Some(path), None) => std::fs::read_to_string(path).map_err(|error| {
                    format!(
                        "the secret of the signing key `{}` cannot be read from `{path}`: {}",
                        key.id,
                        error.kind()
                    )
                })?,
                (None, Some(name)) => environment(name).ok_or_else(|| {
                    format!(
                        "the secret of the signing key `{}` is not in the environment variable `{name}`",
                        key.id
                    )
                })?,
                _ => {
                    return Err(format!(
                        "the key `{}` needs exactly one of secret_file and secret_env",
                        key.id
                    ));
                }
            };
            keys.push(crate::signed_url::SigningKey::new(
                &key.id,
                secret.trim().as_bytes(),
            )?);
        }
        crate::signed_url::SignedUrls::new(keys, self.ttl_secs)
    }
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

/// What the service says when the configuration does not declare `[graph]`.
///
/// The graph is the single authority of the metadata. The embedded engine
/// (SurrealDB over RocksDB) and its operator tools were retired
/// (HSP-213, HSP-314 F4): a deployment that still carries the old profile
/// is refused here, with the way out, and nothing local is created.
pub const GRAPH_PROFILE_REQUIRED: &str = "the embedded engine (SurrealDB over RocksDB) is \
retired (HSP-213, HSP-314 F4): declare a [graph] section so the Dgraph alpha is the single \
authority of the metadata (RUN_MODE=dgraph reads config/dgraph.toml); a local store from an \
older release has to be migrated with a release that still shipped the operator tools";

/// The start of the refusal of a `[database] db_type` that is not the graph.
pub const REFUSED_DATABASE_TYPE: &str = "Unsupported database type";

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
pub struct AppConfig {
    /// Server configuration
    pub server: ServerConfig,

    /// Security configuration
    #[serde(default)]
    pub security: SecurityConfig,

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

    /// The keys that sign the short-lived URLs of the provider mirror.
    #[serde(default)]
    pub signing: SigningConfig,

    /// The per-account quotas: what one account may store and how much
    /// it may do at once.
    #[serde(default)]
    pub quota: QuotaConfig,

    /// The public address of the deployment: the URL the clients are
    /// told about, and the proxies whose forwarded headers are believed.
    #[serde(default)]
    pub public_url: PublicUrlConfig,

    /// The authority of the graph: the Dgraph deployment this service
    /// reads and writes. It is the single authority of the metadata and it
    /// is required: a configuration that does not declare it is refused
    /// with [`GRAPH_PROFILE_REQUIRED`], and a profile that cannot be
    /// validated against the server stops the boot instead of degrading
    /// (HSP-209, HSP-314 F4).
    #[serde(default)]
    pub graph: Option<hodei_graph_client::GraphClientConfig>,

    /// Database configuration
    pub database: DatabaseConfig,

    /// Schema configuration
    pub schema: SchemaConfig,

    /// Logging configuration
    pub logging: LoggingConfig,
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

/// The default value of the security configuration.
///
/// Everything closed and without credentials: a deployment that says nothing
/// does not say yes to anything. And the OCI surface limits have their
/// values, because a limit that is zero is not a limit: it is a
/// surface that does not work.
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
                    "Production security requires exactly one of auth_jwt_secret or auth_jwks_path. Set exactly one of HODEI_SECURITY__AUTH_JWT_SECRET or HODEI_SECURITY__AUTH_JWKS_PATH (or the keys in [security] of config/default.toml) and unset the other"
                        .to_string(),
                ));
            }
            if self.auth_issuer.is_none()
                || self.auth_audience.is_none()
                || self.authorization_policy_path.is_none()
                || self.authorization_schema_path.is_none()
            {
                return Err(ConfigError::Message(
                    "Production security requires auth_issuer, auth_audience, authorization_policy_path and authorization_schema_path together. Set HODEI_SECURITY__AUTH_ISSUER, HODEI_SECURITY__AUTH_AUDIENCE, HODEI_SECURITY__AUTHORIZATION_POLICY_PATH and HODEI_SECURITY__AUTHORIZATION_SCHEMA_PATH, or remove every auth key to run without production auth".to_string(),
                ));
            }
        }
        if self.auth_issuer.as_deref().is_some_and(str::is_empty)
            || self.auth_audience.as_deref().is_some_and(str::is_empty)
            || self.auth_jwt_secret.as_deref().is_some_and(str::is_empty)
            || self.auth_jwks_path.as_deref().is_some_and(str::is_empty)
        {
            return Err(ConfigError::Message(
                "Production auth issuer, audience, JWT secret and JWKS path cannot be empty. Give HODEI_SECURITY__AUTH_ISSUER, HODEI_SECURITY__AUTH_AUDIENCE, HODEI_SECURITY__AUTH_JWT_SECRET and HODEI_SECURITY__AUTH_JWKS_PATH a value, or unset the ones you do not use"
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

/// Database configuration: the declaration of the metadata backend.
///
/// There is a single backend, the graph (`"dgraph"`), and the declaration
/// stays in the file so an operator reads which authority a profile uses.
/// Any other value is refused by name at validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Database type (only "dgraph" supported)
    pub db_type: String,
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

    /// Schema storage type (default: "dgraph")
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
            db_type: "dgraph".to_string(),
        }
    }
}

impl Default for SchemaConfig {
    fn default() -> Self {
        Self {
            register_iam_on_startup: false,
            version: None,
            validate: true,
            storage_type: "dgraph".to_string(),
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
        // The graph is the only authority of the metadata: a configuration
        // that does not declare it is refused with the way out (HSP-209,
        // HSP-314 F4).
        let graph = self
            .graph
            .as_ref()
            .ok_or_else(|| ConfigError::Message(GRAPH_PROFILE_REQUIRED.to_string()))?;
        self.database.validate()?;
        graph
            .validate()
            .map_err(|error| ConfigError::Message(error.to_string()))?;
        self.logging.validate()?;
        self.content.validate()?;
        self.investigation.validate()?;
        self.quota.validate()?;
        self.public_url.validate()?;
        self.signing.validate()?;
        Ok(())
    }

    /// Check that the storage the configuration names can actually be used.
    ///
    /// This is the infrastructure preflight, and it is the part that touches
    /// the filesystem. It runs when the service starts, not when somebody
    /// validates a configuration, and its failures say that the problem is
    /// the storage and not the configuration.
    ///
    /// What the service preflights is its own content store; the reachability
    /// of the alpha is checked with a real connection and a version probe in
    /// the bootstrap, which is where the network question belongs.
    pub fn preflight_storage(&self) -> Result<(), ConfigError> {
        self.content.preflight_storage()
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
        .with_list_parse_key("public_url.trusted_proxies")
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
    /// Validate database configuration: the graph is the only backend, and
    /// any other declaration is refused by name (HSP-209, HSP-314 F4).
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.db_type != "dgraph" {
            return Err(ConfigError::Message(format!(
                "{REFUSED_DATABASE_TYPE} '{}'. Only 'dgraph' is supported: the graph declared in [graph] is the single authority of the metadata. Please set HODEI_DATABASE__DB_TYPE to 'dgraph'",
                self.db_type
            )));
        }
        Ok(())
    }
}

/// The configuration files the loader looks for, relative to the working
/// directory, in ascending precedence.
fn the_config_file_names(run_mode: &str) -> [String; 3] {
    [
        "config/default.toml".to_string(),
        format!("config/{run_mode}.toml"),
        "config/local.toml".to_string(),
    ]
}

/// Replace the value of anything that looks like a secret with `[REDACTED]`.
///
/// Startup errors can echo what the underlying library saw: a connection
/// string, an `Authorization` header, an environment dump. Whatever the
/// source, the text goes through here before it reaches the operator.
pub fn redact_secrets(text: &str) -> String {
    let text = redact_private_key_blocks(text);
    let text = text.as_str();
    const SENSITIVE: [&str; 8] = [
        "token",
        "password",
        "passwd",
        "secret",
        "private_key",
        "api_key",
        "apikey",
        "credential",
    ];
    let is_sensitive = |key: &str| {
        let key = key.to_ascii_lowercase();
        SENSITIVE.iter().any(|word| key.contains(word)) || key.ends_with("_key")
    };
    let mut out: Vec<String> = Vec::new();
    let mut redact_next = false;
    for word in text.split(' ') {
        if redact_next && !word.is_empty() {
            out.push("[REDACTED]".to_string());
            redact_next = false;
            continue;
        }
        let lowered = word.to_ascii_lowercase();
        if lowered == "bearer" || lowered == "basic" {
            redact_next = true;
            out.push(word.to_string());
            continue;
        }
        if let Some(index) = word.find(['=', ':']) {
            let (key, rest) = word.split_at(index);
            let value = &rest[1..];
            if is_sensitive(key) {
                if value.is_empty() {
                    // `password: hunter2` -- the value is the next word.
                    redact_next = true;
                    out.push(word.to_string());
                } else {
                    out.push(format!("{key}{}[REDACTED]", &rest[..1]));
                }
                continue;
            }
        }
        if word.starts_with("AKIA") && word.len() >= 16 {
            out.push("[REDACTED]".to_string());
            continue;
        }
        out.push(word.to_string());
    }
    out.join(" ")
}

/// Replace every PEM private-key block with `[REDACTED]`.
///
/// A key pasted into a variable reaches the error text with its markers and
/// base64 body, on several lines or flattened onto one. Everything from the
/// `-----BEGIN ... PRIVATE KEY-----` marker to the matching `-----END`
/// marker goes; a block that never closes is cut to the end of the text,
/// because what follows an unterminated key is most likely its body.
fn redact_private_key_blocks(text: &str) -> String {
    const BEGIN: &str = "-----BEGIN ";
    const KEY: &str = "PRIVATE KEY-----";
    const END: &str = "-----END ";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(BEGIN) {
        let after_begin = &rest[start + BEGIN.len()..];
        let is_private = after_begin
            .find("-----")
            .is_some_and(|close| after_begin[..close + 5].ends_with(KEY));
        if !is_private {
            out.push_str(&rest[..start + BEGIN.len()]);
            rest = after_begin;
            continue;
        }
        out.push_str(&rest[..start]);
        out.push_str("[REDACTED]");
        let body_start = start + BEGIN.len();
        let tail = &rest[body_start..];
        rest = tail
            .find(END)
            .and_then(|end| {
                let after_end = &tail[end + END.len()..];
                after_end.find(KEY).map(|k| &after_end[k + KEY.len()..])
            })
            .unwrap_or_default();
    }
    out.push_str(rest);
    out
}

/// Explain a failed configuration load to the operator.
///
/// Names the files that were looked for (and which of them exist under
/// `base`), the environment variable that would supply a missing value, and
/// the action to take. The text is redacted before it is returned.
pub fn explain_config_error(error: &ConfigError, base: &std::path::Path, run_mode: &str) -> String {
    let raw = error.to_string();
    let mut lines = vec![format!("Configuration error: {}", redact_secrets(&raw))];
    let files = the_config_file_names(run_mode);
    let found: Vec<&String> = files.iter().filter(|f| base.join(f).is_file()).collect();
    if found.is_empty() {
        lines.push(format!(
            "No configuration file found. Expected '{}' (working directory: '{}'); also looked for '{}' and '{}'.",
            base.join(&files[0]).display(),
            base.display(),
            files[1],
            files[2]
        ));
        lines.push(
            "Action: run the service from the directory that contains config/ (the distribution ships config/default.toml), or copy config/default.toml there, or provide every required value through HODEI_<SECTION>__<KEY> environment variables."
                .to_string(),
        );
    } else {
        let names: Vec<&str> = found.iter().map(|f| f.as_str()).collect();
        lines.push(format!("Configuration files read: {}.", names.join(", ")));
    }
    if let Some(field) = raw
        .split("missing field `")
        .nth(1)
        .and_then(|rest| rest.split('`').next())
    {
        const SECTIONS: [&str; 4] = ["server", "database", "schema", "logging"];
        if SECTIONS.contains(&field) {
            lines.push(format!(
                "Required section missing: '[{field}]'. Add it to config/default.toml, or set its keys as HODEI_{}__<KEY> (for example HODEI_SERVER__PORT=3000).",
                field.to_ascii_uppercase()
            ));
        } else {
            lines.push(format!(
                "Required key missing: '{field}'. Set it in its section of config/default.toml, or as HODEI_<SECTION>__{} (for example HODEI_DATABASE__DB_TYPE=dgraph).",
                field.to_ascii_uppercase()
            ));
        }
    }
    lines.join("\n")
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
    /// The error the loader really produces for a TOML text, not a hand-made one.
    fn the_real_load_error(toml: &str) -> ConfigError {
        Config::builder()
            .add_source(File::from_str(toml, ::config::FileFormat::Toml))
            .build()
            .expect("the text is valid TOML")
            .try_deserialize::<AppConfig>()
            .expect_err("the text lacks required sections")
    }

    #[test]
    fn a_missing_config_file_names_the_expected_path_and_the_action() {
        let tmp = tempfile::tempdir().unwrap();
        let error = the_real_load_error("");
        let text = explain_config_error(&error, tmp.path(), "development");
        let expected = tmp.path().join("config/default.toml");
        assert!(text.contains(&expected.display().to_string()), "{text}");
        assert!(text.contains("Action:"), "{text}");
        assert!(text.contains("HODEI_SERVER__"), "{text}");
    }

    #[test]
    fn a_present_config_file_is_reported_as_read() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("config")).unwrap();
        std::fs::write(tmp.path().join("config/default.toml"), "").unwrap();
        let error = the_real_load_error(
            "[server]\nhost = \"h\"\nport = 1\nrequest_timeout_secs = 1\nmax_body_size = 1\n",
        );
        let text = explain_config_error(&error, tmp.path(), "development");
        assert!(text.contains("config/default.toml"), "{text}");
        assert!(!text.contains("No configuration file found"), "{text}");
        assert!(text.contains("HODEI_DATABASE__"), "{text}");
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "root bypasses permission bits, so it cannot observe the failure; run unprivileged with --ignored (the root-valid path is covered by a_content_directory_below_a_regular_file_names_the_path_and_the_fix)"]
    fn an_unwritable_content_directory_names_the_path_and_the_fix() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("cas");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        assert!(
            std::fs::write(dir.join("probe"), b"x").is_err(),
            "permission bits are not enforced for this user; run this test unprivileged"
        );
        let content = ContentConfig {
            cas_path: dir.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let error = content.preflight_storage().expect_err("not writable");
        let text = error.to_string();
        assert!(text.contains(&dir.display().to_string()), "{text}");
        assert!(text.contains("HODEI_CONTENT__CAS_PATH"), "{text}");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Valid for every user, root included: no permission bit is involved.
    #[test]
    fn a_content_directory_below_a_regular_file_names_the_path_and_the_fix() {
        let tmp = tempfile::tempdir().unwrap();
        let blocker = tmp.path().join("file");
        std::fs::write(&blocker, "x").unwrap();
        let content = ContentConfig {
            cas_path: blocker.join("cas").to_string_lossy().into_owned(),
            ..Default::default()
        };
        let text = content
            .preflight_storage()
            .expect_err("cannot exist")
            .to_string();
        assert!(text.contains(&blocker.display().to_string()), "{text}");
        assert!(text.contains("HODEI_CONTENT__CAS_PATH"), "{text}");
    }

    const PEM_BODY: &str = "MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC7VJTUt9Us8cKj";

    fn the_pem(kind: &str) -> String {
        format!("-----BEGIN {kind}-----\n{PEM_BODY}\nMIIEvQ2ndLine==\n-----END {kind}-----")
    }

    #[test]
    fn a_private_key_block_never_reaches_the_operator() {
        for kind in [
            "PRIVATE KEY",
            "RSA PRIVATE KEY",
            "EC PRIVATE KEY",
            "ENCRYPTED PRIVATE KEY",
        ] {
            let pem = the_pem(kind);
            let flat = pem.replace('\n', " ");
            let escaped = pem.replace('\n', "\\n");
            for block in [pem, flat, escaped] {
                let raw = format!("invalid value {block} for key; endpoint=http://h:9000");
                let redacted = redact_secrets(&raw);
                for leaked in ["BEGIN", "-----", PEM_BODY, "MIIEvQ2ndLine", "END"] {
                    assert!(!redacted.contains(leaked), "{leaked} leaked: {redacted}");
                }
                assert!(redacted.contains("endpoint=http://h:9000"), "{redacted}");
                let error = ConfigError::Message(raw);
                let text = explain_config_error(
                    &error,
                    std::path::Path::new("/nonexistent"),
                    "development",
                );
                for leaked in ["BEGIN", PEM_BODY, "MIIEvQ2ndLine"] {
                    assert!(!text.contains(leaked), "{leaked} leaked: {text}");
                }
            }
        }
    }

    #[test]
    fn an_unterminated_private_key_block_is_cut_to_the_end() {
        let raw = format!("bad: -----BEGIN PRIVATE KEY-----\n{PEM_BODY}");
        let redacted = redact_secrets(&raw);
        assert!(
            !redacted.contains(PEM_BODY) && !redacted.contains("BEGIN"),
            "{redacted}"
        );
    }

    #[test]
    fn a_public_certificate_marker_is_left_alone() {
        let raw = "-----BEGIN CERTIFICATE----- abc -----END CERTIFICATE-----";
        assert_eq!(redact_secrets(raw), raw);
    }

    #[test]
    fn the_production_auth_errors_name_the_variable_and_the_action() {
        let both = SecurityConfig {
            auth_jwt_secret: Some("s".into()),
            auth_jwks_path: Some("p".into()),
            ..SecurityConfig::default()
        };
        let text = both.validate_production_auth().unwrap_err().to_string();
        assert!(text.contains("HODEI_SECURITY__AUTH_JWT_SECRET"), "{text}");
        assert!(text.contains("HODEI_SECURITY__AUTH_JWKS_PATH"), "{text}");

        let partial = SecurityConfig {
            auth_jwt_secret: Some("s".into()),
            ..SecurityConfig::default()
        };
        let text = partial.validate_production_auth().unwrap_err().to_string();
        assert!(text.contains("HODEI_SECURITY__AUTH_ISSUER"), "{text}");
        assert!(
            text.contains("HODEI_SECURITY__AUTHORIZATION_SCHEMA_PATH"),
            "{text}"
        );

        let empty = SecurityConfig {
            auth_jwt_secret: Some("s".into()),
            auth_issuer: Some(String::new()),
            auth_audience: Some("a".into()),
            authorization_policy_path: Some("p".into()),
            authorization_schema_path: Some("s".into()),
            ..SecurityConfig::default()
        };
        let text = empty.validate_production_auth().unwrap_err().to_string();
        assert!(text.contains("HODEI_SECURITY__AUTH_ISSUER"), "{text}");
    }

    #[test]
    fn a_writable_content_directory_passes_the_preflight_and_is_created() {
        let tmp = tempfile::tempdir().unwrap();
        let content = ContentConfig {
            cas_path: tmp.path().join("new/cas").to_string_lossy().into_owned(),
            ..Default::default()
        };
        content.preflight_storage().expect("created and writable");
    }

    #[test]
    fn startup_messages_never_print_secrets() {
        let raw = "connect failed: password=hunter2 token:abc123 Authorization: Bearer eyJhbGciOi.secret.sig \
                   AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI aws_access_key=AKIAIOSFODNN7EXAMPLE endpoint=http://h:9000";
        let redacted = redact_secrets(raw);
        for secret in [
            "hunter2",
            "abc123",
            "eyJhbGciOi",
            "wJalrXUtnFEMI",
            "AKIAIOSFODNN7EXAMPLE",
        ] {
            assert!(!redacted.contains(secret), "{secret} leaked: {redacted}");
        }
        assert!(redacted.contains("endpoint=http://h:9000"), "{redacted}");
        let error = ConfigError::Message("bad value password=hunter2".to_string());
        let text =
            explain_config_error(&error, std::path::Path::new("/nonexistent"), "development");
        assert!(!text.contains("hunter2"), "{text}");
    }
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
            ("HODEI_DATABASE__DB_TYPE", "dgraph"),
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
            config.database.db_type, "dgraph",
            "HODEI_DATABASE__DB_TYPE must override the configured backend"
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

    /// The public address of the deployment has to be configurable
    ///
    /// The base URL and the list of trusted proxies are the only way a
    /// deployment says where it lives, so they have to arrive from the
    /// environment like everything else.
    #[test]
    fn the_public_address_comes_from_the_environment() {
        let mut variables = overrides();
        variables.insert(
            "HODEI_PUBLIC_URL__PUBLIC_BASE_URL".to_string(),
            "https://artifacts.example".to_string(),
        );
        variables.insert(
            "HODEI_PUBLIC_URL__TRUSTED_PROXIES".to_string(),
            "127.0.0.1/32,10.0.0.0/8".to_string(),
        );

        let config = build_config(environment_source().source(Some(variables)))
            .expect("the configuration must be loadable");

        assert_eq!(
            config.public_url.public_base_url.as_deref(),
            Some("https://artifacts.example"),
            "HODEI_PUBLIC_URL__PUBLIC_BASE_URL must reach the configuration"
        );
        assert_eq!(
            config.public_url.trusted_proxies,
            vec!["127.0.0.1/32".to_string(), "10.0.0.0/8".to_string()],
            "HODEI_PUBLIC_URL__TRUSTED_PROXIES must reach the configuration as a list"
        );
    }

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.port, 3000);
        assert_eq!(config.database.db_type, "dgraph");
        assert!(!config.schema.register_iam_on_startup);
        assert_eq!(config.logging.level, "info");
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
        // The graph is required: a configuration without `[graph]` is the
        // retired embedded profile and is refused with the way out.
        let error = AppConfig::default()
            .validate()
            .expect_err("a configuration without [graph] must be refused");
        assert!(
            error.to_string().contains(GRAPH_PROFILE_REQUIRED),
            "the refusal must carry the migration message: {error}"
        );

        let config = a_graph_config();
        assert!(config.validate().is_ok());

        let mut invalid_config = a_graph_config();
        invalid_config.server.port = 0;
        assert!(invalid_config.validate().is_err());

        // Any backend that is not the graph is refused by name, the
        // retired embedded engine included.
        for refused in ["rocksdb", "postgres"] {
            let mut invalid_config = a_graph_config();
            invalid_config.database.db_type = refused.to_string();
            let error = invalid_config
                .validate()
                .expect_err("a backend that is not the graph must be refused");
            let text = error.to_string();
            assert!(
                text.contains(REFUSED_DATABASE_TYPE) && text.contains(refused),
                "the refusal must name the offending value: {text}"
            );
        }

        let mut invalid_config = a_graph_config();
        invalid_config.logging.level = "invalid".to_string();
        assert!(invalid_config.validate().is_err());
    }

    /// A valid graph profile, the base the validation tests perturb.
    fn a_graph_config() -> AppConfig {
        AppConfig {
            graph: Some(hodei_graph_client::GraphClientConfig {
                endpoint: "127.0.0.1:9080".to_string(),
                tls: hodei_graph_client::GraphTlsConfig {
                    enabled: false,
                    ca_cert_path: None,
                    client_cert_path: None,
                    client_key_path: None,
                    server_name: None,
                },
                credentials: None,
                connect_timeout_ms: 5_000,
                request_timeout_ms: 30_000,
                namespace: 0,
            }),
            ..AppConfig::default()
        }
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
        for name in ["config/default", "config/dgraph"] {
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

    fn the_signing_from(toml: &str) -> SigningConfig {
        ::config::Config::builder()
            .add_source(::config::File::from_str(toml, ::config::FileFormat::Toml))
            .build()
            .expect("the table is valid TOML")
            .get::<SigningConfig>("signing")
            .expect("the table deserializes into the signing configuration")
    }

    const THE_SECRET: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0";

    #[test]
    fn a_deployment_that_says_nothing_has_no_signing_key() {
        let signing = SigningConfig::default();
        signing.validate().expect("an empty ring is valid");
        let ring = signing.the_ring().expect("an empty ring builds");
        assert!(!ring.is_configured(), "no key means no archive URLs");
        assert_eq!(ring.ttl_secs(), crate::signed_url::DEFAULT_TTL_SECS);
    }

    #[test]
    fn the_ring_is_read_from_a_list_of_tables() {
        let signing = the_signing_from(
            r#"
[signing]
ttl_secs = 120
keys = [
  { id = "k2", secret_file = "/run/secrets/k2" },
  { id = "k1", secret_env = "SIGNING_K1" },
]
"#,
        );
        signing.validate().expect("a well formed ring");
        assert_eq!(signing.ttl_secs, 120);
        let ids: Vec<&str> = signing.keys.iter().map(|key| key.id.as_str()).collect();
        assert_eq!(ids, ["k2", "k1"], "the order is the order of signing");
    }

    #[test]
    fn the_ring_is_read_from_the_environment_in_index_order() {
        let mut given = std::collections::HashMap::new();
        for (name, value) in [
            ("HODEI_SIGNING__KEYS__1__ID", "old"),
            ("HODEI_SIGNING__KEYS__1__SECRET_ENV", "SIGNING_OLD"),
            ("HODEI_SIGNING__KEYS__0__ID", "new"),
            ("HODEI_SIGNING__KEYS__0__SECRET_FILE", "/run/secrets/new"),
            ("HODEI_SIGNING__TTL_SECS", "60"),
        ] {
            given.insert(name.to_string(), value.to_string());
        }
        let config = build_config(environment_source().source(Some(given)))
            .expect("a ring written in the environment boots");
        let ids: Vec<&str> = config
            .signing
            .keys
            .iter()
            .map(|key| key.id.as_str())
            .collect();
        assert_eq!(ids, ["new", "old"], "index 0 signs");
        assert_eq!(config.signing.ttl_secs, 60);
        assert_eq!(
            config.signing.keys[0].secret_file.as_deref(),
            Some("/run/secrets/new")
        );
    }

    #[test]
    fn a_secret_written_in_the_toml_is_refused_and_never_echoed() {
        let error = ::config::Config::builder()
            .add_source(::config::File::from_str(
                &format!("[signing]\nkeys = [{{ id = \"k1\", secret = \"{THE_SECRET}\" }}]\n"),
                ::config::FileFormat::Toml,
            ))
            .build()
            .expect("valid TOML")
            .get::<SigningConfig>("signing")
            .expect_err("a literal secret is not a field of a key");
        let text =
            explain_config_error(&error, std::path::Path::new("/nonexistent"), "development");
        assert!(!text.contains(THE_SECRET), "the secret leaked: {text}");
    }

    #[test]
    fn the_shape_of_the_ring_is_validated() {
        let key = |id: &str, file: Option<&str>, env: Option<&str>| SigningKeyConfig {
            id: id.to_string(),
            secret_file: file.map(str::to_string),
            secret_env: env.map(str::to_string),
        };
        let ring = |keys: Vec<SigningKeyConfig>, ttl_secs: u64| SigningConfig { keys, ttl_secs };
        let refused = |signing: SigningConfig, needle: &str| {
            let error = signing.validate().expect_err("must be refused").to_string();
            assert!(error.contains(needle), "{needle} not in {error}");
        };
        refused(
            ring(vec![key("a", Some("/f"), Some("E"))], 300),
            "exactly one",
        );
        refused(ring(vec![key("a", None, None)], 300), "exactly one");
        refused(ring(vec![key("a b", Some("/f"), None)], 300), "not valid");
        refused(ring(vec![key("", Some("/f"), None)], 300), "not valid");
        refused(
            ring(
                vec![key("a", Some("/f"), None), key("a", None, Some("E"))],
                300,
            ),
            "repeated",
        );
        refused(ring(vec![key("a", Some(" "), None)], 300), "empty");
        refused(ring(vec![key("a", Some("/f"), None)], 0), "ttl_secs");
        refused(
            ring(
                vec![key("a", Some("/f"), None)],
                crate::signed_url::MAXIMUM_TTL_SECS + 1,
            ),
            "ttl_secs",
        );
        let many = (0..9)
            .map(|index| key(&format!("k{index}"), Some("/f"), None))
            .collect();
        refused(ring(many, 300), "at most");
        ring(
            vec![key("a", Some("/f"), None)],
            crate::signed_url::MAXIMUM_TTL_SECS,
        )
        .validate()
        .expect("the maximum lifetime is allowed");
    }

    #[test]
    fn the_secrets_are_read_from_a_file_or_the_environment() {
        let directory = tempfile::tempdir().expect("a directory");
        let file = directory.path().join("signing-key");
        std::fs::write(&file, format!("{THE_SECRET}\n")).expect("the key file");
        let signing = SigningConfig {
            keys: vec![
                SigningKeyConfig {
                    id: "from-env".into(),
                    secret_file: None,
                    secret_env: Some("A_SECRET_VARIABLE".into()),
                },
                SigningKeyConfig {
                    id: "from-file".into(),
                    secret_file: Some(file.display().to_string()),
                    secret_env: None,
                },
            ],
            ttl_secs: 300,
        };
        let ring = signing
            .the_ring_from(|name| {
                (name == "A_SECRET_VARIABLE").then(|| format!("  {THE_SECRET}\n"))
            })
            .expect("both secrets are readable");
        assert!(ring.is_configured());
        let text = format!("{ring:?} {signing:?}");
        assert!(!text.contains(THE_SECRET), "{text}");
        assert!(!text.contains("A_SECRET_VARIABLE"), "{text}");
        assert!(!text.contains(&file.display().to_string()), "{text}");
    }

    #[test]
    fn a_key_that_cannot_be_used_stops_the_boot_without_echoing_its_secret() {
        let directory = tempfile::tempdir().expect("a directory");
        let short = directory.path().join("short");
        std::fs::write(&short, "too-short-to-be-a-key").expect("the key file");
        let named = |file: Option<String>, env: Option<&str>| SigningConfig {
            keys: vec![SigningKeyConfig {
                id: "k1".into(),
                secret_file: file,
                secret_env: env.map(str::to_string),
            }],
            ttl_secs: 300,
        };
        let error = named(Some(short.display().to_string()), None)
            .the_ring_from(|_| None)
            .expect_err("a short secret");
        assert!(error.contains("at least 32"), "{error}");
        assert!(!error.contains("too-short-to-be-a-key"), "{error}");

        let error = named(Some("/nonexistent/signing-key".into()), None)
            .the_ring_from(|_| None)
            .expect_err("a missing file");
        assert!(
            error.contains("k1") && error.contains("cannot be read"),
            "{error}"
        );

        let error = named(None, Some("UNSET_VARIABLE"))
            .the_ring_from(|_| None)
            .expect_err("an unset variable");
        assert!(error.contains("UNSET_VARIABLE"), "{error}");

        let error = named(None, Some("SHORT_VARIABLE"))
            .the_ring_from(|_| Some("another-short-secret".into()))
            .expect_err("a short secret in the environment");
        assert!(!error.contains("another-short-secret"), "{error}");
    }

    #[test]
    fn the_redaction_of_an_error_covers_the_signing_references() {
        let raw = format!(
            "bad ring: secret_env=SIGNING_K1 secret_file=/run/secrets/k1 secret={THE_SECRET} id=k1"
        );
        let redacted = redact_secrets(&raw);
        for leaked in ["SIGNING_K1", "/run/secrets/k1", THE_SECRET] {
            assert!(!redacted.contains(leaked), "{leaked} leaked: {redacted}");
        }
        assert!(redacted.contains("id=k1"), "{redacted}");
    }
}
