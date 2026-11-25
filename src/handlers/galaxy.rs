//! The **galaxy** surface: `ansible-galaxy collection publish`, `install`
//! and `verify` of a real `ansible-galaxy` client against an Ansible
//! Galaxy repository of Hodei, spoken over the collections API v3.
//!
//! It lives in `/galaxy/{repository}/...` and speaks the dialect the
//! client fixes (`ansible/galaxy/api.py` of ansible-core 2.19.6): the
//! route table and the v3 documents are the ones `hodei_catalog::galaxy`
//! (pure, checked against the client source) serves, and here there are
//! only effects.
//!
//! The surface serves the collection documents from the catalog and the
//! artifact bytes from the CAS: one published collection is **one
//! immutable asset** whose path carries its identity,
//! `artifacts/{namespace}/{name}/{version}.tar.gz`, and whose digest is
//! the sha256 the version document declares to the client — the very one
//! the client checks (`Mismatch artifact hash with downloaded file`) when
//! it downloads. The identity itself is read on publish from the
//! `MANIFEST.json` the artifact carries inside.
//!
//! The modality served today is **hosted**: the galaxy proxy needs an
//! upstream validated with a real client before it can be declared — the
//! real upstream serves the artifacts through signed redirects, a dialect
//! this release has not verified.
//!
//! The authentication is the one of the hub, mapped to the dialect: the
//! client sends its API key as `Authorization: Token <key>`, and the hub
//! verifies the key **as a bearer credential** of the principal. The
//! scoping by account is the one of the hub, not one of the format.

use axum::{
    body::Body,
    extract::{FromRequest, OriginalUri, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::Engine;
use chrono::Utc;
use hodei_authz::{Credentials, VerifiedContext, verify_principal};
use hodei_catalog::{
    AssetRecord, CatalogError, PackageCoordinates, Publication, RepositoryDescriptor,
    RepositoryFormat,
    galaxy::{self, CollectionRef, Route},
};
use hodei_evidence::domain::content::ContentDigest;
use serde_json::{Value, json};

use super::raw::{
    RawApiError, Resolved, apply_cache_headers, apply_common_headers, the_admission_error,
    the_blob_error, the_catalog, the_content_store, the_decision, the_publication_error,
    the_repository_in, the_resolved_asset, write_the_content,
};
use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{correlation_id, record_audit};

/// What happens with a request that cannot be served.
#[derive(Debug)]
pub enum Failure {
    /// An error of the shared surface.
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
        // The v3 dialect carries its errors in the shape the client turns
        // into its message: `errors[].detail`.
        let body = galaxy::error_document("unknown", &message);
        let mut response = (status, axum::Json(body)).into_response();
        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Token"));
        }
        response
    }
}

impl Failure {
    /// An error with a status, a protocol code and a reason.
    fn the_failure(status: StatusCode, code: &str, detail: &str) -> Self {
        let _ = code;
        Self::Plain(status, detail.to_string())
    }
}

/// The rest of the route of `/galaxy/{repository}/{rest}`, without
/// decoding: the protocol grammar rules are the only ones that may look
/// at a `%2f`.
fn the_split(uri: &OriginalUri) -> Result<(String, String), Failure> {
    let rest = uri
        .path()
        .strip_prefix("/galaxy/")
        .ok_or(RawApiError::NotFound)?;
    let (repository, rest) = rest.split_once('/').ok_or(RawApiError::NotFound)?;
    if !crate::raw::path::valid_repository_name(repository) || rest.is_empty() {
        return Err(RawApiError::NotFound.into());
    }
    Ok((repository.to_string(), rest.to_string()))
}

/// Any request of `/galaxy/{repository}/...`.
pub async fn handle(
    State(state): State<AppState>,
    method: Method,
    uri: OriginalUri,
    headers: HeaderMap,
    request: axum::extract::Request<Body>,
) -> Result<Response, Failure> {
    let correlation = correlation_id(&headers, &state.identity);
    let (repository_key, rest) = the_split(&uri)?;
    let Some(route) = galaxy::parse(&rest) else {
        return Err(RawApiError::NotFound.into());
    };

    let context = the_context(&state, &headers).await?;
    let repository = the_repository_in(
        &state,
        &context,
        &repository_key,
        &[RepositoryFormat::Galaxy],
    )
    .await?;

    let reading = method == Method::GET || method == Method::HEAD;
    let publishing = method == Method::POST;
    match (&route, reading, publishing) {
        (_, true, _) => {
            the_decision(
                &state,
                &context,
                crate::actions::download_asset(),
                &repository,
            )?;
            let read = TheRead {
                // The base of every URL the documents carry: the absolute
                // path of the repository surface, the one the client
                // `urljoin`s against its API root.
                base: format!("/galaxy/{repository_key}"),
                query: uri.query().unwrap_or_default(),
                head: method == Method::HEAD,
            };
            read_route(&state, &context, &repository, route, &read, &correlation).await
        }
        (Route::Publish, false, true) => {
            let outcome = async {
                the_decision(
                    &state,
                    &context,
                    crate::actions::upload_asset(),
                    &repository,
                )?;
                if !repository.mode.accepts_client_writes() {
                    return Err(RawApiError::MethodNotAllowed(
                        "this repository does not accept uploads",
                    )
                    .into());
                }
                publish(&state, &context, &repository, request, &correlation).await
            }
            .await;
            // Every write leaves a trace, accepted or not.
            let result = match &outcome {
                Ok(_) => AuditResult::Allowed,
                Err(Failure::Raw(RawApiError::Forbidden)) => AuditResult::Denied,
                Err(_) => AuditResult::Failed,
            };
            record_audit(
                &state,
                context.principal().subject().as_str(),
                context.account().as_str(),
                "publish",
                &format!("{repository_key}/{rest}"),
                result,
                &correlation,
            );
            outcome
        }
        _ => Err(RawApiError::MethodNotAllowed("this galaxy operation is not supported").into()),
    }
}

/// The principal verified out of the client credentials.
///
/// The dialect sends the API key as `Authorization: Token <key>`: the hub
/// verifies the key as a bearer credential of the principal, and the
/// scoping by account is the one the token carries. A key the hub can
/// verify is a verifiable credential; an identity that declares itself
/// without being verified is not an identity.
async fn the_context(state: &AppState, headers: &HeaderMap) -> Result<VerifiedContext, Failure> {
    let key = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Token "))
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .ok_or(Failure::the_failure(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "the galaxy surface needs an API key: configure one with `--api-key`",
        ))?;
    match verify_principal(state.authenticator.as_ref(), &Credentials::bearer(key)).await {
        Ok(principal) => Ok(VerifiedContext::from_principal(principal)),
        Err(hodei_authz::AuthenticationError::ProviderUnavailable(_)) => {
            Err(RawApiError::Unavailable.into())
        }
        Err(_) => Err(Failure::the_failure(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "the API key is not one this hub can verify",
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

/// What a read of the surface carries: the base of the URLs the documents
/// build, the query of the listing, and whether the answer wants a body.
struct TheRead<'a> {
    /// The absolute path of the repository surface.
    base: String,
    /// The query the client asked with.
    query: &'a str,
    /// A `HEAD` answers the same document without the bytes.
    head: bool,
}

async fn read_route(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    route: Route,
    read: &TheRead<'_>,
    correlation: &str,
) -> Result<Response, Failure> {
    let base = &read.base;
    let head = read.head;
    match route {
        Route::ApiRoot => Ok(json_response(
            StatusCode::OK,
            &galaxy::api_root_document(),
            correlation,
        )),
        Route::CollectionDetail(reference) => {
            let records =
                assets_under(state, context, repository, &reference.asset_prefix()).await?;
            if records.is_empty() {
                return Err(Failure::the_failure(
                    StatusCode::NOT_FOUND,
                    "not_found",
                    &format!(
                        "the collection {}.{} is not in this repository",
                        reference.namespace, reference.name
                    ),
                ));
            }
            let (created_at, updated_at) = the_history_of(&records);
            Ok(json_response(
                StatusCode::OK,
                &galaxy::collection_document(base, &reference, created_at, updated_at),
                correlation,
            ))
        }
        Route::CollectionVersions(reference) => {
            let records =
                assets_under(state, context, repository, &reference.asset_prefix()).await?;
            let mut entries = the_version_entries(&reference, &records);
            galaxy::the_sorted_versions(&mut entries);
            let page = galaxy::the_page_of(read.query);
            let start = page.offset.min(entries.len());
            let end = (page.offset + page.limit).min(entries.len());
            let window = &entries[start..end];
            // One more entry than the window says there is a page after
            // this one; the client follows the link and asks again.
            let next = if end < entries.len() {
                Some(galaxy::the_next_link(
                    base,
                    &reference,
                    galaxy::Page {
                        limit: page.limit,
                        offset: end,
                    },
                ))
            } else {
                None
            };
            Ok(json_response(
                StatusCode::OK,
                &galaxy::versions_page_document(base, &reference, window, page, next.as_deref()),
                correlation,
            ))
        }
        Route::CollectionVersion(reference, version) => {
            let Some(record) =
                the_version_asset(state, context, repository, &reference, &version).await?
            else {
                return Err(Failure::the_failure(
                    StatusCode::NOT_FOUND,
                    "not_found",
                    &format!(
                        "the version {version} of the collection {}.{} is not in this repository",
                        reference.namespace, reference.name
                    ),
                ));
            };
            let dependencies = the_dependencies_of(state, &record).await?;
            Ok(json_response(
                StatusCode::OK,
                &galaxy::version_document(
                    base,
                    &reference,
                    &version,
                    &record.digest.value,
                    record.size_bytes,
                    &dependencies,
                ),
                correlation,
            ))
        }
        Route::Artifact(digest, reference, version) => {
            // The URL carries the digest the version document declared:
            // one that does not name the content at the path is a URL
            // nobody built, not one to serve.
            let asset_path = reference.asset_path(&version);
            let resolved =
                the_resolved_asset(state, context, repository.clone(), &asset_path, correlation)
                    .await?;
            let Resolved {
                asset,
                served,
                resolved_by: _,
                repository: backing,
            } = resolved;
            if asset.digest.value != digest {
                return Err(RawApiError::NotFound.into());
            }
            crate::trust::admit(state, context.account().as_str(), &backing, &asset.digest)
                .await
                .map_err(the_admission_error)?;
            let etag = crate::raw::conditional::the_etag(&asset.digest);
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
            let mut response = artifact_response(StatusCode::OK, bytes, &etag, correlation);
            apply_cache_headers(&mut response, &served);
            apply_common_headers(&mut response, &etag, correlation);
            if head && let Ok(value) = HeaderValue::from_str(&length.to_string()) {
                response.headers_mut().insert(header::CONTENT_LENGTH, value);
            }
            Ok(response)
        }
        Route::ImportTask(task) => Ok(json_response(
            StatusCode::OK,
            &galaxy::import_task_document(&task, Utc::now()),
            correlation,
        )),
        Route::Publish => {
            Err(RawApiError::MethodNotAllowed("this galaxy operation is not supported").into())
        }
    }
}

/// The assets under a prefix, in the account of the principal.
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

/// The history of a collection: the day it arrived and the day its
/// content moved last. The client reads the modified one to decide
/// whether its cached listing is stale.
fn the_history_of(records: &[AssetRecord]) -> (chrono::DateTime<Utc>, chrono::DateTime<Utc>) {
    let mut created_at = records
        .iter()
        .map(|record| record.updated_at)
        .min()
        .unwrap_or_else(Utc::now);
    let mut updated_at = records
        .iter()
        .map(|record| record.updated_at)
        .max()
        .unwrap_or(created_at);
    if updated_at < created_at {
        (created_at, updated_at) = (updated_at, created_at);
    }
    (created_at, updated_at)
}

/// The version entries of a collection, out of its assets. One version is
/// one asset: `artifacts/{ns}/{name}/{version}.tar.gz`, the path the
/// publish chose and nothing else.
fn the_version_entries(
    reference: &CollectionRef,
    records: &[AssetRecord],
) -> Vec<galaxy::VersionEntry> {
    let prefix = format!("{}/", reference.asset_prefix());
    let mut entries = Vec::new();
    for record in records {
        let Some(rest) = record.asset_path.strip_prefix(&prefix) else {
            continue;
        };
        let Some(version) = rest.strip_suffix(".tar.gz") else {
            continue;
        };
        if !galaxy::valid_version(version) {
            continue;
        }
        entries.push(galaxy::VersionEntry {
            version: version.to_string(),
            created_at: record.updated_at,
        });
    }
    entries
}

/// The asset of one version of a collection, if it is there.
async fn the_version_asset(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    reference: &CollectionRef,
    version: &str,
) -> Result<Option<AssetRecord>, Failure> {
    let asset_path = reference.asset_path(version);
    let (reader, _) = the_catalog(state)?;
    match reader
        .asset(
            context.account().as_str(),
            &repository.repo_key,
            &asset_path,
        )
        .await
    {
        // The metadata of a version names its digest: what the channel does
        // not admit is not there.
        Ok(record) => Ok(super::raw::the_admitted(
            state,
            context.account().as_str(),
            repository,
            vec![record],
        )
        .await?
        .pop()),
        Err(CatalogError::Missing { .. }) => Ok(None),
        Err(error) => Err(Failure::from(the_publication_error(&error))),
    }
}

/// The dependencies a version declares, read from the artifact itself.
///
/// The manifest travels inside the artifact and the artifact is
/// immutable: reading it again gives the dependencies of the published
/// bytes, not ones the catalog remembered out of another life.
async fn the_dependencies_of(state: &AppState, record: &AssetRecord) -> Result<Value, Failure> {
    let store = the_content_store(state)?;
    let bytes = store
        .get(&record.digest)
        .await
        .map_err(|error| the_blob_error(&error))?
        .to_vec();
    Ok(galaxy::the_collection_identity(&bytes)
        .map(|identity| identity.dependencies)
        .unwrap_or_else(|| json!({})))
}

fn artifact_response(status: StatusCode, body: Vec<u8>, etag: &str, correlation: &str) -> Response {
    let mut response = (status, Body::from(body)).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response.headers_mut().insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    apply_common_headers(&mut response, etag, correlation);
    response
}

// ---------------------------------------------------------------------------
// Publish
// ---------------------------------------------------------------------------

/// The publish: the client sends a multipart form with the artifact and
/// the sha256 it computed for it, and the hub stores the artifact only
/// when the bytes are the ones the form declared and the artifact is a
/// collection this release can read.
///
/// The import runs synchronously: by the time the client reads the task
/// it gets, the collection is already in the catalog, and the task
/// document says completed.
async fn publish(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    request: axum::extract::Request<Body>,
    correlation: &str,
) -> Result<Response, Failure> {
    let (declared_sha256, bytes) = the_publish_form(state, request).await?;

    // The sha256 the form declared is checked **at the commit**: the
    // write only lands in the store when the bytes are the ones the
    // client said it was sending, the same promise the artifact URL
    // carries later on.
    let declared = ContentDigest::sha256(&declared_sha256);
    if !declared.is_well_formed() {
        return Err(Failure::the_failure(
            StatusCode::BAD_REQUEST,
            "invalid",
            "the sha256 of the publish form is not a sha256",
        ));
    }
    let (digest, length) = write_the_content(
        state,
        context.account().as_str(),
        Body::from(bytes.clone()),
        Some(declared),
        None,
    )
    .await
    .map_err(|error| match error {
        RawApiError::BadRequest(_) => Failure::the_failure(
            StatusCode::BAD_REQUEST,
            "invalid",
            "the artifact does not match the sha256 the form declared",
        ),
        other => other.into(),
    })?;

    let identity = match galaxy::the_collection_identity_checked(&bytes) {
        Ok(identity) => identity,
        // A small upload that inflates past the limit is a decompression
        // bomb, not a malformed form: it is refused as too large.
        Err(galaxy::IdentityError::TooLarge) => {
            return Err(Failure::the_failure(
                StatusCode::PAYLOAD_TOO_LARGE,
                "too_large",
                "the artifact expands past the limit this hub reads",
            ));
        }
        Err(galaxy::IdentityError::Unreadable) => {
            return Err(Failure::the_failure(
                StatusCode::BAD_REQUEST,
                "invalid",
                "the artifact does not carry a MANIFEST.json this release can read",
            ));
        }
    };
    let reference = CollectionRef {
        namespace: identity.namespace.clone(),
        name: identity.name.clone(),
    };
    let asset_path = reference.asset_path(&identity.version);
    let coordinates = PackageCoordinates {
        package_name: identity.fqcn(),
        package_version: identity.version.clone(),
    };

    let (_, writer) = the_catalog(state)?;
    let publication = Publication {
        repository: repository.repo_key.clone(),
        asset_path,
        coordinates,
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
            // The task is informational for this release — the import
            // already happened — but it is the task the client will ask
            // for, so it is one the grammar can name back.
            let task = uuid::Uuid::new_v4().simple().to_string();
            Ok(json_response(
                StatusCode::OK,
                &galaxy::publish_document(&task),
                correlation,
            ))
        }
        Err(CatalogError::Conflict { .. }) => Err(Failure::the_failure(
            StatusCode::CONFLICT,
            "conflict",
            &format!(
                "the version {} of the collection {} is already published with another content",
                identity.version,
                identity.fqcn()
            ),
        )),
        Err(error) => Err(Failure::from(the_publication_error(&error))),
    }
}

/// The fields of the publish form: the sha256 the client computed and the
/// artifact bytes. The client sends the file as `file` and the digest as
/// `sha256`, and nothing else matters.
async fn the_publish_form(
    state: &AppState,
    request: axum::extract::Request<Body>,
) -> Result<(String, Vec<u8>), Failure> {
    let mut multipart = axum::extract::Multipart::from_request(request, &state)
        .await
        .map_err(|_| {
            Failure::the_failure(
                StatusCode::BAD_REQUEST,
                "invalid",
                "the publish is not a multipart form",
            )
        })?;
    let limit = state.max_body_size;
    let mut sha256: Option<String> = None;
    let mut file: Option<Vec<u8>> = None;
    let mut total = 0usize;
    while let Some(mut field) = multipart.next_field().await.map_err(|_| {
        Failure::the_failure(
            StatusCode::BAD_REQUEST,
            "invalid",
            "the publish form cannot be read",
        )
    })? {
        let name = field.name().unwrap_or_default().to_string();
        // The client writes the artifact the MIME way: the part carries
        // `Content-Transfer-Encoding: base64` and the bytes of the part
        // are the base64 of the artifact. The transfer encoding is part
        // of the dialect, not of the transport: the artifact is what the
        // part declares, not the bytes of the part as they come.
        let encoded = field
            .headers()
            .get("content-transfer-encoding")
            .and_then(|value| value.to_str().ok())
            .map(|value| value.trim().eq_ignore_ascii_case("base64"))
            .unwrap_or(false);
        let mut bytes = Vec::new();
        while let Some(chunk) = field.chunk().await.map_err(|_| {
            Failure::the_failure(
                StatusCode::BAD_REQUEST,
                "invalid",
                "the publish form cannot be read",
            )
        })? {
            total += chunk.len();
            if total > limit {
                return Err(RawApiError::PayloadTooLarge.into());
            }
            bytes.extend_from_slice(&chunk);
        }
        if name == "sha256" {
            sha256 = Some(String::from_utf8_lossy(&bytes).trim().to_string());
        } else if name == "file" && file.is_none() {
            let content = if encoded {
                // The base64 the client writes is wrapped in CRLF lines,
                // the wrapping MIME does: the alphabet is the standard
                // one, and the line breaks are not part of it.
                let compact: Vec<u8> = bytes
                    .into_iter()
                    .filter(|byte| !byte.is_ascii_whitespace())
                    .collect();
                base64::engine::general_purpose::STANDARD
                    .decode(&compact)
                    .map_err(|_| {
                        Failure::the_failure(
                            StatusCode::BAD_REQUEST,
                            "invalid",
                            "the artifact of the publish form is not the base64 it declared",
                        )
                    })?
            } else {
                bytes
            };
            file = Some(content);
        }
    }
    match (sha256, file) {
        (Some(sha256), Some(file)) => Ok((sha256, file)),
        _ => Err(Failure::the_failure(
            StatusCode::BAD_REQUEST,
            "invalid",
            "the publish form carries no sha256 or no file",
        )),
    }
}
