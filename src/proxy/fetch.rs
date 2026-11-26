//! The client that goes out to fetch content from an upstream.
//!
//! It does a single thing: open a `200` response from an allowed upstream,
//! following redirects **by hand**, so each hop is decided by the egress
//! policy and not by the one the HTTP client carries. What it delivers is
//! not verified content: it is an open response and a
//! [`BodyGuard`] that checks, chunk by chunk and at the end, that what was
//! received is complete, does not exceed the budget and matches what the
//! upstream declared.
//!
//! Decisions worth keeping in view:
//!
//! - **No environment proxies and no automatic redirects.** The client
//!   is built with `no_proxy` and `Policy::none()`: neither `HTTP_PROXY` nor a
//!   `Location` decides where to go.
//! - **Pinned resolution.** The host is resolved here, validated, and the
//!   connection is made to those same addresses (`resolve_to_addrs`). TLS
//!   still verifies the host name, not the IP.
//! - **Client per hop.** Each hop has its own pinned resolution,
//!   so each hop has its own client and no reused connection
//!   can lead to a destination that was not validated.
//! - **The credential does not leave its origin.** It is only sent to the origin of the
//!   configured upstream, never to another one a redirect leads to.
//! - **No compression.** `identity` is requested: an upstream that compresses
//!   anyway is not accepted, because the digest and the length refer to the
//!   bytes exactly as they are on disk.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use hodei_catalog::{ChecksumEvidence, CredentialRef, UpstreamConfig, origin_of};
use reqwest::header::{
    ACCEPT, ACCEPT_ENCODING, AUTHORIZATION, CONTENT_ENCODING, HeaderValue, LOCATION,
    WWW_AUTHENTICATE,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use url::{Host, Url};

use super::egress::{EgressDenied, EgressPolicy, HostResolver, SystemResolver};

/// The header where the upstream declares the content checksum.
pub const CHECKSUM_HEADER: &str = "x-checksum-sha256";

/// A secret: it can be used and cannot be displayed.
///
/// It does not implement `Display`, and its `Debug` hides it: a secret that can
/// be formatted ends up in a log.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    /// Wraps a secret.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

/// Where secrets come from.
pub trait SecretSource: Send + Sync {
    /// The secret of a reference, if it exists.
    fn get(&self, reference: &CredentialRef) -> Option<Secret>;
}

/// The process-environment secrets the service has authorized.
///
/// A repository's configuration only **names** a variable; whether that
/// variable may leave the process is decided by the service operator with
/// a list of names. Without a list none is read: a repository cannot
/// point its `credential_ref` at a secret that is not a proxy one (the
/// database, another account's token) to send it to an upstream.
#[derive(Debug, Default, Clone)]
pub struct EnvSecrets {
    allowed: std::collections::BTreeSet<String>,
}

impl EnvSecrets {
    /// Only these variables may be used as a credential.
    pub fn allowing<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            allowed: names.into_iter().map(Into::into).collect(),
        }
    }
}

impl SecretSource for EnvSecrets {
    fn get(&self, reference: &CredentialRef) -> Option<Secret> {
        if !self.allowed.contains(reference.env_name()) {
            return None;
        }
        std::env::var(reference.env_name())
            .ok()
            .filter(|value| !value.is_empty())
            .map(Secret::new)
    }
}

/// The service limits of an egress call.
#[derive(Debug, Clone, Copy)]
pub struct FetchLimits {
    /// What is waited for connecting and resolving.
    pub connect_timeout: Duration,
    /// How long an entire request lasts, body included.
    pub total_timeout: Duration,
    /// The ceiling of bytes of a response. A repository may narrow it,
    /// not widen it.
    pub max_bytes: u64,
}

impl Default for FetchLimits {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(5),
            total_timeout: Duration::from_secs(300),
            max_bytes: 1 << 30,
        }
    }
}

/// Why content could not be fetched.
///
/// No case carries the URL or the upstream response: the client that
/// requests the content has no business seeing the service's internal network.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    /// The egress policy does not allow going there.
    #[error("egress denied: {0}")]
    Egress(#[from] EgressDenied),
    /// The path is not among the upstream's scopes.
    #[error("the path is outside the scopes of the upstream")]
    OutOfScope,
    /// The upstream does not have that content.
    #[error("the upstream does not have that content")]
    NotFound,
    /// The upstream responded with a status that is not content.
    #[error("the upstream answered with status {0}")]
    Status(u16),
    /// The redirect is not allowed.
    #[error("redirect refused: {0}")]
    Redirect(&'static str),
    /// The upstream did not respond in time.
    #[error("the upstream timed out")]
    Timeout,
    /// Could not talk to the upstream (network, TLS, protocol).
    #[error("the upstream could not be reached")]
    Unreachable,
    /// The response is not usable.
    #[error("the response is not usable: {0}")]
    BadResponse(&'static str),
    /// The response ended earlier than it said.
    #[error("the response was cut short")]
    Truncated,
    /// The response is larger than what is admitted.
    #[error("the content is larger than allowed")]
    TooLarge,
    /// The checksum the upstream declared is not that of the content.
    #[error("the checksum declared by the upstream does not match the content")]
    ChecksumMismatch,
    /// The repository requires a checksum and the upstream did not declare one.
    #[error("the upstream did not declare a checksum and the repository requires one")]
    ChecksumMissing,
    /// The configured credential is not available.
    #[error("the credential of the upstream is not available")]
    Credential,
    /// The upstream asks for authentication and the challenge cannot be satisfied.
    ///
    /// The challenge is carried inside because it says **how** to authenticate: without
    /// it, whoever receives the error cannot even look at the realm.
    #[error("the upstream asks for a credential: {0}")]
    Unauthorized(String),
    /// There are too many simultaneous egress calls.
    #[error("too many fetches are in flight")]
    Busy,
    /// The service does not go out to the network: an egress was requested in offline mode.
    #[error("the service is offline and does not reach any upstream")]
    Offline,
}

/// The validators used to ask the upstream whether content that is already
/// held is still valid.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Validators {
    /// Sent as `If-None-Match`.
    pub etag: Option<String>,
    /// Sent as `If-Modified-Since`.
    pub last_modified: Option<String>,
}

impl Validators {
    /// Whether there is none to send.
    pub fn is_empty(&self) -> bool {
        self.etag.is_none() && self.last_modified.is_none()
    }
}

/// What the upstream answers to a request that may be conditional.
#[derive(Debug)]
pub enum Fetched {
    /// The upstream delivers the content (`200`).
    Content(Box<Opened>),
    /// The upstream confirms that the content held is still valid
    /// (`304`): there is no body.
    NotModified(NotModified),
    /// The upstream asks for authentication (`401`): the challenge says how to ask for it.
    ///
    /// The challenge is **not** followed blindly: whoever receives it decides whether the realm
    /// it names is the one the configuration declares. A challenge that points
    /// elsewhere receives no credential.
    Unauthorized {
        /// The value of the `WWW-Authenticate` header, if the upstream sent it.
        challenge: Option<String>,
    },
}

/// A credential and the origin it may travel to.
///
/// The credential and its origin go together: a credential without an origin is a
/// credential that can end up anywhere, and that is exactly
/// what a hostile redirect tries.
#[derive(Debug)]
struct TheCredential<'a> {
    /// Which credential it is.
    kind: TheCredentialKind<'a>,
    /// The origin that may receive it: scheme, host and port, exact.
    origin: &'a str,
}

/// Which credential it is.
#[derive(Debug)]
enum TheCredentialKind<'a> {
    /// The upstream's credential, as `Bearer`.
    Bearer(&'a Secret),
    /// The upstream's credential, as Basic: the one sent to the challenge's
    /// realm.
    Basic {
        /// The user name, which is informational.
        user: &'a str,
        /// The password.
        password: &'a Secret,
    },
    /// A token already obtained from the challenge's realm.
    Token(&'a str),
}

/// The confirmation of a `304`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotModified {
    /// Where the confirmation came from, after redirects, without query.
    pub source_url: String,
    /// The redirects that were followed.
    pub redirects: u8,
    /// The `ETag` the `304` declared, if it declared one.
    pub etag: Option<String>,
    /// The `Last-Modified` the `304` declared, if it declared one.
    pub last_modified: Option<String>,
}

/// The service's egress client.
#[derive(Clone)]
pub struct ProxyFetcher {
    policy: Arc<EgressPolicy>,
    resolver: Arc<dyn HostResolver>,
    secrets: Arc<dyn SecretSource>,
    limits: FetchLimits,
    permits: Arc<Semaphore>,
    offline: bool,
}

impl std::fmt::Debug for ProxyFetcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyFetcher")
            .field("policy", &self.policy)
            .field("limits", &self.limits)
            .field("offline", &self.offline)
            .finish_non_exhaustive()
    }
}

impl ProxyFetcher {
    /// A client with the system's resolver and secrets.
    pub fn new(policy: EgressPolicy, limits: FetchLimits, max_concurrent: usize) -> Self {
        Self {
            policy: Arc::new(policy),
            resolver: Arc::new(SystemResolver),
            secrets: Arc::new(EnvSecrets::default()),
            limits,
            permits: Arc::new(Semaphore::new(max_concurrent.max(1))),
            offline: false,
        }
    }

    /// A client that does not go out to the network: every request fails with
    /// [`FetchError::Offline`] before resolving a name.
    ///
    /// It is the last line of defense of offline mode: even if the cache decision
    /// were wrong, no code path reaches an upstream.
    #[must_use]
    pub fn with_offline(mut self, offline: bool) -> Self {
        self.offline = offline;
        self
    }

    /// Whether the client is in offline mode.
    pub fn is_offline(&self) -> bool {
        self.offline
    }

    /// With another resolver, to test rebind.
    #[must_use]
    pub fn with_resolver(mut self, resolver: Arc<dyn HostResolver>) -> Self {
        self.resolver = resolver;
        self
    }

    /// With these environment variables as the only possible credentials.
    #[must_use]
    pub fn with_allowed_credentials<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.secrets = Arc::new(EnvSecrets::allowing(names));
        self
    }

    /// With another source of secrets.
    #[must_use]
    pub fn with_secrets(mut self, secrets: Arc<dyn SecretSource>) -> Self {
        self.secrets = secrets;
        self
    }

    /// Opens the response for the content of `asset_path`.
    ///
    /// It only returns a `200` response from a destination allowed on all
    /// of its hops. What it returns is **not verified**: whoever reads it
    /// passes the chunks through the [`BodyGuard`].
    pub async fn open(
        &self,
        upstream: &UpstreamConfig,
        asset_path: &str,
    ) -> Result<Opened, FetchError> {
        match self.open_conditional(upstream, asset_path, None).await? {
            Fetched::Content(opened) => Ok(*opened),
            // Without validators there is nothing to confirm: an unsolicited `304`
            // is a response that is not content.
            Fetched::NotModified(_) => Err(FetchError::Status(304)),
            // A challenge is not content: the caller has to resolve it.
            Fetched::Unauthorized { challenge } => {
                Err(FetchError::Unauthorized(challenge.unwrap_or_default()))
            }
        }
    }

    /// Like [`Self::open`], but asking the upstream whether the content
    /// already held is still valid.
    ///
    /// The validators are sent **only to the upstream's origin**, not to an
    /// origin a redirect leads to. A `304` without validators
    /// sent is not a valid response.
    pub async fn open_conditional(
        &self,
        upstream: &UpstreamConfig,
        asset_path: &str,
        validators: Option<&Validators>,
    ) -> Result<Fetched, FetchError> {
        // What does not fall within a scope does not consume even a permit.
        let url = upstream.target(asset_path).ok_or(FetchError::OutOfScope)?;
        self.open_conditional_at(upstream, url, None, validators)
            .await
    }

    /// Like [`Self::open_conditional`], but with the exact URL and the `Accept`
    /// that is requested.
    ///
    /// Used by the PyPI surface, whose upstream speaks JSON: a project's page
    /// is requested at the URL the upstream itself declares, and with an
    /// `Accept` saying the Simple API JSON is wanted.
    ///
    /// The URL does **not** bypass the egress policy: it is judged like
    /// any other, hop by hop.
    pub async fn open_conditional_at(
        &self,
        upstream: &UpstreamConfig,
        url: Url,
        accept: Option<&str>,
        validators: Option<&Validators>,
    ) -> Result<Fetched, FetchError> {
        self.open_conditional_with_token(upstream, url, accept, validators, None)
            .await
    }

    /// Like [`Self::open_conditional_at`], but with a **token** that is
    /// already held.
    ///
    /// The token is the one the upstream asked for in its challenge: it travels as `Bearer`
    /// and **only** to the upstream's origin. A token is a credential, and a
    /// credential that travels to an origin a redirect leads to is a
    /// leaked credential.
    ///
    /// Without a token, the credential that travels is the one declared by the
    /// upstream's configuration, also only to its origin.
    pub async fn open_conditional_with_token(
        &self,
        upstream: &UpstreamConfig,
        url: Url,
        accept: Option<&str>,
        validators: Option<&Validators>,
        token: Option<&str>,
    ) -> Result<Fetched, FetchError> {
        if self.offline {
            return Err(FetchError::Offline);
        }
        let validators = validators.filter(|validators| !validators.is_empty());
        let permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| FetchError::Busy)?;
        // The time cap is for the entire resolution, not for each hop:
        // a chain of slow redirects does not hold the permit any longer.
        let deadline = tokio::time::Instant::now() + self.limits.total_timeout;
        let budget = upstream
            .max_bytes
            .map_or(self.limits.max_bytes, |own| own.min(self.limits.max_bytes));
        let upstream_origin = upstream.origin();
        // The token, if there is one, takes precedence: it is the credential the upstream
        // just asked for. The configuration's is the fallback.
        let secret = match (token, upstream.credential.as_ref()) {
            (Some(_), _) | (None, None) => None,
            // A repository that names a credential the service cannot
            // provide (not allowed by the operator, or not set) fails
            // closed: it never degrades to an anonymous request.
            (None, Some(credential)) => {
                Some(self.secrets.get(credential).ok_or(FetchError::Credential)?)
            }
        };
        let credential = match token {
            Some(token) => Some(TheCredential {
                kind: TheCredentialKind::Token(token),
                origin: &upstream_origin,
            }),
            None => secret.as_ref().map(|secret| TheCredential {
                kind: TheCredentialKind::Bearer(secret),
                origin: &upstream_origin,
            }),
        };
        let mut url = url;
        let mut redirects: u8 = 0;

        loop {
            let response = self
                .hop(
                    &upstream_origin,
                    &url,
                    deadline,
                    accept,
                    validators,
                    credential.as_ref(),
                )
                .await?;
            let status = response.status().as_u16();
            match status {
                200 => {
                    return Self::accept(response, url, redirects, budget, permit)
                        .map(|opened| Fetched::Content(Box::new(opened)));
                }
                // A `401` is not content: it is a challenge. It is returned as
                // is, with the header that describes it, and whoever receives it
                // decides whether the realm it names is the declared one.
                401 => {
                    return Ok(Fetched::Unauthorized {
                        challenge: header_text(&response, WWW_AUTHENTICATE, 512),
                    });
                }
                // A `304` is only valid from whoever received the validators: another
                // origin a redirect led to does not know which copy it is talking about.
                304 if validators.is_some() && origin_of(&url) == upstream_origin => {
                    return Ok(Fetched::NotModified(NotModified {
                        source_url: format!("{}{}", origin_of(&url), url.path()),
                        redirects,
                        etag: header_text(&response, reqwest::header::ETAG, 200),
                        last_modified: header_text(&response, reqwest::header::LAST_MODIFIED, 100),
                    }));
                }
                301 | 302 | 303 | 307 | 308 => {
                    redirects += 1;
                    if redirects > upstream.max_redirects {
                        return Err(FetchError::Redirect("too many redirects"));
                    }
                    let next = Self::redirect_target(&response, &url, upstream)?;
                    url = next;
                }
                404 | 410 => return Err(FetchError::NotFound),
                other => return Err(FetchError::Status(other)),
            }
        }
    }

    /// The upstream's secret, if the configuration declares it and the
    /// service allows it to leave.
    ///
    /// It is the credential the configuration names, not just any credential:
    /// the name comes from `credential_ref` and the secret comes from the
    /// environment, and the environment only answers to the names the operator has
    /// authorized.
    pub fn the_credential(&self, upstream: &UpstreamConfig) -> Option<Secret> {
        upstream
            .credential
            .as_ref()
            .and_then(|credential| self.secrets.get(credential))
    }

    /// Opens a request with the upstream's credential in **Basic** form.
    ///
    /// The credential only travels to the origin it is told
    /// (`credential_origin`), which is the one the configuration declares: the
    /// challenge's realm. A realm the upstream makes up does not receive the
    /// credential, because whoever decides the origin is the configuration, not the
    /// response.
    ///
    /// The request does **not** follow redirects: a realm that answers with a
    /// redirect to another origin is left without a credential, and the error says so.
    pub async fn open_with_basic(
        &self,
        upstream: &UpstreamConfig,
        url: Url,
        credential_origin: &str,
        user: &str,
        password: &Secret,
    ) -> Result<Opened, FetchError> {
        let credential = TheCredential {
            kind: TheCredentialKind::Basic { user, password },
            origin: credential_origin,
        };
        self.open_token_request(upstream, url, Some(&credential))
            .await
    }

    /// Opens a token endpoint request without Basic credentials for anonymous
    /// bearer token exchange against a declared public registry realm.
    pub async fn open_anonymous_token(
        &self,
        upstream: &UpstreamConfig,
        url: Url,
    ) -> Result<Opened, FetchError> {
        self.open_token_request(upstream, url, None).await
    }

    async fn open_token_request(
        &self,
        upstream: &UpstreamConfig,
        url: Url,
        credential: Option<&TheCredential<'_>>,
    ) -> Result<Opened, FetchError> {
        if self.offline {
            return Err(FetchError::Offline);
        }
        let permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| FetchError::Busy)?;
        let deadline = tokio::time::Instant::now() + self.limits.total_timeout;
        let budget = upstream
            .max_bytes
            .map_or(self.limits.max_bytes, |own| own.min(self.limits.max_bytes));
        let upstream_origin = upstream.origin();
        let response = self
            .hop(
                &upstream_origin,
                &url,
                deadline,
                Some("application/json"),
                None,
                credential,
            )
            .await?;
        let status = response.status().as_u16();
        if status != 200 {
            return Err(FetchError::Status(status));
        }
        Self::accept(response, url, 0, budget, permit)
    }

    /// A request to a destination already admitted by the policy.
    async fn hop(
        &self,
        upstream_origin: &str,
        url: &Url,
        deadline: tokio::time::Instant,
        accept: Option<&str>,
        validators: Option<&Validators>,
        credential: Option<&TheCredential<'_>>,
    ) -> Result<reqwest::Response, FetchError> {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(FetchError::Timeout);
        }
        let entry = self.policy.check_url(url)?;
        let port = url
            .port_or_known_default()
            .ok_or(EgressDenied::PortNotAllowed)?;

        // A name is resolved here, validated and pinned. A literal IP
        // was already validated in `check_url`.
        let pinned: Option<(String, Vec<SocketAddr>)> = match url.host() {
            Some(Host::Domain(domain)) => {
                let addresses = tokio::time::timeout(
                    self.limits.connect_timeout,
                    self.resolver.resolve(domain, port),
                )
                .await
                .map_err(|_| FetchError::Timeout)?
                .map_err(|_| EgressDenied::Unresolved)?;
                EgressPolicy::validate_addresses(entry, &addresses)?;
                Some((
                    domain.to_string(),
                    addresses
                        .into_iter()
                        .map(|ip| SocketAddr::new(ip, port))
                        .collect(),
                ))
            }
            _ => None,
        };

        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(self.limits.connect_timeout)
            .timeout(remaining)
            .pool_max_idle_per_host(0)
            .user_agent(concat!("hodei-artifacts-proxy/", env!("CARGO_PKG_VERSION")));
        if let Some((domain, addresses)) = &pinned {
            builder = builder.resolve_to_addrs(domain, addresses);
        }
        let client = builder.build().map_err(|_| FetchError::Unreachable)?;

        let mut request = client
            .get(url.clone())
            .header(
                ACCEPT,
                accept.unwrap_or("application/octet-stream, */*;q=0.8"),
            )
            .header(ACCEPT_ENCODING, "identity");
        // The credential travels **only** to the origin it is for: the
        // upstream's, or the realm's that the configuration declares. A hop to
        // another origin --a redirect, a realm the upstream made up--
        // is left without a credential.
        if let Some(credential) = credential
            && origin_of(url) == credential.origin
        {
            let mut value = match &credential.kind {
                TheCredentialKind::Bearer(secret) => {
                    HeaderValue::from_str(&format!("Bearer {}", secret.expose()))
                }
                TheCredentialKind::Token(token) => {
                    HeaderValue::from_str(&format!("Bearer {token}"))
                }
                TheCredentialKind::Basic { user, password } => {
                    let pair = format!("{user}:{}", password.expose());
                    HeaderValue::from_str(&format!(
                        "Basic {}",
                        base64::engine::general_purpose::STANDARD.encode(pair)
                    ))
                }
            }
            .map_err(|_| FetchError::Credential)?;
            value.set_sensitive(true);
            request = request.header(AUTHORIZATION, value);
        }
        // The validators are also the upstream's: an origin a redirect leads
        // to has no business seeing the state of the
        // copy that is held.
        if let Some(validators) = validators
            && origin_of(url) == upstream_origin
        {
            if let Some(etag) = validators.etag.as_deref()
                && let Ok(value) = HeaderValue::from_str(etag)
            {
                request = request.header(reqwest::header::IF_NONE_MATCH, value);
            }
            if let Some(since) = validators.last_modified.as_deref()
                && let Ok(value) = HeaderValue::from_str(since)
            {
                request = request.header(reqwest::header::IF_MODIFIED_SINCE, value);
            }
        }

        request.send().await.map_err(|error| {
            let timed_out = error.is_timeout();
            // The reqwest error text carries the URL: it is stripped before
            // leaving it in the trace.
            tracing::warn!(
                target: "hodei_artifacts::proxy",
                host = url.host_str().unwrap_or(""),
                error = %error.without_url(),
                "the upstream could not be reached"
            );
            if timed_out {
                FetchError::Timeout
            } else {
                FetchError::Unreachable
            }
        })
    }

    /// Where a redirect leads, if going there is allowed.
    fn redirect_target(
        response: &reqwest::Response,
        from: &Url,
        upstream: &UpstreamConfig,
    ) -> Result<Url, FetchError> {
        let location = response
            .headers()
            .get(LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or(FetchError::Redirect("the redirect has no usable location"))?;
        let mut next = from
            .join(location)
            .map_err(|_| FetchError::Redirect("the redirect location is not a url"))?;
        next.set_fragment(None);
        if from.scheme() == "https" && next.scheme() != "https" {
            return Err(FetchError::Redirect("a redirect cannot downgrade to http"));
        }
        if !upstream.cross_origin_redirects && origin_of(&next) != origin_of(from) {
            return Err(FetchError::Redirect(
                "a redirect to another origin is not allowed",
            ));
        }
        // The destination is judged with the egress policy at the start of the
        // next hop; here only what is not even a URL that can be
        // judged is discarded.
        Ok(next)
    }

    /// Accepts a `200` response, if its headers are usable.
    fn accept(
        response: reqwest::Response,
        final_url: Url,
        redirects: u8,
        budget: u64,
        permit: OwnedSemaphorePermit,
    ) -> Result<Opened, FetchError> {
        let headers = response.headers();
        if let Some(encoding) = headers.get(CONTENT_ENCODING)
            && !encoding.as_bytes().eq_ignore_ascii_case(b"identity")
        {
            return Err(FetchError::BadResponse(
                "the upstream compressed a response that was asked uncompressed",
            ));
        }
        let content_length = response.content_length();
        if let Some(length) = content_length
            && length > budget
        {
            return Err(FetchError::TooLarge);
        }
        let declared = match headers.get(CHECKSUM_HEADER) {
            None => None,
            Some(value) => Some(value.to_str().ok().and_then(parse_declared_sha256).ok_or(
                FetchError::BadResponse("the checksum declared by the upstream is not a sha256"),
            )?),
        };
        let etag = header_text(&response, reqwest::header::ETAG, 200);
        let last_modified = header_text(&response, reqwest::header::LAST_MODIFIED, 100);
        let link = header_text(&response, reqwest::header::LINK, 200);
        Ok(Opened {
            response,
            final_url,
            redirects,
            content_length,
            declared,
            etag,
            last_modified,
            link,
            budget,
            _permit: permit,
        })
    }
}

/// An open `200` response, not yet read.
pub struct Opened {
    response: reqwest::Response,
    final_url: Url,
    redirects: u8,
    content_length: Option<u64>,
    declared: Option<String>,
    etag: Option<String>,
    last_modified: Option<String>,
    link: Option<String>,
    budget: u64,
    _permit: OwnedSemaphorePermit,
}

/// The text of a header, if it is visible text and not oversized.
fn header_text(
    response: &reqwest::Response,
    name: reqwest::header::HeaderName,
    max_len: usize,
) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= max_len)
        .map(str::to_string)
}

impl std::fmt::Debug for Opened {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Opened")
            .field("redirects", &self.redirects)
            .field("content_length", &self.content_length)
            .finish_non_exhaustive()
    }
}

impl Opened {
    /// The guardian of the bytes that are about to arrive.
    pub fn guard(&self, require_checksum: bool) -> BodyGuard {
        BodyGuard {
            count: 0,
            budget: self.budget,
            expected_length: self.content_length,
            declared: self.declared.clone(),
            require_checksum,
        }
    }

    /// The next chunk, or `None` at the end.
    pub async fn chunk(&mut self) -> Result<Option<bytes::Bytes>, FetchError> {
        self.response.chunk().await.map_err(|error| {
            if error.is_timeout() {
                FetchError::Timeout
            } else {
                // A connection that is cut mid-body is a
                // truncated response: it is not known how much was missing.
                FetchError::Truncated
            }
        })
    }

    /// Where the content came from: origin and path, without credentials or
    /// query.
    pub fn source_url(&self) -> String {
        format!("{}{}", origin_of(&self.final_url), self.final_url.path())
    }

    /// The redirects that were followed.
    pub fn redirects(&self) -> u8 {
        self.redirects
    }

    /// The entity tag the upstream declared.
    pub fn etag(&self) -> Option<&str> {
        self.etag.as_deref()
    }

    /// The `Last-Modified` the upstream declared.
    pub fn last_modified(&self) -> Option<&str> {
        self.last_modified.as_deref()
    }

    /// The `Link` header the upstream declared, if it declared one.
    ///
    /// It is the one that says a **paginated** response has more pages: a
    /// listing that carries it cannot be served as if it were the
    /// whole list.
    pub fn link(&self) -> Option<&str> {
        self.link.as_deref()
    }
}

/// What was checked of received content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    /// The bytes received.
    pub size_bytes: u64,
    /// What it was checked with.
    pub evidence: ChecksumEvidence,
}

/// Checks the content that arrives.
///
/// It does not compute the digest: the write session computes it
/// while the chunks pass, and it is asked here. Two computations of the same
/// content are two chances to disagree.
#[derive(Debug, Clone)]
pub struct BodyGuard {
    count: u64,
    budget: u64,
    expected_length: Option<u64>,
    declared: Option<String>,
    require_checksum: bool,
}

impl BodyGuard {
    /// Counts a chunk. Fails as soon as the budget is exceeded.
    pub fn push(&mut self, chunk_len: usize) -> Result<(), FetchError> {
        self.count = self.count.saturating_add(chunk_len as u64);
        if self.count > self.budget {
            return Err(FetchError::TooLarge);
        }
        if let Some(expected) = self.expected_length
            && self.count > expected
        {
            // More bytes than it announced: the upstream does not know what it
            // is sending.
            return Err(FetchError::BadResponse(
                "the upstream sent more bytes than it announced",
            ));
        }
        Ok(())
    }

    /// Closes the check with the digest (hex, lowercase) of what
    /// arrived.
    pub fn finish(self, computed_hex: &str) -> Result<Verified, FetchError> {
        if let Some(expected) = self.expected_length
            && self.count != expected
        {
            return Err(FetchError::Truncated);
        }
        let evidence = match self.declared {
            Some(declared) => {
                if !declared.eq_ignore_ascii_case(computed_hex) {
                    return Err(FetchError::ChecksumMismatch);
                }
                ChecksumEvidence::Declared {
                    header: CHECKSUM_HEADER.to_string(),
                }
            }
            None if self.require_checksum => return Err(FetchError::ChecksumMissing),
            None => ChecksumEvidence::ComputedOnly,
        };
        Ok(Verified {
            size_bytes: self.count,
            evidence,
        })
    }
}

/// The sha256 a header declares: `<hex>` or `sha256:<hex>`.
///
/// Returns the hexadecimal in lowercase, or `None` if it is not a sha256.
pub fn parse_declared_sha256(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let value = match raw.split_once(':') {
        Some((algorithm, value)) if algorithm.eq_ignore_ascii_case("sha256") => value,
        Some(_) => return None,
        None => raw,
    };
    (value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| value.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEX: &str = "2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae";

    fn guard(budget: u64, length: Option<u64>, declared: Option<&str>, require: bool) -> BodyGuard {
        BodyGuard {
            count: 0,
            budget,
            expected_length: length,
            declared: declared.map(str::to_string),
            require_checksum: require,
        }
    }

    #[test]
    fn a_secret_is_never_formatted() {
        let secret = Secret::new("hunter2");
        assert_eq!(format!("{secret:?}"), "Secret(***)");
    }

    #[test]
    fn the_declared_checksum_is_read_with_or_without_prefix() {
        assert_eq!(parse_declared_sha256(HEX).as_deref(), Some(HEX));
        assert_eq!(
            parse_declared_sha256(&format!("SHA256:{}", HEX.to_uppercase())).as_deref(),
            Some(HEX)
        );
        assert!(parse_declared_sha256("sha1:abcd").is_none());
        assert!(parse_declared_sha256("zz").is_none());
        assert!(parse_declared_sha256(&HEX[..63]).is_none());
        assert!(parse_declared_sha256("").is_none());
    }

    #[test]
    fn a_body_over_the_budget_is_refused_as_it_arrives() {
        let mut guard = guard(10, None, None, false);
        assert!(guard.push(6).is_ok());
        assert_eq!(guard.push(5), Err(FetchError::TooLarge));
    }

    #[test]
    fn more_bytes_than_announced_are_refused() {
        let mut guard = guard(100, Some(5), None, false);
        assert!(guard.push(5).is_ok());
        assert!(matches!(guard.push(1), Err(FetchError::BadResponse(_))));
    }

    #[test]
    fn a_truncated_body_is_not_a_verified_one() {
        let mut guard = guard(100, Some(10), Some(HEX), false);
        guard.push(4).expect("push");
        assert_eq!(guard.finish(HEX), Err(FetchError::Truncated));
    }

    #[test]
    fn a_wrong_declared_checksum_is_refused() {
        let mut guard = guard(100, Some(3), Some(HEX), false);
        guard.push(3).expect("push");
        assert_eq!(
            guard.finish(&"0".repeat(64)),
            Err(FetchError::ChecksumMismatch)
        );
    }

    #[test]
    fn a_matching_declared_checksum_is_recorded_as_declared() {
        let mut guard = guard(100, Some(3), Some(HEX), true);
        guard.push(3).expect("push");
        let verified = guard.finish(HEX).expect("verified");
        assert_eq!(verified.size_bytes, 3);
        assert_eq!(
            verified.evidence,
            ChecksumEvidence::Declared {
                header: CHECKSUM_HEADER.to_string()
            }
        );
    }

    #[test]
    fn a_missing_checksum_is_refused_only_when_required() {
        let mut required = guard(100, None, None, true);
        required.push(3).expect("push");
        assert_eq!(required.finish(HEX), Err(FetchError::ChecksumMissing));

        let mut optional = guard(100, None, None, false);
        optional.push(3).expect("push");
        let verified = optional.finish(HEX).expect("computed only");
        assert_eq!(verified.evidence, ChecksumEvidence::ComputedOnly);
    }

    /// A resolver that answers what the test wants, by name.
    struct Scripted(std::collections::HashMap<&'static str, Vec<std::net::IpAddr>>);

    #[async_trait::async_trait]
    impl crate::proxy::egress::HostResolver for Scripted {
        async fn resolve(&self, host: &str, _port: u16) -> std::io::Result<Vec<std::net::IpAddr>> {
            self.0
                .get(host)
                .cloned()
                .ok_or_else(|| std::io::Error::other("unknown name"))
        }
    }

    /// A server that answers with a fixed response to every request.
    async fn serve_always(answer: String) -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("address").port();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let answer = answer.clone();
                tokio::spawn(async move {
                    let mut buffer = [0u8; 2048];
                    let _ = stream.read(&mut buffer).await;
                    let _ = stream.write_all(answer.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        port
    }

    fn fetcher_with(
        allowed: Vec<crate::proxy::egress::AllowedHost>,
        names: Vec<(&'static str, Vec<std::net::IpAddr>)>,
    ) -> ProxyFetcher {
        ProxyFetcher::new(
            crate::proxy::egress::EgressPolicy::new(allowed).expect("policy"),
            FetchLimits::default(),
            2,
        )
        .with_resolver(Arc::new(Scripted(names.into_iter().collect())))
    }

    fn entry(host: &str, port: Option<u16>, private: bool) -> crate::proxy::egress::AllowedHost {
        crate::proxy::egress::AllowedHost {
            host: host.to_string(),
            port,
            allow_private: private,
            allow_http: true,
        }
    }

    fn upstream(url: &str, extra: serde_json::Value) -> UpstreamConfig {
        let mut config = serde_json::json!({ "id": "lab", "url": url, "scopes": ["/"] });
        for (key, value) in extra.as_object().expect("an object") {
            config[key] = value.clone();
        }
        UpstreamConfig::parse(&config).expect("an upstream")
    }

    #[tokio::test]
    async fn the_connection_goes_to_the_address_that_was_validated() {
        let port = serve_always(
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi".to_string(),
        )
        .await;
        // `pinned.test` does not exist in the system DNS: it is only reached if
        // the connection uses the address the resolver gave and the policy
        // validated -- not a second resolution.
        let fetcher = fetcher_with(
            vec![entry("pinned.test", Some(port), true)],
            vec![("pinned.test", vec!["127.0.0.1".parse().unwrap()])],
        );
        let upstream = upstream(
            &format!("http://pinned.test:{port}/"),
            serde_json::json!({}),
        );
        let mut opened = fetcher.open(&upstream, "a.bin").await.expect("opened");
        let chunk = opened.chunk().await.expect("a chunk").expect("some bytes");
        assert_eq!(&chunk[..], b"hi");
    }

    #[tokio::test]
    async fn a_name_that_rebinds_to_a_private_address_is_refused() {
        let fetcher = fetcher_with(
            vec![entry("rebind.test", None, false)],
            vec![("rebind.test", vec!["10.0.0.5".parse().unwrap()])],
        );
        let upstream = upstream("http://rebind.test/", serde_json::json!({}));
        let error = fetcher.open(&upstream, "a.bin").await.expect_err("refused");
        assert!(matches!(error, FetchError::Egress(_)), "{error:?}");
    }

    #[tokio::test]
    async fn a_redirect_to_a_name_that_resolves_to_a_forbidden_address_is_refused() {
        let second = "second.test";
        let port = serve_always(format!(
            "HTTP/1.1 302 Found\r\nLocation: http://{second}/ok\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        ))
        .await;
        // The first hop is legitimate (authorized loopback); the second
        // name points to the metadata network: the policy sees it as soon as
        // it resolves, before connecting.
        let fetcher = fetcher_with(
            vec![
                entry("first.test", Some(port), true),
                entry(second, None, false),
            ],
            vec![
                ("first.test", vec!["127.0.0.1".parse().unwrap()]),
                (second, vec!["169.254.169.254".parse().unwrap()]),
            ],
        );
        let upstream = upstream(
            &format!("http://first.test:{port}/"),
            serde_json::json!({ "redirects": { "max": 2, "cross_origin": true } }),
        );
        let error = fetcher.open(&upstream, "a.bin").await.expect_err("refused");
        assert!(
            matches!(
                error,
                FetchError::Egress(crate::proxy::egress::EgressDenied::Address(_))
            ),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn a_redirect_across_origins_is_refused_unless_the_repository_allows_it() {
        let port = serve_always(
            "HTTP/1.1 302 Found\r\nLocation: http://other.test/ok\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_string(),
        )
        .await;
        let fetcher = fetcher_with(
            vec![
                entry("first.test", Some(port), true),
                entry("other.test", None, true),
            ],
            vec![
                ("first.test", vec!["127.0.0.1".parse().unwrap()]),
                ("other.test", vec!["127.0.0.1".parse().unwrap()]),
            ],
        );
        let upstream = upstream(&format!("http://first.test:{port}/"), serde_json::json!({}));
        let error = fetcher.open(&upstream, "a.bin").await.expect_err("refused");
        assert!(matches!(error, FetchError::Redirect(_)), "{error:?}");
    }

    #[tokio::test]
    async fn a_compressed_answer_is_refused_because_none_was_asked_for() {
        let port = serve_always(
            "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi"
                .to_string(),
        )
        .await;
        let fetcher = fetcher_with(vec![entry("lab.test", Some(port), true)], lab_names());
        let upstream = upstream(&format!("http://lab.test:{port}/"), serde_json::json!({}));
        let error = fetcher.open(&upstream, "a.bin").await.expect_err("refused");
        assert!(matches!(error, FetchError::BadResponse(_)), "{error:?}");
    }

    #[tokio::test]
    async fn only_a_full_answer_is_a_content() {
        for status in ["206 Partial Content", "304 Not Modified", "204 No Content"] {
            let port = serve_always(format!(
                "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            ))
            .await;
            let fetcher = fetcher_with(vec![entry("lab.test", Some(port), true)], lab_names());
            let upstream = upstream(&format!("http://lab.test:{port}/"), serde_json::json!({}));
            let error = fetcher.open(&upstream, "a.bin").await.expect_err("refused");
            assert!(
                matches!(error, FetchError::Status(_)),
                "{status}: {error:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_saturated_service_answers_busy_and_a_path_out_of_scope_does_not_wait() {
        let port = serve_always(
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi".to_string(),
        )
        .await;
        // The test fetcher allows two simultaneous egress calls.
        let fetcher = fetcher_with(vec![entry("lab.test", Some(port), true)], lab_names());
        let upstream = upstream(
            &format!("http://lab.test:{port}/"),
            serde_json::json!({ "scopes": ["acme/"] }),
        );
        let first = fetcher.open(&upstream, "acme/a").await.expect("first");
        let second = fetcher.open(&upstream, "acme/b").await.expect("second");
        let busy = fetcher.open(&upstream, "acme/c").await.expect_err("busy");
        assert_eq!(busy, FetchError::Busy);
        // Out of scope is reported before requesting a permit.
        let outside = fetcher.open(&upstream, "other/c").await.expect_err("scope");
        assert_eq!(outside, FetchError::OutOfScope);
        drop((first, second));
        fetcher
            .open(&upstream, "acme/d")
            .await
            .expect("a permit came back");
    }

    #[tokio::test]
    async fn the_time_limit_is_for_the_whole_resolution() {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("address").port();
        // An upstream that accepts and never answers.
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buffer = [0u8; 1024];
                    let _ = stream.read(&mut buffer).await;
                    tokio::time::sleep(Duration::from_secs(30)).await;
                });
            }
        });
        let fetcher = ProxyFetcher::new(
            crate::proxy::egress::EgressPolicy::new(vec![entry("lab.test", Some(port), true)])
                .expect("policy"),
            FetchLimits {
                total_timeout: Duration::from_millis(300),
                ..FetchLimits::default()
            },
            1,
        )
        .with_resolver(Arc::new(Scripted(lab_names().into_iter().collect())));
        let upstream = upstream(&format!("http://lab.test:{port}/"), serde_json::json!({}));
        let started = std::time::Instant::now();
        let error = fetcher.open(&upstream, "a.bin").await.expect_err("timeout");
        assert_eq!(error, FetchError::Timeout);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_variable_is_only_a_credential_if_the_operator_allowed_it() {
        // `PATH` exists in any environment: what is measured is the list.
        let reference = CredentialRef::parse("env:PATH").expect("a reference");
        assert!(EnvSecrets::default().get(&reference).is_none());
        assert!(EnvSecrets::allowing(["OTHER"]).get(&reference).is_none());
        assert!(EnvSecrets::allowing(["PATH"]).get(&reference).is_some());
    }

    #[tokio::test]
    async fn a_credential_the_service_cannot_provide_fails_closed_before_any_request() {
        let port = serve_always(
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi".to_string(),
        )
        .await;
        // `PATH` exists in any environment: what is measured is the list.
        let upstream = upstream(
            &format!("http://pinned.test:{port}/"),
            serde_json::json!({ "credential_ref": "env:PATH" }),
        );
        let build = || {
            fetcher_with(
                vec![entry("pinned.test", Some(port), true)],
                vec![("pinned.test", vec!["127.0.0.1".parse().unwrap()])],
            )
        };
        // Not allowed by the operator: no anonymous fallback.
        let error = build()
            .open(&upstream, "a.bin")
            .await
            .expect_err("a refused credential");
        assert_eq!(error, FetchError::Credential);
        // Allowed: the same upstream is reached.
        let allowed = build().with_allowed_credentials(["PATH"]);
        assert!(allowed.open(&upstream, "a.bin").await.is_ok());
    }

    fn lab_names() -> Vec<(&'static str, Vec<std::net::IpAddr>)> {
        vec![("lab.test", vec!["127.0.0.1".parse().unwrap()])]
    }

    // -- Conditional requests and offline mode --------------------------

    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Mutex;

    use super::super::egress::AllowedHost;

    /// A one-use-per-connection server: stores each whole request
    /// and answers what `reply` says.
    fn serve(reply: impl Fn(&str) -> String + Send + 'static) -> (u16, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("binds");
        let port = listener.local_addr().expect("address").port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut request = Vec::new();
                let mut buffer = [0u8; 1024];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    match stream.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => request.extend_from_slice(&buffer[..read]),
                    }
                }
                let text = String::from_utf8_lossy(&request).into_owned();
                let answer = reply(&text);
                log.lock().expect("log").push(text);
                let _ = stream.write_all(answer.as_bytes());
            }
        });
        (port, seen)
    }

    fn fetcher_for(ports: &[u16]) -> ProxyFetcher {
        let hosts = ports
            .iter()
            .map(|port| AllowedHost::from_spec(&format!("127.0.0.1:{port};private;http")))
            .collect::<Result<Vec<_>, _>>()
            .expect("hosts");
        ProxyFetcher::new(
            EgressPolicy::new(hosts).expect("policy"),
            FetchLimits::default(),
            4,
        )
    }

    fn upstream_at(port: u16, cross_origin: bool) -> UpstreamConfig {
        let mut config = hodei_catalog::RepositoryConfig::new(hodei_catalog::DigestScope::Global);
        config.config_settings = serde_json::json!({ "proxy": { "upstream": {
            "id": "lab",
            "url": format!("http://127.0.0.1:{port}/"),
            "scopes": ["/"],
            "redirects": { "max": 3, "cross_origin": cross_origin },
        }}});
        UpstreamConfig::of(&config)
            .expect("valid")
            .expect("an upstream")
    }

    fn validators() -> Validators {
        Validators {
            etag: Some("\"v1\"".to_string()),
            last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".to_string()),
        }
    }

    const NOT_MODIFIED: &str = "HTTP/1.1 304 Not Modified\r\nETag: \"v1\"\r\nLast-Modified: Wed, 21 Oct 2015 07:28:00 GMT\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

    #[tokio::test]
    async fn a_304_confirms_the_copy_and_the_validators_travel_to_the_upstream() {
        let (port, seen) = serve(|_| NOT_MODIFIED.to_string());
        let fetched = fetcher_for(&[port])
            .open_conditional(&upstream_at(port, false), "a.bin", Some(&validators()))
            .await
            .expect("answers");
        let Fetched::NotModified(confirmation) = fetched else {
            panic!("a 304 must be a confirmation");
        };
        assert_eq!(confirmation.etag.as_deref(), Some("\"v1\""));
        assert_eq!(
            confirmation.last_modified.as_deref(),
            Some("Wed, 21 Oct 2015 07:28:00 GMT")
        );
        assert!(!confirmation.source_url.contains('?'));
        let request = seen.lock().expect("log")[0].to_ascii_lowercase();
        assert!(request.contains("if-none-match: \"v1\""), "{request}");
        assert!(
            request.contains("if-modified-since: wed, 21 oct 2015 07:28:00 gmt"),
            "{request}"
        );
    }

    #[tokio::test]
    async fn without_validators_nothing_conditional_is_sent_and_a_304_is_an_error() {
        let (port, seen) = serve(|_| NOT_MODIFIED.to_string());
        let outcome = fetcher_for(&[port])
            .open_conditional(&upstream_at(port, false), "a.bin", None)
            .await;
        assert!(matches!(outcome, Err(FetchError::Status(304))));
        let request = seen.lock().expect("log")[0].to_ascii_lowercase();
        assert!(!request.contains("if-none-match"), "{request}");
        assert!(!request.contains("if-modified-since"), "{request}");
    }

    #[tokio::test]
    async fn validators_stay_at_the_upstream_origin_and_another_origin_cannot_confirm() {
        let (stranger, stranger_seen) = serve(|_| NOT_MODIFIED.to_string());
        let (port, _) = serve(move |_| {
            format!(
                "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{stranger}/a.bin\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
        });
        let outcome = fetcher_for(&[port, stranger])
            .open_conditional(&upstream_at(port, true), "a.bin", Some(&validators()))
            .await;
        // The other origin did not receive the validators, and its `304` is not valid.
        assert!(
            matches!(outcome, Err(FetchError::Status(304))),
            "{outcome:?}"
        );
        let request = stranger_seen.lock().expect("log")[0].to_ascii_lowercase();
        assert!(!request.contains("if-none-match"), "{request}");
        assert!(!request.contains("if-modified-since"), "{request}");
    }

    #[tokio::test]
    async fn a_200_carries_the_last_modified_it_was_served_with() {
        let (port, _) = serve(|_| {
            "HTTP/1.1 200 OK\r\nLast-Modified: Wed, 21 Oct 2015 07:28:00 GMT\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
                .to_string()
        });
        let fetched = fetcher_for(&[port])
            .open_conditional(&upstream_at(port, false), "a.bin", Some(&validators()))
            .await
            .expect("answers");
        let Fetched::Content(opened) = fetched else {
            panic!("a 200 must be content");
        };
        assert_eq!(
            opened.last_modified(),
            Some("Wed, 21 Oct 2015 07:28:00 GMT")
        );
    }

    #[tokio::test]
    async fn offline_refuses_before_opening_any_connection() {
        let (port, seen) = serve(|_| NOT_MODIFIED.to_string());
        let fetcher = fetcher_for(&[port]).with_offline(true);
        assert!(fetcher.is_offline());
        let outcome = fetcher
            .open_conditional(&upstream_at(port, false), "a.bin", None)
            .await;
        assert!(matches!(outcome, Err(FetchError::Offline)), "{outcome:?}");
        assert!(seen.lock().expect("log").is_empty());
    }
}
