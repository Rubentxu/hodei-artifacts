//! The pull of an OCI **proxy** repository.
//!
//! An OCI repository in proxy mode does not store what was published to it:
//! it resolves against a distribution **upstream** and keeps what it downloads.
//! And it does so through the **distribution surface**, not the raw one: a
//! manifest is requested at `/v2/<name>/manifests/<reference>` and a blob at
//! `/v2/<name>/blobs/<digest>`, with the authentication challenge and the
//! `Docker-Content-Digest` the protocol specifies.
//!
//! # What it guarantees
//!
//! - **Identity is the digest.** A manifest requested by `sha256:...`
//!   that arrives with other bytes is not the one that was asked for: it is held and not
//!   served. A blob likewise. What is served is what was computed, not what
//!   the upstream said about itself.
//! - **The tag moves; the digest does not.** A tag is a mutable
//!   reference: when the upstream moves it, the new content is published and
//!   the tag moves with it, and what was behind it stays published
//!   with its digest. A digest never moves.
//! - **The challenge does not decide where the credential goes.** The realm the
//!   challenge names has to be the one the configuration declares: an upstream that
//!   invents a realm receives no credential, nor the request.
//! - **Offline, only what is intact and has a receipt.** Cached content without
//!   provenance is not served, and neither is what is not intact.
//!
//! # What is reused
//!
//! The network egress is that of the other surfaces --the ejection
//! policy, the limits, the queue--, and content verification is that
//! of the raw surface: [`crate::proxy::resolution`]. Only what is specific to
//! distribution is added here: the upstream path, the challenge,
//! the digest that is the identity, and the tag that moves.

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use hodei_authz::VerifiedContext;
use hodei_catalog::{
    PackageCoordinates, Publication, RepositoryDescriptor, UpstreamConfig, origin_of,
};
use hodei_evidence::ContentDigest;
use url::Url;

use crate::app_state::AppState;
use crate::oci::failure::OciFailure;
use crate::oci::target::{is_a_digest, the_digest_of_reference, the_tag_name};
use crate::oci::{the_catalog, the_content_store};
use crate::proxy::fetch::{FetchError, Fetched};
use crate::proxy::resolution::{self, Outcome};

/// The prefix of a manifest's asset path.
const MANIFESTS: &str = "manifests";

/// The prefix of a blob's asset path.
const BLOBS: &str = "blobs";

/// The prefix of a tag pointer's path.
///
/// A tag is a **mutable** reference: the same name can
/// point to other content. The catalog does not move the tags of a
/// proxy repository --a proxy accepts no writes from a client--, so
/// the pointer is stored as what it is: one more asset, at
/// `manifests/tag/<tag>`, that points to the digest.
///
/// And the **receipt** always goes to the digest path (`manifests/<digest>`):
/// the provenance of the content is a single one, whether it was requested by tag or
/// by digest.
const TAGS: &str = "manifests/tag";

/// The path of the tag list on the distribution surface.
///
/// `v2/<name>/tags/list`: the list of what the upstream has.
const TAGS_LIST: &str = "tags/list";

/// The receipts read to know whether a content has provenance.
const RECEIPTS_READ_ON_THE_HOT_PATH: usize = 4;

/// Media types accepted when fetching an OCI or Docker manifest from an upstream registry.
const OCI_MANIFEST_ACCEPT: &str = "application/vnd.oci.image.manifest.v1+json, application/vnd.oci.image.index.v1+json, application/vnd.docker.distribution.manifest.v2+json, application/vnd.docker.distribution.manifest.list.v2+json, application/vnd.docker.distribution.manifest.v1+json";

/// What is requested from the upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TheKind {
    /// A manifest, by tag or by digest.
    Manifest,
    /// A blob, always by digest.
    Blob,
}

impl TheKind {
    /// The asset path prefix in the catalog.
    fn the_prefix(self) -> &'static str {
        match self {
            TheKind::Manifest => MANIFESTS,
            TheKind::Blob => BLOBS,
        }
    }

    /// The upstream path word.
    fn the_upstream_word(self) -> &'static str {
        match self {
            TheKind::Manifest => "manifests",
            TheKind::Blob => "blobs",
        }
    }

    /// The error returned when the upstream does not have it.
    fn the_unknown(self, reference: &str) -> OciFailure {
        match self {
            TheKind::Manifest => {
                OciFailure::manifest_unknown(format!("the manifest {reference:?} is unknown"))
            }
            TheKind::Blob => OciFailure::blob_unknown(format!("the blob {reference:?} is unknown")),
        }
    }
}

/// A manifest from the upstream, by tag or by digest.
///
/// `GET /v2/<name>/manifests/<reference>`
pub async fn the_manifest(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    name: &str,
    reference: &str,
    headers: &HeaderMap,
    head: bool,
) -> Result<Response, OciFailure> {
    the_content(
        state,
        context,
        repository,
        name,
        reference,
        TheKind::Manifest,
        headers,
        head,
    )
    .await
}

/// A blob from the upstream, by digest.
///
/// `GET /v2/<name>/blobs/<digest>`
pub async fn the_blob(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    name: &str,
    digest: &str,
    headers: &HeaderMap,
    head: bool,
) -> Result<Response, OciFailure> {
    the_content(
        state,
        context,
        repository,
        name,
        digest,
        TheKind::Blob,
        headers,
        head,
    )
    .await
}

/// What the upstream answers when asked for its tags.
///
/// The distinction is what decides whether a group's listing is served: an
/// upstream that **responds** with its list contributes tags, and an upstream that
/// cannot be asked for the list --its scope does not cover it-- is a
/// member that cannot enumerate, not a failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TheUpstreamTags {
    /// The upstream gave the list of its tags.
    Listed(Vec<String>),
    /// The upstream **cannot** be asked for its list.
    NotInScope,
}

/// The tags the proxy's upstream has.
///
/// `GET /v2/<name>/tags/list` on the **member's** upstream: the group does not
/// rename its member, and the name that is asked is the member's.
///
/// It is the resolution path **without** the response: what an OCI group
/// queries on a proxy member to enumerate what is there. A proxy has no
/// tag rows in the catalog --its tag is a pointer that only
/// exists if someone already asked for it--, so the proxy's list lives in its
/// upstream.
///
/// The list is **metadata**, not content: it is not stored, not published as
/// an asset of the member and not served. What is checked is the usual: the
/// network egress, the upstream's scope and the credential, which only travels to the
/// declared origin.
pub async fn the_resolved_tags(
    state: &AppState,
    repository: &RepositoryDescriptor,
    name: &str,
    headers: &HeaderMap,
) -> Result<TheUpstreamTags, OciFailure> {
    let plane = state
        .proxy_plane
        .as_ref()
        .ok_or_else(|| OciFailure::unavailable("this service has no egress for remote proxies"))?;
    let correlation = crate::handlers::correlation_id(headers, &state.identity);

    // The upstream configuration. A configuration that is not understood is not
    // replaced by a default value: an unreadable network egress setting
    // cannot degrade to "allowed".
    let upstream = match UpstreamConfig::of(&repository.config) {
        Ok(Some(upstream)) => upstream,
        Ok(None) => return Err(OciFailure::unavailable("the proxy is not configured")),
        Err(_) => {
            return Err(OciFailure::unavailable(
                "the proxy configuration is not valid",
            ));
        }
    };

    // Upstream path: if upstream.base already targets a repository under /v2/<repo>/,
    // append only `tags/list`; otherwise use `v2/<name>/tags/list`.
    let upstream_path = upstream_oci_path(&upstream, name, TAGS_LIST);
    // What does not fall in a scope is not requested from the upstream: an upstream that does not
    // cover the listing is a member that cannot enumerate.
    //
    // The decision is left **visible**: the member contributes no tags to the
    // group's listing, and without a trace the client would see a shorter
    // listing, which is indistinguishable from "this member has no tags".
    let Some(url) = the_listed_url(&upstream, &upstream_path) else {
        tracing::warn!(
            target: "hodei_artifacts::oci",
            repository = %repository.repo_key,
            correlation_id = %correlation,
            "the scope of the upstream does not cover the tag list, so the member cannot be enumerated"
        );
        return Ok(TheUpstreamTags::NotInScope);
    };

    // The token: the one the upstream already requested, if it is still valid.
    let token_key = format!("{}|{}", repository.repo_key, upstream.origin());
    let mut token = plane.tokens.get(&token_key);
    let mut challenged = false;

    let body = loop {
        let fetched = plane
            .fetcher
            .open_conditional_with_token(&upstream, url.clone(), None, None, token.as_deref())
            .await;
        match fetched {
            Ok(Fetched::Content(opened)) => {
                // A **paginated** list is not served as if it were the whole
                // list: the upstream says so with the `Link` header, and what
                // we have is the first page. Serving it would be a
                // partial listing presented as complete.
                //
                // The listing **is not served**. The resolution path already
                // has its cache and its budget, and a badly followed pagination
                // --a cursor the upstream decides-- is worse than
                // a listing that refuses to be served.
                if opened.link().is_some() {
                    tracing::warn!(
                        target: "hodei_artifacts::oci",
                        repository = %repository.repo_key,
                        correlation_id = %correlation,
                        "the upstream paginates its tag list, so the listing is not served"
                    );
                    return Err(OciFailure::unavailable(
                        "the upstream paginates its tag list and the listing is not served",
                    ));
                }
                break the_body(*opened).await?;
            }
            // The upstream asks for a credential: the challenge is resolved and
            // retried **once**. A challenge that cannot be satisfied is an
            // upstream that does not serve, not a loop.
            Ok(Fetched::Unauthorized { challenge }) => {
                if challenged {
                    return Err(OciFailure::unavailable(
                        "the upstream asks for a credential that could not be used",
                    ));
                }
                challenged = true;
                let challenge = challenge.unwrap_or_default();
                let fresh = resolve_challenge_token(plane, &upstream, &challenge).await?;
                plane.tokens.put(
                    &token_key,
                    fresh.clone(),
                    std::time::Duration::from_secs(300),
                );
                token = Some(fresh);
            }
            Ok(Fetched::NotModified(_)) => {
                return Err(OciFailure::unavailable(
                    "the upstream answered a conditional question that was not asked",
                ));
            }
            // An upstream that does not have that repository has no tags:
            // it is a member with nothing to enumerate, not a failure.
            Err(FetchError::NotFound) => return Ok(TheUpstreamTags::Listed(Vec::new())),
            Err(error) => return Err(the_fetch_failure(error)),
        }
    };

    tracing::debug!(
        target: "hodei_artifacts::oci",
        repository = %repository.repo_key,
        correlation_id = %correlation,
        "the tag list of an upstream was read"
    );
    Ok(TheUpstreamTags::Listed(the_tag_names(&body)?))
}

/// Builds the relative upstream path for an OCI distribution endpoint.
///
/// When `upstream.base` already includes `/v2/<repo>/`, the suffix (such as
/// `manifests/<ref>`, `blobs/<digest>`, or `tags/list`) is appended directly so
/// a valid single-segment Hodei `repo_key` can proxy a multi-segment upstream
/// repository (e.g. `ghcr.io/v2/org/charts/name/`).
fn upstream_oci_path(upstream: &UpstreamConfig, name: &str, suffix: &str) -> String {
    let base_path = upstream.base.path();
    if base_path.starts_with("/v2/") && base_path.len() > 4 {
        suffix.to_string()
    } else {
        format!("v2/{name}/{suffix}")
    }
}

/// The listing URL, if the upstream's scope covers it.
///
/// What does not fall in the scope is not requested: an upstream that does not cover the
/// listing is a member that **cannot enumerate**, not a failure.
fn the_listed_url(upstream: &UpstreamConfig, path: &str) -> Option<Url> {
    if !upstream.in_scope(path) {
        return None;
    }
    upstream.target(path)
}

/// The bytes of the upstream response body.
///
/// The bytes are read with the usual guardian: the client's budget
/// and the checksum the upstream declares. What is not declared is not required:
/// a tag list is metadata, and the protocol does not ask it for a digest.
async fn the_body(mut opened: crate::proxy::fetch::Opened) -> Result<Vec<u8>, OciFailure> {
    let mut guard = opened.guard(false);
    let mut body = Vec::new();
    while let Some(chunk) = opened
        .chunk()
        .await
        .map_err(|_| OciFailure::unavailable("the answer of the upstream could not be read"))?
    {
        guard.push(chunk.len()).map_err(|_| {
            OciFailure::unavailable("the answer of the upstream is larger than allowed")
        })?;
        body.extend_from_slice(&chunk);
    }
    let digest = ContentDigest::of_bytes(&body);
    guard
        .finish(&digest.value)
        .map_err(|_| OciFailure::unavailable("the answer of the upstream is not usable"))?;
    Ok(body)
}

/// The tag names of the upstream's list.
///
/// The distribution list is `{"name": ..., "tags": [...]}`. What
/// matters are the names: a list without `tags` is an empty list, and a
/// name that is not text is not a tag.
///
/// The listing only announces **what can be downloaded**: a name is
/// filtered by the same rule that resolution uses ([`the_tag_name`]), so
/// that the list and the resolution tell the same story. A name that
/// resolution rejects --a slash, a control character, more than 128
/// characters-- is downloaded with a `400` that **stops** the resolution of the
/// whole group, so announcing it would be promising something that cannot be
/// requested.
///
/// It is not a failure: an upstream may have garbage in its list, and the listing
/// does not break because of it. What cannot be downloaded is not announced.
fn the_tag_names(body: &[u8]) -> Result<Vec<String>, OciFailure> {
    let answer: serde_json::Value = serde_json::from_slice(body)
        .map_err(|_| OciFailure::unavailable("the tag list of the upstream is not readable"))?;
    let Some(tags) = answer.get("tags").and_then(|value| value.as_array()) else {
        return Ok(Vec::new());
    };
    Ok(tags
        .iter()
        .filter_map(|tag| tag.as_str())
        .filter(|tag| the_tag_name(tag).is_ok())
        .map(str::to_string)
        .collect())
}

/// The network egress failure, in the distribution dialect.
///
/// What the client sees says neither the upstream URL nor the exact reason:
/// the upstream is the service's internal network.
fn the_fetch_failure(error: FetchError) -> OciFailure {
    match error {
        FetchError::Unauthorized(_) => {
            OciFailure::unavailable("the upstream asks for a credential that could not be used")
        }
        FetchError::Offline => {
            OciFailure::unavailable("the proxy is offline and does not reach any upstream")
        }
        _ => OciFailure::unavailable("the upstream did not deliver a usable answer"),
    }
}

/// The content the upstream resolves, with its provenance.
///
/// It is the resolution **without** the HTTP response: an OCI group's path
/// needs the digest a proxy member won, not a `Response` that already
/// knows how to serve. The provenance is that of the receipt the resolver leaves, and the
/// content is published in the **member's** namespace, not in that of whoever
/// asks.
///
/// The bytes do **not** come here: they are read after the trust
/// gate, which decides whether what was resolved can be served.
/// Reading them earlier would materialize what cannot yet be served, and
/// make the conditional path pay for a read it does not need.
pub struct Resolved {
    /// The asset row, in the namespace of the resolved repository.
    pub asset: hodei_catalog::AssetRecord,
}

/// The manifest the upstream resolves, with its provenance.
///
/// It is the path of [`the_manifest`] without the response: what an OCI group
/// queries on a proxy member.
pub async fn the_resolved_manifest(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    name: &str,
    reference: &str,
    headers: &HeaderMap,
) -> Result<Resolved, OciFailure> {
    the_resolved(
        state,
        context,
        repository,
        name,
        reference,
        TheKind::Manifest,
        headers,
    )
    .await
}

/// The blob the upstream resolves, with its provenance.
///
/// It is the path of [`the_blob`] without the response: what an OCI group
/// queries on a proxy member.
pub async fn the_resolved_blob(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    name: &str,
    digest: &str,
    headers: &HeaderMap,
) -> Result<Resolved, OciFailure> {
    the_resolved(
        state,
        context,
        repository,
        name,
        digest,
        TheKind::Blob,
        headers,
    )
    .await
}

/// The upstream's content, resolved and verified.
///
/// The path is the same as that of a **hosted** repository: the account
/// comes from the verified principal, Cedar decides on the repository, and
/// the trust gate decides on the **digest** that is going to be served.
/// What changes is where the bytes come from: an upstream, and with the
/// distribution rules in front.
///
/// Resolution is separate from the response: whoever needs the content
/// --an OCI group, which queries a proxy member-- does not want a
/// `Response`, but the asset row with its digest. The response is assembled
/// afterwards, with what the resolution returns.
#[allow(clippy::too_many_arguments)]
async fn the_content(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    name: &str,
    reference: &str,
    kind: TheKind,
    headers: &HeaderMap,
    head: bool,
) -> Result<Response, OciFailure> {
    let resolved = the_resolved(state, context, repository, name, reference, kind, headers).await?;
    the_served(
        state,
        context,
        repository,
        &resolved.asset,
        kind,
        headers,
        head,
    )
    .await
}

/// The resolution of the upstream's content, with its provenance.
///
/// It is the body of the path: the same as that of a **hosted** repository,
/// with the verified principal's account and the trust gate
/// applied to the digest that is going to be served. What changes is where
/// the bytes come from: an upstream, and with the distribution rules in
/// front.
///
/// The content is published in the namespace of the **repository** that is
/// asked --the member, when whoever asks is a group-- and the receipt
/// that backs it goes with it.
async fn the_resolved(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    name: &str,
    reference: &str,
    kind: TheKind,
    headers: &HeaderMap,
) -> Result<Resolved, OciFailure> {
    let plane = state
        .proxy_plane
        .as_ref()
        .ok_or_else(|| OciFailure::unavailable("this service has no egress for remote proxies"))?;
    let correlation = crate::handlers::correlation_id(headers, &state.identity);

    // The upstream configuration. A configuration that is not understood is not
    // replaced by a default value: an unreadable network egress setting
    // cannot degrade to "allowed".
    let upstream = match UpstreamConfig::of(&repository.config) {
        Ok(Some(upstream)) => upstream,
        Ok(None) => return Err(OciFailure::unavailable("the proxy is not configured")),
        Err(_) => {
            return Err(OciFailure::unavailable(
                "the proxy configuration is not valid",
            ));
        }
    };

    // The reference: a digest (immutable) or a tag (mutable).
    //
    // The upstream path carries the digest **with its algorithm** --the
    // distribution asks for it that way-- and the catalog path carries the
    // bare hexadecimal.
    let (requested, upstream_reference) = match kind {
        TheKind::Blob => {
            // A blob reference arrives already validated and in hexadecimal:
            // the upstream path asks for it with its algorithm.
            let digest = reference.to_string();
            (Some(digest.clone()), format!("sha256:{digest}"))
        }
        TheKind::Manifest if is_a_digest(reference) => {
            let digest = the_digest_of_reference(reference)?;
            (Some(digest.clone()), reference.to_string())
        }
        TheKind::Manifest => (None, reference.to_string()),
    };
    // A tag has rules: it is not just any word.
    if kind == TheKind::Manifest && requested.is_none() {
        the_tag_name(reference)?;
    }

    // Upstream path: if upstream.base already targets a repository under /v2/<repo>/,
    // append `<kind>/<ref>`; otherwise use `v2/<name>/<kind>/<ref>`.
    let upstream_path = upstream_oci_path(
        &upstream,
        name,
        &format!("{}/{upstream_reference}", kind.the_upstream_word()),
    );
    // What does not fall in a scope is not requested from the upstream.
    if !upstream.in_scope(&upstream_path) {
        return Err(kind.the_unknown(reference));
    }
    let Some(url) = upstream.target(&upstream_path) else {
        return Err(kind.the_unknown(reference));
    };

    // The digest that is served: the one requested, or the one the
    // tag points to. A tag not yet known has no digest: it has
    // to be fetched.
    let (reader, writer) = the_catalog(state)?;
    let account = context.account().as_str();
    let resolved = match requested.clone() {
        Some(digest) => Some(digest),
        // The tag is resolved by its pointer: the asset that says which
        // digest it points to.
        None => reader
            .asset(account, name, &format!("{TAGS}/{reference}"))
            .await
            .ok()
            .map(|asset| asset.digest.value),
    };
    let asset_path = resolved
        .as_ref()
        .map(|digest| format!("{}/{digest}", kind.the_prefix()));

    // The path class: a digest does not move, a tag does.
    let cache_kind = match kind {
        TheKind::Blob => hodei_catalog::proxy_cache::CacheKind::Content,
        TheKind::Manifest => match requested {
            Some(_) => hodei_catalog::proxy_cache::CacheKind::Content,
            None => hodei_catalog::proxy_cache::CacheKind::Metadata,
        },
    };

    // What exists today of the content, with the provenance that backs it.
    let current = match asset_path.as_deref() {
        Some(path) => reader.asset(account, name, path).await.ok(),
        None => None,
    };
    let receipts = match current {
        Some(_) => plane
            .ledger
            .latest_receipts(
                account,
                &repository.repo_key,
                asset_path.as_deref().unwrap_or(""),
                RECEIPTS_READ_ON_THE_HOT_PATH,
            )
            .await
            .unwrap_or_default(),
        None => Vec::new(),
    };
    let entry = current.as_ref().map(|asset| {
        hodei_catalog::proxy_cache::CacheEntry::of(&asset.digest, asset.size_bytes, &receipts)
    });
    let config_version = repository.config.version;
    let decision_context = hodei_catalog::proxy_cache::Context {
        format: repository.format,
        policy: &upstream.cache,
        asset_path: asset_path.as_deref().unwrap_or(""),
        upstream_id: &upstream.id,
        in_scope: upstream.in_scope(&upstream_path),
        config_version,
        offline: upstream.cache.offline || plane.is_offline(),
        now: chrono::Utc::now(),
        kind: Some(cache_kind),
    };
    let decision = hodei_catalog::proxy_cache::decide(&decision_context, entry.as_ref());

    use hodei_catalog::proxy_cache::CacheDecision;
    let known = match decision {
        CacheDecision::OutOfScope => return Err(kind.the_unknown(reference)),
        CacheDecision::Fresh => {
            plane.metrics.record_hit();
            let asset = current.ok_or_else(|| {
                OciFailure::unavailable("the catalog and the cache disagree about the content")
            })?;
            return Ok(Resolved { asset });
        }
        CacheDecision::OfflineServe => {
            let asset = current.ok_or_else(|| {
                OciFailure::unavailable("the catalog and the cache disagree about the content")
            })?;
            // Offline there is nobody to fix a damaged blob: it is checked
            // before serving it.
            let store = the_content_store(state)?;
            if store.verify(&asset.digest).await.is_err() {
                plane.metrics.record_offline_miss();
                return Err(OciFailure::unavailable(
                    "the cached content failed its integrity check",
                ));
            }
            plane.metrics.record_offline_hit();
            return Ok(Resolved { asset });
        }
        CacheDecision::OfflineMiss => {
            plane.metrics.record_offline_miss();
            return Err(OciFailure::unavailable(
                "the proxy is offline and does not hold a verified copy of that content",
            ));
        }
        CacheDecision::Quarantined => {
            plane.metrics.record_quarantined();
            return Err(OciFailure::unavailable(
                "the upstream content is withheld pending review",
            ));
        }
        CacheDecision::Fetch => None,
        CacheDecision::Revalidate(known) => Some(known),
    };

    // What is fetched from the upstream, with the identity that is expected.
    let fetched = the_fetched(
        state,
        plane,
        repository,
        &upstream,
        &url,
        kind,
        requested.as_deref(),
        known.as_ref(),
        asset_path.as_deref(),
        context,
        &correlation,
    )
    .await?;

    // The digest that is served: the one that was computed, not the one the upstream said.
    let digest = match &fetched {
        Outcome::Stored(resolved) => resolved.digest.clone(),
        Outcome::Unchanged(receipt) => ContentDigest::sha256(receipt.digest.value.clone()),
        Outcome::Quarantined(_) => {
            return Err(OciFailure::unavailable(
                "the upstream content is withheld pending review",
            ));
        }
    };

    // The content is published in the catalog: a manifest is an asset at
    // `manifests/<digest>` and a blob at `blobs/<digest>`, just as in a
    // hosted repository. Without publishing, the digest is not content of this
    // repository and cannot be served.
    if let Outcome::Stored(resolved) = &fetched {
        let fetched_size = resolved.size_bytes;
        let publication = Publication {
            repository: repository.repo_key.clone(),
            asset_path: format!("{}/{}", kind.the_prefix(), digest.value),
            coordinates: PackageCoordinates {
                package_name: format!("{}/{}", kind.the_prefix(), digest.value),
                package_version: digest.value.clone(),
            },
            digest: digest.clone(),
            size_bytes: fetched_size,
            tags: Vec::new(),
            actor: context.principal().subject().as_str().to_string(),
            correlation_id: correlation.to_string(),
            // The path carries the digest: republishing the same content is the
            // same, and there is nothing to replace.
            overwrite: true,
            precondition: None,
            subject: None,
        };
        writer
            .publish(account, publication)
            .await
            .map_err(|error| {
                tracing::warn!(
                    target: "hodei_artifacts::oci",
                    repository = %repository.repo_key,
                    error = %error,
                    "the content of the upstream could not be published"
                );
                crate::oci::the_catalog_error(&error)
            })?;

        // The tag **moves**: the protocol says a tag is
        // mutable, and what is behind the previous one stays published with
        // its digest. The pointer is one more asset, because the catalog does not
        // move a proxy's tags: that belongs to a client, and a
        // proxy accepts no writes from a client.
        if kind == TheKind::Manifest && requested.is_none() {
            let pointer = Publication {
                repository: repository.repo_key.clone(),
                asset_path: format!("{TAGS}/{reference}"),
                coordinates: PackageCoordinates {
                    package_name: format!("{TAGS}/{reference}"),
                    package_version: digest.value.clone(),
                },
                digest: digest.clone(),
                size_bytes: fetched_size,
                tags: Vec::new(),
                actor: context.principal().subject().as_str().to_string(),
                correlation_id: correlation.to_string(),
                // The pointer moves: it is a mutable reference, and what
                // is behind it stays published with its digest.
                overwrite: true,
                precondition: None,
                subject: None,
            };
            writer.publish(account, pointer).await.map_err(|error| {
                tracing::warn!(
                    target: "hodei_artifacts::oci",
                    repository = %repository.repo_key,
                    error = %error,
                    "the pointer of the tag could not be published"
                );
                crate::oci::the_catalog_error(&error)
            })?;
        }
    }

    // And what is in the catalog is served, by the usual path.
    let (reader, _) = the_catalog(state)?;
    let asset = reader
        .asset(
            account,
            name,
            &format!("{}/{}", kind.the_prefix(), digest.value),
        )
        .await
        .map_err(|_| kind.the_unknown(reference))?;
    Ok(Resolved { asset })
}

/// The bytes the asset row references.
///
/// The bytes are those of the row, not those the request states: the row is the
/// reference of what the repository has. And they are read **after** the
/// trust gate: what cannot be served is not materialized, and
/// the conditional path does not pay for a read it does not need.
pub async fn the_bytes(
    state: &AppState,
    asset: &hodei_catalog::AssetRecord,
) -> Result<Vec<u8>, OciFailure> {
    let store = the_content_store(state)?;
    store
        .get(&asset.digest)
        .await
        .map_err(|_| OciFailure::unavailable("the content is not in the store"))
}

/// The content fetched from the upstream, verified and with a receipt.
///
/// The path is that of the raw surface --fetch, check, store and
/// record-- with two distribution rules in front: the
/// **expected digest** (the content's identity) and the **token** the
/// upstream may ask for in its challenge.
#[allow(clippy::too_many_arguments)]
async fn the_fetched(
    state: &AppState,
    plane: &crate::proxy::ProxyPlane,
    repository: &RepositoryDescriptor,
    upstream: &UpstreamConfig,
    url: &Url,
    kind: TheKind,
    requested: Option<&str>,
    known: Option<&hodei_catalog::proxy_cache::Known>,
    asset_path: Option<&str>,
    context: &VerifiedContext,
    correlation: &str,
) -> Result<Outcome, OciFailure> {
    let store = the_content_store(state)?;
    let token_key = format!("{}|{}", repository.repo_key, upstream.origin());
    let token = plane.tokens.get(&token_key);

    let outcome = match resolution::resolve_url(
        &plane.fetcher,
        store.as_ref(),
        plane.ledger.as_ref(),
        &the_request(
            context,
            repository,
            upstream,
            kind,
            asset_path,
            requested,
            correlation,
            token.as_deref(),
        ),
        known,
        url.clone(),
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(error) => {
            // The upstream asks for a credential: the challenge is resolved and
            // retried **once**. A challenge that cannot be satisfied is
            // an upstream that does not serve, not a loop.
            let Some(challenge) = the_challenge(&error) else {
                return Err(the_failure(error));
            };
            let token = resolve_challenge_token(plane, upstream, &challenge).await?;
            plane.tokens.put(
                &token_key,
                token.clone(),
                std::time::Duration::from_secs(300),
            );
            resolution::resolve_url(
                &plane.fetcher,
                store.as_ref(),
                plane.ledger.as_ref(),
                &the_request(
                    context,
                    repository,
                    upstream,
                    kind,
                    asset_path,
                    requested,
                    correlation,
                    Some(&token),
                ),
                known,
                url.clone(),
            )
            .await
            .map_err(the_failure)?
        }
    };

    // A token the upstream rejected is not reused.
    if matches!(outcome, Outcome::Quarantined(_)) {
        plane.tokens.forget(&token_key);
    }
    Ok(outcome)
}

/// The resolution request, with the distribution rules.
///
/// What is requested by digest has to be what arrives, and the receipt goes to
/// the digest path: a receipt on a tag's path does not back the
/// content that is there.
#[allow(clippy::too_many_arguments)]
fn the_request<'a>(
    context: &'a VerifiedContext,
    repository: &'a RepositoryDescriptor,
    upstream: &'a UpstreamConfig,
    kind: TheKind,
    asset_path: Option<&'a str>,
    requested: Option<&'a str>,
    correlation: &'a str,
    authorization: Option<&'a str>,
) -> resolution::Resolution<'a> {
    resolution::Resolution {
        account: context.account().as_str(),
        repository: &repository.repo_key,
        asset_path: asset_path.unwrap_or(""),
        upstream,
        config_version: repository.config.version,
        actor: context.principal().subject().as_str(),
        correlation_id: correlation,
        accept: match kind {
            TheKind::Manifest => Some(OCI_MANIFEST_ACCEPT),
            TheKind::Blob => None,
        },
        authorization,
        expect_digest: requested,
        digest_path_prefix: Some(kind.the_prefix()),
    }
}

/// The challenge of an upstream that asks for a token.
///
/// The challenge text is the one the upstream sent: `WWW-Authenticate`. From
/// it the realm, the service and the scope are read.
fn the_challenge(error: &resolution::ResolveError) -> Option<String> {
    match error {
        resolution::ResolveError::Fetch(FetchError::Unauthorized(challenge)) => {
            Some(challenge.clone())
        }
        _ => None,
    }
}

/// Resolves a Bearer token challenge, using the configured credential if
/// `credential_ref` is declared, or an anonymous token request otherwise.
async fn resolve_challenge_token(
    plane: &crate::proxy::ProxyPlane,
    upstream: &UpstreamConfig,
    challenge: &str,
) -> Result<String, OciFailure> {
    let credential = if upstream.credential.is_some() {
        let Some(secret) = plane.fetcher.the_credential(upstream) else {
            return Err(OciFailure::unavailable(
                "the upstream asks for a credential and the repository has none",
            ));
        };
        Some(secret)
    } else {
        None
    };
    the_token(plane, upstream, challenge, credential.as_ref()).await
}

/// The realm's token, if the realm is the one the configuration declares.
///
/// The challenge's realm is stated by the upstream, and an upstream can say
/// anything: the credential is only requested if the challenge's realm **is**
/// the declared realm, with the same origin and the same path. A different
/// realm receives not even the request.
async fn the_token(
    plane: &crate::proxy::ProxyPlane,
    upstream: &UpstreamConfig,
    challenge: &str,
    credential: Option<&crate::proxy::fetch::Secret>,
) -> Result<String, OciFailure> {
    let declared = upstream.auth.as_ref().ok_or_else(|| {
        OciFailure::unavailable("the upstream asks for a credential and no realm is declared")
    })?;
    let challenge = the_bearer_challenge(challenge).ok_or_else(|| {
        OciFailure::unavailable(
            "the upstream asks for a credential in a way that is not understood",
        )
    })?;
    if origin_of(&challenge.realm) != origin_of(&declared.realm)
        || challenge.realm.path() != declared.realm.path()
    {
        tracing::warn!(
            target: "hodei_artifacts::proxy",
            upstream = %upstream.id,
            "the upstream asked for a token in a realm that is not the declared one"
        );
        return Err(OciFailure::unavailable(
            "the upstream asks for a credential in a realm that is not the declared one",
        ));
    }

    // The token request: to the declared realm, with the upstream's
    // credential and only to it.
    let mut url = declared.realm.clone();
    {
        let mut query = url.query_pairs_mut();
        if let Some(service) = declared.service.as_deref().or(challenge.service.as_deref()) {
            query.append_pair("service", service);
        }
        if let Some(scope) = challenge.scope.as_deref() {
            query.append_pair("scope", scope);
        }
    }
    let opened_result = match credential {
        Some(secret) => {
            let user = declared.the_username(&upstream.id).to_string();
            plane
                .fetcher
                .open_with_basic(upstream, url, &origin_of(&declared.realm), &user, secret)
                .await
        }
        None => plane.fetcher.open_anonymous_token(upstream, url).await,
    };
    let mut opened = opened_result.map_err(|error| {
        tracing::warn!(
            target: "hodei_artifacts::proxy",
            upstream = %upstream.id,
            error = %error,
            "the token could not be asked for"
        );
        OciFailure::unavailable("the credential of the upstream could not be used")
    })?;
    let mut body = Vec::new();
    while let Some(chunk) = opened.chunk().await.map_err(|_| {
        OciFailure::unavailable("the answer of the token endpoint could not be read")
    })? {
        body.extend_from_slice(&chunk);
    }
    let answer: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|_| OciFailure::unavailable("the answer of the token endpoint is not readable"))?;
    answer
        .get("token")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| OciFailure::unavailable("the answer of the token endpoint has no token"))
}

/// The `Bearer` challenge of a `WWW-Authenticate` header.
///
/// The format is `Bearer realm="...",service="...",scope="..."`. Only the
/// `Bearer` challenge is understood: it is the one the distribution protocol
/// specifies, and a challenge in another dialect is not guessed.
fn the_bearer_challenge(header: &str) -> Option<TheChallenge> {
    let rest = header.trim();
    let rest = rest
        .strip_prefix("Bearer ")
        .or_else(|| rest.strip_prefix("bearer "))?;
    let mut realm = None;
    let mut service = None;
    let mut scope = None;
    for part in rest.split(',') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        match key.trim() {
            "realm" => realm = Some(value.to_string()),
            "service" => service = Some(value.to_string()),
            "scope" => scope = Some(value.to_string()),
            _ => {}
        }
    }
    let realm = Url::parse(&realm?).ok()?;
    Some(TheChallenge {
        realm,
        service,
        scope,
    })
}

/// The authentication challenge of an upstream.
#[derive(Debug, Clone)]
struct TheChallenge {
    /// Where the token is requested.
    realm: Url,
    /// The service that is requested, if the challenge names it.
    service: Option<String>,
    /// The scope that is requested, if the challenge names it.
    scope: Option<String>,
}

/// The resolution failure, in the distribution dialect.
///
/// What the client sees says neither the upstream URL nor the exact reason:
/// the upstream is the service's internal network.
fn the_failure(error: resolution::ResolveError) -> OciFailure {
    match error {
        resolution::ResolveError::Fetch(FetchError::NotFound) => {
            OciFailure::manifest_unknown("the upstream does not have that content")
        }
        resolution::ResolveError::Fetch(FetchError::Unauthorized(_)) => {
            OciFailure::unavailable("the upstream asks for a credential that could not be used")
        }
        resolution::ResolveError::Fetch(FetchError::Offline) => {
            OciFailure::unavailable("the proxy is offline and does not reach any upstream")
        }
        resolution::ResolveError::Fetch(_) => {
            OciFailure::unavailable("the upstream did not deliver a usable content")
        }
        resolution::ResolveError::Blob(_) | resolution::ResolveError::Ledger(_) => {
            OciFailure::unavailable("the content could not be stored")
        }
    }
}

/// The content that is served, with the distribution headers.
///
/// The trust gate goes **before** the read conditions: a
/// `304` or a `412` would say the content exists. And the bytes are read
/// **after** both: what cannot be served is not materialized, and
/// a `304` does not pay for the blob read.
async fn the_served(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    asset: &hodei_catalog::AssetRecord,
    kind: TheKind,
    headers: &HeaderMap,
    head: bool,
) -> Result<Response, OciFailure> {
    let digest = asset.digest.value.clone();
    crate::trust::admit(state, context.account().as_str(), repository, &asset.digest)
        .await
        .map_err(|admission| match admission {
            crate::trust::Admission::Hidden => kind.the_unknown(&digest),
            crate::trust::Admission::Unavailable => {
                OciFailure::unavailable("the trust ledger is not available")
            }
        })?;

    let etag = crate::raw::conditional::the_etag(&asset.digest);
    let outcome = crate::raw::conditional::the_read_condition(
        crate::raw::conditional::the_if_match(headers).as_deref(),
        crate::raw::conditional::the_if_none_match(headers).as_deref(),
        Some(&etag),
    );
    match outcome {
        crate::raw::conditional::ReadOutcome::Failed => {
            return Err(OciFailure::manifest_unknown(
                "the content is not the one the client asked for",
            ));
        }
        crate::raw::conditional::ReadOutcome::NotModified => {
            let mut response = StatusCode::NOT_MODIFIED.into_response();
            let headers = response.headers_mut();
            headers.insert(
                header::ETAG,
                HeaderValue::from_str(&etag).expect("the etag is a header value"),
            );
            headers.insert(
                "Docker-Content-Digest",
                HeaderValue::from_str(&format!("sha256:{digest}"))
                    .expect("the digest is a header value"),
            );
            return Ok(response);
        }
        crate::raw::conditional::ReadOutcome::Serve => {}
    }

    // The bytes are read here, and not earlier: the gate has already said this
    // content can be served, and the conditional path has not paid for the
    // read.
    let bytes = the_bytes(state, asset).await?;

    // The media type is stated by the manifest, not the client: the manifest
    // is the truth of what it is. And they are the same types the registry
    // serves in a hosted repository: a proxy does not serve what the
    // registry does not serve.
    let media_type = match kind {
        TheKind::Blob => "application/octet-stream".to_string(),
        TheKind::Manifest => crate::handlers::oci::the_declared_media_type(&bytes)?,
    };

    // And the negotiation is the same: a client that only names types that
    // this registry does not serve receives the unknown-manifest error, not
    // a response that pretends to be what it asked for.
    if kind == TheKind::Manifest {
        crate::handlers::oci::the_negotiation(headers, &media_type)?;
    }

    let length = bytes.len().to_string();
    let mut response = if head {
        StatusCode::OK.into_response()
    } else {
        (StatusCode::OK, bytes).into_response()
    };
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&media_type).expect("the media type is a header value"),
    );
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&length).expect("the length is a header value"),
    );
    headers.insert(
        header::ETAG,
        HeaderValue::from_str(&etag).expect("the etag is a header value"),
    );
    headers.insert(
        "Docker-Content-Digest",
        HeaderValue::from_str(&format!("sha256:{digest}")).expect("the digest is a header value"),
    );
    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bearer_challenge_is_read() {
        let challenge = the_bearer_challenge(
            "Bearer realm=\"https://auth.example.com/token\",service=\"registry\",scope=\"repository:foo:pull\"",
        )
        .expect("the challenge");
        assert_eq!(challenge.realm.as_str(), "https://auth.example.com/token");
        assert_eq!(challenge.service.as_deref(), Some("registry"));
        assert_eq!(challenge.scope.as_deref(), Some("repository:foo:pull"));
    }

    #[test]
    fn a_challenge_that_is_not_bearer_is_not_understood() {
        assert!(the_bearer_challenge("Basic realm=\"registry\"").is_none());
        assert!(the_bearer_challenge("Bearer").is_none());
        assert!(the_bearer_challenge("").is_none());
        // A realm that is not a URL is not a realm.
        assert!(the_bearer_challenge("Bearer realm=\"not a url\"").is_none());
    }

    #[test]
    fn the_kind_of_a_route_says_what_moves_and_what_does_not() {
        // A digest does not move; a tag does.
        assert_eq!(
            TheKind::Manifest.the_prefix(),
            MANIFESTS,
            "un manifiesto vive en manifests/<digest>"
        );
        assert_eq!(TheKind::Blob.the_prefix(), BLOBS);
    }
}
