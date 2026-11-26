//! Trusted publishers through temporal OIDC (HSP-152, ported in HSP-321 tramo j).
//!
//! A CI workload presents an OIDC ID token and gets a short credential that
//! can publish into ONE repository, and nothing else. No long-lived secret
//! leaves the platform.
//!
//! What decides what:
//!
//! - **The registry decides the binding.** A trusted publisher is a record an
//!   administrator created with the dedicated Cedar action
//!   `ManageTrustedPublishers` on the repository: issuer, audience, JWKS
//!   location, the exact subject and the exact extra claims. The token never
//!   says which account, repository or permission it gets; it only has to
//!   match a record of the repository the caller names. A claim that is
//!   missing or different means no.
//! - **The signature is checked against the issuer's JWKS** with an algorithm
//!   allow-list (RS256, ES256; `none` and HS* are refused before any key is
//!   looked at), exact `iss`/`aud`, `exp`/`nbf` with a small leeway, a
//!   freshness window on `iat`, and a mandatory single-use `jti`.
//! - **The JWKS travels through the administrator's egress** (the proxy
//!   fetcher): allow-list, no redirects, address pinning, byte budget. Keys
//!   are cached for a bounded time; an unknown `kid` triggers one refetch at
//!   most every cooldown (rotation). If the keys cannot be obtained, the
//!   exchange fails closed: there is no stale fallback.
//! - **The credential is minimal**: `upload` on the bound repository, a short
//!   lifetime, a distinct key and token type that no user surface accepts.
//!   The publish surface re-reads the registry, so deleting a record revokes.

pub mod credential;
pub mod dgraph;

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use base64::Engine as _;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId, VerifiedContext, verify_principal,
};
use jsonwebtoken::jwk::{Jwk, JwkSet, PublicKeyUse};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::{Deserialize, Serialize};
use url::Url;

use self::credential::{
    CredentialSigner, PublisherClaims, UPLOAD_ACTION, VerifiedPublisher, new_jti,
    workload_principal,
};
use crate::proxy::fetch::ProxyFetcher;

/// The algorithms an OIDC ID token may be signed with. Everything else
/// (`none`, HS*, PS*, EdDSA) is refused before a key is looked at.
const ALLOWED_ALGORITHMS: [Algorithm; 2] = [Algorithm::RS256, Algorithm::ES256];
/// The most trusted publishers one repository can have. The PUT enforces it
/// (a race may overshoot by a few); the exchange reads them all and refuses
/// to decide on a registry that returns far more, instead of silently
/// ignoring records.
/// The most JWKS locations kept in memory at once.
const MAX_JWKS_SLOTS: usize = 256;

pub const MAX_PUBLISHERS_PER_REPOSITORY: usize = 16;
/// The largest ID token the exchange reads.
const MAX_TOKEN_BYTES: usize = 8 * 1024;
/// Claims a record cannot pin: the standard ones are validated by the
/// exchange itself and `sub` has its own field.
const RESERVED_CLAIMS: [&str; 8] = ["iss", "aud", "exp", "nbf", "iat", "jti", "sub", "azp"];

/// The knobs of the service. The defaults are the safe ones.
#[derive(Debug, Clone)]
pub struct PublisherSettings {
    /// Lifetime of the credential minted by the exchange.
    pub credential_ttl: Duration,
    /// How old (`iat`) an ID token may be when it is exchanged.
    pub max_token_age: Duration,
    /// Clock skew tolerated for `exp`, `nbf` and a future `iat`.
    pub leeway: Duration,
    /// How long a fetched JWKS is trusted.
    pub jwks_ttl: Duration,
    /// The minimum time between two fetches of the same JWKS.
    pub jwks_refetch_cooldown: Duration,
    /// The timeout of one JWKS fetch.
    pub jwks_timeout: Duration,
    /// The byte budget of one JWKS document.
    pub jwks_max_bytes: u64,
    /// The largest number of `jti` remembered. When it is full of live
    /// entries the exchange refuses (fail closed), it never forgets one.
    pub replay_capacity: usize,
    /// Sustained exchanges per second the service attempts globally (a
    /// token bucket charged only when the exchange is about to cost a
    /// registry read or a network fetch).
    pub exchange_rate_per_sec: u32,
    /// The burst of that bucket.
    pub exchange_burst: u32,
}

impl Default for PublisherSettings {
    fn default() -> Self {
        Self {
            credential_ttl: Duration::from_secs(300),
            max_token_age: Duration::from_secs(600),
            leeway: Duration::from_secs(30),
            jwks_ttl: Duration::from_secs(300),
            jwks_refetch_cooldown: Duration::from_secs(30),
            jwks_timeout: Duration::from_secs(5),
            jwks_max_bytes: 64 * 1024,
            replay_capacity: 8192,
            exchange_rate_per_sec: 20,
            exchange_burst: 40,
        }
    }
}

impl PublisherSettings {
    fn clamped(mut self) -> Self {
        self.credential_ttl = self.credential_ttl.min(Duration::from_secs(900));
        self.leeway = self.leeway.min(Duration::from_secs(120));
        self
    }
}

/// A trusted publisher: the administrator's binding of an OIDC identity to
/// the publication into one repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedPublisher {
    pub id: String,
    pub account: String,
    pub repository: String,
    /// The exact `iss` the token must carry.
    pub issuer: String,
    /// The exact `aud` the token must carry.
    pub audience: String,
    /// Where the issuer's keys are fetched from (through the egress).
    pub jwks_url: String,
    /// The exact `sub` the token must carry.
    pub subject: String,
    /// Further claims the token must carry with exactly these string values.
    #[serde(default)]
    pub claims: BTreeMap<String, String>,
    /// Who registered it.
    pub created_by: String,
    pub created_at: String,
}

impl TrustedPublisher {
    /// A digest of everything that binds an identity to the repository (not
    /// of who wrote it or when).
    pub fn fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        let canonical = serde_json::json!([
            self.id,
            self.account,
            self.repository,
            self.issuer,
            self.audience,
            self.jwks_url,
            self.subject,
            self.claims,
        ]);
        Sha256::digest(canonical.to_string().as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

/// What an administrator sends to register a publisher.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublisherSpec {
    pub issuer: String,
    pub audience: String,
    pub jwks_url: String,
    pub subject: String,
    #[serde(default)]
    pub claims: BTreeMap<String, String>,
}

pub fn is_identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

impl PublisherSpec {
    /// Validate the shape of a record. The egress allow-list is checked by
    /// the caller, which owns the fetcher.
    pub fn validate(&self, id: &str) -> Result<(), &'static str> {
        if !is_identifier(id, 64) {
            return Err("the publisher id is not a valid identifier");
        }
        let url_ok = |text: &str| {
            text.len() <= 512
                && Url::parse(text).is_ok_and(|url| {
                    matches!(url.scheme(), "http" | "https")
                        && url.host_str().is_some()
                        && url.username().is_empty()
                        && url.password().is_none()
                        && url.fragment().is_none()
                })
        };
        if !url_ok(&self.issuer) {
            return Err("the issuer is not a valid url");
        }
        if !url_ok(&self.jwks_url) || Url::parse(&self.jwks_url).is_ok_and(|u| u.query().is_some())
        {
            return Err("the jwks_url is not a valid url");
        }
        let text_ok = |text: &str| {
            !text.trim().is_empty()
                && text == text.trim()
                && text.len() <= 256
                && !text.contains('*')
                && !text.chars().any(char::is_control)
        };
        if !text_ok(&self.audience) || !text_ok(&self.subject) {
            return Err("the audience and the subject must be exact, non-empty values");
        }
        if self.claims.len() > 8 {
            return Err("too many pinned claims");
        }
        for (name, value) in &self.claims {
            if !is_identifier(name, 64) || RESERVED_CLAIMS.contains(&name.as_str()) {
                return Err("a pinned claim name is not allowed");
            }
            if !text_ok(value) {
                return Err("a pinned claim value must be exact and non-empty");
            }
        }
        Ok(())
    }
}

/// Why the registry could not answer.
#[derive(Debug, thiserror::Error)]
#[error("the trusted publishers registry is unavailable: {0}")]
pub struct RegistryError(pub String);

/// The registry of trusted publishers.
#[async_trait]
pub trait PublisherRegistry: Send + Sync {
    /// Create or replace the record with this id in the repository.
    async fn put(&self, publisher: &TrustedPublisher) -> Result<(), RegistryError>;
    /// Delete a record. `true` when it existed.
    async fn delete(
        &self,
        account: &str,
        repository: &str,
        id: &str,
    ) -> Result<bool, RegistryError>;
    /// The records of one repository of one account.
    async fn list(
        &self,
        account: &str,
        repository: &str,
    ) -> Result<Vec<TrustedPublisher>, RegistryError>;
}

/// A registry in memory: for tests and tools; the deployment uses Dgraph.
#[derive(Default)]
pub struct InMemoryPublisherRegistry {
    records: Mutex<BTreeMap<(String, String, String), TrustedPublisher>>,
}

#[async_trait]
impl PublisherRegistry for InMemoryPublisherRegistry {
    async fn put(&self, p: &TrustedPublisher) -> Result<(), RegistryError> {
        let mut records = self
            .records
            .lock()
            .map_err(|_| RegistryError("lock".into()))?;
        records.insert(
            (p.account.clone(), p.repository.clone(), p.id.clone()),
            p.clone(),
        );
        Ok(())
    }

    async fn delete(
        &self,
        account: &str,
        repository: &str,
        id: &str,
    ) -> Result<bool, RegistryError> {
        let mut records = self
            .records
            .lock()
            .map_err(|_| RegistryError("lock".into()))?;
        Ok(records
            .remove(&(account.to_string(), repository.to_string(), id.to_string()))
            .is_some())
    }

    async fn list(
        &self,
        account: &str,
        repository: &str,
    ) -> Result<Vec<TrustedPublisher>, RegistryError> {
        let records = self
            .records
            .lock()
            .map_err(|_| RegistryError("lock".into()))?;
        Ok(records
            .iter()
            .filter(|((a, r, _), _)| a == account && r == repository)
            .map(|(_, p)| p.clone())
            .collect())
    }
}

/// Why an exchange did not produce a credential. The detail is for the
/// service trail; the client only ever sees [`ExchangeError::status`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangeError {
    /// The request itself is malformed.
    BadRequest,
    /// The token or its claims did not earn a credential.
    Denied(&'static str),
    /// The registry or the issuer's keys could not be obtained: fail closed.
    Unavailable(&'static str),
    /// The service-wide exchange rate budget is spent.
    RateLimited,
}

impl ExchangeError {
    /// The internal reason (never sent to the client, never has the token).
    pub fn reason(&self) -> &'static str {
        match self {
            Self::BadRequest => "bad_request",
            Self::RateLimited => "rate_limited",
            Self::Denied(reason) | Self::Unavailable(reason) => reason,
        }
    }
}

/// The verified outcome of an exchange.
#[derive(Debug, Clone)]
pub struct Exchange {
    pub credential: String,
    pub workload: String,
    pub account: String,
    pub repository: String,
    pub expires_in: u64,
}

/// The state of one JWKS location: the keys, when they were fetched and when
/// the last fetch FAILED (the negative cache).
#[derive(Default)]
struct JwksCache {
    set: Option<JwkSet>,
    fetched: Option<Instant>,
    failed: Option<Instant>,
}

/// One JWKS location. Reads of the cache never wait for the network; the
/// `flight` lock makes the fetch of THIS url single-flight, and no other url
/// ever waits on it.
#[derive(Default)]
struct JwksSlot {
    cache: Mutex<JwksCache>,
    flight: tokio::sync::Mutex<()>,
}

/// A token bucket.
struct Bucket {
    tokens: f64,
    last: Instant,
}

/// The trusted publishers service.
pub struct PublishersService {
    registry: Arc<dyn PublisherRegistry>,
    fetcher: ProxyFetcher,
    signer: CredentialSigner,
    settings: PublisherSettings,
    jwks: Mutex<HashMap<String, (Arc<JwksSlot>, Instant)>>,
    bucket: Mutex<Bucket>,
    /// `(issuer, jti)` already exchanged, until the token's own expiry.
    exchanged: Mutex<HashMap<String, u64>>,
}

impl PublishersService {
    /// Build the service. The secret must be at least 32 bytes.
    pub fn new(
        registry: Arc<dyn PublisherRegistry>,
        fetcher: ProxyFetcher,
        credential_secret: &str,
        settings: PublisherSettings,
    ) -> Result<Self, String> {
        let burst = f64::from(settings.exchange_burst.max(1));
        Ok(Self {
            registry,
            fetcher,
            signer: CredentialSigner::derive(credential_secret.as_bytes())?,
            settings: settings.clamped(),
            jwks: Mutex::new(HashMap::new()),
            bucket: Mutex::new(Bucket {
                tokens: burst,
                last: Instant::now(),
            }),
            exchanged: Mutex::new(HashMap::new()),
        })
    }

    /// The registry, for the management surface.
    pub fn registry(&self) -> &Arc<dyn PublisherRegistry> {
        &self.registry
    }

    /// The egress client, to judge a JWKS location when it is registered.
    pub fn fetcher(&self) -> &ProxyFetcher {
        &self.fetcher
    }

    /// Exchange an OIDC ID token for a publisher credential into
    /// `repository` of `account`.
    ///
    /// `account` and `repository` are what the caller *asks for*; they only
    /// select the records to check. The grant is exactly what a record of
    /// that repository says, and only if the token proves it.
    pub async fn exchange(
        &self,
        token: &str,
        account: &str,
        repository: &str,
    ) -> Result<Exchange, ExchangeError> {
        if !is_identifier(account, 128) || !is_identifier(repository, 128) {
            return Err(ExchangeError::BadRequest);
        }
        if token.is_empty() || token.len() > MAX_TOKEN_BYTES {
            return Err(ExchangeError::Denied("token_size"));
        }
        // The cheap checks come first: nothing below touches the registry or
        // the network for a token that does not even parse.
        if header_has_crit(token) {
            return Err(ExchangeError::Denied("crit_header"));
        }
        let header = decode_header(token).map_err(|_| ExchangeError::Denied("token_malformed"))?;
        if !ALLOWED_ALGORITHMS.contains(&header.alg) {
            return Err(ExchangeError::Denied("algorithm_not_allowed"));
        }
        let issuer = unverified_issuer(token).ok_or(ExchangeError::Denied("token_malformed"))?;

        // From here on the exchange costs a registry read and maybe a fetch:
        // the service-wide budget is charged.
        self.take_token()?;
        let records = self
            .registry
            .list(account, repository)
            .await
            .map_err(|_| ExchangeError::Unavailable("registry_unavailable"))?;
        if records.len() > 2 * MAX_PUBLISHERS_PER_REPOSITORY {
            // Never decide on a registry that may hide records.
            return Err(ExchangeError::Unavailable("registry_oversized"));
        }
        let mut records = records;
        records.sort_by(|a, b| a.id.cmp(&b.id));
        let candidates: Vec<&TrustedPublisher> = records
            .iter()
            .filter(|record| record.issuer == issuer)
            .collect();
        if candidates.is_empty() {
            return Err(ExchangeError::Denied("no_trusted_publisher"));
        }

        let mut unavailable = false;
        for candidate in candidates {
            let claims = match self.verify_oidc(token, &header, candidate).await {
                Ok(claims) => claims,
                Err(ExchangeError::Unavailable(_)) => {
                    unavailable = true;
                    continue;
                }
                Err(_) => continue,
            };
            if !claims_match(candidate, &claims) {
                continue;
            }
            // Only a token that earned the grant burns its `jti`: an
            // unsigned or foreign token cannot exhaust the replay cache.
            let jti = claims
                .get("jti")
                .and_then(|v| v.as_str())
                .filter(|v| !v.is_empty() && v.len() <= 256)
                .ok_or(ExchangeError::Denied("jti_missing"))?;
            let iat = claims.get("iat").and_then(|v| v.as_u64()).unwrap_or(0);
            self.mark_exchanged(&issuer, jti, iat)?;
            let subject = candidate.subject.as_str();
            return self.mint(candidate, subject);
        }
        if unavailable {
            Err(ExchangeError::Unavailable("jwks_unavailable"))
        } else {
            Err(ExchangeError::Denied("token_not_trusted"))
        }
    }

    fn mint(&self, record: &TrustedPublisher, subject: &str) -> Result<Exchange, ExchangeError> {
        let now = unix_now() as i64;
        let workload = workload_principal(&record.account, subject);
        let ttl = self.settings.credential_ttl.as_secs();
        let claims = PublisherClaims {
            iss: credential::CREDENTIAL_ISSUER.to_string(),
            aud: credential::CREDENTIAL_AUDIENCE.to_string(),
            sub: workload.clone(),
            account: record.account.clone(),
            repo: record.repository.clone(),
            acts: vec![UPLOAD_ACTION.to_string()],
            publisher: record.id.clone(),
            fp: record.fingerprint(),
            iat: now,
            exp: now + ttl as i64,
            jti: new_jti(),
        };
        let credential = self
            .signer
            .mint(&claims)
            .map_err(|_| ExchangeError::Unavailable("credential_signing"))?;
        Ok(Exchange {
            credential,
            workload,
            account: record.account.clone(),
            repository: record.repository.clone(),
            expires_in: ttl,
        })
    }

    /// The signature, issuer, audience and time checks of the ID token
    /// against the record. Returns the raw claims.
    async fn verify_oidc(
        &self,
        token: &str,
        header: &jsonwebtoken::Header,
        record: &TrustedPublisher,
    ) -> Result<serde_json::Map<String, serde_json::Value>, ExchangeError> {
        let jwk = self.the_jwk(record, header).await?;
        if let Some(alg) = jwk.common.key_algorithm
            && format!("{alg:?}") != format!("{:?}", header.alg)
        {
            return Err(ExchangeError::Denied("key_algorithm_mismatch"));
        }
        let key = DecodingKey::from_jwk(&jwk).map_err(|_| ExchangeError::Denied("key_unusable"))?;
        let mut validation = Validation::new(header.alg);
        validation.algorithms = vec![header.alg];
        validation.set_issuer(&[record.issuer.as_str()]);
        validation.set_audience(&[record.audience.as_str()]);
        validation.leeway = self.settings.leeway.as_secs();
        validation.validate_exp = true;
        validation.validate_nbf = true;
        validation.required_spec_claims = ["exp", "iss", "aud", "sub"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let claims = decode::<serde_json::Map<String, serde_json::Value>>(token, &key, &validation)
            .map_err(|_| ExchangeError::Denied("token_invalid"))?
            .claims;
        let now = unix_now();
        let iat = claims
            .get("iat")
            .and_then(|v| v.as_u64())
            .ok_or(ExchangeError::Denied("iat_missing"))?;
        let leeway = self.settings.leeway.as_secs();
        if iat > now + leeway
            || now.saturating_sub(iat) > self.settings.max_token_age.as_secs() + leeway
        {
            return Err(ExchangeError::Denied("token_not_fresh"));
        }
        Ok(claims)
    }

    /// The key of the issuer for the token's `kid`, through the cache and
    /// the egress. Fails closed.
    ///
    /// Reads of a fresh cache never wait for the network. A fetch is
    /// single-flight PER URL (another issuer never waits on it), and a failed
    /// fetch is remembered: until the cooldown passes, every request fails
    /// without touching the network, whether the keys had expired or not.
    async fn the_jwk(
        &self,
        record: &TrustedPublisher,
        header: &jsonwebtoken::Header,
    ) -> Result<Jwk, ExchangeError> {
        let kid = header.kid.as_deref();
        let slot = self.slot(&record.jwks_url)?;
        if let Some(decided) = self.cached_decision(&slot, kid)? {
            return Ok(decided);
        }
        let _flight = slot.flight.lock().await;
        // Another request may have fetched (or failed) while this one waited.
        if let Some(decided) = self.cached_decision(&slot, kid)? {
            return Ok(decided);
        }
        let url = Url::parse(&record.jwks_url).map_err(|_| ExchangeError::Denied("jwks_url"))?;
        let fetched = self
            .fetcher
            .get_bytes(
                &url,
                self.settings.jwks_timeout,
                self.settings.jwks_max_bytes,
            )
            .await
            .ok()
            .and_then(|bytes| serde_json::from_slice::<JwkSet>(&bytes).ok());
        let mut cache = slot
            .cache
            .lock()
            .map_err(|_| ExchangeError::Unavailable("jwks_cache"))?;
        match fetched {
            None => {
                cache.failed = Some(Instant::now());
                Err(ExchangeError::Unavailable("jwks_unavailable"))
            }
            Some(set) => {
                let selected = select_key(&set, kid);
                cache.set = Some(set);
                cache.fetched = Some(Instant::now());
                cache.failed = None;
                selected.ok_or(ExchangeError::Denied("kid_unknown"))
            }
        }
    }

    /// What the cache alone decides: a key, a refusal, or `None` when a
    /// fetch is due.
    fn cached_decision(
        &self,
        slot: &JwksSlot,
        kid: Option<&str>,
    ) -> Result<Option<Jwk>, ExchangeError> {
        let cache = slot
            .cache
            .lock()
            .map_err(|_| ExchangeError::Unavailable("jwks_cache"))?;
        // A fresh set that holds the requested key is served even while the
        // negative cache is active: only going to the network is throttled.
        let mut refetch_throttled = false;
        if let (Some(set), Some(fetched)) = (&cache.set, cache.fetched) {
            let age = fetched.elapsed();
            if age < self.settings.jwks_ttl {
                if let Some(jwk) = select_key(set, kid) {
                    return Ok(Some(jwk));
                }
                // An unknown kid on a fresh set: one refetch per cooldown
                // (the issuer may have rotated), otherwise no.
                refetch_throttled = age < self.settings.jwks_refetch_cooldown;
            }
        }
        if let Some(failed) = cache.failed
            && failed.elapsed() < self.settings.jwks_refetch_cooldown
        {
            return Err(ExchangeError::Unavailable("jwks_unavailable"));
        }
        if refetch_throttled {
            return Err(ExchangeError::Denied("kid_unknown"));
        }
        Ok(None)
    }

    /// The slot of a url. The map is bounded: when it is full the least
    /// recently used slot (preferring one nobody is using) is evicted, so a
    /// flood of distinct urls can only cost refetches, never block anyone.
    fn slot(&self, url: &str) -> Result<Arc<JwksSlot>, ExchangeError> {
        let mut slots = self
            .jwks
            .lock()
            .map_err(|_| ExchangeError::Unavailable("jwks_cache"))?;
        let now = Instant::now();
        if let Some((slot, used)) = slots.get_mut(url) {
            *used = now;
            return Ok(slot.clone());
        }
        if slots.len() >= MAX_JWKS_SLOTS {
            let victim = slots
                .iter()
                .min_by_key(|(_, (slot, used))| (Arc::strong_count(slot) > 1, *used))
                .map(|(key, _)| key.clone());
            if let Some(victim) = victim {
                slots.remove(&victim);
            }
        }
        let slot = Arc::new(JwksSlot::default());
        slots.insert(url.to_string(), (slot.clone(), now));
        Ok(slot)
    }

    /// Charge the service-wide token bucket.
    fn take_token(&self) -> Result<(), ExchangeError> {
        let mut bucket = self
            .bucket
            .lock()
            .map_err(|_| ExchangeError::Unavailable("rate_limiter"))?;
        let rate = f64::from(self.settings.exchange_rate_per_sec.max(1));
        let burst = f64::from(self.settings.exchange_burst.max(1));
        let now = Instant::now();
        bucket.tokens =
            (bucket.tokens + now.duration_since(bucket.last).as_secs_f64() * rate).min(burst);
        bucket.last = now;
        if bucket.tokens < 1.0 {
            return Err(ExchangeError::RateLimited);
        }
        bucket.tokens -= 1.0;
        Ok(())
    }

    fn mark_exchanged(&self, issuer: &str, jti: &str, iat: u64) -> Result<(), ExchangeError> {
        let mut seen = self
            .exchanged
            .lock()
            .map_err(|_| ExchangeError::Unavailable("replay_cache"))?;
        let now = unix_now();
        let key = format!("{issuer}\n{jti}");
        if seen.contains_key(&key) {
            return Err(ExchangeError::Denied("replayed"));
        }
        // An entry lives until the token can no longer pass the freshness
        // window on `iat` (never until its own `exp`, which a token may set
        // to the far future to pin the cache).
        seen.retain(|_, until| *until >= now);
        if seen.len() >= self.settings.replay_capacity {
            return Err(ExchangeError::Unavailable("replay_cache_full"));
        }
        let until = iat
            .saturating_add(self.settings.max_token_age.as_secs())
            .saturating_add(self.settings.leeway.as_secs());
        seen.insert(key, until);
        Ok(())
    }

    /// Verify a publisher credential and that its registry record still
    /// exists. `None` for anything that is not a live publisher credential.
    pub async fn verify_credential(&self, token: &str) -> Result<VerifiedPublisher, GateError> {
        let verified = self.signer.verify(token).map_err(|_| GateError::Invalid)?;
        let claims = &verified.claims;
        let records = self
            .registry
            .list(&claims.account, &claims.repo)
            .await
            .map_err(|_| GateError::Unavailable)?;
        if !records
            .iter()
            .any(|record| record.id == claims.publisher && record.fingerprint() == claims.fp)
        {
            return Err(GateError::Invalid);
        }
        Ok(verified)
    }
}

/// Why the publish surface refused a publisher credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateError {
    /// Not a live publisher credential.
    Invalid,
    /// A valid credential used outside its repository or actions.
    OutsideScope,
    /// The registry could not answer.
    Unavailable,
}

/// The authenticator that turns a verified publisher credential into the
/// same principal seam every identity uses.
struct CredentialAuthenticator<'a>(&'a CredentialSigner);

#[async_trait]
impl PrincipalAuthenticator for CredentialAuthenticator<'_> {
    async fn authenticate(
        &self,
        credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        let Credentials::Bearer(token) = credentials else {
            return Err(AuthenticationError::MissingCredentials);
        };
        let verified = self
            .0
            .verify(token)
            .map_err(|_| AuthenticationError::InvalidCredentials)?;
        Ok(PrincipalClaims::new(
            PrincipalId::new(verified.claims.sub)
                .map_err(|_| AuthenticationError::InvalidCredentials)?,
            AccountId::new(verified.claims.account)
                .map_err(|_| AuthenticationError::InvalidCredentials)?,
        ))
    }
}

impl PublishersService {
    /// The verified context of a publisher credential: the same seam every
    /// other identity comes from, so nothing downstream builds a principal
    /// by hand.
    pub async fn context_of(&self, token: &str) -> Result<VerifiedContext, GateError> {
        let principal = verify_principal(
            &CredentialAuthenticator(&self.signer),
            &Credentials::bearer(token),
        )
        .await
        .map_err(|_| GateError::Invalid)?;
        Ok(VerifiedContext::from_principal(principal))
    }
}

fn select_key(set: &JwkSet, kid: Option<&str>) -> Option<Jwk> {
    let usable = |jwk: &&Jwk| {
        jwk.common
            .public_key_use
            .as_ref()
            .is_none_or(|usage| *usage == PublicKeyUse::Signature)
    };
    match kid {
        Some(kid) => set
            .keys
            .iter()
            .filter(usable)
            .find(|jwk| jwk.common.key_id.as_deref() == Some(kid))
            .cloned(),
        // Which key signed is not decidable with several keys and no kid.
        None if set.keys.len() == 1 => set.keys.iter().find(usable).cloned(),
        None => None,
    }
}

/// Every pin of the record has to be in the token with exactly that value.
fn claims_match(
    record: &TrustedPublisher,
    claims: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    if claims.get("sub").and_then(|v| v.as_str()) != Some(record.subject.as_str()) {
        return false;
    }
    record
        .claims
        .iter()
        .all(|(name, value)| claims.get(name).and_then(|v| v.as_str()) == Some(value.as_str()))
}

/// Whether the protected header carries `crit`: extensions this service does
/// not implement must make the token unacceptable (RFC 7515 4.1.11).
fn header_has_crit(token: &str) -> bool {
    let Some(segment) = token.split('.').next() else {
        return false;
    };
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(segment)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|header| header.get("crit").is_some())
}

/// The `iss` of the payload, unverified: it only selects candidates; the
/// verified decode pins it afterwards.
fn unverified_issuer(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value.get("iss")?.as_str().map(str::to_string)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}
