//! Dependency freeze: create, read, and release an immutable view (HSP-161).
//!
//! - `POST /api/v1/freezes`: freezes the current content of a hosted
//!   repository (optionally under a prefix). Cedar decides **who** with the
//!   dedicated action `ManageDependencyFreeze` on the repository; no other
//!   action implies it.
//! - `GET /api/v1/freezes/{id}`: the manifest, `(path, package, version) ->
//!   digest`.
//! - `GET /api/v1/freezes/{id}/content/{*path}`: the bytes of one path, served
//!   **only from the manifest**. A path the manifest does not name is
//!   `404 E_MISSING_DEPENDENCY`: there is no fallback to the live catalog and
//!   never to "latest". A digest the trusted channel of the origin no longer
//!   admits (revoked or withdrawn) is `403` and is not served.
//! - `POST /api/v1/freezes/unpin`: releases the freeze's holds (and only
//!   those). `ManageDependencyFreeze` again.
//!
//! The account always comes from the verified principal, and a freeze of
//! another account does not exist for the caller.

use axum::{
    Json,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Duration, Utc};
use hodei_authz::VerifiedContext;
use hodei_catalog::{CatalogError, RepositoryDescriptor, RepositoryFormat, RepositoryMode};
use hodei_evidence::ContentDigest;
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::freeze::{
    CODE_MISSING_DEPENDENCY, FreezeError, FreezeManifest, MAX_FREEZES_PER_ACCOUNT, hold_prefix,
    is_registry_hold, is_well_formed_id, manifest_hold_id, registry_account,
};
use crate::handlers::{correlation_id, record_audit};
use crate::resolution::{
    ResolutionError, authorize_repository, the_catalog, the_content_store, the_context,
    the_repository_in,
};

/// The formats a freeze can be taken from in this release.
const FROZEN_FORMATS: [RepositoryFormat; 2] = [RepositoryFormat::Raw, RepositoryFormat::Npm];

/// How a request that cannot be served ends.
#[derive(Debug)]
pub enum FreezeFailure {
    /// An error of the shared surface.
    Surface(ResolutionError),
    /// The freeze does not name this path: explicit, never a fallback.
    Missing(String),
    /// The origin channel no longer admits this digest.
    Withheld(String),
    /// The freeze existed and its lifetime is over.
    Expired,
    /// A bound of the feature was reached (`E_FREEZE_LIMIT`).
    Limit(String),
    /// A status and a reason.
    Plain(StatusCode, &'static str),
}

impl From<ResolutionError> for FreezeFailure {
    fn from(error: ResolutionError) -> Self {
        Self::Surface(error)
    }
}

impl IntoResponse for FreezeFailure {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::Surface(error) => return error.into_response(),
            Self::Missing(reason) => (
                StatusCode::NOT_FOUND,
                CODE_MISSING_DEPENDENCY,
                format!(
                    "{reason}; a freeze never falls back to the latest or to the live repository"
                ),
            ),
            Self::Withheld(reason) => (StatusCode::FORBIDDEN, "E_WITHHELD", reason),
            Self::Expired => (
                StatusCode::GONE,
                "E_FREEZE_EXPIRED",
                "the freeze is past its lifetime and no longer protects or serves its content"
                    .to_string(),
            ),
            Self::Limit(message) => (StatusCode::CONFLICT, "E_FREEZE_LIMIT", message),
            Self::Plain(status, message) => (status, "E_FREEZE", message.to_string()),
        };
        let body = serde_json::json!({ "error": code, "message": message });
        (status, Json(body)).into_response()
    }
}

/// The body of `POST /api/v1/freezes`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateBody {
    /// The origin repository.
    pub repository: String,
    /// Only the paths that start with this prefix. Empty is everything.
    #[serde(default)]
    pub prefix: String,
    /// How long the freeze pins its blobs. Without it, until released.
    #[serde(default)]
    pub ttl_secs: Option<i64>,
}

/// The answer of a creation.
#[derive(Debug, Serialize)]
pub struct CreateResponse {
    /// The identity of the freeze (digest of its manifest).
    pub id: String,
    /// How many paths it names.
    pub entries: usize,
    /// How many distinct contents it pins.
    pub pinned_digests: usize,
    /// When it stops pinning, if it does.
    pub expires_at: Option<DateTime<Utc>>,
}

/// The body of `POST /api/v1/freezes/unpin`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnpinBody {
    /// The freeze to release.
    pub id: String,
}

/// The answer of a release.
#[derive(Debug, Serialize)]
pub struct UnpinResponse {
    /// The freeze released.
    pub id: String,
    /// How many holds were removed.
    pub released_holds: usize,
}

fn gc_of(state: &AppState) -> Result<&std::sync::Arc<crate::gc::GcService>, FreezeFailure> {
    state.gc.as_ref().ok_or_else(|| {
        ResolutionError::NotImplemented(
            "dependency freezes need the garbage collection service: a freeze that cannot pin is not offered",
        )
        .into()
    })
}

fn audit(
    state: &AppState,
    context: &VerifiedContext,
    action: &str,
    resource: &str,
    result: AuditResult,
    correlation: &str,
) {
    record_audit(
        state,
        context.principal().subject().as_str(),
        context.account().as_str(),
        action,
        resource,
        result,
        correlation,
    );
}

/// Cedar decision of `action` on the origin repository; the denial is audited.
fn gate(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    action: hodei_authz::ActionId,
    correlation: &str,
) -> Result<(), FreezeFailure> {
    match authorize_repository(
        &state.authorize_resource,
        context,
        action.clone(),
        repository,
    ) {
        Ok(_) => Ok(()),
        Err(error) => {
            audit(
                state,
                context,
                &action.to_string(),
                &repository.repo_key,
                if matches!(error, ResolutionError::Forbidden) {
                    AuditResult::Denied
                } else {
                    AuditResult::Failed
                },
                correlation,
            );
            Err(error.into())
        }
    }
}

/// A freeze loaded from the store and proven to be a freeze.
struct Loaded {
    manifest: FreezeManifest,
    active: bool,
}

/// The registry hold of a freeze of the caller's account, by a point read.
///
/// The account is checked **first**, from the registry hold alone: nothing
/// about a freeze of another account (expired, malformed, unreadable) is
/// distinguishable from its absence.
async fn registered(
    state: &AppState,
    context: &VerifiedContext,
    id: &str,
) -> Result<hodei_catalog::ContentHold, FreezeFailure> {
    if !is_well_formed_id(id) {
        return Err(ResolutionError::NotFound.into());
    }
    let hold = gc_of(state)?
        .hold(&manifest_hold_id(id))
        .await
        .map_err(|_| ResolutionError::Unavailable)?
        .ok_or(ResolutionError::NotFound)?;
    if registry_account(&hold.reason) != Some(context.account().as_str()) {
        return Err(ResolutionError::NotFound.into());
    }
    Ok(hold)
}

/// Loads a freeze of the caller's account. It exists only if its registry
/// hold does, the manifest bytes digest to the id, and the account matches.
async fn load(
    state: &AppState,
    context: &VerifiedContext,
    id: &str,
) -> Result<Loaded, FreezeFailure> {
    let hold = registered(state, context, id).await?;
    let store = the_content_store(state)?;
    let bytes = match store.get(&ContentDigest::sha256(id)).await {
        Ok(bytes) => bytes,
        // A lapsed freeze lets its manifest be reclaimed like any other
        // unreferenced blob: that is the end of its life, not an outage.
        Err(hodei_hub::blob::BlobError::NotFound) if !hold.is_active(Utc::now()) => {
            return Err(FreezeFailure::Expired);
        }
        Err(_) => return Err(ResolutionError::Unavailable.into()),
    };
    let manifest = FreezeManifest::from_verified_bytes(id, &bytes).map_err(|_| {
        tracing::error!(
            freeze = id,
            "a registered freeze has a manifest that does not verify"
        );
        ResolutionError::Internal
    })?;
    Ok(Loaded {
        active: hold.is_active(Utc::now()) && manifest.is_active(Utc::now()),
        manifest,
    })
}

async fn the_origin(
    state: &AppState,
    context: &VerifiedContext,
    repository: &str,
) -> Result<RepositoryDescriptor, FreezeFailure> {
    let descriptor = the_repository_in(state, context, repository, &FROZEN_FORMATS).await?;
    if descriptor.mode != RepositoryMode::Hosted {
        return Err(ResolutionError::NotImplemented(
            "only hosted repositories can be frozen in this release",
        )
        .into());
    }
    Ok(descriptor)
}

/// `POST /api/v1/freezes`
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request<Body>,
) -> Result<Response, FreezeFailure> {
    let context = the_context(&state, &headers).await?;
    let correlation = correlation_id(&headers, &state.identity);
    let bytes = axum::body::to_bytes(request.into_body(), 16 * 1024)
        .await
        .map_err(|_| ResolutionError::BadRequest("the body is too large"))?;
    let body: CreateBody = serde_json::from_slice(&bytes)
        .map_err(|_| ResolutionError::BadRequest("the body is not a valid freeze request"))?;
    let gc = gc_of(&state)?.clone();
    let origin = the_origin(&state, &context, &body.repository).await?;
    let action = crate::actions::manage_dependency_freeze();
    gate(&state, &context, &origin, action.clone(), &correlation)?;

    let account = context.account().as_str().to_string();
    let actor = context.principal().subject().as_str().to_string();
    // The bound is read once per creation (a rare, Cedar-gated operation);
    // reads of a freeze never scan the holds.
    let active = gc
        .holds()
        .await
        .map_err(|_| ResolutionError::Unavailable)?
        .iter()
        .filter(|hold| {
            is_registry_hold(&hold.id)
                && hold.is_active(Utc::now())
                && registry_account(&hold.reason) == Some(account.as_str())
        })
        .count();
    if active >= MAX_FREEZES_PER_ACCOUNT {
        return Err(FreezeFailure::Limit(format!(
            "the account already holds {MAX_FREEZES_PER_ACCOUNT} active freezes: release one first"
        )));
    }
    let (reader, _) = the_catalog(&state)?;
    let records = reader
        .assets_under(&account, &body.repository, &body.prefix)
        .await
        .map_err(|error| match error {
            CatalogError::Missing { .. } => ResolutionError::NotFound,
            _ => ResolutionError::Unavailable,
        })?;
    // A freeze names only what the origin channel serves now: a digest the
    // trusted channel does not admit is not frozen into a "trusted" view. One
    // block query to the ledger, not one per record.
    let admitted = match crate::trust::servable(&state, &account, &origin)
        .await
        .map_err(crate::resolution::the_admission_error)?
    {
        None => records,
        Some(granted) => records
            .into_iter()
            .filter(|record| granted.contains(&record.digest.value))
            .collect(),
    };
    let manifest = FreezeManifest::build(
        &account,
        &body.repository,
        &body.prefix,
        &actor,
        Utc::now(),
        body.ttl_secs.map(Duration::seconds),
        &admitted,
    )
    .map_err(|error| match error {
        FreezeError::Empty => FreezeFailure::Plain(
            StatusCode::UNPROCESSABLE_ENTITY,
            "the repository has nothing under that prefix to freeze",
        ),
        FreezeError::BadTtl => {
            ResolutionError::BadRequest("ttl_secs is not a usable lifetime").into()
        }
        FreezeError::TooManyEntries => FreezeFailure::Plain(
            StatusCode::PAYLOAD_TOO_LARGE,
            "the freeze would name too many paths: freeze a narrower prefix",
        ),
        FreezeError::UnsupportedDigest { .. } => FreezeFailure::Plain(
            StatusCode::UNPROCESSABLE_ENTITY,
            "the repository holds content that is not sha256 and cannot be frozen",
        ),
        FreezeError::Malformed => FreezeFailure::from(ResolutionError::Internal),
    })?;

    let store = the_content_store(&state)?;
    let id = manifest.id();
    store
        .put(&manifest.bytes())
        .await
        .map_err(|_| ResolutionError::Unavailable)?;
    // Pin first, then verify: any blob the collector could have reclaimed
    // before the pin landed is found missing here, and the freeze is refused
    // instead of promising bytes it no longer has.
    let result = pin_and_verify(&gc, store.as_ref(), &manifest, &id).await;
    if let Err(failure) = result {
        audit(
            &state,
            &context,
            &action.to_string(),
            &id,
            AuditResult::Failed,
            &correlation,
        );
        return Err(failure);
    }
    audit(
        &state,
        &context,
        &action.to_string(),
        &id,
        AuditResult::Allowed,
        &correlation,
    );
    let response = CreateResponse {
        id,
        entries: manifest.entries.len(),
        pinned_digests: manifest.digests().len(),
        expires_at: manifest.expires_at,
    };
    Ok((StatusCode::CREATED, Json(response)).into_response())
}

/// Places the pins, verifies the bytes are there, and only then places the
/// registry hold: a freeze exists only when every pin does. Any failure after
/// the first hold rolls everything of this freeze back (the registry last).
async fn pin_and_verify(
    gc: &crate::gc::GcService,
    store: &dyn hodei_hub::BlobStore,
    manifest: &FreezeManifest,
    id: &str,
) -> Result<(), FreezeFailure> {
    let outcome = place_and_check(gc, store, manifest).await;
    if outcome.is_err() {
        // A rollback that fails is logged, not swallowed: the leftover holds
        // carry the freeze prefix and an administrator can release them.
        if let Err(error) = gc
            .release_holds_with_prefix(&hold_prefix(id), &manifest_hold_id(id))
            .await
        {
            tracing::error!(freeze = id, %error, "the rollback of a failed freeze did not complete");
        }
    }
    outcome
}

async fn place_and_check(
    gc: &crate::gc::GcService,
    store: &dyn hodei_hub::BlobStore,
    manifest: &FreezeManifest,
) -> Result<(), FreezeFailure> {
    let unavailable = |_: crate::gc::GcError| FreezeFailure::from(ResolutionError::Unavailable);
    let holds = manifest.holds();
    let (registry, pins) = holds
        .split_last()
        .ok_or(FreezeFailure::from(ResolutionError::Internal))?;
    gc.place_holds(pins).await.map_err(unavailable)?;
    // Pin first, then verify: a blob the collector reclaimed before the pin
    // landed is found missing here, and the freeze is refused instead of
    // promising bytes it no longer has.
    for digest in manifest.digests() {
        let hex = digest
            .strip_prefix("sha256:")
            .ok_or(FreezeFailure::from(ResolutionError::Internal))?;
        let present = store
            .exists(&ContentDigest::sha256(hex))
            .await
            .map_err(|_| FreezeFailure::from(ResolutionError::Unavailable))?;
        if !present {
            return Err(FreezeFailure::Missing(format!(
                "{digest} is named by the repository but is not in the content store"
            )));
        }
    }
    gc.place_holds(std::slice::from_ref(registry))
        .await
        .map_err(unavailable)?;
    let manifest_present = store
        .exists(&ContentDigest::sha256(manifest.id()))
        .await
        .map_err(|_| FreezeFailure::from(ResolutionError::Unavailable))?;
    if !manifest_present {
        return Err(FreezeFailure::Missing(
            "the manifest blob was reclaimed while the freeze was being made".to_string(),
        ));
    }
    Ok(())
}

/// `GET /api/v1/freezes/{id}`
pub async fn manifest(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, FreezeFailure> {
    let (context, loaded) = reader_gate(&state, &headers, &id).await?;
    let _ = context;
    Ok((StatusCode::OK, Json(loaded.manifest)).into_response())
}

/// `GET /api/v1/freezes/{id}/content/{*path}`
pub async fn content(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, path)): Path<(String, String)>,
) -> Result<Response, FreezeFailure> {
    let (context, loaded) = reader_gate(&state, &headers, &id).await?;
    let manifest = loaded.manifest;
    let Some(entry) = manifest.entry(&path) else {
        return Err(FreezeFailure::Missing(format!(
            "{path} is not in freeze {id}"
        )));
    };
    // Permission is the present tense: the freeze keeps the identity, the
    // origin channel decides today whether it may be served.
    let hex = entry
        .digest
        .strip_prefix("sha256:")
        .ok_or(ResolutionError::Internal)?;
    let digest = ContentDigest::sha256(hex);
    let origin = the_origin(&state, &context, &manifest.repository).await?;
    match crate::trust::admit(&state, &manifest.account, &origin, &digest).await {
        Ok(()) => {}
        Err(crate::trust::Admission::Hidden) => {
            return Err(FreezeFailure::Withheld(format!(
                "{path} is withheld: its digest is no longer admitted by the trusted channel of {}",
                manifest.repository
            )));
        }
        Err(other) => return Err(crate::resolution::the_admission_error(other).into()),
    }
    let store = the_content_store(&state)?;
    let bytes = match store.get(&digest).await {
        Ok(bytes) if bytes.len() as u64 == entry.size_bytes => bytes,
        Ok(_) | Err(hodei_hub::blob::BlobError::NotFound) => {
            return Err(FreezeFailure::Missing(format!(
                "the content {} of {path} is not in the store with the declared size",
                entry.digest
            )));
        }
        Err(error) => return Err(crate::resolution::the_blob_error(&error).into()),
    };
    let mut response = (StatusCode::OK, Body::from(bytes)).into_response();
    let map = response.headers_mut();
    map.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    for (name, value) in [
        ("x-hodei-freeze", id.as_str()),
        ("x-hodei-digest", entry.digest.as_str()),
    ] {
        if let Ok(value) = HeaderValue::from_str(value) {
            map.insert(name, value);
        }
    }
    if let Ok(value) = HeaderValue::from_str(&format!("\"{}\"", entry.digest)) {
        map.insert(header::ETAG, value);
    }
    Ok(response)
}

/// Authenticate, load an active freeze of the caller and ask Cedar for
/// `DownloadAsset` on its origin repository.
async fn reader_gate(
    state: &AppState,
    headers: &HeaderMap,
    id: &str,
) -> Result<(VerifiedContext, Loaded), FreezeFailure> {
    let context = the_context(state, headers).await?;
    let correlation = correlation_id(headers, &state.identity);
    let loaded = load(state, &context, id).await?;
    let origin = the_origin(state, &context, &loaded.manifest.repository).await?;
    gate(
        state,
        &context,
        &origin,
        crate::actions::download_asset(),
        &correlation,
    )?;
    if !loaded.active {
        return Err(FreezeFailure::Expired);
    }
    Ok((context, loaded))
}

/// `POST /api/v1/freezes/unpin`
///
/// The release does not depend on the origin still being there. The caller's
/// account must own the freeze (404 otherwise). Then:
///
/// - if the manifest is readable and its repository still exists, Cedar
///   decides `ManageDependencyFreeze` on that repository;
/// - if the repository is gone or the manifest cannot be read, the most
///   conservative rule applies: only the principal who created the freeze may
///   release it here. Anyone else needs the administrative hold release
///   (`/api/v1/gc/holds/release`), which is its own gated resource.
pub async fn unpin(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request<Body>,
) -> Result<Response, FreezeFailure> {
    let context = the_context(&state, &headers).await?;
    let correlation = correlation_id(&headers, &state.identity);
    let bytes = axum::body::to_bytes(request.into_body(), 4 * 1024)
        .await
        .map_err(|_| ResolutionError::BadRequest("the body is too large"))?;
    let body: UnpinBody = serde_json::from_slice(&bytes)
        .map_err(|_| ResolutionError::BadRequest("the body is not a valid unpin request"))?;
    let hold = registered(&state, &context, &body.id).await?;
    let action = crate::actions::manage_dependency_freeze();

    let origin = match load(&state, &context, &body.id).await {
        Ok(loaded) => the_origin_if_any(&state, &context, &loaded.manifest.repository).await,
        Err(_) => None,
    };
    match origin {
        Some(origin) => gate(&state, &context, &origin, action.clone(), &correlation)?,
        None => {
            if hold.actor != context.principal().subject().as_str() {
                audit(
                    &state,
                    &context,
                    &action.to_string(),
                    &body.id,
                    AuditResult::Denied,
                    &correlation,
                );
                return Err(ResolutionError::Forbidden.into());
            }
        }
    }
    let id = body.id.clone();
    let released = gc_of(&state)?
        .release_holds_with_prefix(&hold_prefix(&id), &manifest_hold_id(&id))
        .await
        .map_err(|_| ResolutionError::Unavailable)?;
    audit(
        &state,
        &context,
        &action.to_string(),
        &body.id,
        AuditResult::Allowed,
        &correlation,
    );
    Ok((
        StatusCode::OK,
        Json(UnpinResponse {
            id: body.id,
            released_holds: released,
        }),
    )
        .into_response())
}

/// The origin repository if it still exists (any format or mode: releasing
/// does not care what it has become), `None` if it is gone or unreadable.
async fn the_origin_if_any(
    state: &AppState,
    context: &VerifiedContext,
    repository: &str,
) -> Option<RepositoryDescriptor> {
    let (reader, _) = the_catalog(state).ok()?;
    reader
        .repository(context.account().as_str(), repository)
        .await
        .ok()
}
