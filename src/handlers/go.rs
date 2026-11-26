//! The **go** surface: `go mod download` / `go build` with `GOPROXY`
//! pointing at a Hodei repository.
//!
//! Lives at `/go/{repository}/...` and speaks GOPROXY: the grammar of paths
//! and documents is in `hodei_catalog::gomod` (pure) and here there is only
//! what needs effects.
//!
//! Each published version leaves **three immutable assets** in the catalog,
//! with the same path as the protocol: `{module}/@v/{version}.info`,
//! `.mod` and `.zip`. The version list and `@latest` are assembled from what
//! is stored, like cargo's sparse index: there is no document
//! to rewrite, and a published version is forever.
//!
//! The hosted mode knows two modalities. In **hosted**, the publisher leaves
//! the three artifacts with `PUT` (the `go` protocol cannot write; writing
//! is specific to the registry and uses the same authorization as the
//! rest of the publications). In **proxy**, each read is fetched from the
//! upstream with the cache, the receipts and the single-flight of the
//! shared infrastructure, and served as is: GOPROXY is opaque by
//! contract, and rewriting a zip would break its checksum.

use axum::{
    body::Body,
    extract::{OriginalUri, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::Engine as _;
use hodei_catalog::{
    CatalogError, PackageCoordinates, Publication, RepositoryDescriptor, RepositoryFormat,
    UpstreamConfig,
    gomod::{self, GoError, Route},
};
use serde_json::{Value, json};

use super::raw::{
    RawApiError, apply_cache_headers, apply_common_headers, the_admission_error, the_blob_error,
    the_catalog, the_content_store, the_context, the_current_asset, the_decision,
    the_publication_error, the_repository_in, write_the_content,
};
use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{correlation_id, record_audit};
use crate::raw::conditional;

/// What happens to a request that cannot be served.
#[derive(Debug)]
pub enum Failure {
    /// An error from the shared surface.
    Raw(RawApiError),
    /// An error from the GOPROXY protocol.
    Go(GoError),
    /// A status and a reason.
    Plain(StatusCode, String),
}

impl From<RawApiError> for Failure {
    fn from(error: RawApiError) -> Self {
        Self::Raw(error)
    }
}

impl From<GoError> for Failure {
    fn from(error: GoError) -> Self {
        Self::Go(error)
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
            Self::Go(error) => (
                if error.is_path_error() {
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

/// The rest of the path of `/go/{repository}/{rest}`, undecoded: the
/// `go` tool escapes names by itself, and a `%2f` must not open paths.
fn the_split(uri: &OriginalUri) -> Result<(String, String), Failure> {
    let rest = uri
        .path()
        .strip_prefix("/go/")
        .ok_or(RawApiError::NotFound)?;
    let (repository, rest) = rest.split_once('/').ok_or(RawApiError::NotFound)?;
    if !crate::raw::path::valid_repository_name(repository) || rest.is_empty() {
        return Err(RawApiError::NotFound.into());
    }
    Ok((repository.to_string(), rest.to_string()))
}

/// The `go` client authenticates with Basic from its `.netrc`, and the credential
/// goes in the password: the user is informative. The shared surface
/// only speaks Bearer, so the header is normalized here,
/// just as the cargo channel does with the scheme-less token.
fn normalize_authorization(headers: &mut HeaderMap) {
    let encoded = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .strip_prefix("Basic ")
                .or_else(|| value.strip_prefix("basic "))
                .map(str::to_string)
        })
        .and_then(|value| {
            base64::engine::general_purpose::STANDARD
                .decode(value.trim())
                .ok()
        })
        .and_then(|decoded| String::from_utf8(decoded).ok())
        .and_then(|decoded| {
            decoded
                .split_once(':')
                .filter(|(_, password)| !password.is_empty())
                .map(|(_, password)| password.to_string())
        });
    if let Some(token) = encoded
        && let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}"))
    {
        headers.insert(header::AUTHORIZATION, value);
    }
}

/// Any request to `/go/{repository}/...`.
pub async fn handle(
    State(state): State<AppState>,
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
    let repository =
        the_repository_in(&state, &context, &repository_key, &[RepositoryFormat::Go]).await?;

    let reading = method == Method::GET || method == Method::HEAD;
    let writing = method == Method::PUT;
    let outcome = match (&route, reading, writing) {
        (_, true, _) => {
            read(
                &state,
                &context,
                &repository,
                &route,
                method == Method::HEAD,
                &correlation,
            )
            .await
        }
        (Route::Info { .. } | Route::Mod { .. } | Route::Zip { .. }, false, true) => {
            publish(&state, &context, &repository, &route, request, &correlation).await
        }
        (_, false, true) => Err(RawApiError::MethodNotAllowed(
            "a go version is published with a PUT of its .info, .mod or .zip",
        )
        .into()),
        _ => Err(RawApiError::MethodNotAllowed("this GOPROXY operation is not supported").into()),
    };
    record_outcome(
        &state,
        &context,
        &repository_key,
        route.module(),
        &outcome,
        &correlation,
    );
    outcome
}

fn record_outcome(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &str,
    module: &str,
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
        &format!("{repository}/{module}"),
        result,
        correlation,
    );
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

/// A read is a hop to the proxy or a visit to the own store, and the
/// repository's mode decides: a proxy does not invent versions, and a
/// hosted one does not go out to the network.
async fn read(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    route: &Route,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::download_asset(), repository)?;
    match repository.mode {
        hodei_catalog::RepositoryMode::Proxy => {
            return read_proxy(state, context, repository, route, head, correlation).await;
        }
        // A go group assembles the union of its members' versions
        // and resolves each path by precedence. The precedence, the MISS
        // distinct from ERROR and the namespace reservation are those of the
        // group engine of the raw surface: they are not reimplemented here.
        hodei_catalog::RepositoryMode::Group => {
            return read_group(state, context, repository, route, head, correlation).await;
        }
        _ => {}
    }
    match route {
        Route::List { module } => {
            let versions = hosted_versions(state, context, repository, module).await?;
            if versions.is_empty() {
                // The client distinguishes "unknown module" from "module without
                // published versions": the first is a 404.
                return Err(Failure::plain(
                    StatusCode::NOT_FOUND,
                    "there is no module of that name in this repository",
                ));
            }
            let body = versions
                .iter()
                .map(|(_, version)| format!("{version}\n"))
                .collect::<String>();
            Ok(text_response(body, head, correlation))
        }
        Route::Latest { module } => {
            let versions = hosted_versions(state, context, repository, module).await?;
            let Some((_, latest)) = versions.last() else {
                return Err(Failure::plain(
                    StatusCode::NOT_FOUND,
                    "this module has no published versions",
                ));
            };
            read_asset_named(
                state,
                context,
                repository,
                &format!("{module}/@v/{latest}.info"),
                gomod::content_type("info"),
                head,
                correlation,
            )
            .await
        }
        Route::Info { .. } | Route::Mod { .. } | Route::Zip { .. } => {
            let path = route.asset_path().expect("a version route names its asset");
            let extension = path.rsplit('.').next().unwrap_or_default();
            read_asset_named(
                state,
                context,
                repository,
                &path,
                gomod::content_type(extension),
                head,
                correlation,
            )
            .await
        }
    }
}

/// The stored versions of a module, ordered by semver.
async fn hosted_versions(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    module: &str,
) -> Result<Vec<(semver::Version, String)>, Failure> {
    let (reader, _) = the_catalog(state)?;
    let records = reader
        .assets_under(
            context.account().as_str(),
            &repository.repo_key,
            &format!("{module}/@v/"),
        )
        .await
        .map_err(|error| Failure::from(the_publication_error_ref(&error)))?;
    let mut versions: Vec<(semver::Version, String)> = Vec::new();
    for record in records {
        // Only .info files count as versions: a bare .mod without .info
        // is not a published version as far as the protocol is concerned.
        if let Some(version) = record
            .asset_path
            .strip_prefix(&format!("{module}/@v/"))
            .and_then(|file| file.strip_suffix(".info"))
            && let Some(parsed) = gomod::parse_version(version)
        {
            versions.push((parsed, version.to_string()));
        }
    }
    versions.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(versions)
}

/// The bytes of a published asset, verified against the catalog, with its
/// protocol content type.
async fn read_asset_named(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    asset_path: &str,
    content_type: &str,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    let resolved =
        super::raw::the_resolved_asset(state, context, repository.clone(), asset_path, correlation)
            .await?;
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
    let mut response = body_response(StatusCode::OK, bytes, content_type, &etag, correlation);
    apply_cache_headers(&mut response, &served);
    if head && let Ok(value) = HeaderValue::from_str(&length.to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    Ok(response)
}

// ---------------------------------------------------------------------------
// Group
// ---------------------------------------------------------------------------

/// The scope policy of a group member's content.
///
/// This release only serves the global scope, so a partitioned-scope
/// member is **not** served. Its failure is an ERROR that stops the
/// group resolution, not a MISS that passes to the next member: it is the
/// semantics and the message of the raw surface, and two surfaces
/// cannot answer differently for the same repository.
fn the_member_scope(member: &RepositoryDescriptor) -> Result<(), Failure> {
    super::raw_group::global_only(member)?;
    Ok(())
}

/// What a group member offers for a module.
enum Offering {
    /// The member has the module, with these versions.
    Versions(Vec<(semver::Version, String)>),
    /// The member does not have it: a 404 from the upstream, a MISS.
    Missing,
    /// The member is down: no network and no cached copy.
    ///
    /// A down member is just one more MISS: the next one is tried, and the
    /// error surfaces only if **nothing** responded.
    Offline(RawApiError),
}

/// The leaves the group lets through for a module, with the namespace
/// reservation already applied.
///
/// The reservation is consulted **before** the members: a reserved module is
/// not requested from the group's public upstream, and the hosted members are the
/// internal source and it does not touch it.
async fn the_governed(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    group: &RepositoryDescriptor,
    module: &str,
) -> Result<
    (
        Vec<RepositoryDescriptor>,
        Vec<String>,
        Option<hodei_catalog::Judgement>,
    ),
    Failure,
> {
    super::raw_group::the_governed_leaves(state, context, group, module)
        .await
        .map_err(Failure::from)
}

/// The bytes a proxy member fetches from its upstream.
///
/// The path is that of the raw surface: the cache, the receipts, the
/// single-flight and the offline mode are the same, and what is downloaded is published
/// in the member's namespace.
async fn the_proxy_bytes(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    member: &RepositoryDescriptor,
    asset_path: &str,
    correlation: &str,
) -> Result<Vec<u8>, Failure> {
    let served =
        super::raw::the_proxy_asset(state, context, member, asset_path, correlation).await?;
    let bytes = the_content_store(state)?
        .get(&served.asset.digest)
        .await
        .map_err(|error| the_blob_error(&error))?
        .to_vec();
    Ok(bytes)
}

/// The versions of a member, with the same rule as a hosted one: a
/// version is a version if it has `.info`.
///
/// A proxy member asks its upstream for the **module name**, not
/// the group name: the group does not rename its member.
async fn the_member_versions(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    member: &RepositoryDescriptor,
    module: &str,
    correlation: &str,
) -> Result<Offering, Failure> {
    if member.mode == hodei_catalog::RepositoryMode::Proxy {
        let asset_path = format!("{module}/@v/list");
        let bytes = match the_proxy_bytes(state, context, member, &asset_path, correlation).await {
            Ok(bytes) => bytes,
            // A 404 from the upstream is a MISS: the next member
            // is tried. A down member is also a MISS: the next one is
            // tried, and the error surfaces only if nothing responded.
            // Any other failure is an ERROR and stops the resolution: a
            // transient failure cannot silently change what is
            // downloaded.
            Err(Failure::Raw(RawApiError::NotFound)) => return Ok(Offering::Missing),
            Err(Failure::Raw(RawApiError::Offline(reason))) => {
                return Ok(Offering::Offline(RawApiError::Offline(reason)));
            }
            Err(other) => return Err(other),
        };
        let text = String::from_utf8_lossy(&bytes);
        let mut versions = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if let Some(parsed) = gomod::parse_version(line) {
                versions.push((parsed, line.to_string()));
            }
        }
        return Ok(Offering::Versions(versions));
    }
    Ok(Offering::Versions(
        hosted_versions(state, context, member, module).await?,
    ))
}

/// The union of the versions of the group's members, in semver order.
///
/// The union is not destructive: if two members have the same version, it appears
/// **once**. A down upstream is a MISS: the next member is tried,
/// and its error surfaces only if **nothing** responded.
async fn the_group_versions(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    leaves: &[RepositoryDescriptor],
    module: &str,
    correlation: &str,
) -> Result<Vec<(semver::Version, String)>, Failure> {
    let mut versions: Vec<(semver::Version, String)> = Vec::new();
    // The first down member: its error is remembered so as to surface it if
    // none responded in the end.
    let mut offline: Option<RawApiError> = None;
    for member in leaves {
        // The scope is checked **before** querying the member: a
        // partitioned-scope member is not served, and its error stops the
        // group resolution.
        the_member_scope(member)?;
        match the_member_versions(state, context, member, module, correlation).await? {
            Offering::Versions(found) => {
                for entry in found {
                    if !versions.iter().any(|(_, known)| known == &entry.1) {
                        versions.push(entry);
                    }
                }
            }
            Offering::Missing => {}
            Offering::Offline(error) => {
                if offline.is_none() {
                    offline = Some(error);
                }
            }
        }
    }
    // If nothing responded, the error of the one that is down is the verdict: an
    // empty listing cannot be the answer for a module that exists.
    if versions.is_empty()
        && let Some(error) = offline
    {
        return Err(Failure::Raw(error));
    }
    versions.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(versions)
}

/// The digest the catalog knows for an artifact of a version.
///
/// `None` means that member does not have (or has not downloaded) the artifact: it is not
/// compared with anyone.
async fn the_known_digest(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    member: &RepositoryDescriptor,
    asset_path: &str,
) -> Result<Option<String>, Failure> {
    Ok(
        super::raw::the_current_asset(state, context, &member.repo_key, asset_path)
            .await?
            .map(|asset| asset.digest.to_string()),
    )
}

/// The divergence between members: the same version with different content.
///
/// The go protocol has no way of saying "there are two": the client cannot
/// know which of the two is the right one. That is why the group **rejects** with a
/// defined error instead of choosing silently.
///
/// The `.mod` and `.zip` are compared, which are the two digests the
/// client writes in its `go.sum`. Only members whose content the catalog
/// knows are compared; one that has not yet downloaded that version is not
/// compared, and there precedence decides.
async fn the_divergence(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    leaves: &[RepositoryDescriptor],
    module: &str,
    version: &str,
) -> Result<Option<String>, Failure> {
    for extension in ["mod", "zip"] {
        let asset_path = format!("{module}/@v/{version}.{extension}");
        let mut seen: Option<(String, String)> = None;
        for member in leaves {
            // The same scope as in the listing: a partitioned-scope member
            // is not compared with anyone, and its error stops the
            // resolution.
            the_member_scope(member)?;
            let Some(digest) = the_known_digest(state, context, member, &asset_path).await? else {
                continue;
            };
            match &seen {
                None => seen = Some((member.repo_key.clone(), digest)),
                Some((known_key, known_digest)) if known_digest != &digest => {
                    return Ok(Some(format!(
                        "the version {version} of {module} has different content in the members {known_key} and {}",
                        member.repo_key
                    )));
                }
                Some(_) => {}
            }
        }
    }
    Ok(None)
}

/// The state of a go group with respect to a module.
///
/// These are the leaves the namespace reservation lets through, the members it
/// left out and the reservation's verdict: the resolution always consults them,
/// and they are not re-queried for each artifact.
struct TheGroup<'a> {
    /// The group repository.
    group: &'a RepositoryDescriptor,
    /// The leaves that are queried, in precedence order.
    leaves: Vec<RepositoryDescriptor>,
    /// The proxy members the namespace reservation left out.
    denied: Vec<String>,
    /// The reservation's verdict, if there was one.
    judgement: Option<hodei_catalog::Judgement>,
}

/// The member that owns a version: the first, in the configured order, that
/// has it.
///
/// The version is recognized by its `.info`, which is the same criterion used by
/// `hosted_versions`: an `.info` with a parseable version is a version.
///
/// The owner decides **what** is served: the three artifacts (`.info`, `.mod` and
/// `.zip`) come from that same member. The `.mod` of one member is never mixed
/// with the `.zip` of another, because a `go.sum` that does not match is a
/// client that fails with an error that explains nothing.
async fn the_owner_of(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    group: &TheGroup<'_>,
    module: &str,
    version: &str,
    correlation: &str,
) -> Result<RepositoryDescriptor, Failure> {
    // The first down member: a member without network and without a cached copy does not
    // have the version yet, so the next one is tried. Its error
    // surfaces only if **no** member owns it.
    let mut offline: Option<RawApiError> = None;
    for member in &group.leaves {
        // The owner is decided with the same scope policy: a
        // partitioned-scope member cannot be the owner of a version, and its
        // error stops the resolution.
        the_member_scope(member)?;
        let asset_path = format!("{module}/@v/{version}.info");
        let has = if member.mode == hodei_catalog::RepositoryMode::Proxy {
            // A proxy member responds with its upstream: a 404 is a MISS
            // (the member does not have that version), a down member is also
            // a MISS, and anything else is an ERROR that stops the
            // resolution.
            match the_proxy_bytes(state, context, member, &asset_path, correlation).await {
                Ok(_) => true,
                Err(Failure::Raw(RawApiError::NotFound)) => false,
                Err(Failure::Raw(RawApiError::Offline(reason))) => {
                    if offline.is_none() {
                        offline = Some(RawApiError::Offline(reason));
                    }
                    false
                }
                Err(other) => return Err(other),
            }
        } else {
            super::raw::the_current_asset(state, context, &member.repo_key, &asset_path)
                .await?
                .is_some()
        };
        if has {
            return Ok(member.clone());
        }
    }
    // A down member wins over the reservation explanation: the verdict
    // says it could not be known, not that the namespace was reserved.
    if let Some(error) = offline {
        return Err(Failure::Raw(error));
    }
    // A reserved case does not fall back: it explains why it is not served.
    if !group.denied.is_empty()
        && let Some(judgement) = &group.judgement
    {
        return Err(super::raw_group::the_group_reservation_explanation(
            group.group,
            judgement,
            &group.denied,
        )
        .into());
    }
    Err(Failure::plain(
        StatusCode::NOT_FOUND,
        "this module has no such version in this group",
    ))
}

/// What a go group serves at a path.
///
/// A group is not a proxy under another name: it assembles the **union** of the
/// versions of its members and resolves each version by precedence.
async fn read_group(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    group: &RepositoryDescriptor,
    route: &Route,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    let module = route.module().to_string();
    let (leaves, denied, judgement) = the_governed(state, context, group, &module).await?;
    let governed = TheGroup {
        group,
        leaves,
        denied,
        judgement,
    };
    match route {
        Route::List { .. } => {
            let versions =
                the_group_versions(state, context, &governed.leaves, &module, correlation).await?;
            if versions.is_empty() {
                // A reserved case does not fall back: it explains why it is not
                // served instead of looking like a module that does not exist.
                if !governed.denied.is_empty()
                    && let Some(judgement) = &governed.judgement
                {
                    return Err(super::raw_group::the_group_reservation_explanation(
                        governed.group,
                        judgement,
                        &governed.denied,
                    )
                    .into());
                }
                return Err(Failure::plain(
                    StatusCode::NOT_FOUND,
                    "there is no module of that name in this group",
                ));
            }
            let body = versions
                .iter()
                .map(|(_, version)| format!("{version}\n"))
                .collect::<String>();
            Ok(text_response(body, head, correlation))
        }
        Route::Latest { .. } => {
            // A group's "latest" is the latest of the **union**, not that of the
            // last member to respond.
            let versions =
                the_group_versions(state, context, &governed.leaves, &module, correlation).await?;
            let Some((_, latest)) = versions.last() else {
                if !governed.denied.is_empty()
                    && let Some(judgement) = &governed.judgement
                {
                    return Err(super::raw_group::the_group_reservation_explanation(
                        governed.group,
                        judgement,
                        &governed.denied,
                    )
                    .into());
                }
                return Err(Failure::plain(
                    StatusCode::NOT_FOUND,
                    "this module has no published versions",
                ));
            };
            let owned = Route::Info {
                module: module.clone(),
                version: latest.clone(),
            };
            serve_owned(state, context, &governed, &owned, head, correlation).await
        }
        Route::Info { .. } | Route::Mod { .. } | Route::Zip { .. } => {
            serve_owned(state, context, &governed, route, head, correlation).await
        }
    }
}

/// Serves a version artifact from the member that owns it.
async fn serve_owned(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    group: &TheGroup<'_>,
    route: &Route,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    let (module, version) = match route {
        Route::Info { module, version }
        | Route::Mod { module, version }
        | Route::Zip { module, version } => (module.clone(), version.clone()),
        _ => return Err(RawApiError::NotFound.into()),
    };
    if let Some(conflict) = the_divergence(state, context, &group.leaves, &module, &version).await?
    {
        // The divergence is rejected with a defined error: the client can
        // distinguish "I do not have it" from "this is in conflict".
        return Err(Failure::plain(StatusCode::CONFLICT, &conflict));
    }
    let owner = the_owner_of(state, context, group, &module, &version, correlation).await?;
    match owner.mode {
        hodei_catalog::RepositoryMode::Proxy => {
            read_proxy(state, context, &owner, route, head, correlation).await
        }
        _ => {
            let path = route.asset_path().expect("a version route names its asset");
            let extension = path.rsplit('.').next().unwrap_or_default();
            read_asset_named(
                state,
                context,
                &owner,
                &path,
                gomod::content_type(extension),
                head,
                correlation,
            )
            .await
        }
    }
}

// ---------------------------------------------------------------------------
// Proxy
// ---------------------------------------------------------------------------

/// A GOPROXY proxy serves what the upstream says, byte for byte: the list,
/// the document, the go.mod and the zip are cached under their protocol path
/// and re-served without rewriting anything.
async fn read_proxy(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    route: &Route,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    let upstream = UpstreamConfig::of(&repository.config)
        .ok()
        .flatten()
        .ok_or_else(|| Failure::plain(StatusCode::BAD_GATEWAY, "the proxy is not configured"))?;
    let (asset_path, content_type) = match route {
        Route::List { module } => (format!("{module}/@v/list"), gomod::content_type("list")),
        Route::Latest { module } => (format!("{module}/@latest"), gomod::content_type("info")),
        Route::Info { .. } | Route::Mod { .. } | Route::Zip { .. } => {
            let extension = match route {
                Route::Mod { .. } => "mod",
                Route::Zip { .. } => "zip",
                _ => "info",
            };
            (
                route.asset_path().expect("a version route names its asset"),
                gomod::content_type(extension),
            )
        }
    };
    let url = upstream.target(&asset_path).ok_or(RawApiError::NotFound)?;
    let served = super::raw::the_proxy_asset_at(
        state,
        context,
        repository,
        &asset_path,
        Some(url),
        Some("application/octet-stream"),
        correlation,
    )
    .await?;
    // The trust gate: a proxy is also a repository, and its
    // channel decides whether that digest may be served. Without this question, a
    // proxy on the trusted channel would serve content without a grant.
    crate::trust::admit(
        state,
        context.account().as_str(),
        repository,
        &served.asset.digest,
    )
    .await
    .map_err(the_admission_error)?;
    let bytes = if head {
        Vec::new()
    } else {
        the_content_store(state)?
            .get(&served.asset.digest)
            .await
            .map_err(|error| the_blob_error(&error))?
            .to_vec()
    };
    let length = if head {
        served.asset.size_bytes as usize
    } else {
        bytes.len()
    };
    let etag = conditional::the_etag(&served.asset.digest);
    let mut response = body_response(StatusCode::OK, bytes, content_type, &etag, correlation);
    apply_cache_headers(&mut response, &Some((served.status, served.provenance)));
    if head && let Ok(value) = HeaderValue::from_str(&length.to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    Ok(response)
}

// ---------------------------------------------------------------------------
// Publishing (hosted)
// ---------------------------------------------------------------------------

/// Publish a version artifact. Immutability rules: the same
/// digest is a yes, a different digest is a 409 that explains why.
async fn publish(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    route: &Route,
    request: axum::extract::Request<Body>,
    correlation: &str,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::upload_asset(), repository)?;
    if !repository.mode.accepts_client_writes() {
        return Err(
            RawApiError::MethodNotAllowed("this repository does not accept uploads").into(),
        );
    }
    let asset_path = route.asset_path().expect("a version route names its asset");
    let extension = asset_path.rsplit('.').next().unwrap_or_default();
    let limit = state.max_body_size.saturating_add(1024 * 1024);
    let body = axum::body::to_bytes(request.into_body(), limit)
        .await
        .map_err(|_| Failure::Raw(RawApiError::PayloadTooLarge))?;

    let (module, version) = match route {
        Route::Info { module, version }
        | Route::Mod { module, version }
        | Route::Zip { module, version } => (module.clone(), version.clone()),
        _ => return Err(RawApiError::NotFound.into()),
    };
    match extension {
        "info" => gomod::check_info(&body, &version)?,
        "mod" => gomod::check_mod(&body, &module)?,
        "zip" => {
            if body.is_empty() {
                return Err(GoError::InvalidDocument("the module zip is empty".into()).into());
            }
        }
        _ => return Err(RawApiError::NotFound.into()),
    }

    let (digest, length) = write_the_content(
        state,
        context.account().as_str(),
        Body::from(body.to_vec()),
        None,
        None,
    )
    .await?;

    // An already-published version is forever: the same digest is the echo
    // of an interrupted publication, a different digest is a 409 that
    // explains why.
    let immutable = format!(
        "version {version} of {module} is already published; a published version is immutable"
    );
    if let Some(existing) =
        the_current_asset(state, context, &repository.repo_key, &asset_path).await?
        && existing.digest != digest
    {
        return Err(Failure::plain(StatusCode::CONFLICT, &immutable));
    }

    let (_, writer) = the_catalog(state)?;
    // The zip is the content of the version; the `.info` and the `.mod` are its
    // metadata. The catalog keeps one version row per coordinates,
    // so the metadata is registered under its own name, just as
    // cargo does with its index: otherwise, publishing the `.mod` would be
    // rewriting the version the `.zip` just fixed.
    let package_name = match extension {
        "zip" => module.clone(),
        _ => format!("{module}#{extension}"),
    };
    let publication = Publication {
        repository: repository.repo_key.clone(),
        asset_path: asset_path.clone(),
        coordinates: PackageCoordinates {
            package_name,
            package_version: version.clone(),
        },
        digest,
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
        Ok(_) => Ok(json_response(
            StatusCode::OK,
            &json!({ "ok": true }),
            None,
            correlation,
        )),
        Err(CatalogError::Conflict { .. }) => Err(Failure::plain(StatusCode::CONFLICT, &immutable)),
        Err(error) => Err(the_publication_error(&error).into()),
    }
}

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

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

/// The hosted list and `@latest`: text with its real size, and
/// `go` does not ask for an ETag for the list but appreciates the nosniff.
fn text_response(body: String, head: bool, correlation: &str) -> Response {
    let length = body.len();
    let mut response = body_response(
        StatusCode::OK,
        if head { Vec::new() } else { body.into_bytes() },
        gomod::content_type("list"),
        "-",
        correlation,
    );
    if head && let Ok(value) = HeaderValue::from_str(&length.to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    response
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

/// The catalog error of a read: a read does not cause conflicts,
/// but the translator knows what each variant means.
fn the_publication_error_ref(error: &CatalogError) -> RawApiError {
    match error {
        CatalogError::Denied { .. } => RawApiError::Forbidden,
        CatalogError::Missing { .. } => RawApiError::NotFound,
        CatalogError::Unavailable { .. } | CatalogError::Corrupt { .. } => RawApiError::Internal,
        other => RawApiError::BadRequest(Box::leak(other.to_string().into_boxed_str())),
    }
}
