//! The resolution of content against its upstream: the first time and each
//! revalidation.
//!
//! It joins the three pieces of the proxy: the egress client ([`ProxyFetcher`]),
//! the content store and the provenance ledger. The order matters and is
//! what gives the guarantee:
//!
//! 1. The bytes go into a write session that is **not visible**.
//! 2. When the body ends it is checked: complete, within the budget
//!    and with the checksum the upstream declared.
//! 3. Only then is the blob published (`commit`) and the receipt written.
//!
//! A fetch that fails at any point aborts the session and leaves neither
//! content nor a receipt: nothing left in the system can be presented
//! as verified content from an upstream.
//!
//! If content is already held for the path ([`Known`]) the request is
//! conditional and there are three outcomes besides fetching it:
//!
//! - the upstream confirms (`304`) or delivers the same bytes: a
//!   `Revalidated` receipt is written and nothing is moved;
//! - it delivers **other** bytes for immutable content: they are not published, and
//!   the `Quarantined` receipt keeps what was offered;
//! - it delivers other bytes for mutable metadata: it is an update, and it is
//!   published.

use chrono::Utc;
use hodei_catalog::proxy_cache::Known;
use hodei_catalog::{
    CatalogError, ChecksumEvidence, ProvenanceLedger, ProvenanceReceipt, ReceiptOutcome,
    UpstreamConfig,
};
use hodei_evidence::domain::content::ContentDigest;
use hodei_hub::blob::{BlobError, BlobStore, CommitExpectation};
use url::Url;

use super::fetch::{FetchError, Fetched, ProxyFetcher, Validators};

/// What needs to be known to resolve a path.
#[derive(Debug, Clone)]
pub struct Resolution<'a> {
    /// The account that owns the repository.
    pub account: &'a str,
    /// The proxy repository key.
    pub repository: &'a str,
    /// The path being requested.
    pub asset_path: &'a str,
    /// The configured upstream.
    pub upstream: &'a UpstreamConfig,
    /// The version of the repository configuration.
    pub config_version: u64,
    /// Who requests the content.
    pub actor: &'a str,
    /// The request's correlation.
    pub correlation_id: &'a str,
    /// The `Accept` requested from the upstream, if different from the usual one.
    pub accept: Option<&'a str>,
    /// A token the upstream already asked for and that travels as `Bearer`.
    ///
    /// Only used by the OCI surface, whose upstream may ask for a token in
    /// its challenge. The token travels **only** to the upstream's origin.
    pub authorization: Option<&'a str>,
    /// The digest the content **must** have.
    ///
    /// Used by the OCI surface, where the content's identity is the
    /// digest: a manifest requested by `sha256:...` that arrives with other
    /// bytes is not the one that was requested, and is not published.
    pub expect_digest: Option<&'a str>,
    /// The asset path prefix, when the path **is** the digest.
    ///
    /// The OCI surface stores the manifest at `manifests/<digest>` and the
    /// blob at `blobs/<digest>`, and the receipt has to go there: a receipt at
    /// the path it was requested by --a tag-- does not back the content
    /// that is there, and unbacked content is not served without network.
    pub digest_path_prefix: Option<&'a str>,
}

/// Content that was resolved and verified.
#[derive(Debug, Clone)]
pub struct Resolved {
    /// The content digest, already published in the store.
    pub digest: ContentDigest,
    /// Its bytes.
    pub size_bytes: u64,
    /// The receipt that was written.
    pub receipt: ProvenanceReceipt,
}

/// How a resolution that did not fail ended.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// There are new bytes, published in the store.
    Stored(Resolved),
    /// The upstream confirms the content that was already held.
    Unchanged(ProvenanceReceipt),
    /// The upstream delivered other bytes for immutable content: they were not
    /// published.
    Quarantined(ProvenanceReceipt),
}

/// Why it could not be resolved.
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    /// The upstream did not deliver usable content.
    #[error(transparent)]
    Fetch(#[from] FetchError),
    /// The store could not save the content.
    #[error("the content store failed: {0}")]
    Blob(BlobError),
    /// The provenance ledger could not save the receipt.
    #[error("the provenance ledger failed: {0}")]
    Ledger(#[from] CatalogError),
}

impl From<BlobError> for ResolveError {
    fn from(error: BlobError) -> Self {
        Self::Blob(error)
    }
}

/// Fetches, verifies, stores and records provenance.
///
/// If it returns `Ok`, the outcome is in the ledger: the new content is
/// published in the store **and** its receipt written, or the receipt says it was
/// confirmed or held back. If it returns `Err`, there is no receipt and nothing is served
/// this way: a failure before the commit leaves no content; one from the ledger,
/// after the commit, leaves an uncataloged blob that the GC collects (a blob
/// already present with that same digest is not touched: it is the same content).
pub async fn resolve(
    fetcher: &ProxyFetcher,
    store: &dyn BlobStore,
    ledger: &dyn ProvenanceLedger,
    request: &Resolution<'_>,
    known: Option<&Known>,
) -> Result<Outcome, ResolveError> {
    let Some(url) = request.upstream.target(request.asset_path) else {
        return Err(FetchError::OutOfScope.into());
    };
    resolve_url(fetcher, store, ledger, request, known, url).await
}

/// Like [`resolve`], but with the exact URL the content is fetched from.
///
/// Used by the PyPI surface: a project's page and each distribution
/// are requested at the URL the upstream declares, which is not the asset's
/// path. The URL is judged with the same egress policy as any
/// other.
pub async fn resolve_url(
    fetcher: &ProxyFetcher,
    store: &dyn BlobStore,
    ledger: &dyn ProvenanceLedger,
    request: &Resolution<'_>,
    known: Option<&Known>,
    url: Url,
) -> Result<Outcome, ResolveError> {
    let validators = known.and_then(|known| {
        let validators = Validators {
            etag: known.etag.clone(),
            last_modified: known.last_modified.clone(),
        };
        (!validators.is_empty()).then_some(validators)
    });
    let fetched = fetcher
        .open_conditional_with_token(
            request.upstream,
            url,
            request.accept,
            validators.as_ref(),
            request.authorization,
        )
        .await?;
    let mut opened = match fetched {
        Fetched::Content(opened) => *opened,
        // The upstream asks for a credential and there is none yet: the
        // caller has to resolve the challenge before retrying. The
        // challenge travels in the error, which is where what the
        // upstream says about how to authenticate is read.
        Fetched::Unauthorized { challenge } => {
            return Err(FetchError::Unauthorized(challenge.unwrap_or_default()).into());
        }
        Fetched::NotModified(confirmation) => {
            // A `304` is only accepted if validators were sent: without
            // `known` there is nothing to relate it to.
            let Some(known) = known else {
                return Err(FetchError::Status(304).into());
            };
            let receipt = ProvenanceReceipt {
                id: format!("prv-{}", uuid::Uuid::new_v4()),
                account: request.account.to_string(),
                repository: request.repository.to_string(),
                asset_path: the_asset_path(request, &known.digest.value),
                upstream_id: request.upstream.id.clone(),
                source_url: confirmation.source_url,
                config_version: request.config_version,
                digest: known.digest.clone(),
                size_bytes: known.size_bytes,
                // A `304` has no body to check: it inherits the
                // evidence of the receipt that backed those bytes.
                checksum: known
                    .checksum
                    .clone()
                    .unwrap_or(ChecksumEvidence::ComputedOnly),
                status: 304,
                redirects: confirmation.redirects,
                etag: confirmation.etag.or_else(|| known.etag.clone()),
                last_modified: confirmation
                    .last_modified
                    .or_else(|| known.last_modified.clone()),
                outcome: ReceiptOutcome::Revalidated,
                actor: request.actor.to_string(),
                correlation_id: request.correlation_id.to_string(),
                fetched_at: Utc::now(),
            };
            ledger.record(&receipt).await?;
            return Ok(Outcome::Unchanged(receipt));
        }
    };
    let mut guard = opened.guard(request.upstream.require_checksum);
    let mut session = store.begin().await?;

    // Any failure from here on discards the write in progress.
    loop {
        match opened.chunk().await {
            Ok(Some(chunk)) => {
                if let Err(error) = guard.push(chunk.len()) {
                    let _ = session.abort().await;
                    return Err(error.into());
                }
                if let Err(error) = session.append(&chunk).await {
                    let _ = session.abort().await;
                    return Err(error.into());
                }
            }
            Ok(None) => break,
            Err(error) => {
                let _ = session.abort().await;
                return Err(error.into());
            }
        }
    }

    // The digest is computed by the session while the chunks pass: it is the one
    // the commit will check and the one compared with what was declared.
    let computed = session.the_digest();
    let verified = match guard.finish(&computed.value) {
        Ok(verified) => verified,
        Err(error) => {
            let _ = session.abort().await;
            return Err(error.into());
        }
    };

    // The expected digest: the content has to **be** the one that was requested.
    //
    // The OCI surface needs it: a manifest requested by `sha256:...`
    // that arrives with other bytes is not the one that was requested, and content that
    // is not the requested one is not published --nor served, nor left with a receipt that
    // backs it. It is held back, and the receipt says so.
    if let Some(expected) = request.expect_digest
        && expected != computed.value
    {
        let _ = session.abort().await;
        let receipt = ProvenanceReceipt {
            id: format!("prv-{}", uuid::Uuid::new_v4()),
            account: request.account.to_string(),
            repository: request.repository.to_string(),
            // The receipt goes to the path of the **requested** content: it is the one
            // that has to know that what was offered was not its own.
            asset_path: the_asset_path(request, expected),
            upstream_id: request.upstream.id.clone(),
            source_url: opened.source_url(),
            config_version: request.config_version,
            digest: computed.clone(),
            size_bytes: verified.size_bytes,
            checksum: verified.evidence.clone(),
            status: 200,
            redirects: opened.redirects(),
            etag: opened.etag().map(str::to_string),
            last_modified: opened.last_modified().map(str::to_string),
            outcome: ReceiptOutcome::Quarantined,
            actor: request.actor.to_string(),
            correlation_id: request.correlation_id.to_string(),
            fetched_at: Utc::now(),
        };
        ledger.record(&receipt).await?;
        return Ok(Outcome::Quarantined(receipt));
    }

    let mut receipt = ProvenanceReceipt {
        id: format!("prv-{}", uuid::Uuid::new_v4()),
        account: request.account.to_string(),
        repository: request.repository.to_string(),
        asset_path: the_asset_path(request, &computed.value),
        upstream_id: request.upstream.id.clone(),
        source_url: opened.source_url(),
        config_version: request.config_version,
        digest: computed.clone(),
        size_bytes: verified.size_bytes,
        checksum: verified.evidence.clone(),
        status: 200,
        redirects: opened.redirects(),
        etag: opened.etag().map(str::to_string),
        last_modified: opened.last_modified().map(str::to_string),
        outcome: ReceiptOutcome::Fetched,
        actor: request.actor.to_string(),
        correlation_id: request.correlation_id.to_string(),
        fetched_at: Utc::now(),
    };

    // Content was already held: the question is whether these bytes are the same.
    if let Some(known) = known {
        let same = known.digest == computed;
        if same || known.immutable {
            // Neither the same bytes nor those offered as a change to
            // immutable content are published: there is nothing to write.
            let _ = session.abort().await;
            receipt.outcome = if same {
                ReceiptOutcome::Revalidated
            } else {
                ReceiptOutcome::Quarantined
            };
            ledger.record(&receipt).await?;
            return Ok(if same {
                Outcome::Unchanged(receipt)
            } else {
                Outcome::Quarantined(receipt)
            });
        }
    }

    let digest = match session
        .commit(CommitExpectation {
            digest: computed,
            length: verified.size_bytes,
        })
        .await
    {
        Ok(digest) => digest,
        Err(error) => {
            let _ = session.abort().await;
            return Err(error.into());
        }
    };
    // Without a receipt there is no provenance: if the ledger fails, the already
    // published content is left as one more uncataloged blob (collected by GC) and
    // the request fails; it is never served without its receipt.
    ledger.record(&receipt).await?;

    Ok(Outcome::Stored(Resolved {
        digest,
        size_bytes: verified.size_bytes,
        receipt,
    }))
}

/// The asset path that backs the receipt.
///
/// When the path **is** the content's digest --as in the OCI
/// surface, where the manifest lives at `manifests/<digest>`-- the receipt goes there
/// and not to the path it was requested by: a receipt at a tag's path
/// does not back the content that is there, and unbacked content is not
/// served without network.
fn the_asset_path(request: &Resolution<'_>, digest: &str) -> String {
    match request.digest_path_prefix {
        Some(prefix) => format!("{prefix}/{digest}"),
        None => request.asset_path.to_string(),
    }
}
