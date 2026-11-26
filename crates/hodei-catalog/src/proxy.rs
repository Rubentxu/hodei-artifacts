//! The upstream of a proxy repository and the receipt of what was pulled from it.
//!
//! A `Proxy` repository does not store what someone publishes: it resolves against
//! a **declared upstream** and keeps what it pulls down. Whoever asks does not choose
//! where the server goes: they ask for a **relative** path, and the destination comes from the
//! repository configuration, which is versioned.
//!
//! This module is pure: it describes and validates the configuration, and defines the
//! receipt and its ledger. What touches the network (resolving names, connecting,
//! following redirects) lives in the service, which is the one that knows the
//! egress policy of the machine.
//!
//! # The configuration
//!
//! It lives in `config_settings.proxy.upstream`:
//!
//! ```json
//! { "proxy": { "upstream": {
//!     "id": "central",
//!     "url": "https://repo.example.com/releases/",
//!     "scopes": ["acme/"],
//!     "credential_ref": "env:CENTRAL_TOKEN",
//!     "redirects": { "max": 3, "cross_origin": false },
//!     "max_bytes": 104857600,
//!     "require_checksum": false
//! } } }
//! ```
//!
//! - `scopes` are allowed path prefixes: whatever does not fall in a scope is
//!   not requested from the upstream. `"/"` is the whole upstream. There are no default
//!   scopes: without scopes the configuration is not valid.
//! - `credential_ref` is a **reference** (`env:NAME`), never the
//!   secret. The secret is read at request time and only travels
//!   to the origin of the upstream.
//! - `redirects` says how many redirects are followed and whether they are allowed
//!   to another origin. By default, three and only to the same origin.
//!
//! Alongside `proxy.upstream`, `proxy.cache` declares the cache policy
//! (TTL, negative cache, stale and offline): see [`crate::proxy_cache`].

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_evidence::ContentDigest;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::proxy_cache::CachePolicy;
use crate::{CatalogError, RepositoryConfig};

/// The cap on redirects that a configuration may ask for.
pub const MAX_REDIRECTS_CEILING: u8 = 10;

/// The redirects that are followed if the configuration says nothing else.
pub const DEFAULT_REDIRECTS: u8 = 3;

/// Why an upstream configuration is not valid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the upstream configuration is not valid: {0}")]
pub struct UpstreamConfigError(pub String);

fn invalid(reason: impl Into<String>) -> UpstreamConfigError {
    UpstreamConfigError(reason.into())
}

/// The reference to a credential.
///
/// It is the **name** of where the secret comes from, not the secret. A
/// versioned and audited configuration cannot carry a token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialRef(String);
impl CredentialRef {
    /// Reads an `env:NAME` reference.
    ///
    /// Only what is unmistakably a reference is accepted: anything
    /// else could be a secret written where it should not be, and it is rejected
    /// without repeating it in the error.
    pub fn parse(raw: &str) -> Result<Self, UpstreamConfigError> {
        let Some(name) = raw.strip_prefix("env:") else {
            return Err(invalid(
                "credential_ref must be a reference of the form env:NAME",
            ));
        };
        let well_formed = !name.is_empty()
            && name.len() <= 128
            && name
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
            && !name.as_bytes()[0].is_ascii_digit();
        if !well_formed {
            return Err(invalid(
                "credential_ref must be a reference of the form env:NAME",
            ));
        }
        Ok(Self(raw.to_string()))
    }

    /// The reference, as it was written.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The name of the environment variable that holds the secret.
    pub fn env_name(&self) -> &str {
        self.0.strip_prefix("env:").unwrap_or_default()
    }
}

/// The token realm, when the upstream declares it.
///
/// An upstream that requires authentication does not ask for it with a credentials
/// header: it answers with a **challenge** that says where to ask for a
/// token. That challenge is stated by the upstream, and an upstream can say anything:
/// that is why the **declared** realm is the one that counts, and a challenge that does not
/// name that realm receives no credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenAuth {
    /// Where the token is requested.
    pub realm: Url,
    /// The name of the requested service, if the upstream distinguishes it.
    pub service: Option<String>,
    /// The user the token is requested with.
    ///
    /// It is informative: what authenticates is the credential. Without a declared
    /// user the upstream identifier is used.
    pub username: Option<String>,
}

impl TokenAuth {
    /// The user the token is requested with.
    pub fn the_username<'a>(&'a self, upstream_id: &'a str) -> &'a str {
        self.username.as_deref().unwrap_or(upstream_id)
    }
}

/// The upstream of a proxy repository, already validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamConfig {
    /// The stable identifier of the upstream. It goes in the receipts.
    pub id: String,
    /// The base URL. It ends in `/`, with no credentials, query or fragment.
    pub base: Url,
    /// The path prefixes that may be requested. Never empty.
    pub scopes: Vec<String>,
    /// Where the credential comes from, if the upstream asks for one.
    pub credential: Option<CredentialRef>,
    /// How many redirects are followed at most.
    pub max_redirects: u8,
    /// Whether a redirect to another origin is allowed.
    pub cross_origin_redirects: bool,
    /// The byte cap of a response, if the configuration narrows it.
    pub max_bytes: Option<u64>,
    /// Whether the upstream has to declare the checksum of the content.
    ///
    /// With `true`, a response that does not declare it is rejected: the content
    /// cannot be presented as verified against the upstream.
    pub require_checksum: bool,
    /// The token realm, if the upstream requires authentication.
    pub auth: Option<TokenAuth>,
    /// The cache policy (`proxy.cache`); the default values if the
    /// configuration does not declare it.
    pub cache: CachePolicy,
    /// The public key with which the upstream signs its repodata, armored.
    ///
    /// It is a **public** key: it is not a secret and may go in the
    /// versioned configuration of the repository. The client (dnf) requests it
    /// over HTTP and uses it to verify the signature of the upstream that Hodei
    /// serves without rewriting.
    pub gpg_key_armor: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawUpstream {
    id: String,
    url: String,
    scopes: Vec<String>,
    #[serde(default)]
    credential_ref: Option<String>,
    #[serde(default)]
    redirects: Option<RawRedirects>,
    #[serde(default)]
    max_bytes: Option<u64>,
    #[serde(default)]
    require_checksum: bool,
    #[serde(default)]
    auth: Option<RawAuth>,
    /// The public key of the upstream, armored: the one the client uses
    /// to verify the signature of the repodata that the proxy serves as is.
    #[serde(default)]
    gpg_key_armor: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAuth {
    realm: String,
    #[serde(default)]
    service: Option<String>,
    #[serde(default)]
    username: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRedirects {
    #[serde(default)]
    max: Option<u8>,
    #[serde(default)]
    cross_origin: bool,
}

impl UpstreamConfig {
    /// The upstream that the configuration of a repository declares.
    ///
    /// `Ok(None)` is "declares none"; a declaration that exists and is not
    /// understood is an error, never a default value: an unreadable
    /// network egress setting cannot degrade to "allowed".
    pub fn of(config: &RepositoryConfig) -> Result<Option<Self>, UpstreamConfigError> {
        let Some(proxy) = config.config_settings.get("proxy") else {
            return Ok(None);
        };
        // An unknown key (`upstraem`, `caches`) is not ignored: a
        // setting that looks set and is not applied is worse than an error.
        if let Some(object) = proxy.as_object()
            && let Some(unknown) = object
                .keys()
                .find(|key| !matches!(key.as_str(), "upstream" | "cache"))
        {
            return Err(invalid(format!(
                "the proxy settings do not know the key {unknown:?}"
            )));
        }
        let Some(upstream) = proxy.get("upstream") else {
            return Err(invalid("the proxy settings do not declare an upstream"));
        };
        let mut config = Self::parse(upstream)?;
        if let Some(cache) = proxy.get("cache") {
            config.cache = CachePolicy::parse(cache)?;
        }
        Ok(Some(config))
    }

    /// Validates an upstream written as JSON.
    pub fn parse(value: &Value) -> Result<Self, UpstreamConfigError> {
        let raw: RawUpstream = serde_json::from_value(value.clone())
            .map_err(|error| invalid(format!("it cannot be read: {error}")))?;

        let id_ok = !raw.id.is_empty()
            && raw.id.len() <= 64
            && raw.id.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
            });
        if !id_ok {
            return Err(invalid("id must be 1-64 characters of [a-z0-9_-]"));
        }

        let mut base = Url::parse(&raw.url).map_err(|_| invalid("url is not an absolute URL"))?;
        if !matches!(base.scheme(), "http" | "https") {
            return Err(invalid("url must use http or https"));
        }
        if base.host().is_none() {
            return Err(invalid("url has no host"));
        }
        // A URL with credentials would put them in every trace that names it.
        if !base.username().is_empty() || base.password().is_some() {
            return Err(invalid("url must not carry credentials"));
        }
        if base.query().is_some() || base.fragment().is_some() {
            return Err(invalid("url must not carry a query or a fragment"));
        }
        if !base.path().ends_with('/') {
            let path = format!("{}/", base.path());
            base.set_path(&path);
        }

        if raw.scopes.is_empty() {
            return Err(invalid("scopes must declare at least one prefix"));
        }
        let mut scopes = Vec::with_capacity(raw.scopes.len());
        for scope in &raw.scopes {
            scopes.push(Self::scope(scope)?);
        }

        let credential = raw
            .credential_ref
            .as_deref()
            .map(CredentialRef::parse)
            .transpose()?;

        let (max_redirects, cross_origin_redirects) = match raw.redirects {
            Some(redirects) => (
                redirects.max.unwrap_or(DEFAULT_REDIRECTS),
                redirects.cross_origin,
            ),
            None => (DEFAULT_REDIRECTS, false),
        };
        if max_redirects > MAX_REDIRECTS_CEILING {
            return Err(invalid(format!(
                "redirects.max cannot be more than {MAX_REDIRECTS_CEILING}"
            )));
        }
        if raw.max_bytes == Some(0) {
            return Err(invalid("max_bytes cannot be zero"));
        }

        let auth = raw.auth.map(Self::auth).transpose()?;

        // The upstream key is public: it is the one the client requests over
        // HTTP to verify the repodata signature. A blank one is not
        // a key: it is a configuration nobody has written. The text
        // is stored as is: what is served is what was configured.
        let gpg_key_armor = raw.gpg_key_armor.filter(|armor| !armor.trim().is_empty());

        Ok(Self {
            id: raw.id,
            base,
            scopes,
            credential,
            max_redirects,
            cross_origin_redirects,
            max_bytes: raw.max_bytes,
            require_checksum: raw.require_checksum,
            auth,
            cache: CachePolicy::default(),
            gpg_key_armor,
        })
    }

    /// The token realm, if the declaration is understood.
    ///
    /// The realm is a URL like any other: absolute, with a network
    /// scheme and no credentials. A realm with credentials would put them in
    /// every trace that names it, and a realm without a host leads
    /// nowhere.
    fn auth(raw: RawAuth) -> Result<TokenAuth, UpstreamConfigError> {
        let realm =
            Url::parse(&raw.realm).map_err(|_| invalid("auth.realm is not an absolute URL"))?;
        if !matches!(realm.scheme(), "http" | "https") {
            return Err(invalid("auth.realm must use http or https"));
        }
        if realm.host().is_none() {
            return Err(invalid("auth.realm has no host"));
        }
        if !realm.username().is_empty() || realm.password().is_some() {
            return Err(invalid("auth.realm must not carry credentials"));
        }
        let service = raw
            .service
            .filter(|service| !service.is_empty() && service.len() <= 128);
        let username = raw
            .username
            .filter(|username| !username.is_empty() && username.len() <= 128);
        Ok(TokenAuth {
            realm,
            service,
            username,
        })
    }

    fn scope(raw: &str) -> Result<String, UpstreamConfigError> {
        if raw == "/" {
            return Ok(String::new());
        }
        let bad = raw.is_empty()
            // The prefix is per segment: without the trailing slash, `acme` would let
            // `acme-evil/` through.
            || !raw.ends_with('/')
            || raw.starts_with('/')
            || raw.contains('\\')
            || raw.contains('?')
            || raw.contains('#')
            || raw.chars().any(char::is_control)
            || raw.split('/').any(|segment| segment == ".." || segment == ".");
        if bad {
            return Err(invalid(
                "a scope is a relative prefix without dot segments, or /",
            ));
        }
        Ok(raw.to_string())
    }

    /// Whether a relative path falls within any scope.
    pub fn in_scope(&self, asset_path: &str) -> bool {
        self.scopes
            .iter()
            .any(|scope| scope.is_empty() || asset_path.starts_with(scope.as_str()))
    }

    /// The upstream URL for a relative path.
    ///
    /// The path is **appended** to the base segment by segment and encoded: it
    /// cannot change the host, the scheme or escape the base. `None` if the
    /// path is not in a scope or is not a clean relative path.
    pub fn target(&self, asset_path: &str) -> Option<Url> {
        if asset_path.is_empty() || !self.in_scope(asset_path) {
            return None;
        }
        let mut url = self.base.clone();
        {
            let mut segments = url.path_segments_mut().ok()?;
            // The base ends in `/`: the last empty segment is removed
            // so that the path does not generate a `//`.
            segments.pop_if_empty();
            for segment in asset_path.split('/') {
                if segment.is_empty() || segment == "." || segment == ".." {
                    return None;
                }
                segments.push(segment);
            }
        }
        Some(url)
    }

    /// The origin of the upstream: scheme, host and port, without a path.
    pub fn origin(&self) -> String {
        origin_of(&self.base)
    }
}

/// The origin of a URL (`scheme://host:port`), without path or credentials.
pub fn origin_of(url: &Url) -> String {
    format!(
        "{}://{}:{}",
        url.scheme(),
        url.host_str().unwrap_or_default(),
        url.port_or_known_default().unwrap_or_default()
    )
}

/// What the content pulled from the upstream was checked with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChecksumEvidence {
    /// The upstream declared the checksum and it matches what was received.
    Declared {
        /// The header that declared it.
        header: String,
    },
    /// The upstream declared nothing: the digest is the one computed here.
    ///
    /// It is content identity, not verification against the upstream, and the
    /// receipt says so.
    ComputedOnly,
}

/// What happened in the resolution that the receipt reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptOutcome {
    /// The bytes were fetched from the upstream and published.
    #[default]
    Fetched,
    /// The upstream confirmed that the content held is still valid
    /// (`304`, or the same bytes): nothing moved.
    Revalidated,
    /// The bytes did not come from a network egress, but from an imported
    /// resolution bundle: the receipt keeps the original provenance and
    /// the authority is the configuration of the destination repository.
    Imported,
    /// The upstream delivered **different** bytes for immutable content. The
    /// bytes are not published: the receipt keeps what was offered, and the
    /// content that was held stays retained until the
    /// configuration changes.
    Quarantined,
}

impl ReceiptOutcome {
    /// The stable name it is stored and shown with.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fetched => "fetched",
            Self::Revalidated => "revalidated",
            Self::Imported => "imported",
            Self::Quarantined => "quarantined",
        }
    }

    /// Reads a stored name. `None` if it is not known.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "fetched" => Some(Self::Fetched),
            "revalidated" => Some(Self::Revalidated),
            "imported" => Some(Self::Imported),
            "quarantined" => Some(Self::Quarantined),
            _ => None,
        }
    }
}

/// The receipt of a resolution: where the content came from and what it was
/// checked with.
///
/// It is written when the content has been received **in full** and has passed the
/// check, and before publishing it to the catalog. A fetch that fails
/// leaves no receipt: there is nothing to assert.
///
/// It carries no credentials or URL query: it is what gets shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvenanceReceipt {
    /// The receipt identifier.
    pub id: String,
    /// The account that owns the repository.
    pub account: String,
    /// The proxy repository.
    pub repository: String,
    /// The path that was resolved.
    pub asset_path: String,
    /// The upstream that responded.
    pub upstream_id: String,
    /// The URL the content came from, after redirects, without
    /// credentials or query.
    pub source_url: String,
    /// The version of the configuration it was resolved with.
    pub config_version: u64,
    /// The digest of the received content.
    pub digest: ContentDigest,
    /// The bytes received.
    pub size_bytes: u64,
    /// What it was checked with.
    pub checksum: ChecksumEvidence,
    /// The HTTP status of the response that was accepted.
    pub status: u16,
    /// The redirects that were followed.
    pub redirects: u8,
    /// The entity tag the upstream declared, if it declared one.
    pub etag: Option<String>,
    /// The `Last-Modified` the upstream declared, if it declared one.
    #[serde(default)]
    pub last_modified: Option<String>,
    /// What happened: it was fetched, revalidated or quarantined.
    #[serde(default)]
    pub outcome: ReceiptOutcome,
    /// Who requested the content.
    pub actor: String,
    /// The correlation of the request.
    pub correlation_id: String,
    /// When it was received.
    pub fetched_at: DateTime<Utc>,
}

/// The ledger of provenance receipts.
#[async_trait]
pub trait ProvenanceLedger: Send + Sync {
    /// Stores a receipt. Repeating the same `id` leaves it as it was.
    async fn record(&self, receipt: &ProvenanceReceipt) -> Result<(), CatalogError>;

    /// The receipts of a path, most recent first.
    async fn receipts_of(
        &self,
        account: &str,
        repository: &str,
        asset_path: &str,
    ) -> Result<Vec<ProvenanceReceipt>, CatalogError>;

    /// The `limit` most recent receipts of a path, most recent first.
    ///
    /// It is the read of the hot path: every revalidation leaves a receipt and
    /// with TTL 0 every read writes one, so the ledger of a path has no
    /// bound and reading all of it on every request does not scale.
    async fn latest_receipts(
        &self,
        account: &str,
        repository: &str,
        asset_path: &str,
        limit: usize,
    ) -> Result<Vec<ProvenanceReceipt>, CatalogError> {
        let mut receipts = self.receipts_of(account, repository, asset_path).await?;
        receipts.truncate(limit);
        Ok(receipts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DigestScope;
    use serde_json::json;

    fn upstream(extra: Value) -> Value {
        let mut base = json!({
            "id": "central",
            "url": "https://repo.example.com/releases",
            "scopes": ["acme/"],
        });
        if let (Some(base), Some(extra)) = (base.as_object_mut(), extra.as_object()) {
            for (key, value) in extra {
                base.insert(key.clone(), value.clone());
            }
        }
        base
    }

    #[test]
    fn a_complete_configuration_is_read_with_its_defaults() {
        let config = UpstreamConfig::parse(&upstream(json!({}))).expect("valid");
        assert_eq!(config.id, "central");
        // The base always ends in `/`.
        assert_eq!(config.base.as_str(), "https://repo.example.com/releases/");
        assert_eq!(config.max_redirects, DEFAULT_REDIRECTS);
        assert!(!config.cross_origin_redirects);
        assert!(config.credential.is_none());
        assert!(!config.require_checksum);
    }

    #[test]
    fn what_cannot_be_read_is_refused_instead_of_defaulted() {
        let cases = [
            json!({"url": "ftp://repo.example.com/"}),
            json!({"url": "https://user:secret@repo.example.com/"}),
            json!({"url": "https://repo.example.com/?token=1"}),
            json!({"url": "https://repo.example.com/#x"}),
            json!({"url": "not a url"}),
            json!({"scopes": []}),
            json!({"scopes": ["../x/"]}),
            json!({"scopes": ["/abs/"]}),
            json!({"scopes": [""]}),
            json!({"scopes": ["acme"]}),
            json!({"id": "Has Spaces"}),
            json!({"credential_ref": "hunter2"}),
            json!({"credential_ref": "env:lower"}),
            json!({"credential_ref": "env:"}),
            json!({"redirects": {"max": 200}}),
            json!({"max_bytes": 0}),
            // A field that is not known is a field that was misspelled.
            json!({"allow_private": true}),
        ];
        for case in cases {
            assert!(
                UpstreamConfig::parse(&upstream(case.clone())).is_err(),
                "{case} should be refused"
            );
        }
    }

    #[test]
    fn a_secret_written_in_the_reference_is_not_echoed_in_the_error() {
        let error = UpstreamConfig::parse(&upstream(json!({"credential_ref": "s3cr3t-token"})))
            .expect_err("not a reference");
        assert!(!error.to_string().contains("s3cr3t-token"));
    }

    #[test]
    fn a_path_is_appended_to_the_base_and_cannot_leave_it() {
        let config = UpstreamConfig::parse(&upstream(json!({"scopes": ["/"]}))).expect("valid");
        assert_eq!(
            config.target("acme/lib 1.jar").expect("target").as_str(),
            "https://repo.example.com/releases/acme/lib%201.jar"
        );
        // No attempt to change the destination produces a URL.
        for hostile in ["../etc/passwd", "a/../../b", "a//b", "", "a/./b"] {
            assert!(config.target(hostile).is_none(), "{hostile:?}");
        }
        // A path that looks like a URL is just an encoded segment.
        let url = config.target("http:%2F%2Fevil.test/x").expect("segment");
        assert_eq!(url.host_str(), Some("repo.example.com"));
    }

    #[test]
    fn a_path_outside_the_scopes_has_no_target() {
        let config = UpstreamConfig::parse(&upstream(json!({}))).expect("valid");
        assert!(config.target("acme/lib.jar").is_some());
        assert!(config.target("other/lib.jar").is_none());
        // The prefix is per segment: `acme-evil/` does not fall in `acme/`.
        assert!(config.target("acme-evil/lib.jar").is_none());
    }

    #[test]
    fn the_settings_of_a_repository_without_proxy_are_not_an_upstream() {
        let config = RepositoryConfig::new(DigestScope::Global);
        assert_eq!(UpstreamConfig::of(&config), Ok(None));

        let mut broken = RepositoryConfig::new(DigestScope::Global);
        broken.config_settings = json!({"proxy": {}});
        assert!(UpstreamConfig::of(&broken).is_err());
    }

    #[test]
    fn a_key_the_proxy_settings_do_not_know_is_an_error_not_an_ignored_setting() {
        let mut config = RepositoryConfig::new(DigestScope::Global);
        config.config_settings =
            json!({"proxy": {"upstream": upstream(json!({})), "upstraem": {}}});
        assert!(UpstreamConfig::of(&config).is_err());
    }

    #[test]
    fn the_cache_policy_is_read_from_proxy_cache_and_defaults_when_absent() {
        let mut config = RepositoryConfig::new(DigestScope::Global);
        config.config_settings = json!({"proxy": {"upstream": upstream(json!({}))}});
        let plain = UpstreamConfig::of(&config)
            .expect("valid")
            .expect("upstream");
        assert_eq!(plain.cache, CachePolicy::default());

        config.config_settings = json!({"proxy": {
            "upstream": upstream(json!({})),
            "cache": {"ttl": {"metadata_secs": 60}, "negative_secs": 30, "offline": true},
        }});
        let cached = UpstreamConfig::of(&config)
            .expect("valid")
            .expect("upstream");
        assert_eq!(cached.cache.metadata_ttl, Some(60));
        assert_eq!(cached.cache.negative_secs, 30);
        assert!(cached.cache.offline);

        // A cache policy that is not understood breaks the whole
        // configuration: it does not degrade to "no cache".
        config.config_settings = json!({"proxy": {
            "upstream": upstream(json!({})),
            "cache": {"ttl": {"metadata_secs": "soon"}},
        }});
        assert!(UpstreamConfig::of(&config).is_err());
    }

    #[test]
    fn a_receipt_written_before_the_cache_existed_reads_as_fetched() {
        let old = json!({
            "id": "prv-1", "account": "acme", "repository": "central",
            "asset_path": "a/b", "upstream_id": "central",
            "source_url": "https://repo.example.com/a/b", "config_version": 1,
            "digest": serde_json::to_value(ContentDigest::of_bytes(b"x")).unwrap(),
            "size_bytes": 1, "checksum": {"kind": "computed_only"},
            "status": 200, "redirects": 0, "etag": null,
            "actor": "u", "correlation_id": "c",
            "fetched_at": "2024-01-01T00:00:00Z",
        });
        let receipt: ProvenanceReceipt = serde_json::from_value(old).expect("old shape");
        assert_eq!(receipt.outcome, ReceiptOutcome::Fetched);
        assert_eq!(receipt.last_modified, None);
        for outcome in [
            ReceiptOutcome::Fetched,
            ReceiptOutcome::Revalidated,
            ReceiptOutcome::Imported,
            ReceiptOutcome::Quarantined,
        ] {
            assert_eq!(ReceiptOutcome::parse(outcome.as_str()), Some(outcome));
            assert_eq!(
                serde_json::to_value(outcome).unwrap(),
                json!(outcome.as_str()),
                "the stored name and the wire name are one name"
            );
        }
        assert_eq!(ReceiptOutcome::parse("other"), None);
    }

    #[test]
    fn the_origin_has_no_path_and_no_credentials() {
        let url = Url::parse("https://repo.example.com/a/b?x=1").unwrap();
        assert_eq!(origin_of(&url), "https://repo.example.com:443");
    }
}
