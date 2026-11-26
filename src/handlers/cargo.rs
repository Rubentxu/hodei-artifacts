//! The **cargo** surface: `cargo publish` and the sparse resolution of
//! `cargo add` / `cargo build` against a Hodei repository.
//!
//! Lives at `/cargo/{repository}/...` and speaks the registry protocol in
//! its sparse variant: the grammar of paths and documents is in
//! `hodei_catalog::cargo` (pure) and here there is only what needs effects.
//!
//! Each published version leaves **two immutable assets**: the bytes of the
//! .crate at `{name}/-/{name}-{version}.crate` and its index line at
//! `{name}/-/index/{version}.json`. The index is assembled by listing those
//! documents, never by rewriting them: a published version is forever,
//! and two concurrent publishers cannot overwrite each other because each
//! one writes its own version.
//!
//! # Proxy
//!
//! A **proxy** cargo repository resolves against the sparse upstream its
//! configuration declares: a crate's index document is fetched from the
//! sparse path the client asked for (and served **byte for byte**, without
//! reordering or rewriting: the order and the cksums are the upstream's) and
//! the .crate is fetched through the `dl` template the upstream's config.json
//! declares. The index and the config are **metadata** revalidated by TTL
//! with the common proxy cache (an upstream yank shows up when the TTL
//! expires); the .crate is immutable content. The download verifies the
//! cksum its index line declares: bytes that do not match the index are not
//! served.
//!
//! The proxy **never publishes**: `cargo publish` against it is a 405. And a
//! name that a **hosted** repository of the account already knows is
//! reserved: the proxy blocks it and does not fall through to the upstream,
//! neither from cache nor live: the upstream cannot impersonate what the
//! hosted one already published.

use axum::{
    body::Body,
    extract::{OriginalUri, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use hodei_catalog::{
    CatalogError, PackageCoordinates, Publication, RepositoryDescriptor, RepositoryFormat,
    RepositoryMode,
    cargo::{self, CargoError, Route},
};
use hodei_evidence::ContentDigest;
use serde_json::{Value, json};
use url::Url;

use super::raw::{
    ProxyServed, RawApiError, apply_cache_headers, apply_common_headers, the_admission_error,
    the_blob_error, the_catalog, the_content_store, the_context, the_decision, the_proxy_asset,
    the_proxy_asset_at, the_publication_error, the_repository_in, write_the_content,
};
use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{correlation_id, record_audit};
use crate::public_url::TheOrigin;
use crate::raw::conditional;

/// What happens to a request that cannot be served.
#[derive(Debug)]
pub enum Failure {
    /// An error from the shared surface.
    Raw(RawApiError),
    /// An error from the cargo protocol.
    Cargo(CargoError),
    /// A status and a reason.
    Plain(StatusCode, String),
}

impl From<RawApiError> for Failure {
    fn from(error: RawApiError) -> Self {
        Self::Raw(error)
    }
}

impl From<CargoError> for Failure {
    fn from(error: CargoError) -> Self {
        Self::Cargo(error)
    }
}

impl Failure {
    fn plain(status: StatusCode, reason: &str) -> Self {
        Self::Plain(status, reason.to_string())
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Raw(error) => error.the_verdict(),
            Self::Cargo(error) => (
                if matches!(error, CargoError::InvalidPath(_)) {
                    StatusCode::NOT_FOUND
                } else {
                    StatusCode::BAD_REQUEST
                },
                error.message(),
            ),
            Self::Plain(status, message) => (status, message),
        };
        let mut response = (status, axum::Json(json!({ "error": message }))).into_response();
        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
    }
}

/// The rest of the path of `/cargo/{repository}/{rest}`, undecoded:
/// the protocol rules are the only ones that may interpret a `%2f`.
fn the_split(uri: &OriginalUri) -> Result<(String, String), Failure> {
    let rest = uri
        .path()
        .strip_prefix("/cargo/")
        .ok_or(RawApiError::NotFound)?;
    let (repository, rest) = rest.split_once('/').ok_or(RawApiError::NotFound)?;
    if !crate::raw::path::valid_repository_name(repository) || rest.is_empty() {
        return Err(RawApiError::NotFound.into());
    }
    Ok((repository.to_string(), rest.to_string()))
}

/// The base URL of the index as the client sees it.
///
/// The origin is decided by the deployment and not by the request: the `Host` and
/// `X-Forwarded-Proto` of a request can be written by anyone, and
/// a URL the client moves is a URL the client poisons.
fn the_base(origin: &TheOrigin, repository: &str) -> Result<String, Failure> {
    origin
        .url(&format!("/cargo/{repository}"))
        .ok_or(RawApiError::BadRequest(
            "the public address of the registry is unknown: the Host header is missing or invalid",
        ))
        .map_err(Failure::from)
}

/// Cargo sends the token without a scheme: `Authorization: <jwt>`. The shared
/// surface only speaks Bearer, so the header is normalized here.
fn normalize_authorization(headers: &mut HeaderMap) {
    let raw = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            !value.is_empty()
                && !value.contains(' ')
                && !value.to_ascii_lowercase().starts_with("bearer")
        })
        .map(str::to_string);
    if let Some(token) = raw
        && let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}"))
    {
        headers.insert(header::AUTHORIZATION, value);
    }
}

/// Any request to `/cargo/{repository}/...`.
pub async fn handle(
    State(state): State<AppState>,
    origin: TheOrigin,
    method: Method,
    uri: OriginalUri,
    mut headers: HeaderMap,
    request: axum::extract::Request<Body>,
) -> Result<Response, Failure> {
    normalize_authorization(&mut headers);
    let context = the_context(&state, &headers).await?;
    let correlation = correlation_id(&headers, &state.identity);
    let (repository_key, rest) = the_split(&uri)?;
    let route = Route::parse(&rest)?;
    let repository = the_repository_in(
        &state,
        &context,
        &repository_key,
        &[RepositoryFormat::Cargo],
    )
    .await?;

    let reading = method == Method::GET || method == Method::HEAD;
    let publishing = method == Method::PUT;
    match (&route, reading, publishing) {
        (Route::Config, true, _) => {
            the_decision(
                &state,
                &context,
                crate::actions::download_asset(),
                &repository,
            )?;
            // The config.json is ALWAYS this service's, also on a proxy:
            // the client downloads here, not from the upstream.
            let body = cargo::config_json(&the_base(&origin, &repository_key)?);
            Ok(json_response(StatusCode::OK, &body, None, &correlation))
        }
        (Route::Index { name }, true, _) => {
            let outcome = if repository.mode == RepositoryMode::Proxy {
                read_proxy_index(
                    &state,
                    &context,
                    &repository,
                    name,
                    method == Method::HEAD,
                    &correlation,
                )
                .await
            } else {
                read_index(
                    &state,
                    &context,
                    &repository,
                    name,
                    method == Method::HEAD,
                    &headers,
                    &correlation,
                )
                .await
            };
            record_outcome(
                &state,
                &context,
                &repository_key,
                name,
                &outcome,
                &correlation,
            );
            outcome
        }
        (Route::Download { name, version }, true, _) => {
            let outcome = if repository.mode == RepositoryMode::Proxy {
                read_proxy_download(
                    &state,
                    &context,
                    &repository,
                    name,
                    version,
                    method == Method::HEAD,
                    &correlation,
                )
                .await
            } else {
                let asset_path = format!("{name}/-/{name}-{version}.crate");
                read_crate(
                    &state,
                    &context,
                    repository.clone(),
                    &asset_path,
                    method == Method::HEAD,
                    &correlation,
                )
                .await
            };
            record_outcome(
                &state,
                &context,
                &repository_key,
                name,
                &outcome,
                &correlation,
            );
            outcome
        }
        (Route::Publish, false, true) => {
            if repository.mode == RepositoryMode::Proxy {
                return Err(Failure::plain(
                    StatusCode::METHOD_NOT_ALLOWED,
                    "a proxy does not publish: the upstream is the only writer",
                ));
            }
            publish(&state, &context, &repository, request, &correlation).await
        }
        (Route::Publish, false, _) => {
            Err(RawApiError::MethodNotAllowed("cargo publish is a PUT of the whole payload").into())
        }
        _ => Err(RawApiError::MethodNotAllowed("this cargo operation is not supported").into()),
    }
}

fn json_response(
    status: StatusCode,
    value: &Value,
    etag: Option<&str>,
    correlation: &str,
) -> Response {
    let mut response = (status, axum::Json(value.clone())).into_response();
    if let Some(etag) = etag {
        apply_common_headers(&mut response, etag, correlation);
    }
    response
}

fn record_outcome(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &str,
    name: &str,
    outcome: &Result<Response, Failure>,
    correlation: &str,
) {
    let result = match outcome {
        Ok(_) => AuditResult::Allowed,
        Err(Failure::Raw(RawApiError::Forbidden)) => AuditResult::Denied,
        Err(_) => AuditResult::Failed,
    };
    record_audit(
        state,
        context.principal().subject().as_str(),
        context.account().as_str(),
        "download",
        &format!("{repository}/{name}"),
        result,
        correlation,
    );
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

/// The sparse index of a crate: the NDJSON concatenation of its lines,
/// ordered by version. There is no document to rewrite: it is assembled.
async fn read_index(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    name: &str,
    head: bool,
    headers: &HeaderMap,
    correlation: &str,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::download_asset(), repository)?;
    let (reader, _) = the_catalog(state)?;
    let store = the_content_store(state)?;
    let records = reader
        .assets_under(
            context.account().as_str(),
            &repository.repo_key,
            &cargo::Route::version_prefix(name),
        )
        .await
        .map_err(|error| Failure::from(the_publication_error_ref(&error)))?;

    let mut lines: Vec<(semver::Version, Value)> = Vec::new();
    for record in records {
        let bytes = store
            .get(&record.digest)
            .await
            .map_err(|error| the_blob_error(&error))?;
        let line: Value = serde_json::from_slice(&bytes).map_err(|_| {
            Failure::plain(
                StatusCode::BAD_GATEWAY,
                "a stored index document is not a valid line",
            )
        })?;
        let version =
            semver::Version::parse(line["vers"].as_str().unwrap_or_default()).map_err(|_| {
                Failure::plain(
                    StatusCode::BAD_GATEWAY,
                    "a stored index document has an unusable version",
                )
            })?;
        lines.push((version, line));
    }
    if lines.is_empty() {
        // Cargo expects a 404 saying the crate does not exist here.
        return Err(Failure::plain(
            StatusCode::NOT_FOUND,
            "there is no crate of that name in this repository",
        ));
    }
    lines.sort_by(|left, right| left.0.cmp(&right.0));
    let mut body = String::new();
    for (_, line) in &lines {
        body.push_str(&serde_json::to_string(line).expect("a line serializes"));
        body.push('\n');
    }
    let digest = ContentDigest::of_bytes(body.as_bytes());
    let etag = conditional::the_etag(&digest);
    if let Some(response) = conditional_read(&etag, headers, correlation, head, body.as_bytes())? {
        return Ok(response);
    }
    Ok(body_response(
        StatusCode::OK,
        body.into_bytes(),
        "application/json",
        &etag,
        correlation,
    ))
}

/// The bytes of a .crate, verified against its asset.
async fn read_crate(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: RepositoryDescriptor,
    asset_path: &str,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    the_decision(
        state,
        context,
        crate::actions::download_asset(),
        &repository,
    )?;
    let resolved =
        super::raw::the_resolved_asset(state, context, repository, asset_path, correlation).await?;
    let super::raw::Resolved {
        asset,
        served,
        resolved_by: _,
        repository: backing,
    } = resolved;
    crate::trust::admit(state, context.account().as_str(), &backing, &asset.digest)
        .await
        .map_err(the_admission_error)?;
    let etag = conditional::the_etag(&asset.digest);
    let bytes = if head {
        Vec::new()
    } else {
        let store = the_content_store(state)?;
        store
            .get(&asset.digest)
            .await
            .map_err(|error| the_blob_error(&error))?
            .to_vec()
    };
    let length = if head {
        asset.size_bytes as usize
    } else {
        bytes.len()
    };
    let mut response = body_response(
        StatusCode::OK,
        bytes,
        "application/octet-stream",
        &etag,
        correlation,
    );
    apply_cache_headers(&mut response, &served);
    apply_common_headers(&mut response, &etag, correlation);
    if head && let Ok(value) = HeaderValue::from_str(&length.to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    Ok(response)
}

/// `If-None-Match` against the index etag: a 304 says it did not change.
fn conditional_read(
    etag: &str,
    headers: &HeaderMap,
    correlation: &str,
    head: bool,
    body: &[u8],
) -> Result<Option<Response>, Failure> {
    match conditional::the_read_condition(
        conditional::the_if_match(headers).as_deref(),
        conditional::the_if_none_match(headers).as_deref(),
        Some(etag),
    ) {
        conditional::ReadOutcome::Failed => Err(RawApiError::PreconditionFailed.into()),
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

fn body_response(
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
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    response
}

/// Publish: the version is immutable, and that is stated before writing anything.
async fn publish(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    request: axum::extract::Request<Body>,
    correlation: &str,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::upload_asset(), repository)?;
    if !repository.mode.accepts_client_writes() {
        return Err(
            RawApiError::MethodNotAllowed("this repository does not accept uploads").into(),
        );
    }
    let limit = state.max_body_size.saturating_add(1024 * 1024);
    let body = axum::body::to_bytes(request.into_body(), limit)
        .await
        .map_err(|_| Failure::Raw(RawApiError::PayloadTooLarge))?;
    let new = cargo::parse_publish(&body)?;
    let declared_name = new
        .header
        .get("name")
        .and_then(Value::as_str)
        .ok_or(CargoError::InvalidPayload("the header has no crate name"))?;
    let name = cargo::normalize(declared_name)?;
    let version = new
        .header
        .get("vers")
        .and_then(Value::as_str)
        .filter(|version| cargo::valid_version(version))
        .ok_or(CargoError::InvalidPayload(
            "the header has no version in strict semver",
        ))?
        .to_string();
    let cksum = cargo::cksum(new.file);

    let doc_path = cargo::Route::crate_asset_path(&name, &version);
    let crate_path = format!("{name}/-/{name}-{version}.crate");
    let (_, writer) = the_catalog(state)?;

    // An already-published version is forever: an identical one is a yes,
    // a different one is a no that explains why.
    if let Some(existing) = existing_line(state, context, &repository.repo_key, &doc_path).await? {
        if existing["cksum"].as_str() == Some(cksum.as_str()) {
            return Ok(json_response(
                StatusCode::OK,
                &json!({ "ok": true }),
                None,
                correlation,
            ));
        }
        return Err(Failure::plain(
            StatusCode::CONFLICT,
            &format!(
                "version {version} of {name} is already published; a published version is immutable"
            ),
        ));
    }

    let line = cargo::index_line(&new.header, &cksum)?;
    let line_bytes = serde_json::to_vec(&line).expect("a line serializes");

    // 1. The .crate bytes, swapped in before the catalog.
    let (crate_digest, crate_length) = write_the_content(
        state,
        context.account().as_str(),
        Body::from(new.file.to_vec()),
        None,
        None,
    )
    .await?;
    publish_asset(
        context,
        writer.as_ref(),
        repository,
        &crate_path,
        PackageCoordinates {
            package_name: name.clone(),
            package_version: version.clone(),
        },
        crate_digest,
        crate_length,
        &cksum_mismatch(&name, &version),
        correlation,
    )
    .await?;

    // 2. The index line, immutable like its version.
    let (doc_digest, doc_length) = write_the_content(
        state,
        context.account().as_str(),
        Body::from(line_bytes),
        None,
        None,
    )
    .await?;
    publish_asset(
        context,
        writer.as_ref(),
        repository,
        &doc_path,
        PackageCoordinates {
            package_name: format!("{name}#index"),
            package_version: version.clone(),
        },
        doc_digest,
        doc_length,
        &cksum_mismatch(&name, &version),
        correlation,
    )
    .await?;

    Ok(json_response(
        StatusCode::OK,
        &json!({ "ok": true }),
        None,
        correlation,
    ))
}

fn cksum_mismatch(name: &str, version: &str) -> String {
    format!("version {version} of {name} is already published; a published version is immutable")
}

async fn existing_line(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &str,
    doc_path: &str,
) -> Result<Option<Value>, Failure> {
    let Some(asset) = super::raw::the_current_asset(state, context, repository, doc_path).await?
    else {
        return Ok(None);
    };
    let store = the_content_store(state)?;
    let bytes = store
        .get(&asset.digest)
        .await
        .map_err(|error| the_blob_error(&error))?;
    Ok(serde_json::from_slice(&bytes).ok())
}

/// Writes an immutable asset; a conflict with the same digest is the echo
/// of an interrupted publication, not a rewrite.
#[allow(clippy::too_many_arguments)]
async fn publish_asset(
    context: &hodei_authz::VerifiedContext,
    writer: &dyn hodei_catalog::ports::CatalogWriter,
    repository: &RepositoryDescriptor,
    asset_path: &str,
    coordinates: PackageCoordinates,
    digest: ContentDigest,
    size_bytes: u64,
    conflict_message: &str,
    correlation: &str,
) -> Result<(), Failure> {
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
        Err(CatalogError::Conflict { .. }) => {
            Err(Failure::plain(StatusCode::CONFLICT, conflict_message))
        }
        Err(error) => Err(the_publication_error(&error).into()),
    }
}

/// The publication error of a list read: the list is not caused
/// by a conflict, but the same translator knows what each variant means.
fn the_publication_error_ref(error: &CatalogError) -> RawApiError {
    match error {
        CatalogError::Denied { .. } => RawApiError::Forbidden,
        CatalogError::Missing { .. } => RawApiError::NotFound,
        CatalogError::Unavailable { .. } | CatalogError::Corrupt { .. } => RawApiError::Internal,
        other => RawApiError::BadRequest(Box::leak(other.to_string().into_boxed_str())),
    }
}

// ---------------------------------------------------------------------------
// Proxy
// ---------------------------------------------------------------------------

/// The name reserved by a hosted repository of the account, if any.
///
/// A crate that a **hosted** cargo repository of the same account already
/// knows (any asset under `{name}/`) is reserved: the proxy resolves
/// neither its index nor its download against the upstream, neither live
/// nor from the cache. An upstream that answered for a name the hosted one
/// already published would be an impersonation, and the silent fallback is
/// exactly what this surface does not do.
async fn the_hosted_collision(
    state: &AppState,
    account: &str,
    name: &str,
) -> Result<Option<String>, Failure> {
    let search = state
        .catalog_search
        .as_ref()
        .ok_or(RawApiError::Unavailable)?;
    let reader = state
        .catalog_reader
        .as_ref()
        .ok_or(RawApiError::Unavailable)?;
    let repositories = search
        .repositories(account)
        .await
        .map_err(|error| Failure::from(the_publication_error_ref(&error)))?;
    let prefix = format!("{name}/");
    for repository in repositories {
        if repository.format != RepositoryFormat::Cargo || repository.mode != RepositoryMode::Hosted
        {
            continue;
        }
        let known = reader
            .assets_under(account, &repository.repo_key, &prefix)
            .await
            .map_err(|error| Failure::from(the_publication_error_ref(&error)))?;
        if !known.is_empty() {
            return Ok(Some(repository.repo_key));
        }
    }
    Ok(None)
}

/// The reservation is blocked with a 409 that says who holds it.
async fn guard_against_hosted(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    name: &str,
) -> Result<(), Failure> {
    if let Some(reserved_by) = the_hosted_collision(state, context.account().as_str(), name).await?
    {
        return Err(Failure::plain(
            StatusCode::CONFLICT,
            &format!(
                "the crate {name} is reserved by the hosted repository {reserved_by}: \
                 the proxy does not resolve it from upstream"
            ),
        ));
    }
    Ok(())
}

/// The sparse index of a crate on a proxy: the upstream's document, byte
/// for byte.
///
/// The cached path is the deterministic sparse path of the normalized
/// name: the same one the client computes. What the upstream answered is
/// served as is: the order of versions and the cksums are the upstream's,
/// and reordering them would rewrite a document the client verifies. It is
/// metadata: the TTL revalidates it, and an upstream yank shows up when the
/// revalidation brings it.
async fn read_proxy_index(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    name: &str,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    let normalized = cargo::normalize(name)?;
    guard_against_hosted(state, context, &normalized).await?;
    let index_path = cargo::Route::index_path(&normalized)?;
    let served = the_proxy_asset(state, context, repository, &index_path, correlation).await?;
    serve_proxy_document(state, context, repository, served, head, correlation).await
}

/// The download of a version on a proxy.
///
/// The upstream's index line states the cksum; the `dl` template of the
/// upstream's config.json states where the bytes come from. The download is
/// served only if the sha256 of the bytes is the one its line declares: an
/// upstream that delivers other bytes never serves the version.
async fn read_proxy_download(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    name: &str,
    version: &str,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    let normalized = cargo::normalize(name)?;
    guard_against_hosted(state, context, &normalized).await?;
    let index_path = cargo::Route::index_path(&normalized)?;

    // The version's line: without it there is no download to verify. The
    // line is looked up even for a HEAD: the existence of the version is
    // checked all the same.
    let index = the_proxy_asset(state, context, repository, &index_path, correlation).await?;
    let bytes = proxy_bytes(state, &index.asset, false).await?;
    let cksum = the_version_cksum(&bytes, version)?
        .ok_or_else(|| {
            Failure::plain(
                StatusCode::NOT_FOUND,
                "the version is not in the index of the upstream",
            )
        })?
        .to_lowercase();

    // The upstream's template: where the .crate lives according to what the
    // upstream itself declares.
    let upstream = hodei_catalog::UpstreamConfig::of(&repository.config)
        .ok()
        .flatten()
        .ok_or_else(|| Failure::plain(StatusCode::BAD_GATEWAY, "the proxy is not configured"))?;
    let url = the_upstream_target(
        state,
        context,
        repository,
        &upstream,
        &normalized,
        version,
        correlation,
    )
    .await?;

    let crate_path = format!("{normalized}/-/{normalized}-{version}.crate");
    let served = the_proxy_asset_at(
        state,
        context,
        repository,
        &crate_path,
        Some(url),
        None,
        correlation,
    )
    .await?;
    if served.asset.digest.value.to_lowercase() != cksum {
        tracing::warn!(
            target: "hodei_artifacts::cargo_proxy",
            repository = %repository.repo_key,
            crate = %normalized,
            version = %version,
            declared = %cksum,
            got = %served.asset.digest.value,
            "the upstream crate does not match the checksum of its index line"
        );
        return Err(Failure::plain(
            StatusCode::BAD_GATEWAY,
            "the upstream crate does not match the checksum declared by its index line",
        ));
    }
    serve_proxy_document(state, context, repository, served, head, correlation).await
}

/// The sha256 that the version line declares in the upstream index.
fn the_version_cksum(index: &[u8], version: &str) -> Result<Option<String>, Failure> {
    let text = std::str::from_utf8(index)
        .map_err(|_| Failure::plain(StatusCode::BAD_GATEWAY, "the upstream index is not text"))?;
    for line in text.lines() {
        let line: Value = serde_json::from_str(line).map_err(|_| {
            Failure::plain(
                StatusCode::BAD_GATEWAY,
                "the upstream index has an unusable line",
            )
        })?;
        if line["vers"].as_str() == Some(version) {
            return Ok(line["cksum"]
                .as_str()
                .filter(|cksum| !cksum.is_empty())
                .map(str::to_string));
        }
    }
    Ok(None)
}

/// The download URL: the upstream's `dl` template, and if the upstream
/// declares no config.json, the tail the protocol appends to the base.
async fn the_upstream_target(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    upstream: &hodei_catalog::UpstreamConfig,
    name: &str,
    version: &str,
    correlation: &str,
) -> Result<Url, Failure> {
    let dl = match the_proxy_asset(state, context, repository, "config.json", correlation).await {
        Ok(served) => {
            let bytes = proxy_bytes(state, &served.asset, false).await?;
            cargo::the_upstream_dl(&serde_json::from_slice(&bytes).map_err(|_| {
                Failure::plain(StatusCode::BAD_GATEWAY, "the upstream config is not JSON")
            })?)
            .map(str::to_string)
        }
        Err(_) => None,
    }
    .unwrap_or_else(|| upstream.base.to_string());
    let template = cargo::download_url(&dl, name, version);
    if let Ok(absolute) = Url::parse(&template) {
        return Ok(absolute);
    }
    upstream
        .base
        .join(&template)
        .map_err(|_| Failure::plain(StatusCode::BAD_GATEWAY, "the upstream dl is not a URL"))
}

/// The bytes of a document served by the proxy; a HEAD does not read them.
async fn proxy_bytes(
    state: &AppState,
    asset: &hodei_catalog::AssetRecord,
    head: bool,
) -> Result<Vec<u8>, Failure> {
    if head {
        return Ok(Vec::new());
    }
    let store = the_content_store(state)?;
    Ok(store
        .get(&asset.digest)
        .await
        .map_err(|error| the_blob_error(&error))?
        .to_vec())
}

/// Serves a proxy document: the trust gate, the ETag of its content and
/// the state of the cache.
async fn serve_proxy_document(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    served: ProxyServed,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    // The trust gate runs before the conditions: a 304 would say the
    // content exists.
    crate::trust::admit(
        state,
        context.account().as_str(),
        repository,
        &served.asset.digest,
    )
    .await
    .map_err(the_admission_error)?;
    let etag = conditional::the_etag(&served.asset.digest);
    let bytes = proxy_bytes(state, &served.asset, head).await?;
    let mut response = body_response(
        StatusCode::OK,
        bytes,
        "application/octet-stream",
        &etag,
        correlation,
    );
    apply_cache_headers(&mut response, &Some((served.status, served.provenance)));
    if head {
        let length = served.asset.size_bytes as usize;
        if let Ok(value) = HeaderValue::from_str(&length.to_string()) {
            response.headers_mut().insert(header::CONTENT_LENGTH, value);
        }
    }
    Ok(response)
}
