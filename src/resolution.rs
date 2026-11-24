//! Shared application resolution seam for repository protocols.
//!
//! Protocol handlers (`raw`, `npm`, `pypi`, and others) share a small set of
//! application-level operations:
//! - authenticating a caller into a typed [`VerifiedContext`],
//! - resolving a repository in the caller's verified account,
//! - evaluating a Cedar policy decision (`Permit` / `Deny` / `Indeterminate`)
//!   without ever degrading `Denied`/`Forbidden` into `404`,
//! - looking up and publishing assets in the catalog and CAS content store,
//! - resolving assets across hosted, proxy, and group repositories with
//!   explicit origin, outcome, and digest types.
//!
//! Each protocol retains its own route parsing, response serialization, and
//! protocol-specific semantics.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures_util::StreamExt;
use hodei_authz::{
    AuthorizationOutcome, Credentials, VerifiedContext, model::ActionId, verify_principal,
};
use hodei_catalog::{
    AssetRecord, CatalogError, CatalogReader, CatalogWriter, DigestScope, PackageCoordinates,
    Publication, RepositoryDescriptor, RepositoryFormat, RepositoryMode,
};
use hodei_evidence::domain::content::ContentDigest;
use hodei_hub::blob::{BlobError, BlobStore, CommitExpectation};
use sha2::{Digest, Sha256};
use url::Url;

use crate::app_state::AppState;
use crate::handlers::bearer_token;
use crate::raw::range;

/// Number of recent receipts inspected on the proxy hot path.
const RECEIPTS_READ_ON_THE_HOT_PATH: usize = 32;

/// Correlation header name.
const CORRELATION_HEADER: &str = "x-request-id";

/// Errors produced by the shared resolution seam and mapped to HTTP status codes.
#[derive(Debug)]
pub enum ResolutionError {
    /// Missing or invalid credentials.
    Unauthorized,
    /// Cedar denied the requested action on the resource.
    Forbidden,
    /// The requested repository or asset does not exist in this scope.
    NotFound,
    /// Malformed request path, header, or body.
    BadRequest(&'static str),
    /// Conditional request precondition failed.
    PreconditionFailed,
    /// Requested byte range is not satisfiable.
    RangeNotSatisfiable(u64),
    /// Request body exceeds the configured maximum size.
    PayloadTooLarge,
    /// Account quota refused the upload.
    Quota(crate::quota::QuotaRefusal),
    /// Target path already holds different content and cannot be overwritten.
    Conflict(&'static str),
    /// Required catalog, store, or identity provider is unavailable.
    Unavailable,
    /// Operation or configuration is not supported in this release.
    NotImplemented(&'static str),
    /// HTTP method is not allowed for this repository or route.
    MethodNotAllowed(&'static str),
    /// Upstream proxy failed to deliver usable content.
    BadGateway(&'static str),
    /// Upstream proxy timed out.
    GatewayTimeout,
    /// Proxy is offline and holds no verified cached copy.
    Offline(&'static str),
    /// Path falls under a reserved namespace and is blocked from public upstreams.
    ReservedNamespace(String),
    /// Unexpected internal failure.
    Internal,
}

impl ResolutionError {
    /// Returns the canonical HTTP status code and plain message for this error.
    pub(crate) fn the_verdict(&self) -> (StatusCode, String) {
        match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized".to_string()),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden".to_string()),
            Self::NotFound => (StatusCode::NOT_FOUND, "not found".to_string()),
            Self::BadRequest(reason) => (StatusCode::BAD_REQUEST, reason.to_string()),
            Self::PreconditionFailed => (
                StatusCode::PRECONDITION_FAILED,
                "precondition failed".to_string(),
            ),
            Self::RangeNotSatisfiable(_) => (
                StatusCode::RANGE_NOT_SATISFIABLE,
                "the range is not satisfiable".to_string(),
            ),
            Self::PayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "the content is larger than this service accepts".to_string(),
            ),
            Self::Quota(refusal) => match refusal {
                crate::quota::QuotaRefusal::StorageOver { .. } => (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "the content would take this account past its stored quota".to_string(),
                ),
                crate::quota::QuotaRefusal::UploadsSaturated { .. } => (
                    StatusCode::TOO_MANY_REQUESTS,
                    "this account already has as many uploads in flight as it may have".to_string(),
                ),
            },
            Self::Conflict(reason) => (StatusCode::CONFLICT, reason.to_string()),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "the surface is not available".to_string(),
            ),
            Self::NotImplemented(reason) => (StatusCode::NOT_IMPLEMENTED, reason.to_string()),
            Self::MethodNotAllowed(reason) => (StatusCode::METHOD_NOT_ALLOWED, reason.to_string()),
            Self::BadGateway(reason) => (StatusCode::BAD_GATEWAY, reason.to_string()),
            Self::GatewayTimeout => (
                StatusCode::GATEWAY_TIMEOUT,
                "the upstream did not answer in time".to_string(),
            ),
            Self::Offline(reason) => (StatusCode::SERVICE_UNAVAILABLE, reason.to_string()),
            Self::ReservedNamespace(reason) => (StatusCode::FORBIDDEN, reason.clone()),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal error".to_string(),
            ),
        }
    }

    /// Returns the `Retry-After` seconds when the error is due to upload saturation.
    pub(crate) fn the_retry_after(&self) -> Option<u64> {
        match self {
            Self::Quota(crate::quota::QuotaRefusal::UploadsSaturated { retry_after_secs }) => {
                Some(*retry_after_secs)
            }
            _ => None,
        }
    }
}

impl IntoResponse for ResolutionError {
    fn into_response(self) -> Response {
        let unsatisfiable = match &self {
            Self::RangeNotSatisfiable(length) => Some(*length),
            _ => None,
        };
        let retry_after = self.the_retry_after();
        let (status, body) = self.the_verdict();
        let mut response = (status, body).into_response();
        if let Some(seconds) = retry_after
            && let Ok(value) = HeaderValue::from_str(&seconds.to_string())
        {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        if let Some(length) = unsatisfiable
            && let Ok(value) = HeaderValue::from_str(&range::the_unsatisfiable_range(length))
        {
            response.headers_mut().insert(header::CONTENT_RANGE, value);
        }
        response
    }
}

/// Explicit Cedar policy decision for a repository action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDecision {
    pub action: ActionId,
    pub outcome: AuthorizationOutcome,
    pub repository_revision: u64,
}

/// How a proxy served a resolved asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheStatus {
    /// Served from a fresh cache entry without contacting the upstream.
    Hit,
    /// Fetched from the upstream because no cached entry existed.
    Miss,
    /// Expired cache entry revalidated with the upstream (same bytes).
    Revalidated,
    /// Expired cache entry refreshed with new bytes from the upstream.
    Refreshed,
    /// Upstream failed; stale entry served within the configured window.
    Stale,
    /// Service is offline; verified cached entry served with receipt provenance.
    Offline,
}

impl CacheStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Miss => "miss",
            Self::Revalidated => "revalidated",
            Self::Refreshed => "refreshed",
            Self::Stale => "stale",
            Self::Offline => "offline",
        }
    }
}

/// Content resolved from a proxy repository, ready for trust admission.
#[derive(Debug, Clone)]
pub struct ProxyServed {
    pub asset: AssetRecord,
    pub status: CacheStatus,
    /// Upstream identifier and confirmation timestamp when served stale or offline.
    pub provenance: Option<(String, String)>,
}

impl ProxyServed {
    fn new(asset: AssetRecord, status: CacheStatus) -> Self {
        Self {
            asset,
            status,
            provenance: None,
        }
    }
}

/// Explicit origin of a resolved asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionOrigin {
    /// Served directly from a hosted repository.
    Hosted,
    /// Served from a proxy repository.
    Proxy {
        status: CacheStatus,
        provenance: Option<(String, String)>,
    },
    /// Served from a group repository via a winning member.
    Group {
        member: String,
        group_version: u64,
        proxy: Option<(CacheStatus, Option<(String, String)>)>,
    },
}

impl ResolutionOrigin {
    /// Returns proxy cache status and provenance if this resolution came from a proxy.
    pub fn proxy_served(&self) -> Option<(CacheStatus, Option<(String, String)>)> {
        match self {
            Self::Hosted => None,
            Self::Proxy { status, provenance } => Some((*status, provenance.clone())),
            Self::Group { proxy, .. } => proxy.clone(),
        }
    }

    /// Returns the winning group member key and group config version if resolved via a group.
    pub fn group_member(&self) -> Option<(&str, u64)> {
        match self {
            Self::Group {
                member,
                group_version,
                ..
            } => Some((member.as_str(), *group_version)),
            Self::Hosted | Self::Proxy { .. } => None,
        }
    }
}

/// Explicit outcome of resolving an asset path against a repository.
#[derive(Debug, Clone)]
pub struct ResolvedAsset {
    pub asset: AssetRecord,
    pub digest: ContentDigest,
    pub origin: ResolutionOrigin,
    pub repository: RepositoryDescriptor,
}

/// Legacy flat view of a resolved asset used by handlers not yet migrated to [`ResolvedAsset`].
pub(crate) struct Resolved {
    pub(crate) asset: AssetRecord,
    pub(crate) served: Option<(CacheStatus, Option<(String, String)>)>,
    pub(crate) resolved_by: Option<(String, u64)>,
    pub(crate) repository: RepositoryDescriptor,
}

impl From<ResolvedAsset> for Resolved {
    fn from(resolved: ResolvedAsset) -> Self {
        let served = resolved.origin.proxy_served();
        let resolved_by = resolved
            .origin
            .group_member()
            .map(|(member, version)| (member.to_string(), version));
        Self {
            asset: resolved.asset,
            served,
            resolved_by,
            repository: resolved.repository,
        }
    }
}

/// Explicit outcome of writing content into the CAS store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenContent {
    pub digest: ContentDigest,
    pub length: u64,
}

/// Read/write catalog pair from `AppState`.
pub(crate) type TheCatalog<'a> = (&'a Arc<dyn CatalogReader>, &'a Arc<dyn CatalogWriter>);

/// Returns the catalog reader and writer, or `Unavailable` if not mounted.
pub(crate) fn the_catalog(state: &AppState) -> Result<TheCatalog<'_>, ResolutionError> {
    let reader = state
        .catalog_reader
        .as_ref()
        .ok_or(ResolutionError::Unavailable)?;
    let writer = state
        .catalog_writer
        .as_ref()
        .ok_or(ResolutionError::Unavailable)?;
    Ok((reader, writer))
}

/// Returns the CAS blob store, or `Unavailable` if not mounted.
pub(crate) fn the_content_store(state: &AppState) -> Result<&Arc<dyn BlobStore>, ResolutionError> {
    state
        .content_store
        .as_ref()
        .ok_or(ResolutionError::Unavailable)
}

/// Authenticates a Bearer token from HTTP headers into a [`VerifiedContext`].
pub(crate) async fn the_context(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<VerifiedContext, ResolutionError> {
    let token = bearer_token(headers).ok_or(ResolutionError::Unauthorized)?;
    verify_credential_context(state, &token).await
}

/// Verifies a raw credential string into a [`VerifiedContext`].
pub(crate) async fn verify_credential_context(
    state: &AppState,
    credential: &str,
) -> Result<VerifiedContext, ResolutionError> {
    let principal = match verify_principal(
        state.authenticator.as_ref(),
        &Credentials::bearer(credential),
    )
    .await
    {
        Ok(principal) => principal,
        Err(hodei_authz::AuthenticationError::ProviderUnavailable(_)) => {
            return Err(ResolutionError::Unavailable);
        }
        Err(_) => return Err(ResolutionError::Unauthorized),
    };
    Ok(VerifiedContext::from_principal(principal))
}

/// Resolves a repository in the caller's verified account, checking format,
/// release mode, and digest scope.
pub(crate) async fn the_repository_in(
    state: &AppState,
    context: &VerifiedContext,
    repository: &str,
    served: &[RepositoryFormat],
) -> Result<RepositoryDescriptor, ResolutionError> {
    let (reader, _) = the_catalog(state)?;
    let descriptor = match reader
        .repository(context.account().as_str(), repository)
        .await
    {
        Ok(descriptor) => descriptor,
        Err(CatalogError::Missing { .. }) | Err(CatalogError::Denied { .. }) => {
            return Err(ResolutionError::NotFound);
        }
        Err(_) => return Err(ResolutionError::Unavailable),
    };
    let served_format = served.contains(&descriptor.format);
    if !served_format || !descriptor.mode.is_served_by_this_release() {
        return Err(ResolutionError::NotFound);
    }
    if descriptor.config.digest_scope != DigestScope::Global {
        return Err(ResolutionError::NotImplemented(
            "this service serves the content of a repository only in the global scope",
        ));
    }
    Ok(descriptor)
}

/// Evaluates Cedar authorization for `action` on `repository`, returning an
/// explicit [`PolicyDecision`] on `Permit` and `Forbidden` (403) on `Deny`.
///
/// A `Deny` decision never degrades to `NotFound` (404).
pub(crate) fn authorize_repository(
    state: &AppState,
    context: &VerifiedContext,
    action: ActionId,
    repository: &RepositoryDescriptor,
) -> Result<PolicyDecision, ResolutionError> {
    let outcome = state
        .authorize_resource
        .execute_canonical(context.clone(), action.clone(), &repository.hrn)
        .map_err(|_| ResolutionError::Internal)?;
    match &outcome {
        AuthorizationOutcome::Permit => Ok(PolicyDecision {
            action,
            outcome,
            repository_revision: repository.config.version,
        }),
        AuthorizationOutcome::Deny(_) => Err(ResolutionError::Forbidden),
        AuthorizationOutcome::Indeterminate(_) => Err(ResolutionError::Internal),
    }
}

/// Evaluates Cedar authorization for `action` on `repository`.
pub(crate) fn the_decision(
    state: &AppState,
    context: &VerifiedContext,
    action: ActionId,
    repository: &RepositoryDescriptor,
) -> Result<(), ResolutionError> {
    authorize_repository(state, context, action, repository).map(|_| ())
}

/// Maps a trust admission refusal into a [`ResolutionError`].
pub(crate) fn the_admission_error(admission: crate::trust::Admission) -> ResolutionError {
    match admission {
        crate::trust::Admission::Hidden => ResolutionError::NotFound,
        crate::trust::Admission::Unavailable => ResolutionError::Unavailable,
    }
}

/// Looks up the current asset record at `asset_path` in `repository`.
pub(crate) async fn the_current_asset(
    state: &AppState,
    context: &VerifiedContext,
    repository: &str,
    asset_path: &str,
) -> Result<Option<AssetRecord>, ResolutionError> {
    let (reader, _) = the_catalog(state)?;
    match reader
        .asset(context.account().as_str(), repository, asset_path)
        .await
    {
        Ok(asset) => Ok(Some(asset)),
        Err(CatalogError::Missing { .. }) => Ok(None),
        Err(_) => Err(ResolutionError::Unavailable),
    }
}

/// Extracts the namespace reservation policy from a repository configuration.
pub(crate) fn the_namespace_policy(
    config: &hodei_catalog::RepositoryConfig,
) -> Result<Option<hodei_catalog::NamespacePolicy>, ResolutionError> {
    hodei_catalog::NamespacePolicy::of(config).map_err(|error| {
        tracing::warn!(error = %error, "the namespace policy of a repository cannot be read");
        ResolutionError::Unavailable
    })
}

/// Extracts configured upstream identifiers for reservation diagnostics.
pub(crate) fn the_upstream_names(config: &hodei_catalog::RepositoryConfig) -> Vec<String> {
    hodei_catalog::UpstreamConfig::of(config)
        .ok()
        .flatten()
        .map(|upstream| vec![upstream.id])
        .unwrap_or_default()
}

/// Formats a deterministic explanation when a reserved namespace blocks proxy egress.
pub(crate) fn the_reservation_explanation(
    judgement: &hodei_catalog::Judgement,
    sources: &[String],
) -> String {
    let hodei_catalog::Judgement::Reserved { matched, alias_of } = judgement else {
        return "the path is not under a reserved namespace".to_string();
    };
    let mut text = format!("the namespace {matched} is reserved and is never served from ");
    match sources {
        [] => text.push_str("its upstream"),
        [one] => text.push_str(&format!("the public upstream {one}")),
        many => {
            let names = many.join(", ");
            text.push_str(&format!("the public upstreams {names}"));
        }
    }
    if let Some(alias) = alias_of {
        text.push_str(&format!(
            " (the request names it as {alias}, which normalizes to the same namespace)"
        ));
    }
    text
}

/// Resolves an asset path through a proxy repository.
pub(crate) async fn the_proxy_asset(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    asset_path: &str,
    correlation: &str,
) -> Result<ProxyServed, ResolutionError> {
    the_proxy_asset_at(
        state,
        context,
        repository,
        asset_path,
        None,
        None,
        correlation,
    )
    .await
}

/// Resolves an asset path through a proxy repository with an explicit upstream URL
/// and `Accept` header.
pub(crate) async fn the_proxy_asset_at(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    asset_path: &str,
    url: Option<Url>,
    accept: Option<&str>,
    correlation: &str,
) -> Result<ProxyServed, ResolutionError> {
    use hodei_catalog::proxy_cache::{CacheDecision, CacheEntry, Context, decide, stale_allowed};

    use crate::proxy::cache::PathKey;

    if let Some(policy) = the_namespace_policy(&repository.config)? {
        let judgement = policy.judge(repository.format, asset_path);
        if judgement.is_reserved() {
            tracing::warn!(
                target: "hodei_artifacts::proxy",
                repository = %repository.repo_key,
                path = asset_path,
                "a reserved namespace is not served from the proxy upstream"
            );
            return Err(ResolutionError::ReservedNamespace(
                the_reservation_explanation(&judgement, &the_upstream_names(&repository.config)),
            ));
        }
    }

    let plane = state
        .proxy_plane
        .as_ref()
        .ok_or(ResolutionError::NotImplemented(
            "this service has no egress for remote proxies",
        ))?;
    if repository.format == RepositoryFormat::Maven2
        && !crate::handlers::maven::admits_path(repository, asset_path)?
    {
        return Err(ResolutionError::NotFound);
    }
    let store = the_content_store(state)?;
    let account = context.account().as_str();
    let current = the_current_asset(state, context, &repository.repo_key, asset_path).await?;

    let upstream = match hodei_catalog::UpstreamConfig::of(&repository.config) {
        Ok(Some(upstream)) => upstream,
        _ => {
            return match current {
                Some(asset) => Ok(ProxyServed::new(asset, CacheStatus::Hit)),
                None => Err(ResolutionError::BadGateway("the proxy is not configured")),
            };
        }
    };

    let receipts = match &current {
        Some(_) => plane
            .ledger
            .latest_receipts(
                account,
                &repository.repo_key,
                asset_path,
                RECEIPTS_READ_ON_THE_HOT_PATH,
            )
            .await
            .map_err(|error| the_publication_error(&error))?,
        None => Vec::new(),
    };
    let entry = current
        .as_ref()
        .map(|asset| CacheEntry::of(&asset.digest, asset.size_bytes, &receipts));
    let config_version = repository.config.version;
    let decision_context = Context {
        format: repository.format,
        policy: &upstream.cache,
        asset_path,
        upstream_id: &upstream.id,
        in_scope: upstream.in_scope(asset_path),
        config_version,
        offline: upstream.cache.offline || plane.is_offline(),
        now: chrono::Utc::now(),
        kind: None,
    };
    let decision = decide(&decision_context, entry.as_ref());
    tracing::debug!(
        target: "hodei_artifacts::proxy",
        repository = %repository.repo_key,
        path = asset_path,
        decision = ?std::mem::discriminant(&decision),
        "the proxy decided what to do with the path"
    );
    let confirmed = |entry: Option<&CacheEntry>| {
        entry
            .and_then(|entry| entry.confirmation.as_ref())
            .map(|confirmation| {
                (
                    confirmation.upstream_id.clone(),
                    confirmation.at.to_rfc3339(),
                )
            })
    };

    let known = match decision {
        CacheDecision::OutOfScope => return Err(ResolutionError::NotFound),
        CacheDecision::Fresh => {
            plane.metrics.record_hit();
            let asset = current.ok_or(ResolutionError::Internal)?;
            return Ok(ProxyServed::new(asset, CacheStatus::Hit));
        }
        CacheDecision::OfflineServe => {
            let asset = current.ok_or(ResolutionError::Internal)?;
            if let Err(error) = store.verify(&asset.digest).await {
                tracing::error!(
                    target: "hodei_artifacts::proxy",
                    repository = %repository.repo_key,
                    error = %error,
                    "the cached content failed its integrity check"
                );
                plane.metrics.record_offline_miss();
                return Err(ResolutionError::Offline(
                    "the cached content failed its integrity check",
                ));
            }
            plane.metrics.record_offline_hit();
            return Ok(ProxyServed {
                provenance: confirmed(entry.as_ref()),
                ..ProxyServed::new(asset, CacheStatus::Offline)
            });
        }
        CacheDecision::OfflineMiss => {
            plane.metrics.record_offline_miss();
            return Err(ResolutionError::Offline(
                "the proxy is offline and does not hold a verified copy of that content",
            ));
        }
        CacheDecision::Quarantined => {
            plane.metrics.record_quarantined();
            return Err(ResolutionError::BadGateway(
                "the upstream content is withheld pending review",
            ));
        }
        CacheDecision::Fetch => None,
        CacheDecision::Revalidate(known) => Some(known),
    };

    let key = PathKey::of(account, &repository.repo_key, asset_path);
    if current.is_none()
        && plane
            .negatives
            .contains(&key, config_version, std::time::Instant::now())
    {
        plane.metrics.record_negative_hit();
        return Err(ResolutionError::NotFound);
    }

    let (_, writer) = the_catalog(state)?;
    let actor = context.principal().subject().as_str();
    let overwrite = current.is_some();
    let flight = plane
        .flights
        .run((key.clone(), config_version), || {
            fetch_and_publish(
                plane,
                store.as_ref(),
                writer.as_ref(),
                &upstream,
                repository,
                asset_path,
                account,
                actor,
                correlation,
                known.as_ref(),
                overwrite,
                url,
                accept,
            )
        })
        .await;
    if flight.is_followed() {
        plane.metrics.record_coalesced();
    }

    use crate::proxy::resolution::Outcome;
    let status = match flight.into_inner() {
        Ok(Outcome::Stored(_)) if known.is_none() => {
            plane.metrics.record_miss();
            plane.negatives.forget(&key);
            CacheStatus::Miss
        }
        Ok(Outcome::Stored(_)) => {
            plane.metrics.record_refreshed();
            CacheStatus::Refreshed
        }
        Ok(Outcome::Unchanged(_)) => {
            plane.metrics.record_revalidated();
            CacheStatus::Revalidated
        }
        Ok(Outcome::Quarantined(receipt)) => {
            plane.metrics.record_quarantined();
            tracing::warn!(
                target: "hodei_artifacts::proxy",
                repository = %repository.repo_key,
                path = asset_path,
                receipt = %receipt.id,
                "the upstream offered other bytes for immutable content: withheld"
            );
            return Err(ResolutionError::BadGateway(
                "the upstream content is withheld pending review",
            ));
        }
        Err(failure) => {
            if failure == crate::proxy::cache::ProxyFailure::NotFound && current.is_none() {
                plane.negatives.remember(
                    &key,
                    config_version,
                    std::time::Duration::from_secs(upstream.cache.negative_secs),
                    std::time::Instant::now(),
                );
            }
            if failure.is_upstream_error()
                && let Some(asset) = current
                && entry
                    .as_ref()
                    .is_some_and(|entry| stale_allowed(&decision_context, entry))
            {
                plane.metrics.record_stale();
                tracing::warn!(
                    target: "hodei_artifacts::proxy",
                    repository = %repository.repo_key,
                    path = asset_path,
                    error = %failure,
                    "the upstream failed: serving the stale content"
                );
                return Ok(ProxyServed {
                    provenance: confirmed(entry.as_ref()),
                    ..ProxyServed::new(asset, CacheStatus::Stale)
                });
            }
            return Err(the_proxy_error(&failure));
        }
    };

    let asset = the_current_asset(state, context, &repository.repo_key, asset_path)
        .await?
        .ok_or(ResolutionError::Unavailable)?;
    Ok(ProxyServed::new(asset, status))
}

#[allow(clippy::too_many_arguments)]
async fn fetch_and_publish(
    plane: &crate::proxy::ProxyPlane,
    store: &dyn BlobStore,
    writer: &dyn CatalogWriter,
    upstream: &hodei_catalog::UpstreamConfig,
    repository: &RepositoryDescriptor,
    asset_path: &str,
    account: &str,
    actor: &str,
    correlation: &str,
    known: Option<&hodei_catalog::proxy_cache::Known>,
    overwrite: bool,
    url: Option<Url>,
    accept: Option<&str>,
) -> crate::proxy::FlightResult {
    use crate::proxy::resolution::{self, Outcome};

    plane.metrics.record_upstream_request();
    let request = resolution::Resolution {
        account,
        repository: &repository.repo_key,
        asset_path,
        upstream,
        config_version: repository.config.version,
        actor,
        correlation_id: correlation,
        accept,
        authorization: None,
        expect_digest: None,
        digest_path_prefix: None,
    };
    let outcome = match url {
        Some(url) => {
            resolution::resolve_url(
                &plane.fetcher,
                store,
                plane.ledger.as_ref(),
                &request,
                known,
                url,
            )
            .await
        }
        None => {
            resolution::resolve(
                &plane.fetcher,
                store,
                plane.ledger.as_ref(),
                &request,
                known,
            )
            .await
        }
    }
    .map_err(|error| {
        tracing::warn!(
            target: "hodei_artifacts::proxy",
            repository = %repository.repo_key,
            error = %error,
            "the proxy could not resolve the content"
        );
        let failure = the_proxy_failure(error);
        if failure.is_upstream_error() {
            plane.metrics.record_upstream_error();
        }
        failure
    })?;

    if let Outcome::Stored(resolved) = &outcome {
        let publication = Publication {
            repository: repository.repo_key.clone(),
            asset_path: asset_path.to_string(),
            coordinates: PackageCoordinates {
                package_name: asset_path.to_string(),
                package_version: resolved.digest.value.clone(),
            },
            digest: resolved.digest.clone(),
            size_bytes: resolved.size_bytes,
            tags: Vec::new(),
            actor: actor.to_string(),
            correlation_id: correlation.to_string(),
            overwrite,
            precondition: None,
            subject: None,
        };
        match writer.publish(account, publication).await {
            Ok(_) => {}
            Err(CatalogError::Conflict { .. }) if !overwrite => {}
            Err(error) => {
                return Err(crate::proxy::cache::ProxyFailure::Local(the_local_failure(
                    &the_publication_error(&error),
                )));
            }
        }
    }
    Ok(outcome)
}

fn the_proxy_failure(
    error: crate::proxy::resolution::ResolveError,
) -> crate::proxy::cache::ProxyFailure {
    use crate::proxy::cache::ProxyFailure;
    use crate::proxy::fetch::FetchError;
    use crate::proxy::resolution::ResolveError;

    match error {
        ResolveError::Fetch(FetchError::NotFound) => ProxyFailure::NotFound,
        ResolveError::Fetch(FetchError::Busy) => ProxyFailure::Busy,
        ResolveError::Fetch(FetchError::Timeout) => ProxyFailure::Timeout,
        ResolveError::Fetch(FetchError::OutOfScope) => ProxyFailure::OutOfScope,
        ResolveError::Fetch(FetchError::Offline) => ProxyFailure::Offline,
        ResolveError::Fetch(FetchError::Egress(_)) => ProxyFailure::NotReachable,
        ResolveError::Fetch(FetchError::ChecksumMismatch | FetchError::ChecksumMissing) => {
            ProxyFailure::Unverified
        }
        ResolveError::Fetch(FetchError::Truncated | FetchError::TooLarge) => {
            ProxyFailure::Incomplete
        }
        ResolveError::Fetch(_) => ProxyFailure::UpstreamFailed,
        ResolveError::Blob(error) => {
            ProxyFailure::Local(the_local_failure(&the_blob_error(&error)))
        }
        ResolveError::Ledger(error) => {
            ProxyFailure::Local(the_local_failure(&the_publication_error(&error)))
        }
    }
}

fn the_local_failure(error: &ResolutionError) -> crate::proxy::cache::LocalFailure {
    use crate::proxy::cache::LocalFailure;
    match error {
        ResolutionError::Unavailable => LocalFailure::Unavailable,
        ResolutionError::Conflict(_)
        | ResolutionError::PreconditionFailed
        | ResolutionError::Forbidden
        | ResolutionError::BadRequest(_) => LocalFailure::Rejected,
        _ => LocalFailure::Internal,
    }
}

fn the_proxy_error(failure: &crate::proxy::cache::ProxyFailure) -> ResolutionError {
    use crate::proxy::cache::{LocalFailure, ProxyFailure};
    match failure {
        ProxyFailure::NotFound | ProxyFailure::OutOfScope => ResolutionError::NotFound,
        ProxyFailure::Busy => ResolutionError::Unavailable,
        ProxyFailure::Timeout => ResolutionError::GatewayTimeout,
        ProxyFailure::NotReachable => {
            ResolutionError::BadGateway("the upstream is not reachable from this service")
        }
        ProxyFailure::Unverified => {
            ResolutionError::BadGateway("the upstream content could not be verified")
        }
        ProxyFailure::Incomplete => {
            ResolutionError::BadGateway("the upstream content is incomplete or too large")
        }
        ProxyFailure::UpstreamFailed => ResolutionError::BadGateway("the upstream did not deliver"),
        ProxyFailure::Offline => ResolutionError::Offline(
            "the proxy is offline and does not hold a verified copy of that content",
        ),
        ProxyFailure::Local(LocalFailure::Unavailable) => ResolutionError::Unavailable,
        ProxyFailure::Local(LocalFailure::Internal) => ResolutionError::Internal,
        ProxyFailure::Local(LocalFailure::Rejected) => {
            ResolutionError::Conflict("the publication of the upstream content was rejected")
        }
    }
}

/// Resolves an asset path according to the repository mode (hosted, proxy, or group),
/// returning an explicit [`ResolvedAsset`].
pub(crate) async fn resolve_asset(
    state: &AppState,
    context: &VerifiedContext,
    repository: RepositoryDescriptor,
    asset_path: &str,
    correlation: &str,
) -> Result<ResolvedAsset, ResolutionError> {
    if repository.mode == RepositoryMode::Group {
        let version = repository.config.version;
        let resolution = crate::handlers::raw_group::the_group_asset(
            state,
            context,
            &repository,
            asset_path,
            correlation,
        )
        .await;
        crate::handlers::raw_group::record_resolution(
            state,
            context,
            &repository,
            asset_path,
            &resolution.trail_line(),
            resolution.outcome.is_ok(),
            correlation,
        );
        let won = resolution.outcome?;
        let digest = won.asset.digest.clone();
        Ok(ResolvedAsset {
            asset: won.asset,
            digest,
            origin: ResolutionOrigin::Group {
                member: won.member.repo_key.clone(),
                group_version: version,
                proxy: won.served,
            },
            repository: won.member,
        })
    } else if repository.mode == RepositoryMode::Proxy {
        let served = the_proxy_asset(state, context, &repository, asset_path, correlation).await?;
        let digest = served.asset.digest.clone();
        Ok(ResolvedAsset {
            asset: served.asset,
            digest,
            origin: ResolutionOrigin::Proxy {
                status: served.status,
                provenance: served.provenance,
            },
            repository,
        })
    } else {
        let asset = the_current_asset(state, context, &repository.repo_key, asset_path)
            .await?
            .ok_or(ResolutionError::NotFound)?;
        let digest = asset.digest.clone();
        Ok(ResolvedAsset {
            asset,
            digest,
            origin: ResolutionOrigin::Hosted,
            repository,
        })
    }
}

/// Resolves an asset path and returns the legacy [`Resolved`] view.
pub(crate) async fn the_resolved_asset(
    state: &AppState,
    context: &VerifiedContext,
    repository: RepositoryDescriptor,
    asset_path: &str,
    correlation: &str,
) -> Result<Resolved, ResolutionError> {
    resolve_asset(state, context, repository, asset_path, correlation)
        .await
        .map(Resolved::from)
}

/// Streams a request body into the CAS blob store while enforcing size limits,
/// account quotas, declared digests, and immutability rules.
pub(crate) async fn write_content(
    state: &AppState,
    account: &str,
    body: Body,
    declared: Option<ContentDigest>,
    refuse_replacement: Option<&ContentDigest>,
) -> Result<WrittenContent, ResolutionError> {
    let store = the_content_store(state)?;
    let mut ticket = state
        .quota
        .begin(account, None)
        .map_err(ResolutionError::Quota)?;
    let mut session = store
        .begin()
        .await
        .map_err(|error| the_blob_error(&error))?;
    let mut hasher = Sha256::new();
    let mut length: u64 = 0;
    let limit = state.max_body_size as u64;

    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| ResolutionError::BadRequest("the body could not be read"))?;
        length = length.saturating_add(chunk.len() as u64);
        if length > limit {
            let _ = session.abort().await;
            return Err(ResolutionError::PayloadTooLarge);
        }
        if let Err(refusal) = ticket.admit(length) {
            let _ = session.abort().await;
            return Err(ResolutionError::Quota(refusal));
        }
        hasher.update(&chunk);
        session
            .append(&chunk)
            .await
            .map_err(|error| the_blob_error(&error))?;
    }

    let found = ContentDigest::sha256(hex_of(&hasher.finalize()));
    if let Some(declared) = declared
        && declared != found
    {
        let _ = session.abort().await;
        return Err(ResolutionError::BadRequest(
            "the content does not match the declared digest",
        ));
    }

    if let Some(current) = refuse_replacement
        && &found != current
    {
        let _ = session.abort().await;
        return Err(ResolutionError::Conflict(
            "the path already holds another content",
        ));
    }

    let already_had = store
        .exists(&found)
        .await
        .map_err(|error| the_blob_error(&error))?;
    let digest = session
        .commit(CommitExpectation {
            digest: found.clone(),
            length,
        })
        .await
        .map_err(|error| the_blob_error(&error))?;
    ticket.settle(if already_had { 0 } else { length });
    Ok(WrittenContent { digest, length })
}

/// Streams a request body into the CAS blob store, returning `(digest, length)`.
pub(crate) async fn write_the_content(
    state: &AppState,
    account: &str,
    body: Body,
    declared: Option<ContentDigest>,
    refuse_replacement: Option<&ContentDigest>,
) -> Result<(ContentDigest, u64), ResolutionError> {
    let written = write_content(state, account, body, declared, refuse_replacement).await?;
    Ok((written.digest, written.length))
}

/// Maps a blob store error into a [`ResolutionError`].
pub(crate) fn the_blob_error(error: &BlobError) -> ResolutionError {
    match error {
        BlobError::NotFound => ResolutionError::NotFound,
        BlobError::Corrupted { .. }
        | BlobError::LengthMismatch { .. }
        | BlobError::DigestMismatch { .. }
        | BlobError::UnsupportedAlgorithm(_) => ResolutionError::Internal,
        BlobError::SessionClosed => ResolutionError::Internal,
        BlobError::Unavailable(_) => ResolutionError::Unavailable,
    }
}

/// Maps a catalog publication error into a [`ResolutionError`].
pub(crate) fn the_publication_error(error: &CatalogError) -> ResolutionError {
    match error {
        CatalogError::Missing { .. } => ResolutionError::NotFound,
        CatalogError::Denied { .. } => ResolutionError::Forbidden,
        CatalogError::Conflict { .. } => {
            ResolutionError::Conflict("the path already holds another content")
        }
        CatalogError::PreconditionFailed { .. } => ResolutionError::PreconditionFailed,
        CatalogError::InvalidRequest { .. } => {
            ResolutionError::BadRequest("the publication is not well formed")
        }
        CatalogError::ModeNotImplemented { .. } => ResolutionError::NotFound,
        CatalogError::SchemaMismatch { .. } | CatalogError::Corrupt { .. } => {
            ResolutionError::Internal
        }
        CatalogError::Unavailable { .. } => ResolutionError::Unavailable,
    }
}

/// Applies `ETag` and `X-Request-Id` headers to an HTTP response.
pub(crate) fn apply_common_headers(response: &mut Response, etag: &str, correlation: &str) {
    if let Ok(value) = HeaderValue::from_str(etag) {
        response.headers_mut().insert(header::ETAG, value);
    }
    if let Ok(value) = HeaderValue::from_str(correlation) {
        response
            .headers_mut()
            .insert(HeaderName::from_static(CORRELATION_HEADER), value);
    }
}

/// Applies proxy cache headers (`X-Hodei-Cache`, `X-Hodei-Upstream`, `X-Hodei-Confirmed-At`).
pub(crate) fn apply_cache_headers(
    response: &mut Response,
    served: &Option<(CacheStatus, Option<(String, String)>)>,
) {
    let Some((status, provenance)) = served else {
        return;
    };
    response.headers_mut().insert(
        header::HeaderName::from_static("x-hodei-cache"),
        HeaderValue::from_static(status.as_str()),
    );
    if let Some((upstream, confirmed_at)) = provenance {
        if let Ok(value) = HeaderValue::from_str(upstream) {
            response
                .headers_mut()
                .insert(header::HeaderName::from_static("x-hodei-upstream"), value);
        }
        if let Ok(value) = HeaderValue::from_str(confirmed_at) {
            response.headers_mut().insert(
                header::HeaderName::from_static("x-hodei-confirmed-at"),
                value,
            );
        }
    }
}

/// Applies group resolution headers (`X-Hodei-Resolved-From`, `X-Hodei-Group-Version`).
pub(crate) fn apply_group_headers(response: &mut Response, resolved_by: &Option<(String, u64)>) {
    let Some((member, version)) = resolved_by else {
        return;
    };
    if let Ok(value) = HeaderValue::from_str(member) {
        response
            .headers_mut()
            .insert(HeaderName::from_static("x-hodei-resolved-from"), value);
    }
    response.headers_mut().insert(
        HeaderName::from_static("x-hodei-group-version"),
        HeaderValue::from(*version),
    );
}

/// Applies both proxy cache headers and group resolution headers from a [`ResolutionOrigin`].
pub(crate) fn apply_origin_headers(response: &mut Response, origin: &ResolutionOrigin) {
    apply_cache_headers(response, &origin.proxy_served());
    let resolved_by = origin
        .group_member()
        .map(|(member, version)| (member.to_string(), version));
    apply_group_headers(response, &resolved_by);
}

/// Encodes bytes as lowercase hexadecimal.
pub(crate) fn hex_of(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use axum::http::StatusCode;
    use hodei_authz::{
        AccountId, AuthenticationError, AuthorizationOutcome, AuthorizationRequest, Credentials,
        DenyReason, IndeterminateReason, PrincipalAuthenticator, PrincipalClaims, PrincipalId,
        VerifiedContext, verify_principal,
    };
    use hodei_catalog::{
        DigestScope, RepositoryConfig, RepositoryDescriptor, RepositoryFormat, RepositoryMode,
    };
    use kernel::Hrn as CanonicalHrn;

    use super::{
        CacheStatus, ResolutionError, ResolutionOrigin, authorize_repository, the_publication_error,
    };
    use crate::authorization::{AuthorizeResourceUseCase, AuthorizerPort};
    use crate::bootstrap::{BootstrapConfig, bootstrap};
    use crate::config::AppConfig;

    struct PermitAuthorizer;
    impl AuthorizerPort for PermitAuthorizer {
        fn authorize(&self, _request: &AuthorizationRequest) -> AuthorizationOutcome {
            AuthorizationOutcome::Permit
        }
    }

    struct DenyAuthorizer;
    impl AuthorizerPort for DenyAuthorizer {
        fn authorize(&self, _request: &AuthorizationRequest) -> AuthorizationOutcome {
            AuthorizationOutcome::Deny(DenyReason::NoMatchingPolicy)
        }
    }

    struct IndeterminateAuthorizer;
    impl AuthorizerPort for IndeterminateAuthorizer {
        fn authorize(&self, _request: &AuthorizationRequest) -> AuthorizationOutcome {
            AuthorizationOutcome::Indeterminate(IndeterminateReason::EvaluationError)
        }
    }

    struct StaticAuth;
    #[async_trait]
    impl PrincipalAuthenticator for StaticAuth {
        async fn authenticate(
            &self,
            _credentials: &Credentials,
        ) -> Result<PrincipalClaims, AuthenticationError> {
            Ok(PrincipalClaims::new(
                PrincipalId::new("hrn:hodei:iam::acme:User/alice").unwrap(),
                AccountId::new("acme").unwrap(),
            ))
        }
    }

    async fn test_context() -> VerifiedContext {
        VerifiedContext::from_principal(
            verify_principal(&StaticAuth, &Credentials::bearer("token"))
                .await
                .unwrap(),
        )
    }

    fn test_repository() -> RepositoryDescriptor {
        let now = chrono::Utc::now();
        RepositoryDescriptor {
            repo_key: "npm-hosted".to_string(),
            account: "acme".to_string(),
            hrn: CanonicalHrn::from_string("hrn:hodei:artifacts:eu:acme:repository/npm-hosted")
                .unwrap(),
            format: RepositoryFormat::Npm,
            mode: RepositoryMode::Hosted,
            config: RepositoryConfig {
                version: 7,
                digest_scope: DigestScope::Global,
                config_settings: serde_json::json!({}),
            },
            created_at: now,
            updated_at: now,
        }
    }

    async fn test_state(
        authorizer: Arc<dyn AuthorizerPort>,
    ) -> (tempfile::TempDir, crate::AppState) {
        let dir = tempfile::tempdir().unwrap();
        let mut config = AppConfig::default();
        config.rocksdb.path = dir
            .path()
            .join("test.rocksdb")
            .to_string_lossy()
            .to_string();
        let mut state = bootstrap(
            &config,
            BootstrapConfig {
                register_iam_schema: false,
                schema_version: None,
                validate_schemas: false,
            },
        )
        .await
        .unwrap();
        state.authorize_resource = Arc::new(AuthorizeResourceUseCase::new(authorizer));
        (dir, state)
    }

    #[tokio::test]
    async fn permit_decision_carries_policy_revision_and_outcome() {
        let (_dir, state) = test_state(Arc::new(PermitAuthorizer)).await;
        let context = test_context().await;
        let repository = test_repository();

        let decision = authorize_repository(
            &state,
            &context,
            crate::actions::download_asset(),
            &repository,
        )
        .expect("permit should succeed");

        assert_eq!(decision.outcome, AuthorizationOutcome::Permit);
        assert_eq!(decision.repository_revision, 7);
        assert_eq!(decision.action, crate::actions::download_asset());
    }

    #[tokio::test]
    async fn deny_decision_never_degrades_to_not_found() {
        let (_dir, state) = test_state(Arc::new(DenyAuthorizer)).await;
        let context = test_context().await;
        let repository = test_repository();

        let error = authorize_repository(
            &state,
            &context,
            crate::actions::download_asset(),
            &repository,
        )
        .expect_err("deny must fail with Forbidden");

        assert!(matches!(error, ResolutionError::Forbidden));
        assert_eq!(error.the_verdict().0, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn indeterminate_decision_never_degrades_to_not_found() {
        let (_dir, state) = test_state(Arc::new(IndeterminateAuthorizer)).await;
        let context = test_context().await;
        let repository = test_repository();

        let error = authorize_repository(
            &state,
            &context,
            crate::actions::download_asset(),
            &repository,
        )
        .expect_err("indeterminate must fail with Internal");

        assert!(matches!(error, ResolutionError::Internal));
        assert_eq!(error.the_verdict().0, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn catalog_denied_never_degrades_to_not_found() {
        let error = the_publication_error(&hodei_catalog::CatalogError::Denied {
            action: "read",
            kind: "repository",
            key: "npm-hosted".to_string(),
            reason: "policy denied".to_string(),
        });
        assert!(matches!(error, ResolutionError::Forbidden));
        assert_eq!(error.the_verdict().0, StatusCode::FORBIDDEN);
    }

    #[test]
    fn resolution_origin_exposes_typed_proxy_and_group_details() {
        let hosted = ResolutionOrigin::Hosted;
        assert_eq!(hosted.proxy_served(), None);
        assert_eq!(hosted.group_member(), None);

        let proxy = ResolutionOrigin::Proxy {
            status: CacheStatus::Hit,
            provenance: None,
        };
        assert_eq!(proxy.proxy_served(), Some((CacheStatus::Hit, None)));
        assert_eq!(proxy.group_member(), None);

        let group = ResolutionOrigin::Group {
            member: "npm-proxy".to_string(),
            group_version: 3,
            proxy: Some((CacheStatus::Miss, None)),
        };
        assert_eq!(group.proxy_served(), Some((CacheStatus::Miss, None)));
        assert_eq!(group.group_member(), Some(("npm-proxy", 3)));
    }
}
