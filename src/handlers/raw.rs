//! The **raw** content surface: publish, download and describe.
//!
//! A raw repository stores bytes at a path. There are no packages, nor
//! versions, nor formats: there is an address and a content.
//!
//! The three requests do the same in the first three steps, and so
//! they do it in the same place:
//!
//! 1. **Authenticate.** The token goes first, always: a request without a
//!    credential is a 401 before looking at the path, the repository or
//!    anything else.
//! 2. **Read the path.** With the rules of [`crate::raw::path`], which
//!    reject traversal, double encoding, control characters
//!    and ambiguous paths.
//! 3. **Look up the repository** in the **verified** account. A
//!    repository of another account does not exist for this account, and it is
//!    answered with the same 404 as a repository that does not exist.
//!
//! And then, the Cedar decision with the corresponding action:
//! `UploadAsset` to publish, `DownloadAsset` to download. Without
//! permission there is no content, and without content it is not said that there is any.

use axum::{
    body::Body,
    extract::{OriginalUri, State},
    http::{HeaderMap, HeaderName, HeaderValue, Request, StatusCode, header},
    response::{IntoResponse, Response},
};
use hodei_authz::VerifiedContext;
use hodei_catalog::{
    AssetDisposition, CatalogError, CatalogWriter, PackageCoordinates, Publication,
    RepositoryDescriptor, RepositoryFormat, RepositoryMode,
};
use hodei_evidence::domain::content::ContentDigest;

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{correlation_id, record_audit};
use crate::raw::{conditional, path, policy, range};

const CONTENT_TYPE: &str = "application/octet-stream";

/// The header where the client declares the content digest.
///
/// It is not mandatory: if absent, the digest is computed over what
/// arrives. If present, it is **checked**: a client that declares a digest
/// and sends something else is broken, and a server that accepts it without
/// checking is lying about what it has.
const CHECKSUM_HEADER: &str = "x-checksum-sha256";

pub use crate::resolution::ResolutionError as RawApiError;
#[allow(unused_imports)]
pub(super) use crate::resolution::{
    ProxyServed, Resolved, apply_cache_headers, apply_common_headers, apply_group_headers, hex_of,
    the_admission_error, the_admitted, the_blob_error, the_catalog, the_content_store, the_context,
    the_current_asset, the_decision, the_proxy_asset, the_proxy_asset_at, the_proxy_asset_verified,
    the_publication_error, the_repository_in, the_resolved_asset, write_the_content,
};

/// The requested path, with its rules.
fn the_target(uri: &OriginalUri) -> Result<path::RawTarget, RawApiError> {
    path::the_target(uri.path()).map_err(|error| RawApiError::BadRequest(error.message()))
}

/// The repository, in the verified account.
///
/// The account comes from the principal: a repository of another account does not
/// exist for this one, and it is answered with the same 404 as one that
/// never existed.
async fn the_repository(
    state: &AppState,
    context: &VerifiedContext,
    repository: &str,
) -> Result<RepositoryDescriptor, RawApiError> {
    the_repository_in(
        state,
        context,
        repository,
        &[RepositoryFormat::Raw, RepositoryFormat::Maven2],
    )
    .await
}

/// `PUT /repository/{repository}/{*path}`
///
/// Publishes the body content at that address.
///
/// The content is written **in chunks** to the content store: a
/// one-gigabyte file does not fit in memory, and a server that
/// keeps it in memory cannot serve a one-gigabyte artifact.
///
/// And the asset and its audit event are written in the **same**
/// catalog transaction: if the event cannot be written, the asset
/// is not published. What does remain is the unreferenced blob, which is
/// reconcilable.
pub async fn publish(
    State(state): State<AppState>,
    uri: OriginalUri,
    headers: HeaderMap,
    request: Request<Body>,
) -> Result<Response, RawApiError> {
    let context = the_context(&state, &headers).await?;
    let target = the_target(&uri)?;
    let repository = the_repository(&state, &context, &target.repository).await?;
    the_decision(
        &state,
        &context,
        crate::actions::upload_asset(),
        &repository,
    )?;
    // What a proxy says comes from its upstream, not from whoever asks. It goes
    // after Cedar: whoever cannot upload receives the same 403 as in
    // any other repository, and does not find out the mode.
    if !repository.mode.accepts_client_writes() {
        return Err(RawApiError::MethodNotAllowed(
            "this repository does not accept uploads",
        ));
    }

    let correlation = correlation_id(&headers, &state.identity);
    // A Maven 2 repository decides here what is stored and what is only validated.
    let maven_file = if repository.format == RepositoryFormat::Maven2 {
        match super::maven::the_write(&repository, &target.asset_path)? {
            super::maven::Write::Absorbed(absorbed) => {
                return super::maven::absorb(
                    &state,
                    &context,
                    &repository,
                    &target.asset_path,
                    absorbed,
                    request.into_body(),
                    &correlation,
                )
                .await;
            }
            super::maven::Write::File { file, replace } => Some((file, replace)),
        }
    } else {
        None
    };
    let current =
        the_current_asset(&state, &context, &target.repository, &target.asset_path).await?;
    let current_etag = current
        .as_ref()
        .map(|asset| conditional::the_etag(&asset.digest));
    let current_digest = current.as_ref().map(|asset| asset.digest.clone());

    // The condition is checked **before** writing the bytes: a
    // request that is not going to be carried out leaves no bytes in the store.
    let outcome = conditional::the_write_condition(
        conditional::the_if_match(&headers).as_deref(),
        conditional::the_if_none_match(&headers).as_deref(),
        current_etag.as_deref(),
    );
    if matches!(outcome, conditional::WriteOutcome::Failed) {
        return Err(RawApiError::PreconditionFailed);
    }

    // If the address publishes once, publishing other content is a
    // conflict. And this is said **before** reading the body when the client
    // has declared the digest: a client that already knows it cannot
    // publish has no reason to send the content.
    // In Maven the replacement is decided by the file's version, not by `raw.replace`.
    let replace = match &maven_file {
        Some((_, replace)) => *replace,
        None => policy::the_replace_policy(&repository.config.config_settings),
    };
    let declared = declared_digest(&headers)?;
    if replace == policy::ReplacePolicy::Deny
        && let Some(current) = current_digest.as_ref()
        && let Some(declared) = declared.as_ref()
        && declared != current
    {
        return Err(RawApiError::Conflict(
            "the path already holds another content",
        ));
    }

    // When the address publishes once, the incoming content must
    // be **the same** as what is already there. If it is not, the write is
    // aborted and not a single byte is left in the store.
    let refuse_replacement = match (replace, current_digest.as_ref()) {
        (policy::ReplacePolicy::Deny, Some(current)) => Some(current),
        _ => None,
    };
    let body = match &maven_file {
        Some((file, _)) => super::maven::the_file_body(file, request.into_body()).await?,
        None => request.into_body(),
    };
    let (digest, length) = write_the_content(
        &state,
        context.account().as_str(),
        body,
        declared,
        refuse_replacement,
    )
    .await?;

    // Admission (HSP-153): a repository that declares it decides the digest
    // **before** the catalog is written. A refusal returns here, so nothing is
    // published; the unreferenced blob is the GC's, never served.
    if let Err(refusal) =
        crate::admission::gate(&state, context.account().as_str(), &repository, &digest).await
    {
        record_audit(
            &state,
            context.principal().subject().as_str(),
            context.account().as_str(),
            "admission.refused",
            &format!("{}/{}", target.repository, digest),
            AuditResult::Denied,
            &correlation,
        );
        return Err(refusal);
    }

    let publication = Publication {
        repository: target.repository.clone(),
        asset_path: target.asset_path.clone(),
        // The version of raw content is the **content**: a
        // published version does not change, and if the content changes, it is another
        // version. The path is the package name, because in a
        // raw repository the path is what names the content.
        coordinates: PackageCoordinates {
            package_name: target.asset_path.clone(),
            package_version: digest.value.clone(),
        },
        digest: digest.clone(),
        size_bytes: length,
        tags: Vec::new(),
        actor: context.principal().subject().as_str().to_string(),
        correlation_id: correlation.clone(),
        // Replacement is decided by the **repository**, not the client: an
        // address publishes once unless the repository's configuration
        // says it can be replaced. What is not rewritten
        // is the previous version, which is still there.
        overwrite: replace == policy::ReplacePolicy::Allow,
        // The client's condition travels to the catalog, which checks it
        // **inside** the transaction that writes. The check
        // above is a courtesy: it says early what is going to happen, but the
        // one that counts is the catalog's, because there is no window there between
        // checking and writing.
        precondition: conditional::the_precondition(
            conditional::the_if_match(&headers).as_deref(),
            conditional::the_if_none_match(&headers).as_deref(),
        ),
        // Raw content hangs from nothing: the `subject` is a
        // relation of OCI manifests, and here there is no manifest.
        subject: None,
    };

    let (_, writer) = the_catalog(&state)?;
    let outcome = writer
        .publish(context.account().as_str(), publication)
        .await
        .map_err(|error| the_publication_error(&error))?;

    let etag = conditional::the_etag(&outcome.reference.digest);
    // The status is stated by the **catalog**, not by a read made before
    // writing: what is there now is the truth, and an earlier read
    // may be stale. The body says the same, and does not contradict
    // itself.
    let (status, created, replaced) = match outcome.disposition {
        AssetDisposition::Created => (StatusCode::CREATED, true, false),
        AssetDisposition::Replaced => (StatusCode::OK, false, true),
        AssetDisposition::Unchanged => (StatusCode::OK, false, false),
    };
    let body = serde_json::json!({
        "repository": target.repository,
        "path": target.asset_path,
        "digest": outcome.reference.digest.to_string(),
        "size_bytes": outcome.reference.size_bytes,
        "etag": etag,
        "created": created,
        "replaced": replaced,
    });
    let mut response = (status, axum::Json(body)).into_response();
    apply_common_headers(&mut response, &etag, &correlation);
    // The `Location` is a **URI reference**: the path that was just
    // published, as it can be written in a URI. The admitted segments
    // are those of RFC 3986, so the reference is valid without
    // encoding anything.
    if let Ok(location) = HeaderValue::from_str(&format!(
        "/repository/{}/{}",
        target.repository, target.asset_path
    )) {
        response.headers_mut().insert(header::LOCATION, location);
    }
    Ok(response)
}

/// What the provenance read asks for.
#[derive(Debug, serde::Deserialize)]
pub struct ProvenanceQuery {
    /// The repository key.
    pub repository: String,
    /// The path of the content.
    pub path: String,
}

/// `GET /api/v1/provenance?repository=&path=`
///
/// The provenance receipts of a proxy path, the most recent
/// first. It is authorized by the same action as reading the content
/// (`DownloadAsset`): whoever can download it can know where it came from.
///
/// The response carries no credentials or queries: the receipt already
/// excludes them when written.
pub async fn provenance(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<ProvenanceQuery>,
    headers: HeaderMap,
) -> Result<Response, RawApiError> {
    let context = the_context(&state, &headers).await?;
    // The path is validated with the same rules as the content's.
    let target = path::the_target(&format!("/repository/{}/{}", query.repository, query.path))
        .map_err(|error| RawApiError::BadRequest(error.message()))?;
    let repository = the_repository(&state, &context, &target.repository).await?;
    the_decision(
        &state,
        &context,
        crate::actions::download_asset(),
        &repository,
    )?;
    let plane = state
        .proxy_plane
        .as_ref()
        .ok_or(RawApiError::NotImplemented(
            "this service keeps no provenance ledger",
        ))?;
    let receipts = plane
        .ledger
        .receipts_of(
            context.account().as_str(),
            &target.repository,
            &target.asset_path,
        )
        .await
        .map_err(|error| the_publication_error(&error))?;
    // A receipt speaks of a digest: if the channel does not let that digest be served,
    // it does not say it exists either. It is the same gate as the read.
    let mut admitted = Vec::with_capacity(receipts.len());
    for receipt in receipts {
        match crate::trust::admit(
            &state,
            context.account().as_str(),
            &repository,
            &receipt.digest,
        )
        .await
        {
            Ok(_) => admitted.push(receipt),
            Err(crate::trust::Admission::Hidden) => {}
            Err(other) => return Err(the_admission_error(other)),
        }
    }
    let receipts: Vec<serde_json::Value> = admitted
        .iter()
        .map(|receipt| {
            serde_json::json!({
                "id": receipt.id,
                "repository": receipt.repository,
                "path": receipt.asset_path,
                "upstream": receipt.upstream_id,
                "source_url": receipt.source_url,
                "config_version": receipt.config_version,
                "digest": receipt.digest.to_string(),
                "size_bytes": receipt.size_bytes,
                "checksum": receipt.checksum,
                "status": receipt.status,
                "redirects": receipt.redirects,
                "etag": receipt.etag,
                "last_modified": receipt.last_modified,
                "outcome": receipt.outcome.as_str(),
                "actor": receipt.actor,
                "correlation_id": receipt.correlation_id,
                "fetched_at": receipt.fetched_at,
            })
        })
        .collect();
    Ok(axum::Json(serde_json::json!({ "receipts": receipts })).into_response())
}

/// `GET /repository/{repository}/{*path}`
///
/// Returns the content at that address.
///
/// Supports `Range` and the `If-Match` and `If-None-Match` conditions, per
/// RFC 9110. And it says it supports ranges with `Accept-Ranges`, which is what
/// a client reads to know whether it can request a chunk.
pub async fn download(
    State(state): State<AppState>,
    uri: OriginalUri,
    headers: HeaderMap,
) -> Result<Response, RawApiError> {
    let (response, _) = the_content(state, uri, headers, false).await?;
    Ok(response)
}

/// `HEAD /repository/{repository}/{*path}`
///
/// The same as `GET`, without the body. The headers are the same, and
/// `Content-Length` is that of the response the `GET` would have given: that
/// of the whole content, or that of the chunk when the request asks for a
/// chunk. A `HEAD` that states one length and then a `GET` states another
/// is a server that does not know what it has.
pub async fn describe(
    State(state): State<AppState>,
    uri: OriginalUri,
    headers: HeaderMap,
) -> Result<Response, RawApiError> {
    let (response, _) = the_content(state, uri, headers, true).await?;
    Ok(response)
}

/// The content of an address, with its conditions and its range.
///
/// The `head` parameter says whether the response carries the body: `HEAD` brings
/// the same headers and no bytes.
async fn the_content(
    state: AppState,
    uri: OriginalUri,
    headers: HeaderMap,
    head: bool,
) -> Result<(Response, String), RawApiError> {
    let context = the_context(&state, &headers).await?;
    let correlation = correlation_id(&headers, &state.identity);
    // The trail says what happened, **also** when what happened is that
    // it could not be served: a log that does not say who tried to read and
    // was rejected is a log with a hole.
    let resource = match the_target(&uri) {
        Ok(target) => format!("{}/{}", target.repository, target.asset_path),
        // A path that cannot be read has no name: the one that
        // arrived is stated, which is what the client sent.
        Err(_) => uri.path().to_string(),
    };
    let outcome = the_content_of(&state, &context, &uri, &headers, head, &correlation).await;
    let result = match &outcome {
        Ok(_) => AuditResult::Allowed,
        Err(RawApiError::Forbidden) => AuditResult::Denied,
        Err(_) => AuditResult::Failed,
    };
    record_audit(
        &state,
        context.principal().subject().as_str(),
        context.account().as_str(),
        "download",
        &resource,
        result,
        &correlation,
    );
    outcome
}

/// The content, once it is known who requests it.
async fn the_content_of(
    state: &AppState,
    context: &VerifiedContext,
    uri: &OriginalUri,
    headers: &HeaderMap,
    head: bool,
    correlation: &str,
) -> Result<(Response, String), RawApiError> {
    let target = the_target(uri)?;
    let repository = the_repository(state, context, &target.repository).await?;
    the_decision(
        state,
        context,
        crate::actions::download_asset(),
        &repository,
    )?;

    let correlation = correlation.to_string();
    // In Maven, metadata and checksums are not stored in a hosted one: they are derived
    // from what is published. A group composes them from what its members contribute
    // (the union of versions, the checksum of the file that wins). A proxy
    // serves those of the upstream as is, like any other content. The
    // rest of the paths follow the normal flow.
    if repository.format == RepositoryFormat::Maven2 {
        let derived = match repository.mode {
            RepositoryMode::Hosted => {
                super::maven::the_derived(
                    state,
                    context,
                    &repository,
                    &target.asset_path,
                    headers,
                    head,
                    &correlation,
                )
                .await?
            }
            RepositoryMode::Group => {
                super::maven::the_group_derived(
                    state,
                    context,
                    &repository,
                    &target.asset_path,
                    headers,
                    head,
                    &correlation,
                )
                .await?
            }
            _ => None,
        };
        if let Some(response) = derived {
            return Ok((response, correlation));
        }
    }
    let Resolved {
        asset,
        served,
        resolved_by,
        repository,
    } = the_resolved_asset(state, context, repository, &target.asset_path, &correlation).await?;

    // The trust gate: Cedar already said this person can
    // read the repository; it remains to say whether this **digest** may be served
    // through its channel. It goes before the read conditions: a 304 or a
    // 412 would say the content exists.
    crate::trust::admit(
        state,
        context.account().as_str(),
        &repository,
        &asset.digest,
    )
    .await
    .map_err(the_admission_error)?;

    let etag = conditional::the_etag(&asset.digest);
    let outcome = conditional::the_read_condition(
        conditional::the_if_match(headers).as_deref(),
        conditional::the_if_none_match(headers).as_deref(),
        Some(&etag),
    );
    match outcome {
        conditional::ReadOutcome::Failed => return Err(RawApiError::PreconditionFailed),
        conditional::ReadOutcome::NotModified => {
            // The client already has this content: 304, with the tag
            // so that it knows which one it has.
            let mut response = StatusCode::NOT_MODIFIED.into_response();
            apply_common_headers(&mut response, &etag, &correlation);
            apply_cache_headers(&mut response, &served);
            apply_group_headers(&mut response, &resolved_by);
            return Ok((response, correlation));
        }
        conditional::ReadOutcome::Serve => {}
    }

    let store = the_content_store(state)?;
    let length = asset.size_bytes;
    let request = match range::the_range(
        headers
            .get(header::RANGE)
            .and_then(|value| value.to_str().ok()),
        length,
    ) {
        Ok(request) => request,
        Err(range::RangeError::Unsatisfiable) => {
            return Err(RawApiError::RangeNotSatisfiable(length));
        }
    };

    // `HEAD` brings no body, but it brings **the same headers** that
    // `GET` would bring: the length it states is that of the response, which
    // with a chunk is the chunk's, and not that of the whole content. A
    // `HEAD` that says `Content-Length: 100` and a `GET` that returns ten
    // bytes are two servers that do not know each other.
    let (body, status, content_range, response_length) = match request {
        range::RangeRequest::Whole => {
            if head {
                (Body::empty(), StatusCode::OK, None, length)
            } else {
                let bytes = store
                    .get(&asset.digest)
                    .await
                    .map_err(|error| the_blob_error(&error))?;
                let served = bytes.len() as u64;
                (Body::from(bytes), StatusCode::OK, None, served)
            }
        }
        range::RangeRequest::Single { first, last } => {
            let served = last - first + 1;
            let content_range = range::the_content_range(first, last, length);
            if head {
                (
                    Body::empty(),
                    StatusCode::PARTIAL_CONTENT,
                    Some(content_range),
                    served,
                )
            } else {
                let bytes = store
                    .read_range(&asset.digest, first, served)
                    .await
                    .map_err(|error| the_blob_error(&error))?;
                let served = bytes.len() as u64;
                (
                    Body::from(bytes),
                    StatusCode::PARTIAL_CONTENT,
                    Some(content_range),
                    served,
                )
            }
        }
    };

    let mut response = (status, body).into_response();
    apply_common_headers(&mut response, &etag, &correlation);
    apply_cache_headers(&mut response, &served);
    apply_group_headers(&mut response, &resolved_by);
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(CONTENT_TYPE));
    response
        .headers_mut()
        .insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    // The content is binary and is not rendered: this is stated, so that the
    // browser does not make it up.
    response.headers_mut().insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    if let Some(content_range) = content_range
        && let Ok(value) = HeaderValue::from_str(&content_range)
    {
        response.headers_mut().insert(header::CONTENT_RANGE, value);
    }
    if head {
        // A HEAD says how much the response brings, and the response brings
        // nothing: what it states is the length the `GET` would have.
        if let Ok(value) = HeaderValue::from_str(&response_length.to_string()) {
            response.headers_mut().insert(header::CONTENT_LENGTH, value);
        }
    }
    Ok((response, correlation))
}

/// The digest the client declares, if it declares one.
///
/// A header that is not a digest is a **malformed** header, not an
/// absent one: the client has said something, and what it said makes no
/// sense. Accepting it silently would be accepting a promise that cannot
/// be checked.
fn declared_digest(headers: &HeaderMap) -> Result<Option<ContentDigest>, RawApiError> {
    let Some(value) = headers.get(CHECKSUM_HEADER) else {
        return Ok(None);
    };
    let value = value
        .to_str()
        .map_err(|_| RawApiError::BadRequest("the declared digest is not valid"))?
        .trim();
    // The algorithm name is case-insensitive: `SHA256:` says the
    // same as `sha256:`. A prefix that is not that of this algorithm is
    // a promise that cannot be checked.
    let value = match value.split_once(':') {
        Some((algorithm, value)) if algorithm.eq_ignore_ascii_case("sha256") => value,
        Some(_) => {
            return Err(RawApiError::BadRequest(
                "the declared digest is not a sha256",
            ));
        }
        None => value,
    };
    let digest = ContentDigest::sha256(value);
    if !digest.is_well_formed() {
        return Err(RawApiError::BadRequest(
            "the declared digest is not a valid sha256",
        ));
    }
    Ok(Some(digest))
}

// ---------------------------------------------------------------------------
// Common plumbing of the operating system surfaces (apt, yum)
// ---------------------------------------------------------------------------

/// The common response of a document: status, body, type, etag,
/// common headers and `nosniff`. It is shared by apt and yum because the
/// HTTP contract is the same: deterministic bytes with conditionals.
pub(super) fn system_body_response(
    status: StatusCode,
    body: Vec<u8>,
    content_type: &str,
    etag: &str,
    correlation: &str,
) -> Response {
    let mut response = (status, Body::from(body)).into_response();
    apply_common_headers(&mut response, etag, correlation);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(content_type).expect("a content type"),
    );
    response.headers_mut().insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    response
}

/// `If-None-Match`/`If-Match` against a document's etag: a 304
/// says it did not change; `None` says the caller must serve the
/// body. With HEAD no body is sent, only the size.
pub(super) fn system_conditional_read(
    etag: &str,
    headers: &HeaderMap,
    correlation: &str,
    head: bool,
    body: &[u8],
) -> Result<Option<Response>, RawApiError> {
    match conditional::the_read_condition(
        conditional::the_if_match(headers).as_deref(),
        conditional::the_if_none_match(headers).as_deref(),
        Some(etag),
    ) {
        conditional::ReadOutcome::Failed => Err(RawApiError::PreconditionFailed),
        conditional::ReadOutcome::NotModified => {
            let mut response = StatusCode::NOT_MODIFIED.into_response();
            apply_common_headers(&mut response, etag, correlation);
            Ok(Some(response))
        }
        conditional::ReadOutcome::Serve => Ok(if head {
            let mut response = StatusCode::OK.into_response();
            apply_common_headers(&mut response, etag, correlation);
            if let Ok(value) = HeaderValue::from_str(&body.len().to_string()) {
                response.headers_mut().insert(header::CONTENT_LENGTH, value);
            }
            Some(response)
        } else {
            None
        }),
    }
}

/// Serves a derived document (index, header, signature): its etag is the
/// digest of the bytes that are answered, and the conditionals apply
/// over that same document.
pub(super) fn serve_system_bytes(
    bytes: Vec<u8>,
    content_type: &str,
    headers: &HeaderMap,
    head: bool,
    correlation: &str,
) -> Result<Response, RawApiError> {
    let digest = ContentDigest::of_bytes(&bytes);
    let etag = conditional::the_etag(&digest);
    if let Some(response) = system_conditional_read(&etag, headers, correlation, head, &bytes)? {
        return Ok(response);
    }
    Ok(system_body_response(
        StatusCode::OK,
        bytes,
        content_type,
        &etag,
        correlation,
    ))
}

/// Writes an immutable asset to the catalog. A conflict with the same
/// digest is the echo of an interrupted publication, not a
/// rewrite; with other bytes it is a 409 with the given reason.
#[allow(clippy::too_many_arguments)]
pub(super) async fn publish_system_asset(
    context: &VerifiedContext,
    writer: &dyn CatalogWriter,
    repository: &RepositoryDescriptor,
    asset_path: &str,
    coordinates: PackageCoordinates,
    digest: ContentDigest,
    size_bytes: u64,
    conflict_message: &str,
    correlation: &str,
) -> Result<(), RawApiError> {
    let publication = Publication {
        repository: repository.repo_key.clone(),
        asset_path: asset_path.to_string(),
        coordinates,
        digest,
        size_bytes,
        tags: Vec::new(),
        actor: context.principal().subject().as_str().to_string(),
        correlation_id: correlation.to_string(),
        overwrite: false,
        precondition: None,
        subject: None,
    };
    match writer
        .publish(context.account().as_str(), publication)
        .await
    {
        Ok(_) => Ok(()),
        Err(CatalogError::Conflict { .. }) => Err(RawApiError::Conflict(Box::leak(
            conflict_message.to_string().into_boxed_str(),
        ))),
        Err(error) => Err(the_publication_error(&error)),
    }
}

/// The zero moment: without publications, an index's date is that of the
/// epoch so that the document is deterministic.
pub(super) fn epoch_moment() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(0, 0).expect("the epoch is a valid moment")
}
