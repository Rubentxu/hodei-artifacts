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
    CODE_MISSING_DEPENDENCY, FreezeError, FreezeManifest, hold_prefix, is_well_formed_id,
    manifest_hold_id,
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

/// Loads a freeze of the caller's account. It exists only if its registry
/// hold does, the manifest bytes digest to the id, and the account matches.
async fn load(
    state: &AppState,
    context: &VerifiedContext,
    id: &str,
) -> Result<Loaded, FreezeFailure> {
    if !is_well_formed_id(id) {
        return Err(ResolutionError::NotFound.into());
    }
    let gc = gc_of(state)?;
    let registry = manifest_hold_id(id);
    let holds = gc.holds().await.map_err(|_| ResolutionError::Unavailable)?;
    let Some(hold) = holds.iter().find(|hold| hold.id == registry) else {
        return Err(ResolutionError::NotFound.into());
    };
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
    if manifest.account != context.account().as_str() {
        // A freeze of another account does not exist for this caller.
        return Err(ResolutionError::NotFound.into());
    }
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
    let (reader, _) = the_catalog(&state)?;
    let records = reader
        .assets_under(&account, &body.repository, &body.prefix)
        .await
        .map_err(|error| match error {
            CatalogError::Missing { .. } => ResolutionError::NotFound,
            _ => ResolutionError::Unavailable,
        })?;
    // A freeze names only what the origin channel serves now: a digest the
    // trusted channel does not admit is not frozen into a "trusted" view.
    let mut admitted = Vec::with_capacity(records.len());
    for record in records {
        match crate::trust::admit(&state, &account, &origin, &record.digest).await {
            Ok(()) => admitted.push(record),
            Err(crate::trust::Admission::Hidden) => {}
            Err(other) => return Err(crate::resolution::the_admission_error(other).into()),
        }
    }
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

async fn pin_and_verify(
    gc: &crate::gc::GcService,
    store: &dyn hodei_hub::BlobStore,
    manifest: &FreezeManifest,
    id: &str,
) -> Result<(), FreezeFailure> {
    let unavailable = |_| FreezeFailure::from(ResolutionError::Unavailable);
    gc.place_holds(&manifest.holds())
        .await
        .map_err(unavailable)?;
    for digest in manifest.digests() {
        let Some(hex) = digest.strip_prefix("sha256:") else {
            continue;
        };
        let present = store
            .exists(&ContentDigest::sha256(hex))
            .await
            .map_err(|_| FreezeFailure::from(ResolutionError::Unavailable))?;
        if !present {
            // Do not leave a half-made pin behind.
            let _ = gc.release_holds_with_prefix(&hold_prefix(id)).await;
            return Err(FreezeFailure::Missing(format!(
                "{digest} is named by the repository but is not in the content store"
            )));
        }
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
    // An expired freeze can still be released; only a missing one is 404.
    let loaded = load(&state, &context, &body.id).await?;
    let origin = the_origin(&state, &context, &loaded.manifest.repository).await?;
    let action = crate::actions::manage_dependency_freeze();
    gate(&state, &context, &origin, action.clone(), &correlation)?;
    let released = gc_of(&state)?
        .release_holds_with_prefix(&hold_prefix(&body.id))
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
