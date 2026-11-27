//! The **npm** surface: `npm publish`, `npm install`, `npm dist-tag` and
//! `npm deprecate` against a Hodei repository.
//!
//! Lives at `/npm/{repository}/...` and speaks the registry protocol: the
//! grammar of paths and documents is in `hodei_catalog::npm` (pure) and
//! here there is only what needs effects.
//!
//! # Hosted
//!
//! The packument is a **snapshot**: each publication, deprecation or
//! dist-tag move writes a new document in the content store
//! and moves the path pointer with a precondition on the previous
//! digest (the catalog's `If-Match`). The bytes of earlier snapshots
//! are not touched, and two concurrent writers do not overwrite each other: the one that
//! loses re-reads and retries.
//!
//! Tarballs are immutable content at `{name}/-/{file}.tgz`:
//! publishing the same version again is a `403`, as in the npm registry
//! (a `409` would make `libnpmpublish` try to merge documents).
//!
//! # Unpublish
//!
//! The policy is conservative: **nothing is deleted**. `npm unpublish` receives
//! a `403` saying to use `npm deprecate`, and the attempt is left in the
//! audit trail.
//!
//! # Proxy and group
//!
//! They reuse the same resolution as the rest of the formats: the packument is
//! treated as revalidatable metadata (TTL, offline) and the tarball as
//! immutable content. In a group the first member that responds wins.

use axum::{
    body::Body,
    extract::{OriginalUri, State},
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::Engine as _;
use hodei_authz::VerifiedContext;
use hodei_catalog::{
    CatalogError, PackageCoordinates, Precondition, Publication, RepositoryDescriptor,
    RepositoryFormat,
    npm::{self, NpmConfig, NpmError, PackageName, Route},
};
use hodei_evidence::domain::content::ContentDigest;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha512};

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{correlation_id, record_audit};
use crate::public_url::TheOrigin;
use crate::raw::{conditional, path};
use crate::resolution::{
    ResolutionError as RawApiError, ResolvedAsset, WrittenContent, apply_common_headers,
    apply_origin_headers, authorize_repository, hex_of, resolve_asset, the_admission_error,
    the_blob_error, the_catalog, the_content_store, the_context, the_current_asset, the_decision,
    the_publication_error, the_repository_in, write_content,
};

/// How many times a packument write that loses the race to another
/// is retried.
const MAX_ATTEMPTS: usize = 6;

/// What the client is told when the retries run out.
const CONTENDED: &str = "the package is being updated concurrently; retry";

/// What happens to a request that cannot be served.
///
/// npm reads the body of an error as JSON: `{"error": "..."}`.
#[derive(Debug)]
pub enum Failure {
    /// An error from the shared surface.
    Raw(RawApiError),
    /// An error from the npm protocol.
    Npm(NpmError),
    /// A status and a reason.
    Plain(StatusCode, String),
}

impl From<RawApiError> for Failure {
    fn from(error: RawApiError) -> Self {
        Self::Raw(error)
    }
}

impl From<NpmError> for Failure {
    fn from(error: NpmError) -> Self {
        Self::Npm(error)
    }
}

impl Failure {
    fn plain(status: StatusCode, reason: &str) -> Self {
        Self::Plain(status, reason.to_string())
    }

    fn status_and_message(&self) -> (StatusCode, String) {
        match self {
            Self::Raw(error) => match error {
                RawApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized".into()),
                RawApiError::Forbidden => (StatusCode::FORBIDDEN, "forbidden".into()),
                RawApiError::NotFound => (StatusCode::NOT_FOUND, "not found".into()),
                RawApiError::BadRequest(reason) => (StatusCode::BAD_REQUEST, (*reason).into()),
                RawApiError::PreconditionFailed => (
                    StatusCode::PRECONDITION_FAILED,
                    "precondition failed".into(),
                ),
                RawApiError::RangeNotSatisfiable(_) => (
                    StatusCode::RANGE_NOT_SATISFIABLE,
                    "the range is not satisfiable".into(),
                ),
                RawApiError::PayloadTooLarge => (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "the content is larger than this service accepts".into(),
                ),
                RawApiError::Quota(refusal) => match refusal {
                    crate::quota::QuotaRefusal::StorageOver { .. } => (
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "the content would take this account past its stored quota".into(),
                    ),
                    crate::quota::QuotaRefusal::UploadsSaturated { .. } => (
                        StatusCode::TOO_MANY_REQUESTS,
                        "this account already has as many uploads in flight as it may have".into(),
                    ),
                },
                RawApiError::Conflict(reason) => (StatusCode::CONFLICT, (*reason).into()),
                RawApiError::Unavailable => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "the npm surface is not available".into(),
                ),
                RawApiError::NotImplemented(reason) => {
                    (StatusCode::NOT_IMPLEMENTED, (*reason).into())
                }
                RawApiError::MethodNotAllowed(reason) => {
                    (StatusCode::METHOD_NOT_ALLOWED, (*reason).into())
                }
                RawApiError::BadGateway(reason) => (StatusCode::BAD_GATEWAY, (*reason).into()),
                RawApiError::GatewayTimeout => (
                    StatusCode::GATEWAY_TIMEOUT,
                    "the upstream did not answer in time".into(),
                ),
                RawApiError::Offline(reason) => (StatusCode::SERVICE_UNAVAILABLE, (*reason).into()),
                RawApiError::ReservedNamespace(reason) => (StatusCode::FORBIDDEN, reason.clone()),
                RawApiError::Admission(_) => error.the_verdict(),
                RawApiError::Internal => {
                    (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
                }
            },
            Self::Npm(error) => {
                let status = match error {
                    // npm answers 403 when publishing over an existing version.
                    NpmError::VersionExists(_) | NpmError::Unpublish => StatusCode::FORBIDDEN,
                    NpmError::InvalidPath(_) | NpmError::UnknownVersion(_) => StatusCode::NOT_FOUND,
                    NpmError::InvalidName(_)
                    | NpmError::InvalidDocument(_)
                    | NpmError::InvalidTag(_) => StatusCode::BAD_REQUEST,
                };
                (status, error.message())
            }
            Self::Plain(status, message) => (*status, message.clone()),
        }
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let (status, message) = self.status_and_message();
        let mut response = (status, axum::Json(json!({ "error": message }))).into_response();
        // The advice on when to come back travels with the refusal for
        // saturation, in this surface as well: the verdict is a single
        // one, and the header is not a detail of the raw transport.
        if let Self::Raw(error) = &self
            && let Some(seconds) = error.the_retry_after()
            && let Ok(value) = HeaderValue::from_str(&seconds.to_string())
        {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
    }
}

/// The repository and the rest of the path of `/npm/{repository}/{rest}`.
///
/// The rest is read **undecoded**: the name of a scoped package
/// arrives as `@scope%2fname` and decoding it too early would
/// confuse it with two segments.
fn the_split(uri: &OriginalUri) -> Result<(String, String), Failure> {
    let rest = uri
        .path()
        .strip_prefix("/npm/")
        .ok_or(RawApiError::NotFound)?;
    let (repository, rest) = rest.split_once('/').ok_or(RawApiError::NotFound)?;
    if !path::valid_repository_name(repository) || rest.is_empty() {
        return Err(RawApiError::NotFound.into());
    }
    Ok((repository.to_string(), rest.to_string()))
}

/// The base URL of this repository as the client sees it.
///
/// The origin is decided by the deployment and not by the request: the `Host` and
/// `X-Forwarded-Proto` of a request can be written by anyone, and
/// a URL the client moves is a URL the client poisons.
fn the_base(origin: &TheOrigin, repository: &str) -> Result<String, Failure> {
    origin
        .url(&format!("/npm/{repository}"))
        .ok_or(RawApiError::BadRequest(
            "the public address of the registry is unknown: the Host header is missing or invalid",
        ))
        .map_err(Failure::from)
}

fn the_now(state: &AppState) -> String {
    state
        .identity
        .now()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
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

/// Any request to `/npm/{repository}/...`.
pub async fn handle(
    State(state): State<AppState>,
    origin: TheOrigin,
    method: Method,
    uri: OriginalUri,
    headers: HeaderMap,
    request: Request<Body>,
) -> Result<Response, Failure> {
    // Authentication is the first gate, as in the rest of the surfaces.
    let context = the_context(&state, &headers).await?;
    let correlation = correlation_id(&headers, &state.identity);
    let (repository_key, rest) = the_split(&uri)?;
    let route = npm::route(&rest)?;
    let repository =
        the_repository_in(&state, &context, &repository_key, &[RepositoryFormat::Npm]).await?;

    let reading = method == Method::GET || method == Method::HEAD;
    match (&route, reading) {
        (Route::Ping, true) => {
            the_decision(
                &state,
                &context,
                crate::actions::download_asset(),
                &repository,
            )?;
            Ok(json_response(
                StatusCode::OK,
                &json!({}),
                None,
                &correlation,
            ))
        }
        (Route::Whoami, true) => {
            the_decision(
                &state,
                &context,
                crate::actions::download_asset(),
                &repository,
            )?;
            let username = context.principal().subject().as_str();
            Ok(json_response(
                StatusCode::OK,
                &json!({ "username": username }),
                None,
                &correlation,
            ))
        }
        (
            Route::Packument { .. }
            | Route::Tarball { .. }
            | Route::DistTags { .. }
            | Route::DistTag { .. },
            true,
        ) => {
            let resource = format!(
                "{repository_key}/{}",
                route.asset_path().unwrap_or_default()
            );
            let outcome = read(
                &state,
                &context,
                repository,
                &route,
                method == Method::HEAD,
                &headers,
                &origin,
                &correlation,
            )
            .await;
            let result = match &outcome {
                Ok(_) => AuditResult::Allowed,
                Err(Failure::Raw(RawApiError::Forbidden)) => AuditResult::Denied,
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
        (Route::Packument { name }, false) if method == Method::PUT => {
            put_packument(&state, &context, &repository, name, request, &correlation).await
        }
        (Route::DistTag { name, tag }, false) if method == Method::PUT => {
            put_dist_tag(
                &state,
                &context,
                &repository,
                name,
                tag,
                request,
                &correlation,
            )
            .await
        }
        (Route::DistTag { name, tag }, false) if method == Method::DELETE => {
            delete_dist_tag(&state, &context, &repository, name, tag, &correlation).await
        }
        // `npm unpublish <name>@<version>` does `PUT .../-rev/{rev}` with the
        // document without that version; the whole package, a `DELETE`.
        (Route::UnpublishPackage { name } | Route::UnpublishTarball { name, .. }, false)
            if method == Method::DELETE || method == Method::PUT =>
        {
            refuse_unpublish(&state, &context, &repository, name, &correlation)
        }
        _ => Err(RawApiError::MethodNotAllowed("this npm operation is not supported").into()),
    }
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn read(
    state: &AppState,
    context: &VerifiedContext,
    repository: RepositoryDescriptor,
    route: &Route,
    head: bool,
    headers: &HeaderMap,
    origin: &TheOrigin,
    correlation: &str,
) -> Result<Response, Failure> {
    let _decision = authorize_repository(
        &state.authorize_resource,
        context,
        crate::actions::download_asset(),
        &repository,
    )?;
    let requested = repository.repo_key.clone();
    let asset_path = route.asset_path().ok_or(RawApiError::NotFound)?;
    let ResolvedAsset {
        asset,
        digest,
        origin: resolution_origin,
        repository: backing,
    } = resolve_asset(state, context, repository, &asset_path, correlation).await?;
    // The trust gate goes before the read conditions: a
    // 304 would say the content exists.
    crate::trust::admit(state, context.account().as_str(), &backing, &digest)
        .await
        .map_err(the_admission_error)?;

    let etag = conditional::the_etag(&digest);
    match conditional::the_read_condition(
        conditional::the_if_match(headers).as_deref(),
        conditional::the_if_none_match(headers).as_deref(),
        Some(&etag),
    ) {
        conditional::ReadOutcome::Failed => return Err(RawApiError::PreconditionFailed.into()),
        conditional::ReadOutcome::NotModified => {
            let mut response = StatusCode::NOT_MODIFIED.into_response();
            apply_common_headers(&mut response, &etag, correlation);
            apply_origin_headers(&mut response, &resolution_origin);
            return Ok(response);
        }
        conditional::ReadOutcome::Serve => {}
    }

    let store = the_content_store(state)?;
    let (body, content_type, length) = match route {
        Route::Tarball { .. } => {
            if head {
                (
                    Vec::new(),
                    "application/octet-stream",
                    asset.size_bytes as usize,
                )
            } else {
                let bytes = store
                    .get(&asset.digest)
                    .await
                    .map_err(|error| the_blob_error(&error))?;
                let length = bytes.len();
                (bytes.to_vec(), "application/octet-stream", length)
            }
        }
        _ => {
            let bytes = store
                .get(&asset.digest)
                .await
                .map_err(|error| the_blob_error(&error))?;
            let mut packument = npm::parse_packument(&bytes).map_err(|_| {
                Failure::plain(
                    StatusCode::BAD_GATEWAY,
                    "the stored packument is not a valid document",
                )
            })?;
            let document = match route {
                Route::DistTags { .. } => npm::dist_tags(&packument),
                Route::DistTag { tag, .. } => {
                    // `npm view` and clients that request a tag read the
                    // version as a JSON string.
                    packument
                        .get("dist-tags")
                        .and_then(Value::as_object)
                        .and_then(|tags| tags.get(tag))
                        .cloned()
                        .ok_or(Failure::plain(StatusCode::NOT_FOUND, "no such dist-tag"))?
                }
                _ => {
                    let name = match route {
                        Route::Packument { name } => name.full(),
                        _ => return Err(RawApiError::NotFound.into()),
                    };
                    npm::rewrite_tarballs(&mut packument, name, &the_base(origin, &requested)?);
                    Value::Object(packument)
                }
            };
            let serialized = serde_json::to_vec(&document).map_err(|_| RawApiError::Internal)?;
            let length = serialized.len();
            (
                if head { Vec::new() } else { serialized },
                "application/json",
                length,
            )
        }
    };

    let mut response = (StatusCode::OK, Body::from(body)).into_response();
    apply_common_headers(&mut response, &etag, correlation);
    apply_origin_headers(&mut response, &resolution_origin);
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response.headers_mut().insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    if head && let Ok(value) = HeaderValue::from_str(&length.to_string()) {
        // A HEAD says how much the GET would bring.
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    Ok(response)
}

// ---------------------------------------------------------------------------
// Writes
// ---------------------------------------------------------------------------

/// What a packument write left behind.
struct Written {
    digest: ContentDigest,
    changed: bool,
}

/// The permission to write: Cedar and the repository mode.
fn the_write_gate(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
) -> Result<(), Failure> {
    the_decision(state, context, crate::actions::upload_asset(), repository)?;
    // What a proxy says comes from its upstream. It goes after Cedar: whoever
    // cannot upload receives the same 403 in any repository.
    if !repository.mode.accepts_client_writes() {
        return Err(
            RawApiError::MethodNotAllowed("this repository does not accept uploads").into(),
        );
    }
    Ok(())
}

/// Applies a change to the packument and publishes it as a new snapshot.
///
/// The change is **re-evaluated** against what is there on each attempt: if another
/// writer moved the pointer between the read and the write, the catalog
/// rejects the precondition, it is re-read and applied again.
async fn update_packument<F>(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    name: &PackageName,
    create: bool,
    correlation: &str,
    mut change: F,
) -> Result<Written, Failure>
where
    F: FnMut(&mut Map<String, Value>, &str) -> Result<bool, Failure>,
{
    let (_, writer) = the_catalog(state)?;
    let store = the_content_store(state)?;
    for _ in 0..MAX_ATTEMPTS {
        let now = the_now(state);
        let current = the_current_asset(state, context, &repository.repo_key, name.full()).await?;
        let (mut packument, precondition) = match &current {
            Some(asset) => {
                let bytes = store
                    .get(&asset.digest)
                    .await
                    .map_err(|error| the_blob_error(&error))?;
                let packument = npm::parse_packument(&bytes).map_err(|_| RawApiError::Internal)?;
                (packument, Precondition::Present(vec![asset.digest.clone()]))
            }
            None if create => (npm::empty_packument(name, &now), Precondition::Absent),
            None => return Err(RawApiError::NotFound.into()),
        };
        let changed = change(&mut packument, &now)?;
        if !changed && let Some(asset) = &current {
            return Ok(Written {
                digest: asset.digest.clone(),
                changed: false,
            });
        }
        let bytes =
            serde_json::to_vec(&Value::Object(packument)).map_err(|_| RawApiError::Internal)?;
        let WrittenContent { digest, length } = write_content(
            state,
            context.account().as_str(),
            Body::from(bytes),
            None,
            None,
        )
        .await?;
        let publication = Publication {
            repository: repository.repo_key.clone(),
            asset_path: name.full().to_string(),
            // Each snapshot is a distinct version of the packument: it is
            // identified by its content, and the previous one is still there.
            coordinates: PackageCoordinates {
                package_name: format!("{}{}", name.full(), npm::PACKUMENT_COORDINATES_SUFFIX),
                package_version: digest.value.clone(),
            },
            digest: digest.clone(),
            size_bytes: length,
            tags: Vec::new(),
            actor: context.principal().subject().as_str().to_string(),
            correlation_id: correlation.to_string(),
            overwrite: true,
            precondition: Some(precondition),
            subject: None,
        };
        match writer
            .publish(context.account().as_str(), publication)
            .await
        {
            Ok(outcome) => {
                return Ok(Written {
                    digest: outcome.reference.digest,
                    changed: true,
                });
            }
            Err(CatalogError::PreconditionFailed { .. }) => continue,
            // The engine may arbitrate the race with its guard and report
            // a conflict AFTER another writer moved the pointer
            // between the read and the commit: the loser re-reads and
            // retries, just like the one that loses the precondition. The
            // coordinates of this snapshot carry the digest of THIS
            // attempt, so a version conflict cannot be one.
            Err(CatalogError::Conflict { .. }) => continue,
            Err(error) => return Err(the_publication_error(&error).into()),
        }
    }
    Err(Failure::plain(StatusCode::CONFLICT, CONTENDED))
}

/// The `rev` npm expects to see in write responses.
fn the_rev(digest: &ContentDigest) -> String {
    let short: String = digest.value.chars().take(32).collect();
    format!("1-{short}")
}

fn written_response(name: &PackageName, written: &Written, correlation: &str) -> Response {
    let etag = conditional::the_etag(&written.digest);
    json_response(
        StatusCode::CREATED,
        &json!({ "ok": true, "id": name.full(), "rev": the_rev(&written.digest) }),
        Some(&etag),
        correlation,
    )
}

/// The body of a write, with a cap.
///
/// An `npm publish` carries the tarball in base64 inside the JSON: the cap is that
/// of the content plus the 4/3 of the base64 and a margin for the `package.json`.
async fn read_body(state: &AppState, request: Request<Body>) -> Result<bytes::Bytes, Failure> {
    let limit = state.max_body_size / 3 * 4 + 1024 * 1024 + 1024;
    axum::body::to_bytes(request.into_body(), limit)
        .await
        .map_err(|_| RawApiError::PayloadTooLarge.into())
}

/// The repository's scope policy: applies to every write of a
/// package (publish, deprecate, move or remove a dist-tag).
fn the_scope_gate(repository: &RepositoryDescriptor, name: &PackageName) -> Result<(), Failure> {
    let config = NpmConfig::of_settings(&repository.config.config_settings)
        .map_err(|_| RawApiError::Internal)?;
    if config.admits(name) {
        Ok(())
    } else {
        Err(Failure::plain(
            StatusCode::FORBIDDEN,
            "this repository does not host packages of that scope",
        ))
    }
}

/// What the client reads when the look-alike policy refuses a new name: the
/// matched name and the rule stay in the operator's log.
const TYPOSQUAT_REFUSED: &str = "this repository does not accept that package name";

/// `PUT /npm/{repository}/{name}`: publish a version or deprecate.
async fn put_packument(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    route_name: &PackageName,
    request: Request<Body>,
    correlation: &str,
) -> Result<Response, Failure> {
    the_write_gate(state, context, repository)?;
    // Publishing requires the strict rules for new names.
    let name = PackageName::parse(route_name.full(), true)?;
    the_scope_gate(repository, &name)?;
    let body = read_body(state, request).await?;
    let update = npm::parse_update(&name, &body, &|encoded| {
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .ok()
    })?;
    match update {
        npm::Update::Publish(new) => {
            publish_version(state, context, repository, &name, &new, correlation).await
        }
        npm::Update::Metadata(document) => {
            let outcome = update_packument(
                state,
                context,
                repository,
                &name,
                false,
                correlation,
                |packument, now| Ok(npm::apply_metadata(packument, &document, now)?),
            )
            .await;
            match outcome {
                Ok(written) => {
                    let mut response = written_response(&name, &written, correlation);
                    *response.status_mut() = StatusCode::OK;
                    Ok(response)
                }
                Err(Failure::Npm(NpmError::Unpublish)) => {
                    // Removing a version in a PUT is a disguised unpublish.
                    audit_unpublish(state, context, repository, &name, correlation);
                    Err(Failure::Npm(NpmError::Unpublish))
                }
                Err(other) => Err(other),
            }
        }
    }
}

async fn publish_version(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    name: &PackageName,
    new: &npm::NewVersion,
    correlation: &str,
) -> Result<Response, Failure> {
    let bytes = &new.attachment.bytes;
    // What the service knows about the tarball is what it computes, not what the
    // client says: if the client declares otherwise, it is rejected.
    let facts = npm::TarballFacts {
        shasum: hex_of(&sha1::Sha1::digest(bytes)),
        integrity: format!(
            "sha512-{}",
            base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes))
        ),
    };
    npm::check_declared_dist(&new.manifest, &facts)?;

    // A published version is not rewritten: this is said before writing bytes.
    let existing = the_current_asset(state, context, &repository.repo_key, name.full()).await?;
    // A NEW name is judged against the look-alike policy the repository
    // declares (HSP-322), before any byte is written. A new version of an
    // existing package is the package itself and is not judged.
    if existing.is_none() {
        match crate::typosquat::gate(state, context, repository, name.full(), correlation).await {
            Ok(()) => {}
            Err(crate::typosquat::Refusal::Rejected) => {
                return Err(Failure::plain(StatusCode::FORBIDDEN, TYPOSQUAT_REFUSED));
            }
            Err(crate::typosquat::Refusal::Unavailable) => {
                return Err(RawApiError::Unavailable.into());
            }
        }
    }
    if let Some(current) = existing {
        let stored = the_content_store(state)?
            .get(&current.digest)
            .await
            .map_err(|error| the_blob_error(&error))?;
        let packument = npm::parse_packument(&stored).map_err(|_| RawApiError::Internal)?;
        if npm::has_version(&packument, &new.version) {
            return Err(NpmError::VersionExists(new.version.clone()).into());
        }
    }

    // 1. The tarball, as immutable content at its own path.
    let WrittenContent { digest, length } = write_content(
        state,
        context.account().as_str(),
        Body::from(bytes.clone()),
        None,
        None,
    )
    .await?;
    let (_, writer) = the_catalog(state)?;
    let publication = Publication {
        repository: repository.repo_key.clone(),
        asset_path: name.tarball_path(&new.version),
        coordinates: PackageCoordinates {
            package_name: name.full().to_string(),
            package_version: new.version.clone(),
        },
        digest: digest.clone(),
        size_bytes: length,
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
        Ok(_) => {}
        // Other content at the same version: the version already exists.
        Err(CatalogError::Conflict { .. }) => {
            return Err(NpmError::VersionExists(new.version.clone()).into());
        }
        Err(error) => return Err(the_publication_error(&error).into()),
    }

    // 2. The packument: a new snapshot that points to that tarball.
    let origin_actor = context.principal().subject().as_str().to_string();
    let written = update_packument(
        state,
        context,
        repository,
        name,
        true,
        correlation,
        |packument, now| {
            let origin = npm::Origin {
                publisher: origin_actor.clone(),
                at: now.to_string(),
                correlation: correlation.to_string(),
            };
            npm::add_version(packument, new, &facts, &origin)?;
            Ok(true)
        },
    )
    .await?;
    Ok(written_response(name, &written, correlation))
}

/// `PUT /npm/{repository}/-/package/{name}/dist-tags/{tag}`
async fn put_dist_tag(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    name: &PackageName,
    tag: &str,
    request: Request<Body>,
    correlation: &str,
) -> Result<Response, Failure> {
    the_write_gate(state, context, repository)?;
    the_scope_gate(repository, name)?;
    let body = read_body(state, request).await?;
    // npm sends the version as a JSON string.
    let version: String = serde_json::from_slice(&body).map_err(|_| {
        NpmError::InvalidDocument("the body has to be a JSON string with a version")
    })?;
    npm::valid_version(&version)?;
    let written = update_packument(
        state,
        context,
        repository,
        name,
        false,
        correlation,
        |packument, now| Ok(npm::set_dist_tag(packument, tag, &version, now)?),
    )
    .await?;
    let mut response = written_response(name, &written, correlation);
    *response.status_mut() = StatusCode::OK;
    Ok(response)
}

/// `DELETE /npm/{repository}/-/package/{name}/dist-tags/{tag}`
async fn delete_dist_tag(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    name: &PackageName,
    tag: &str,
    correlation: &str,
) -> Result<Response, Failure> {
    the_write_gate(state, context, repository)?;
    the_scope_gate(repository, name)?;
    let written = update_packument(
        state,
        context,
        repository,
        name,
        false,
        correlation,
        |packument, now| Ok(npm::remove_dist_tag(packument, tag, now)?),
    )
    .await?;
    if !written.changed {
        return Err(Failure::plain(StatusCode::NOT_FOUND, "no such dist-tag"));
    }
    let mut response = written_response(name, &written, correlation);
    *response.status_mut() = StatusCode::OK;
    Ok(response)
}

/// `DELETE .../-rev/{rev}`: nothing is deleted, and the attempt is left in the trail.
fn refuse_unpublish(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    name: &PackageName,
    correlation: &str,
) -> Result<Response, Failure> {
    the_write_gate(state, context, repository)?;
    audit_unpublish(state, context, repository, name, correlation);
    Err(NpmError::Unpublish.into())
}

fn audit_unpublish(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    name: &PackageName,
    correlation: &str,
) {
    record_audit(
        state,
        context.principal().subject().as_str(),
        context.account().as_str(),
        "unpublish",
        &format!("{}/{}", repository.repo_key, name.full()),
        AuditResult::Denied,
        correlation,
    );
}
