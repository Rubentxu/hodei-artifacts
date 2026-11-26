//! The **NuGet v3** surface: `dotnet restore` and `dotnet nuget push`
//! against a Hodei repository.
//!
//! Lives at `/nuget/{repository}/...` and speaks the grammar of the
//! resource hub: the service index, the flat container, the registration
//! leaf and the publish endpoint. The pure grammar --paths,
//! versions, package identity and documents-- is in
//! `hodei_catalog::nuget`; here there is only what needs effects.
//!
//! Each published version leaves **a single immutable asset** in the
//! catalog, with its canonical protocol address:
//! `flatcontainer/{id}/{version}/{id}.nupkg`. The version list and
//! the registration leaf are assembled from what is stored, like cargo's
//! sparse index: there is no document to rewrite, and a published
//! version is forever. The package metadata is not duplicated:
//! it lives inside the nupkg itself, and the server reads it on publish
//! instead of rewriting it.
//!
//! The **hosted** mode writes via `PUT package/{id}/{version}` with
//! the nupkg as body; the identity the URL carries is a promise,
//! and the nuspec's wins: if they do not match, the publication is rejected.
//! The **proxy** mode re-serves the flat container and the registrations
//! byte for byte: a rewritten nupkg breaks its hash, and NuGet validates the
//! package against the registration's `contentHash`.

use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{OriginalUri, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::Engine as _;
use hodei_catalog::{
    CatalogError, PackageCoordinates, Publication, RepositoryDescriptor, RepositoryFormat,
    RepositoryMode, UpstreamConfig,
    nuget::{self, FlatRequest, NuGetRoute, NuGetVersion, NugetError},
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
use crate::public_url::TheOrigin;
use crate::raw::conditional;

/// What happens to a request that cannot be served.
#[derive(Debug)]
pub enum Failure {
    /// An error from the shared surface.
    Raw(RawApiError),
    /// An error from the NuGet grammar.
    NuGet(NugetError),
    /// A status and a reason.
    Plain(StatusCode, String),
}

impl From<RawApiError> for Failure {
    fn from(error: RawApiError) -> Self {
        Self::Raw(error)
    }
}

impl From<NugetError> for Failure {
    fn from(error: NugetError) -> Self {
        Self::NuGet(error)
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
            Self::NuGet(error) => (
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
            // The challenge is Basic, not Bearer: NuGet only attaches the
            // source credentials when the 401 requests them with that
            // scheme, and the hub dialect already accepts Basic with the
            // key as the password.
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                HeaderValue::from_static("Basic realm=\"hodei\""),
            );
        }
        response
    }
}

/// The rest of the path of `/nuget/{repository}/{rest}`, undecoded:
/// a package identifier may carry `%2e` and a `%2f` must not
/// open paths.
fn the_split(uri: &OriginalUri) -> Result<(String, String), Failure> {
    let rest = uri
        .path()
        .strip_prefix("/nuget/")
        .ok_or(RawApiError::NotFound)?;
    let (repository, rest) = match rest.split_once('/') {
        Some((repository, rest)) => (repository, rest),
        // A bare `/nuget/{repository}` is also the service index.
        None => (rest, ""),
    };
    if !crate::raw::path::valid_repository_name(repository) {
        return Err(RawApiError::NotFound.into());
    }
    Ok((repository.to_string(), rest.to_string()))
}

/// The base URL of the feed as the client sees it: the service index
///
/// The origin is decided by the deployment and not by the request: the `Host` and
/// `X-Forwarded-Proto` of a request can be written by anyone, and
/// a URL the client moves is a URL the client poisons.
/// declares absolute addresses, and absolute for the host that asked.
fn the_base(origin: &TheOrigin, repository: &str) -> Result<String, Failure> {
    origin
        .url(&format!("/nuget/{repository}"))
        .ok_or(RawApiError::BadRequest(
            "the public address of the registry is unknown: the Host header is missing or invalid",
        ))
        .map_err(Failure::from)
}

/// The NuGet client arrives in two ways: with Basic from its
/// source credentials (the password is the key, the user is
/// informative) or with the `X-NuGet-ApiKey` header that
/// `dotnet nuget push` sends. The API key may travel base64 by contract
/// of the client; the shared surface only speaks Bearer, so
/// both headers are normalized here.
fn normalize_authorization(headers: &mut HeaderMap) {
    let basic = headers
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
    let api_key = headers
        .get("x-nuget-apikey")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            // The client allows the key in base64: if it decodes to something
            // readable, that is what the server must believe.
            match base64::engine::general_purpose::STANDARD.decode(value) {
                Ok(decoded) => String::from_utf8(decoded)
                    .map(|decoded| {
                        decoded
                            .split_once(':')
                            .map_or(value.to_string(), |(_, password)| password.to_string())
                    })
                    .unwrap_or_else(|_| value.to_string()),
                Err(_) => value.to_string(),
            }
        });
    let token = basic.or(api_key);
    if let Some(token) = token
        && let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}"))
    {
        headers.insert(header::AUTHORIZATION, value);
    }
}

/// The identity the nuspec declared for a successful publication,
/// so that the audit names the package even if the URL did not.
#[derive(Clone)]
struct PublishedId(String);

/// Any request to `/nuget/{repository}/...`.
pub async fn handle(
    State(state): State<AppState>,
    origin: TheOrigin,
    method: Method,
    uri: OriginalUri,
    mut headers: HeaderMap,
    request: axum::extract::Request<Body>,
) -> Result<Response, Failure> {
    normalize_authorization(&mut headers);
    // The service index is anonymous by client contract: `dotnet
    // nuget push` loads it **without credentials** before sending the API
    // key to the publish endpoint. The document does not touch the catalog:
    // it says where each protocol of the repository lives, and nothing of
    // content. Any other request without credentials continues
    // dying in the context, as always.
    if !headers.contains_key(header::AUTHORIZATION) && matches!(method, Method::GET | Method::HEAD)
    {
        let anonymous = (|| -> Result<Option<Response>, Failure> {
            let (repository_key, rest) = the_split(&uri)?;
            if matches!(nuget::parse_route(&rest)?, NuGetRoute::ServiceIndex) {
                Ok(Some(json_response(
                    StatusCode::OK,
                    &nuget::service_index(&the_base(&origin, &repository_key)?),
                    None,
                    &correlation_id(&headers, &state.identity),
                )))
            } else {
                Ok(None)
            }
        })();
        // An error here is not swallowed: if the request carries no credentials
        // and the service index cannot be built --because the `Host` is not
        // an authority-- the response says that, and is not disguised as
        // a 401 from a path that was no longer this one.
        match anonymous {
            Ok(Some(response)) => return Ok(response),
            Ok(None) => {}
            Err(failure) => return Err(failure),
        }
    }
    let context = the_context(&state, &headers).await?;
    let correlation = correlation_id(&headers, &state.identity);
    let (repository_key, rest) = the_split(&uri)?;
    let route = nuget::parse_route(&rest)?;
    let repository = the_repository_in(
        &state,
        &context,
        &repository_key,
        &[RepositoryFormat::NuGet],
    )
    .await?;

    let reading = method == Method::GET || method == Method::HEAD;
    let writing = method == Method::PUT;
    let outcome = match (&route, reading, writing) {
        (NuGetRoute::ServiceIndex, true, _) => Ok(json_response(
            StatusCode::OK,
            &nuget::service_index(&the_base(&origin, &repository_key)?),
            None,
            &correlation,
        )),
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
        (NuGetRoute::Publish { .. }, false, true) => {
            publish(&state, &context, &repository, &route, request, &correlation).await
        }
        (_, false, true) => Err(RawApiError::MethodNotAllowed(
            "a nuget package is published with a PUT of its nupkg to the publish endpoint",
        )
        .into()),
        _ => Err(RawApiError::MethodNotAllowed("this NuGet operation is not supported").into()),
    };
    let audited = outcome
        .as_ref()
        .ok()
        .and_then(|response| response.extensions().get::<PublishedId>().cloned())
        .map(|published| published.0)
        .unwrap_or_else(|| route.package().unwrap_or("index").to_string());
    record_outcome(
        &state,
        &context,
        &repository_key,
        &audited,
        &outcome,
        &correlation,
    );
    outcome
}

fn record_outcome(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &str,
    package: &str,
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
        &format!("{repository}/{package}"),
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
    route: &NuGetRoute,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::download_asset(), repository)?;
    match repository.mode {
        RepositoryMode::Proxy => {
            return read_proxy(state, context, repository, route, head, correlation).await;
        }
        RepositoryMode::Group => {
            return read_group(state, context, repository, route, head, correlation).await;
        }
        _ => {}
    }
    match route {
        NuGetRoute::Flat(FlatRequest::Versions { id }) => {
            let versions = hosted_versions(state, context, repository, id).await?;
            if versions.is_empty() {
                return Err(Failure::plain(
                    StatusCode::NOT_FOUND,
                    "there is no package of that name in this repository",
                ));
            }
            Ok(json_response(
                StatusCode::OK,
                &nuget::version_index(&versions.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>()),
                None,
                correlation,
            ))
        }
        NuGetRoute::Registration { id } => {
            let versions = hosted_versions(state, context, repository, id).await?;
            if versions.is_empty() {
                return Err(Failure::plain(
                    StatusCode::NOT_FOUND,
                    "there is no package of that name in this repository",
                ));
            }
            Ok(json_response(
                StatusCode::OK,
                &nuget::registration_leaf(
                    id,
                    &versions.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>(),
                ),
                None,
                correlation,
            ))
        }
        NuGetRoute::Flat(FlatRequest::File { id, version, file }) => {
            // In the flat container there is only what was published: a
            // `PUT` leaves the nupkg, and any other name under that
            // version names nothing. The contract admits two forms:
            // the canonical address `{id}.nupkg` and the one the client
            // actually requests when downloading, `{id}.{version}.nupkg`.
            if *file != format!("{id}.nupkg") && *file != format!("{id}.{version}.nupkg") {
                return Err(Failure::plain(
                    StatusCode::NOT_FOUND,
                    "the flat container holds the nupkg of a published version, and nothing else",
                ));
            }
            read_asset_named(
                state,
                context,
                repository,
                &nuget::package_asset_path(id, version),
                "application/octet-stream",
                head,
                correlation,
            )
            .await
        }
        _ => Err(RawApiError::NotFound.into()),
    }
}

/// The stored versions of a package, ordered by NuGet's
/// rule: numbers first and prereleases before their base version.
async fn hosted_versions(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    id: &str,
) -> Result<Vec<(nuget::NuGetVersion, String)>, Failure> {
    let (reader, _) = the_catalog(state)?;
    let records = reader
        .assets_under(
            context.account().as_str(),
            &repository.repo_key,
            &format!("flatcontainer/{id}/"),
        )
        .await
        .map_err(|error| Failure::from(the_publication_error(&error)))?;
    let mut versions: Vec<(nuget::NuGetVersion, String)> = Vec::new();
    for record in records {
        let Some(version) = record
            .asset_path
            .strip_prefix(&format!("flatcontainer/{id}/"))
            .and_then(|rest| rest.strip_suffix(&format!("/{id}.nupkg")))
        else {
            continue;
        };
        if let Some(parsed) = nuget::parse_version(version) {
            versions.push((parsed, version.to_string()));
        }
    }
    versions.sort_by(|left, right| left.0.compare(&right.0));
    versions.dedup_by(|left, right| left.1 == right.1);
    Ok(versions)
}

/// The bytes of a published asset, verified against the catalog, with
/// its protocol content type.
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

/// What a group member offers for a package id.
enum Offering {
    /// The member has these versions of the package.
    Versions(Vec<(NuGetVersion, String)>),
    /// The member does not have the package (upstream 404).
    Missing,
    /// The member is offline with no cached copy.
    Offline(RawApiError),
}

/// Governed state of a NuGet group for a given package id.
struct NuGetGroup<'a> {
    group: &'a RepositoryDescriptor,
    leaves: Vec<RepositoryDescriptor>,
    denied: Vec<String>,
    judgement: Option<hodei_catalog::Judgement>,
}

/// Resolves the governed leaves of a NuGet group for a package id,
/// applying namespace reservation before consulting any member.
async fn the_governed<'a>(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    group: &'a RepositoryDescriptor,
    id: &str,
) -> Result<NuGetGroup<'a>, Failure> {
    let (leaves, denied, judgement) =
        super::raw_group::the_governed_leaves(state, context, group, id)
            .await
            .map_err(Failure::from)?;
    Ok(NuGetGroup {
        group,
        leaves,
        denied,
        judgement,
    })
}

/// Fetches the versions of a package from a single member (hosted or proxy).
async fn member_versions(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    member: &RepositoryDescriptor,
    id: &str,
    correlation: &str,
) -> Result<Offering, Failure> {
    if member.mode == RepositoryMode::Proxy {
        let route = NuGetRoute::Flat(FlatRequest::Versions { id: id.to_string() });
        let bytes = match proxy_bytes(state, context, member, &route, correlation).await {
            Ok(bytes) => bytes,
            Err(Failure::Raw(RawApiError::NotFound)) => return Ok(Offering::Missing),
            Err(Failure::Raw(RawApiError::Offline(reason))) => {
                return Ok(Offering::Offline(RawApiError::Offline(reason)));
            }
            Err(other) => return Err(other),
        };
        let doc: Value = serde_json::from_slice(&bytes).map_err(|_| {
            Failure::plain(
                StatusCode::BAD_GATEWAY,
                "the upstream flat container index is not valid JSON",
            )
        })?;
        let mut versions = Vec::new();
        if let Some(entries) = doc.get("versions").and_then(Value::as_array) {
            for raw in entries.iter().filter_map(Value::as_str) {
                if let Some(parsed) = nuget::parse_version(raw) {
                    let normalized = parsed.normalized();
                    versions.push((parsed, normalized));
                }
            }
        }
        return Ok(Offering::Versions(versions));
    }
    Ok(Offering::Versions(
        hosted_versions(state, context, member, id).await?,
    ))
}

/// Computes the union of versions across all governed leaves, attributing each
/// version to the first member in precedence order that offers it.
async fn group_version_owners(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    governed: &NuGetGroup<'_>,
    id: &str,
    correlation: &str,
) -> Result<Vec<(NuGetVersion, String, RepositoryDescriptor)>, Failure> {
    let mut owners: Vec<(NuGetVersion, String, RepositoryDescriptor)> = Vec::new();
    let mut offline: Option<RawApiError> = None;

    for member in &governed.leaves {
        match member_versions(state, context, member, id, correlation).await? {
            Offering::Versions(found) => {
                for (parsed, normalized) in found {
                    if !owners.iter().any(|(_, known, _)| known == &normalized) {
                        owners.push((parsed, normalized, member.clone()));
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

    if owners.is_empty() {
        if let Some(error) = offline {
            return Err(Failure::Raw(error));
        }
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
            "there is no package of that name in this group",
        ));
    }

    owners.sort_by(|left, right| left.0.compare(&right.0));
    Ok(owners)
}

/// Finds the deterministic owner (first member in configured precedence order)
/// for a specific `(id, version)`.
async fn the_owner_of(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    governed: &NuGetGroup<'_>,
    id: &str,
    version: &str,
    correlation: &str,
) -> Result<RepositoryDescriptor, Failure> {
    let Some(target) = nuget::normalize_version(version) else {
        return Err(Failure::plain(
            StatusCode::NOT_FOUND,
            "this package has no such version in this group",
        ));
    };
    let mut offline: Option<RawApiError> = None;
    for member in &governed.leaves {
        let has = if member.mode == RepositoryMode::Proxy {
            match member_versions(state, context, member, id, correlation).await? {
                Offering::Versions(versions) => versions.iter().any(|(_, v)| v == &target),
                Offering::Missing => false,
                Offering::Offline(error) => {
                    if offline.is_none() {
                        offline = Some(error);
                    }
                    false
                }
            }
        } else {
            super::raw::the_current_asset(
                state,
                context,
                &member.repo_key,
                &nuget::package_asset_path(id, &target),
            )
            .await?
            .is_some()
        };
        if has {
            return Ok(member.clone());
        }
    }
    if let Some(error) = offline {
        return Err(Failure::Raw(error));
    }
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
    Err(Failure::plain(
        StatusCode::NOT_FOUND,
        "this package has no such version in this group",
    ))
}

/// Serves a NuGet v3 route from a group repository, composing version lists
/// and registration leaves without mixing metadata and bytes from different members.
async fn read_group(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    group: &RepositoryDescriptor,
    route: &NuGetRoute,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    let Some(id) = route.package() else {
        return Err(RawApiError::NotFound.into());
    };
    let governed = the_governed(state, context, group, id).await?;
    match route {
        NuGetRoute::Flat(FlatRequest::Versions { id }) => {
            let owners = group_version_owners(state, context, &governed, id, correlation).await?;
            let versions: Vec<String> = owners.into_iter().map(|(_, v, _)| v).collect();
            Ok(json_response(
                StatusCode::OK,
                &nuget::version_index(&versions),
                None,
                correlation,
            ))
        }
        NuGetRoute::Registration { id } => {
            let owners = group_version_owners(state, context, &governed, id, correlation).await?;
            let latest = owners.last().map(|(_, v, _)| v.clone()).unwrap_or_default();
            let mut proxy_docs: HashMap<String, Option<Value>> = HashMap::new();
            let mut items = Vec::with_capacity(owners.len());
            for (_, version, owner) in owners {
                let item = if owner.mode == RepositoryMode::Proxy {
                    if !proxy_docs.contains_key(&owner.repo_key) {
                        let reg_route = NuGetRoute::Registration { id: id.clone() };
                        let doc = match proxy_bytes(state, context, &owner, &reg_route, correlation)
                            .await
                        {
                            Ok(bytes) => serde_json::from_slice::<Value>(&bytes).ok(),
                            Err(Failure::Raw(RawApiError::NotFound)) => None,
                            Err(other) => return Err(other),
                        };
                        proxy_docs.insert(owner.repo_key.clone(), doc);
                    }
                    proxy_docs
                        .get(&owner.repo_key)
                        .and_then(|opt| opt.as_ref())
                        .and_then(|doc| nuget::extract_registration_item(doc, id, &version))
                        .unwrap_or_else(|| nuget::registration_item(id, &version))
                } else {
                    nuget::registration_item(id, &version)
                };
                items.push(item);
            }
            Ok(json_response(
                StatusCode::OK,
                &nuget::registration_document(id, &latest, items),
                None,
                correlation,
            ))
        }
        NuGetRoute::Flat(FlatRequest::File { id, version, file }) => {
            let owner = the_owner_of(state, context, &governed, id, version, correlation).await?;
            if owner.mode == RepositoryMode::Proxy {
                return read_proxy(state, context, &owner, route, head, correlation).await;
            }
            let normalized = nuget::normalize_version(version).ok_or(RawApiError::NotFound)?;
            if *file != format!("{id}.nupkg") && *file != format!("{id}.{version}.nupkg") {
                return Err(Failure::plain(
                    StatusCode::NOT_FOUND,
                    "the flat container holds the nupkg of a published version, and nothing else",
                ));
            }
            read_asset_named(
                state,
                context,
                &owner,
                &nuget::package_asset_path(id, &normalized),
                "application/octet-stream",
                head,
                correlation,
            )
            .await
        }
        _ => Err(RawApiError::NotFound.into()),
    }
}

// ---------------------------------------------------------------------------
// Proxy
// ---------------------------------------------------------------------------

/// Joins a relative path onto a base URL string discovered from a service index.
fn join_resource_url(base: &str, relative: &str) -> Option<reqwest::Url> {
    let mut url = reqwest::Url::parse(base).ok()?;
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    url.join(relative).ok()
}

/// Resolves the catalog asset path, upstream target URL, and content type for a
/// NuGet proxy route. Supports both flat-layout upstreams and NuGet v3 service
/// index URLs (such as `https://api.nuget.org/v3/index.json`).
async fn proxy_target(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    route: &NuGetRoute,
    correlation: &str,
) -> Result<(String, reqwest::Url, &'static str), Failure> {
    let upstream = UpstreamConfig::of(&repository.config)
        .ok()
        .flatten()
        .ok_or_else(|| Failure::plain(StatusCode::BAD_GATEWAY, "the proxy is not configured"))?;
    let (asset_path, content_type) = match route {
        NuGetRoute::Flat(FlatRequest::Versions { id }) => {
            (format!("flatcontainer/{id}/index.json"), "application/json")
        }
        NuGetRoute::Flat(FlatRequest::File { id, version, file }) => (
            format!("flatcontainer/{id}/{version}/{file}"),
            "application/octet-stream",
        ),
        NuGetRoute::Registration { id } => {
            (format!("registrations/{id}/index.json"), "application/json")
        }
        _ => return Err(RawApiError::NotFound.into()),
    };

    if upstream.base.path().ends_with("/index.json/") {
        let mut index_url = upstream.base.clone();
        let trimmed = index_url.path().trim_end_matches('/').to_string();
        index_url.set_path(&trimmed);
        let served = super::raw::the_proxy_asset_at(
            state,
            context,
            repository,
            "v3/index.json",
            Some(index_url),
            Some("application/json"),
            correlation,
        )
        .await?;
        let bytes = the_content_store(state)?
            .get(&served.asset.digest)
            .await
            .map_err(|error| the_blob_error(&error))?;
        let doc: Value = serde_json::from_slice(&bytes).map_err(|_| {
            Failure::plain(
                StatusCode::BAD_GATEWAY,
                "the upstream NuGet service index is not valid JSON",
            )
        })?;
        let resources = nuget::service_index_resources(&doc).ok_or_else(|| {
            Failure::plain(
                StatusCode::BAD_GATEWAY,
                "the upstream NuGet service index does not declare PackageBaseAddress",
            )
        })?;
        let url = match route {
            NuGetRoute::Flat(FlatRequest::Versions { id }) => {
                join_resource_url(&resources.flat_container, &format!("{id}/index.json"))
            }
            NuGetRoute::Flat(FlatRequest::File { id, version, file }) => {
                join_resource_url(&resources.flat_container, &format!("{id}/{version}/{file}"))
            }
            NuGetRoute::Registration { id } => {
                let reg_base = resources
                    .registrations
                    .as_deref()
                    .ok_or(RawApiError::NotFound)?;
                join_resource_url(reg_base, &format!("{id}/index.json"))
            }
            _ => None,
        }
        .ok_or(RawApiError::NotFound)?;
        return Ok((asset_path, url, content_type));
    }

    let url = upstream.target(&asset_path).ok_or(RawApiError::NotFound)?;
    Ok((asset_path, url, content_type))
}

/// Fetches and caches a NuGet proxy asset, returning its raw bytes after trust admission.
async fn proxy_bytes(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    route: &NuGetRoute,
    correlation: &str,
) -> Result<Vec<u8>, Failure> {
    let (asset_path, url, _) = proxy_target(state, context, repository, route, correlation).await?;
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
    crate::trust::admit(
        state,
        context.account().as_str(),
        repository,
        &served.asset.digest,
    )
    .await
    .map_err(the_admission_error)?;
    let bytes = the_content_store(state)?
        .get(&served.asset.digest)
        .await
        .map_err(|error| the_blob_error(&error))?
        .to_vec();
    Ok(bytes)
}

/// A NuGet proxy re-serves the upstream document as is: the
/// flat container and the registrations are cached under their protocol path
/// and a rewritten nupkg would stop matching the `contentHash` the
/// upstream itself declares.
async fn read_proxy(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    route: &NuGetRoute,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    let (asset_path, url, content_type) =
        proxy_target(state, context, repository, route, correlation).await?;
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
    let etag = conditional::the_etag(&served.asset.digest);
    let mut response = body_response(StatusCode::OK, bytes, content_type, &etag, correlation);
    apply_cache_headers(&mut response, &Some((served.status, served.provenance)));
    Ok(response)
}

// ---------------------------------------------------------------------------
// Publishing (hosted)
// ---------------------------------------------------------------------------

/// Publish a nupkg. The nuspec identity wins over the URL, and
/// immutability wins over everything else: the same digest is a yes,
/// a different digest is a 409 that explains why.
async fn publish(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    route: &NuGetRoute,
    request: axum::extract::Request<Body>,
    correlation: &str,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::upload_asset(), repository)?;
    if !repository.mode.accepts_client_writes() {
        return Err(
            RawApiError::MethodNotAllowed("this repository does not accept uploads").into(),
        );
    }
    let NuGetRoute::Publish { id, version } = route else {
        return Err(RawApiError::NotFound.into());
    };
    let limit = state.max_body_size.saturating_add(1024 * 1024);
    let body = axum::body::to_bytes(request.into_body(), limit)
        .await
        .map_err(|_| Failure::Raw(RawApiError::PayloadTooLarge))?;

    // The real client sends the nupkg inside a multipart form
    // and without identity in the URL; the raw body is also a valid
    // publication. In both cases the inner identity is the authority: the
    // URL is a promise by the client about the nuspec, and here it is kept
    // or rejected.
    let payload = if body.starts_with(b"PK") {
        body.to_vec()
    } else {
        nuget::multipart_package(&body)?
    };
    let declared = nuget::package_identity(&payload)?;
    if let Some(promise) = id
        && declared.id != *promise
    {
        return Err(NugetError::InvalidPackage(format!(
            "the nuspec declares {}, but the URL says {promise}",
            declared.id
        ))
        .into());
    }
    if let Some(promise) = version
        && declared.version != *promise
    {
        return Err(NugetError::InvalidPackage(format!(
            "the nuspec declares version {}, but the URL says {promise}",
            declared.version
        ))
        .into());
    }
    let (id, version) = (declared.id.clone(), declared.version.clone());

    let asset_path = nuget::package_asset_path(&id, &version);
    let (digest, length) = write_the_content(
        state,
        context.account().as_str(),
        Body::from(payload),
        None,
        None,
    )
    .await?;

    // An already-published version is forever: the same digest is the echo
    // of an interrupted publication, a different digest is a 409 that
    // explains why.
    let immutable =
        format!("version {version} of {id} is already published; a published version is immutable");
    if let Some(existing) =
        the_current_asset(state, context, &repository.repo_key, &asset_path).await?
        && existing.digest != digest
    {
        return Err(Failure::plain(StatusCode::CONFLICT, &immutable));
    }

    let (_, writer) = the_catalog(state)?;
    let publication = Publication {
        repository: repository.repo_key.clone(),
        asset_path,
        coordinates: PackageCoordinates {
            package_name: id.clone(),
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
        Ok(_) => {
            let mut response = json_response(
                StatusCode::CREATED,
                &json!({ "ok": true }),
                None,
                correlation,
            );
            // The audit names the package even if the URL did not.
            response.extensions_mut().insert(PublishedId(id));
            Ok(response)
        }
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
