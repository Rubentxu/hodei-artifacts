//! The **hf** surface: the read dialect of the Hugging Face Hub
//! (`hf download` of `huggingface_hub` 1.x, which speaks httpx) over a
//! **hosted** Hodei repository.
//!
//! Lives at `/hf/{repository}/...` and speaks the dialect fixed by the
//! client, verified in the HSP-169 spike against huggingface.co and
//! against a local fixture (FMT-07 of `uat/UAT-FORMATS.md`): the route table,
//! the JSON revision document and the resolve headers are
//! those served by `hodei_catalog::hf` (pure), and here there are only effects.
//!
//! The client flow is that of the classic hub:
//!
//! 1. `GET /api/models/{repo_id}/revision/{rev}`: the revision
//!    document; the document's `sha` is the commit the client later uses
//!    in the resolves.
//! 2. `HEAD /{repo_id}/resolve/{rev}/{rfilename}`: a `302` with
//!    `X-Repo-Commit`, `X-Linked-Etag` (the sha256), `X-Linked-Size` and a
//!    **relative** `Location` (a path, without scheme or host) to the
//!    blob. The client only follows relative redirects.
//! 3. `HEAD`/`GET {Location}`: the metadata is read from the final response and
//!    the bytes are **streamed**, in CAS chunks, without a full buffer
//!    in memory: a model's weights are the typical large
//!    artifact.
//!
//! The blob's identity is its sha256: it is the etag the client uses to
//! name its cache and the digest the CAS guarantees when serving. The
//! classic client ONLY verifies the size --it does not check the sha256 declared in
//! `X-Linked-Etag`--: integrity is guaranteed by the server's CAS
//! (connection with HSP-105), which does not deliver bytes that are not
//! its digest.
//!
//! The server does not announce `X-Xet-*`: the client falls back to the classic
//! HTTP route, the dialect this release serves (a decision from the spike
//! fixture, documented). Writing is not declared: the classic upload and
//! xet are out of scope, and content enters through the hub's seeding.
//!
//! Authentication is the hub's, mapped to the dialect: the client carries
//! its token as `Authorization: Bearer <key>`, and the hub verifies the
//! credential as the principal's bearer. Per-account scoping is the
//! hub's, not the format's.

use axum::{
    body::{Body, Bytes},
    extract::{OriginalUri, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use hodei_authz::{Credentials, VerifiedContext, verify_principal};
use hodei_catalog::{
    AssetRecord, RepositoryDescriptor, RepositoryFormat, RepositoryMode,
    hf::{self, BlobMeta, ModelRef, Route},
};
use hodei_evidence::domain::content::ContentDigest;
use hodei_hub::BlobStore;
use serde_json::{Value, json};
use std::sync::Arc;

use super::raw::{
    RawApiError, Resolved, apply_cache_headers, apply_common_headers, the_admission_error,
    the_catalog, the_content_store, the_decision, the_publication_error, the_repository_in,
    the_resolved_asset,
};
use crate::app_state::AppState;
use crate::handlers::{bearer_token, correlation_id};

/// The chunk the CAS delivers per read: the same size with which
/// the spike fixture served its 256 MiB, and the one that keeps the server's
/// RSS bounded while the client writes to disk.
const THE_CHUNK: u64 = 64 * 1024;

/// What happens to a request that cannot be served.
#[derive(Debug)]
pub enum Failure {
    /// An error from the shared surface.
    Raw(RawApiError),
    /// A status and a reason.
    Plain(StatusCode, String),
}

impl From<RawApiError> for Failure {
    fn from(error: RawApiError) -> Self {
        Self::Raw(error)
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Raw(error) => error.the_verdict(),
            Self::Plain(status, message) => (status, message),
        };
        // The hub dialect carries its errors as plain JSON: the CLI
        // prints the status and the reason, and there is no protocol shape
        // that the client would turn into anything else.
        let body = json!({ "error": message });
        let mut response = (status, axum::Json(body)).into_response();
        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
    }
}

impl Failure {
    /// An error with a status and a reason.
    fn the_failure(status: StatusCode, detail: &str) -> Self {
        Self::Plain(status, detail.to_string())
    }

    /// What is not there does not say it once was: a `404` with no explanation of whether it
    /// existed elsewhere.
    fn the_absence() -> Self {
        Self::Raw(RawApiError::NotFound)
    }
}

/// The rest of the path of `/hf/{repository}/{rest}`, undecoded: the
/// protocol grammar rules are the only ones that may look at
/// a `%2f`.
fn the_split(uri: &OriginalUri) -> Result<(String, String), Failure> {
    let rest = uri
        .path()
        .strip_prefix("/hf/")
        .ok_or_else(Failure::the_absence)?;
    let (repository, rest) = rest.split_once('/').ok_or_else(Failure::the_absence)?;
    if !crate::raw::path::valid_repository_name(repository) || rest.is_empty() {
        return Err(Failure::the_absence());
    }
    Ok((repository.to_string(), rest.to_string()))
}

/// Any request to `/hf/{repository}/...`.
pub async fn handle(
    State(state): State<AppState>,
    method: Method,
    uri: OriginalUri,
    headers: HeaderMap,
) -> Result<Response, Failure> {
    let correlation = correlation_id(&headers, &state.identity);
    let (repository_key, rest) = the_split(&uri)?;
    let Some(route) = hf::parse(&rest) else {
        return Err(Failure::the_absence());
    };

    let context = the_context(&state, &headers).await?;
    let repository =
        the_repository_in(&state, &context, &repository_key, &[RepositoryFormat::Hf]).await?;
    // The mode served is hosted: a proxy of the real hub is another
    // dialect (the upstream announces xet) that this release has not
    // verified, and it is not declared.
    if repository.mode != RepositoryMode::Hosted {
        return Err(Failure::the_absence());
    }
    let reading = method == Method::GET || method == Method::HEAD;
    if !reading {
        return Err(Failure::the_failure(
            StatusCode::METHOD_NOT_ALLOWED,
            "this hf operation is not supported",
        ));
    }
    the_decision(
        &state,
        &context,
        crate::actions::download_asset(),
        &repository,
    )?;
    read(
        &state,
        &context,
        &repository,
        &repository_key,
        route,
        method == Method::HEAD,
        &correlation,
    )
    .await
}

/// The verified principal of the client's credentials.
///
/// The CLI carries the token as `Authorization: Bearer <key>`: the hub
/// verifies the credential as the principal's bearer, and per-account
/// scoping is whatever the token carries. A credential the hub cannot
/// verify is not an identity.
async fn the_context(state: &AppState, headers: &HeaderMap) -> Result<VerifiedContext, Failure> {
    let token = bearer_token(headers).ok_or(Failure::the_failure(
        StatusCode::UNAUTHORIZED,
        "the hf surface needs an API key as a Bearer credential",
    ))?;
    match verify_principal(state.authenticator.as_ref(), &Credentials::bearer(&token)).await {
        Ok(principal) => Ok(VerifiedContext::from_principal(principal)),
        Err(hodei_authz::AuthenticationError::ProviderUnavailable(_)) => {
            Err(RawApiError::Unavailable.into())
        }
        Err(_) => Err(Failure::the_failure(
            StatusCode::UNAUTHORIZED,
            "the credential is not one this hub can verify",
        )),
    }
}

fn json_response(status: StatusCode, value: &Value, correlation: &str) -> Response {
    let mut response = (status, axum::Json(value.clone())).into_response();
    apply_common_headers(&mut response, "\"json\"", correlation);
    response
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

/// A read of the surface: the revision document, the `302` of the
/// resolve and the blob bytes.
#[allow(clippy::too_many_arguments)]
async fn read(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    repository_key: &str,
    route: Route,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    match route {
        Route::ApiRevision(model, rev) => {
            let snapshot = the_snapshot(state, context, repository, &model).await?;
            // A revision that is neither the `main` branch nor the snapshot's
            // commit is a revision the hub never got to serve: the
            // catalog keeps the current state, not a branch history.
            if rev != "main" && rev != snapshot.commit {
                return Err(Failure::the_absence());
            }
            let document = hf::revision_document(
                &model,
                &snapshot.commit,
                snapshot.last_modified,
                &snapshot.the_filenames(),
            );
            Ok(json_response(StatusCode::OK, &document, correlation))
        }
        Route::Resolve(model, rev, filename) => {
            let snapshot = the_snapshot(state, context, repository, &model).await?;
            if rev != "main" && rev != snapshot.commit {
                return Err(Failure::the_absence());
            }
            let Some(record) = snapshot.the_record_of(&filename) else {
                return Err(Failure::the_absence());
            };
            let meta = BlobMeta {
                commit: snapshot.commit.clone(),
                sha256: record.digest.value.clone(),
                size: record.size_bytes,
            };
            if head {
                Ok(the_redirect(
                    repository_key,
                    &model,
                    &meta,
                    &filename,
                    correlation,
                ))
            } else {
                // A resolve `GET` delivers the same bytes as the
                // `Location`: the redirect is a way of naming the
                // blob, not a way of changing what is served.
                let asset_path = model.asset_path(&filename);
                the_blob(
                    state,
                    context,
                    repository,
                    &asset_path,
                    &meta,
                    false,
                    correlation,
                )
                .await
            }
        }
        Route::Blob(model, digest, filename) => {
            // The path's digest names the content: a URL that
            // nobody built with the sha256 of what is there is a URL that
            // is not served.
            let snapshot = the_snapshot(state, context, repository, &model).await?;
            let Some(record) = snapshot.the_record_of(&filename) else {
                return Err(Failure::the_absence());
            };
            if record.digest.value != digest {
                return Err(Failure::the_absence());
            }
            let meta = BlobMeta {
                commit: snapshot.commit.clone(),
                sha256: record.digest.value.clone(),
                size: record.size_bytes,
            };
            let asset_path = model.asset_path(&filename);
            the_blob(
                state,
                context,
                repository,
                &asset_path,
                &meta,
                head,
                correlation,
            )
            .await
        }
    }
}

/// The snapshot of a model: the files the catalog holds today.
///
/// The commit is the digest of the listing that forms it: two snapshots with the
/// same content are the same commit, and the commit only changes when
/// the content it declares changes.
struct Snapshot {
    /// The snapshot's model.
    model: ModelRef,
    /// The snapshot's commit.
    commit: String,
    /// The moment the content last moved.
    last_modified: DateTime<Utc>,
    /// The model's catalog assets, exactly as they arrived.
    records: Vec<AssetRecord>,
}

impl Snapshot {
    /// The asset of a snapshot file, if present.
    fn the_record_of(&self, filename: &str) -> Option<&AssetRecord> {
        let prefix = format!("{}/", self.model.asset_prefix());
        self.records
            .iter()
            .find(|record| record.asset_path.strip_prefix(&prefix) == Some(filename))
    }

    /// The `rfilename`s of the snapshot, in the deterministic order of the
    /// listing.
    fn the_filenames(&self) -> Vec<String> {
        let prefix = format!("{}/", self.model.asset_prefix());
        let mut names: Vec<String> = self
            .records
            .iter()
            .filter_map(|record| record.asset_path.strip_prefix(&prefix).map(str::to_string))
            .collect();
        names.sort();
        names
    }
}

/// The snapshot of a model, read from the catalog.
async fn the_snapshot(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    model: &ModelRef,
) -> Result<Snapshot, Failure> {
    let records = assets_under(state, context, repository, &model.asset_prefix()).await?;
    if records.is_empty() {
        return Err(Failure::the_absence());
    }
    let entries: Vec<(&str, &str)> = records
        .iter()
        .filter_map(|record| {
            let prefix = format!("{}/", model.asset_prefix());
            let filename = record.asset_path.strip_prefix(&prefix)?;
            Some((filename, record.digest.value.as_str()))
        })
        .collect();
    let commit = hf::snapshot_commit(&entries);
    let last_modified = records
        .iter()
        .map(|record| record.updated_at)
        .max()
        .unwrap_or_else(Utc::now);
    Ok(Snapshot {
        model: model.clone(),
        commit,
        last_modified,
        records,
    })
}

/// The assets of a model, in the principal's account.
async fn assets_under(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    prefix: &str,
) -> Result<Vec<AssetRecord>, Failure> {
    let (reader, _) = the_catalog(state)?;
    // A scan stops at the segment boundary: `c` must not match `c2`.
    let prefix = if prefix.ends_with('/') {
        prefix.to_string()
    } else {
        format!("{prefix}/")
    };
    let records = reader
        .assets_under(context.account().as_str(), &repository.repo_key, &prefix)
        .await
        .map_err(|error| Failure::from(the_publication_error(&error)))?;
    // A listing exposes digests and existence: what the channel does not
    // admit is not in it.
    super::raw::the_admitted(state, context.account().as_str(), repository, records)
        .await
        .map_err(Failure::from)
}

/// The resolve `302`: the metadata travels in headers and the `Location`
/// is a relative path to the blob. There is no body: the client only
/// reads headers here.
fn the_redirect(
    repository_key: &str,
    model: &ModelRef,
    meta: &BlobMeta,
    filename: &str,
    correlation: &str,
) -> Response {
    let location =
        hf::the_download_location(&format!("/hf/{repository_key}"), model, meta, filename);
    let mut response = (StatusCode::FOUND, Body::empty()).into_response();
    for (name, value) in meta.the_linked_headers() {
        if let Ok(value) = HeaderValue::from_str(&value) {
            response
                .headers_mut()
                .insert(header::HeaderName::from_static(name), value);
        }
    }
    if let Ok(value) = HeaderValue::from_str(&location) {
        response.headers_mut().insert(header::LOCATION, value);
    }
    if let Ok(value) = HeaderValue::from_str("0") {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    apply_common_headers(&mut response, &meta.sha256, correlation);
    response
}

/// The blob bytes, served from the CAS.
///
/// The content is **streamed**: the CAS is read in 64 KiB chunks and
/// each chunk goes out as is, without a full buffer in memory. The declared
/// length is that of the catalog asset, the same one the
/// `Content-Length` announces to the client; a chunk that does not arrive is a
/// transport error, not short content. A `HEAD` answers the final
/// redirect metadata (the one the client reads) without the bytes.
async fn the_blob(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    asset_path: &str,
    meta: &BlobMeta,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    let resolved =
        the_resolved_asset(state, context, repository.clone(), asset_path, correlation).await?;
    let Resolved {
        asset,
        served,
        resolved_by: _,
        repository: backing,
    } = resolved;
    // The content served is the one the URL names: the asset's etag and
    // the path's sha256 are the same digest, and if they are not,
    // the URL was for other content that is no longer there.
    if asset.digest.value != meta.sha256 {
        return Err(Failure::the_absence());
    }
    crate::trust::admit(state, context.account().as_str(), &backing, &asset.digest)
        .await
        .map_err(the_admission_error)?;
    let etag = crate::raw::conditional::the_etag(&asset.digest);
    let length = asset.size_bytes;
    let body = if head {
        Body::empty()
    } else {
        let store = Arc::clone(the_content_store(state)?);
        let digest = asset.digest.clone();
        the_streamed_body(store, digest, length)
    };
    let mut response = (StatusCode::OK, body).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response.headers_mut().insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    // The dialect metadata: the final `HEAD` of the redirect is where the
    // client reads it, and the `GET` repeats it out of the same honesty.
    for (name, value) in meta.the_linked_headers() {
        if let Ok(value) = HeaderValue::from_str(&value) {
            response
                .headers_mut()
                .insert(header::HeaderName::from_static(name), value);
        }
    }
    apply_cache_headers(&mut response, &served);
    apply_common_headers(&mut response, &etag, correlation);
    // The dialect's `ETag` is the quoted sha256, the form of the classic
    // hub (that of the spike fixture): the hub's conditional etag
    // travels in the `X-Linked-Etag` that is already set.
    if let Ok(value) = HeaderValue::from_str(&format!("\"{}\"", meta.sha256)) {
        response.headers_mut().insert(header::ETAG, value);
    }
    if let Ok(value) = HeaderValue::from_str(&length.to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    Ok(response)
}

/// The blob bytes as a streaming body: CAS chunks that
/// go out as they arrive.
fn the_streamed_body(store: Arc<dyn BlobStore>, digest: ContentDigest, size: u64) -> Body {
    let stream = futures_util::stream::try_unfold(
        (store, digest, size, 0u64),
        |(store, digest, size, offset)| async move {
            if offset >= size {
                return Ok::<
                    Option<(Bytes, (Arc<dyn BlobStore>, ContentDigest, u64, u64))>,
                    std::io::Error,
                >(None);
            }
            let length = THE_CHUNK.min(size - offset);
            let bytes = store
                .read_range(&digest, offset, length)
                .await
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            Ok(Some((
                Bytes::from(bytes),
                (store, digest, size, offset + length),
            )))
        },
    );
    Body::from_stream(stream)
}
