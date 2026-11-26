//! The **PyPI** surface: `twine upload` and `pip install` against a
//! Hodei repository.
//!
//! Lives at `/pypi/{repository}/...` and speaks the Simple API: the grammar
//! of names, files and documents is in `hodei_catalog::pypi`
//! (pure) and here there is only what needs effects.
//!
//! # The routes
//!
//! - `GET /pypi/{repository}/simple/` — the index of projects.
//! - `GET /pypi/{repository}/simple/{project}/` — the page of a
//!   project.
//! - `GET /pypi/{repository}/files/{project}/{file}` — the distribution.
//! - `POST /pypi/{repository}/` — the upload (`twine upload`).
//! - `POST /pypi/{repository}/yank` — withdraw a version.
//!
//! # How it authenticates
//!
//! `pip` and `twine` cannot send a `Bearer`: both use
//! basic authentication. That is why this surface accepts both forms: the
//! `Bearer` header with the platform credential, and the basic one, whose
//! password **is** the platform credential (the user is
//! informative), as in the OCI token endpoint.
//!
//! What can be done is decided by Cedar on each request, as in the rest
//! of the surfaces: the credential carries an identity, not permissions.
//!
//! # The index is written by this page
//!
//! The HTML that comes out of here is rendered by this service, with the values
//! escaped. A proxy does not return the upstream's HTML: it renders it again
//! with what the upstream declared in its JSON. An upstream may
//! publish whatever it wants in its HTML; what the client receives from here
//! cannot be injected by it.
//!
//! # A project's document
//!
//! It is a **snapshot**: each publication and each withdrawal writes a new
//! document in the content store and moves the path pointer
//! with a precondition on the previous digest. Distributions
//! are immutable content at their own path, and are not touched when rewriting
//! the document.
//!
//! # What is checked on an upload
//!
//! The hash the client declares must be that of the bytes that
//! arrived, and the name and version must be those of the file
//! name. A client that lies does not publish: the index cannot promise
//! a hash that is not that of the content being downloaded.

use axum::{
    body::Body,
    extract::{FromRequest, OriginalUri, State},
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::Engine as _;
use hodei_authz::VerifiedContext;
use hodei_catalog::{
    CatalogError, PackageCoordinates, Precondition, Publication, RepositoryDescriptor,
    RepositoryFormat, RepositoryMode, UpstreamConfig,
    pypi::{self, FileEntry, ProjectDocument, ProjectName, Route},
};
use hodei_evidence::domain::content::ContentDigest;
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{bearer_token, correlation_id, record_audit};
use crate::public_url::TheOrigin;
use crate::raw::conditional;
use crate::resolution::{
    ResolutionError as RawApiError, apply_common_headers, hex_of, the_admission_error,
    the_blob_error, the_catalog, the_content_store, the_current_asset, the_decision,
    the_proxy_asset_at, the_publication_error, the_repository_in, verify_credential_context,
    write_the_content,
};

/// How many times a document write that loses the race to another
/// is retried.
const MAX_ATTEMPTS: usize = 6;

/// How many fields an upload may carry.
///
/// `twine` sends a few: the action, the protocol version, the
/// name, the version, the hash, the `requires-python` and the file. A
/// limit that is far too high and one that is too low give away a client that is not a
/// client.
const MAX_PARTS: usize = 64;

/// What the client is told when the retries run out.
const CONTENDED: &str = "the project is being updated concurrently; retry it";

/// What happens to a request that cannot be served.
#[derive(Debug)]
pub enum Failure {
    /// An error from the shared surface.
    Raw(RawApiError),
    /// An error from the PyPI protocol.
    Pypi(pypi::PypiError),
    /// A status and a reason.
    Plain(StatusCode, String),
}

impl From<RawApiError> for Failure {
    fn from(error: RawApiError) -> Self {
        Self::Raw(error)
    }
}

impl From<pypi::PypiError> for Failure {
    fn from(error: pypi::PypiError) -> Self {
        Self::Pypi(error)
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
                    "the PyPI surface is not available".into(),
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
                    "the upstream did not answer".into(),
                ),
                RawApiError::Offline(reason) => (StatusCode::SERVICE_UNAVAILABLE, (*reason).into()),
                RawApiError::ReservedNamespace(reason) => (StatusCode::FORBIDDEN, reason.clone()),
                RawApiError::Admission(_) => error.the_verdict(),
                RawApiError::Internal => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "the service failed to answer".into(),
                ),
            },
            Self::Pypi(error) => match error {
                pypi::PypiError::InvalidName(_)
                | pypi::PypiError::InvalidFileName(_)
                | pypi::PypiError::InvalidVersion(_)
                | pypi::PypiError::MissingField(_)
                | pypi::PypiError::MissingFile
                | pypi::PypiError::Mismatch(_)
                | pypi::PypiError::DigestMismatch
                | pypi::PypiError::InvalidPath(_) => (StatusCode::BAD_REQUEST, error.message()),
                pypi::PypiError::FileExists => (
                    StatusCode::CONFLICT,
                    "the distribution is already published in this project".into(),
                ),
                pypi::PypiError::UnknownFile(_) => (StatusCode::NOT_FOUND, "not found".into()),
                pypi::PypiError::InvalidDocument(_) => (
                    StatusCode::BAD_GATEWAY,
                    "the stored project document is not valid".into(),
                ),
            },
            Self::Plain(status, reason) => (*status, reason.clone()),
        }
    }

    /// The content length when the response is a `416`.
    ///
    /// RFC 9110 asks that the response to a range that cannot be served
    /// state in `Content-Range` the length of what exists: without that
    /// header the client does not even know how much there is.
    fn unsatisfiable_length(&self) -> Option<u64> {
        match self {
            Self::Raw(RawApiError::RangeNotSatisfiable(length)) => Some(*length),
            _ => None,
        }
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let (status, message) = self.status_and_message();
        let mut response = (status, message).into_response();
        // The advice on when to come back travels with the refusal for
        // saturation, in this surface as well: the verdict is a single
        // one, and the header is not a detail of the raw transport.
        if let Self::Raw(error) = &self
            && let Some(seconds) = error.the_retry_after()
            && let Ok(value) = HeaderValue::from_str(&seconds.to_string())
        {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        );
        if status == StatusCode::UNAUTHORIZED {
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                HeaderValue::from_static("Basic realm=\"hodei\", Bearer"),
            );
        }
        if let Some(length) = self.unsatisfiable_length()
            && let Ok(value) =
                HeaderValue::from_str(&crate::raw::range::the_unsatisfiable_range(length))
        {
            response.headers_mut().insert(header::CONTENT_RANGE, value);
        }
        response
    }
}

/// The repository and the rest of the path of `/pypi/{repository}/{rest}`.
fn the_split(uri: &OriginalUri) -> Result<(String, String), Failure> {
    let rest = uri
        .path()
        .strip_prefix("/pypi/")
        .ok_or(RawApiError::NotFound)?;
    let (repository, rest) = match rest.split_once('/') {
        Some((repository, rest)) => (repository, rest),
        // `/pypi/{repository}` alone: the `twine` upload, which publishes
        // at the root of the repository.
        None => (rest, ""),
    };
    if repository.is_empty() || !crate::raw::path::valid_repository_name(repository) {
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
        .url(&format!("/pypi/{repository}"))
        .ok_or(RawApiError::BadRequest(
            "the public address of the registry is unknown: the Host header is missing or invalid",
        ))
        .map_err(Failure::from)
}

/// The prefix where the files live, as the document stores it.
fn the_files_base(repository: &str) -> String {
    format!("/pypi/{}/files", repository)
}

/// The credential of the request.
///
/// The `Bearer` header carries the platform credential. Basic
/// credentials do too, in the **password**: the user is
/// informative, and that is how the Docker protocol carries them.
fn the_credential(headers: &HeaderMap) -> Option<String> {
    if let Some(token) = bearer_token(headers) {
        return Some(token);
    }
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let encoded = value
        .strip_prefix("Basic ")
        .or_else(|| value.strip_prefix("basic "))?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (_user, password) = decoded.split_once(':')?;
    if password.is_empty() {
        return None;
    }
    Some(password.to_string())
}

/// The verified identity of whoever asks.
async fn the_pypi_context(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<VerifiedContext, Failure> {
    let credential = the_credential(headers).ok_or(RawApiError::Unauthorized)?;
    Ok(verify_credential_context(state, &credential).await?)
}

/// What the request needs to know about itself.
struct Ask<'a> {
    /// Whether what is requested is only the head of the response.
    head: bool,
    /// The headers of the request.
    headers: &'a HeaderMap,
    /// The correlation of the request.
    correlation: &'a str,
}

/// A file of a project and the repository it comes from.
struct FileView {
    /// What the index knows about the file.
    entry: FileEntry,
    /// The repository that has it.
    owner: RepositoryDescriptor,
}

/// The document of a project, with the provenance of each file.
struct ProjectView {
    /// The normalized name of the project.
    name: String,
    /// The files, in the order they are published.
    files: Vec<FileView>,
}

/// Any request to `/pypi/{repository}/...`.
pub async fn handle(
    State(state): State<AppState>,
    origin: TheOrigin,
    method: Method,
    uri: OriginalUri,
    headers: HeaderMap,
    request: Request<Body>,
) -> Result<Response, Failure> {
    // Authentication is the first gate, as in the rest of the surfaces.
    let context = the_pypi_context(&state, &headers).await?;
    let correlation = correlation_id(&headers, &state.identity);
    let (repository_key, rest) = the_split(&uri)?;
    let route = pypi::route(&rest)?;
    let repository =
        the_repository_in(&state, &context, &repository_key, &[RepositoryFormat::PyPi]).await?;

    let reading = method == Method::GET || method == Method::HEAD;
    match (&route, reading) {
        (Route::Root, true) => {
            let resource = format!("{repository_key}/simple/");
            let outcome = the_root(
                &state,
                &context,
                &repository,
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
        (Route::Project { project }, true) => {
            let resource = format!("{repository_key}/simple/{}/", project.as_str());
            let outcome = the_project_page(
                &state,
                &context,
                &repository,
                project,
                &Ask {
                    head: method == Method::HEAD,
                    headers: &headers,
                    correlation: &correlation,
                },
                &origin,
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
        (Route::File { project, filename }, true) => {
            let resource = format!("{repository_key}/files/{project}/{filename}");
            let outcome = the_file(
                &state,
                &context,
                &repository,
                project,
                filename,
                &Ask {
                    head: method == Method::HEAD,
                    headers: &headers,
                    correlation: &correlation,
                },
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
        (Route::Upload, false) if method == Method::POST => {
            let resource = format!("{repository_key}/");
            let outcome = upload(&state, &context, &repository, request, &correlation).await;
            let result = match &outcome {
                Ok(_) => AuditResult::Allowed,
                Err(Failure::Raw(RawApiError::Forbidden)) => AuditResult::Denied,
                Err(_) => AuditResult::Failed,
            };
            record_audit(
                &state,
                context.principal().subject().as_str(),
                context.account().as_str(),
                "upload",
                &resource,
                result,
                &correlation,
            );
            outcome
        }
        (Route::Yank, false) if method == Method::POST => {
            let resource = format!("{repository_key}/yank");
            let outcome = yank(&state, &context, &repository, request, &correlation).await;
            let result = match &outcome {
                Ok(_) => AuditResult::Allowed,
                Err(Failure::Raw(RawApiError::Forbidden)) => AuditResult::Denied,
                Err(_) => AuditResult::Failed,
            };
            record_audit(
                &state,
                context.principal().subject().as_str(),
                context.account().as_str(),
                "yank",
                &resource,
                result,
                &correlation,
            );
            outcome
        }
        (Route::Upload, _) | (Route::Yank, _) => Err(Failure::plain(
            StatusCode::METHOD_NOT_ALLOWED,
            "this PyPI operation is not supported",
        )),
        _ => Err(RawApiError::MethodNotAllowed("this PyPI operation is not supported").into()),
    }
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

/// The index of the repository's projects.
///
/// A proxy cannot enumerate what it does not know: its index is empty, and the
/// client asks for the project it wants. A hosted one and a group can:
/// the first has its assets, and the second those of its members.
async fn the_root(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    headers: &HeaderMap,
    origin: &TheOrigin,
    correlation: &str,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::download_asset(), repository)?;
    let projects = the_projects(state, context, repository, correlation).await?;
    // The repository's projects, in order.
    let base = the_base(origin, &repository.repo_key)?;
    let representation = pypi::negotiate(headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()));
    let (body, content_type) = match representation {
        pypi::Representation::Json => {
            let value = pypi::render_root_json(&projects);
            let bytes = serde_json::to_vec(&value).map_err(|_| RawApiError::Internal)?;
            (bytes, pypi::JSON_CONTENT_TYPE)
        }
        pypi::Representation::Html => (
            pypi::render_root_html(&projects, &base).into_bytes(),
            "text/html; charset=utf-8",
        ),
    };
    Ok(the_page(body, content_type, None, correlation))
}

/// The repository's projects, in order.
async fn the_projects(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    correlation: &str,
) -> Result<Vec<String>, Failure> {
    match repository.mode {
        RepositoryMode::Group => {
            let mut projects: Vec<String> = Vec::new();
            // The members of a group are leaves: none is another group.
            for member in super::raw_group::the_leaves(state, context, repository).await? {
                for project in the_leaf_projects(state, context, &member, correlation).await? {
                    if !projects.contains(&project) {
                        projects.push(project);
                    }
                }
            }
            projects.sort();
            Ok(projects)
        }
        _ => the_leaf_projects(state, context, repository, correlation).await,
    }
}

/// The projects of a repository that is not a group.
async fn the_leaf_projects(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    correlation: &str,
) -> Result<Vec<String>, Failure> {
    if repository.mode == RepositoryMode::Proxy {
        // What is in the upstream cannot be enumerated without requesting it
        // project by project.
        let _ = correlation;
        return Ok(Vec::new());
    }
    let (reader, _) = the_catalog(state)?;
    let assets = reader
        .assets_under(context.account().as_str(), &repository.repo_key, "")
        .await
        .map_err(|error| the_publication_error(&error))?;
    let mut projects: Vec<String> = assets
        .iter()
        .map(|asset| asset.asset_path.clone())
        // A project document has no separator; the
        // distributions do.
        .filter(|path| !path.contains('/'))
        .collect();
    projects.sort();
    projects.dedup();
    Ok(projects)
}

/// The page of a project.
async fn the_project_page(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    project: &ProjectName,
    ask: &Ask<'_>,
    origin: &TheOrigin,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::download_asset(), repository)?;
    let view = the_project(state, context, repository, project, ask.correlation).await?;
    let document = ProjectDocument {
        name: view.name.clone(),
        files: view.files.iter().map(|file| file.entry.clone()).collect(),
    };
    let base = the_base(origin, &repository.repo_key)?;
    let representation = pypi::negotiate(
        ask.headers
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok()),
    );
    let (body, content_type) = match representation {
        pypi::Representation::Json => {
            let value = pypi::render_project_json(&document, &base);
            let bytes = serde_json::to_vec(&value).map_err(|_| RawApiError::Internal)?;
            (bytes, pypi::JSON_CONTENT_TYPE)
        }
        pypi::Representation::Html => (
            pypi::render_project_html(&document, &base).into_bytes(),
            "text/html; charset=utf-8",
        ),
    };
    let digest = ContentDigest::of_bytes(&body);
    let etag = conditional::the_etag(&digest);
    match conditional::the_read_condition(
        conditional::the_if_match(ask.headers).as_deref(),
        conditional::the_if_none_match(ask.headers).as_deref(),
        Some(&etag),
    ) {
        conditional::ReadOutcome::Failed => return Err(RawApiError::PreconditionFailed.into()),
        conditional::ReadOutcome::NotModified => {
            let mut response = StatusCode::NOT_MODIFIED.into_response();
            apply_common_headers(&mut response, &etag, ask.correlation);
            return Ok(response);
        }
        conditional::ReadOutcome::Serve => {}
    }
    let body = if ask.head { Vec::new() } else { body };
    Ok(the_page(body, content_type, Some(&etag), ask.correlation))
}

/// The document of a project, with the provenance of each file.
async fn the_project(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    project: &ProjectName,
    correlation: &str,
) -> Result<ProjectView, Failure> {
    match repository.mode {
        RepositoryMode::Group => {
            let mut files: Vec<FileView> = Vec::new();
            // The group's namespace reservation is consulted with the project
            // name: a public upstream does not contribute files to a
            // reserved name, even if it has higher versions. The
            // members left out are not consulted.
            let (leaves, denied, judgement) =
                super::raw_group::the_governed_leaves(state, context, repository, project.as_str())
                    .await?;
            for member in leaves {
                // A member that does not have the project is not a failure: the
                // next one is tried.
                let view =
                    match the_leaf_project(state, context, &member, project, correlation).await {
                        Ok(view) => view,
                        Err(Failure::Raw(RawApiError::NotFound)) => continue,
                        Err(other) => return Err(other),
                    };
                for file in view.files {
                    // The first member that has the file wins.
                    if !files
                        .iter()
                        .any(|known| known.entry.filename == file.entry.filename)
                    {
                        files.push(file);
                    }
                }
            }
            if files.is_empty() {
                if !denied.is_empty()
                    && let Some(judgement) = judgement
                {
                    return Err(super::raw_group::the_group_reservation_explanation(
                        repository, &judgement, &denied,
                    )
                    .into());
                }
                return Err(RawApiError::NotFound.into());
            }
            Ok(ProjectView {
                name: project.as_str().to_string(),
                files,
            })
        }
        _ => the_leaf_project(state, context, repository, project, correlation).await,
    }
}

/// The document of a repository that is not a group.
async fn the_leaf_project(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    project: &ProjectName,
    correlation: &str,
) -> Result<ProjectView, Failure> {
    if repository.mode == RepositoryMode::Proxy {
        return the_proxy_project(state, context, repository, project, correlation).await;
    }
    let asset_path = project.as_str();
    let asset = the_current_asset(state, context, &repository.repo_key, asset_path)
        .await?
        .ok_or(RawApiError::NotFound)?;
    let bytes = the_content_store(state)?
        .get(&asset.digest)
        .await
        .map_err(|error| the_blob_error(&error))?;
    let document = the_document(&bytes)?;
    let files = the_admitted_files(state, context, repository, document.files).await?;
    Ok(ProjectView {
        name: document.name,
        files: files
            .into_iter()
            .map(|entry| FileView {
                entry,
                owner: repository.clone(),
            })
            .collect(),
    })
}

/// The files the trust channel lets be served.
///
/// The open channel filters nothing. The `trusted` channel only lets be served the
/// digests with an active grant, and the index does not announce what cannot be
/// downloaded: it is the same as what the OCI surface does with its listings.
async fn the_admitted_files(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    files: Vec<FileEntry>,
) -> Result<Vec<FileEntry>, Failure> {
    match crate::trust::servable(state, context.account().as_str(), repository).await {
        Ok(None) => Ok(files),
        Ok(Some(allowed)) => Ok(files
            .into_iter()
            .filter(|file| allowed.contains(&file.sha256))
            .collect()),
        Err(error) => Err(the_admission_error(error).into()),
    }
}

/// The document of a project exactly as the upstream published it.
async fn the_proxy_project(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    project: &ProjectName,
    correlation: &str,
) -> Result<ProjectView, Failure> {
    let upstream = the_upstream(repository)?;
    let asset_path = project.as_str();
    let url = upstream.target(asset_path).ok_or(RawApiError::NotFound)?;
    let served = the_proxy_asset_at(
        state,
        context,
        repository,
        asset_path,
        Some(url),
        Some(pypi::JSON_CONTENT_TYPE),
        correlation,
    )
    .await?;
    let bytes = the_content_store(state)?
        .get(&served.asset.digest)
        .await
        .map_err(|error| the_blob_error(&error))?;
    let document = the_document(&bytes)?;
    let files = the_admitted_files(state, context, repository, document.files).await?;
    let files = files
        .into_iter()
        .filter_map(|entry| {
            // An upstream may declare a name that is not a
            // distribution. It is not announced: a link that cannot be
            // downloaded is a broken link, and the index does not publish them.
            pypi::Distribution::parse(&entry.filename).ok()?;
            // The upstream URL may be relative: it is relative to its base.
            let entry = match entry.source.as_deref() {
                Some(source) if !source.contains("://") => {
                    let source = upstream
                        .base
                        .join(source)
                        .map(|url| url.to_string())
                        .unwrap_or_else(|_| source.to_string());
                    FileEntry {
                        source: Some(source),
                        ..entry
                    }
                }
                _ => entry,
            };
            Some(FileView {
                entry,
                owner: repository.clone(),
            })
        })
        .collect();
    Ok(ProjectView {
        // The upstream name may come unnormalized: the index
        // states it as PEP 503 does, or does not state it.
        name: ProjectName::parse(&document.name)
            .map(|name| name.as_str().to_string())
            .unwrap_or_else(|_| project.as_str().to_string()),
        files,
    })
}

/// Reads a project document.
fn the_document(bytes: &[u8]) -> Result<ProjectDocument, Failure> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| RawApiError::BadGateway("the stored project document is not valid"))?;
    pypi::ProjectDocument::from_value(&value)
        .map_err(|_| RawApiError::BadGateway("the stored project document is not valid"))
        .map_err(Failure::Raw)
}

/// The distribution of a project.
async fn the_file(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    project: &ProjectName,
    filename: &str,
    ask: &Ask<'_>,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::download_asset(), repository)?;
    let view = the_project(state, context, repository, project, ask.correlation).await?;
    let file = view
        .files
        .iter()
        .find(|file| file.entry.filename == filename)
        .ok_or(RawApiError::NotFound)?;
    let asset_path = format!("{project}/{filename}");
    let asset = the_distribution(
        state,
        context,
        &file.owner,
        &asset_path,
        file,
        ask.correlation,
    )
    .await?;
    // What is served must be what the index promised.
    if asset.digest.value != file.entry.sha256 {
        return Err(RawApiError::BadGateway(
            "the distribution does not match the hash the index published",
        )
        .into());
    }
    let etag = conditional::the_etag(&asset.digest);
    match conditional::the_read_condition(
        conditional::the_if_match(ask.headers).as_deref(),
        conditional::the_if_none_match(ask.headers).as_deref(),
        Some(&etag),
    ) {
        conditional::ReadOutcome::Failed => return Err(RawApiError::PreconditionFailed.into()),
        conditional::ReadOutcome::NotModified => {
            let mut response = StatusCode::NOT_MODIFIED.into_response();
            apply_common_headers(&mut response, &etag, ask.correlation);
            return Ok(response);
        }
        conditional::ReadOutcome::Serve => {}
    }
    let store = the_content_store(state)?;
    let length = asset.size_bytes;
    // The range is served as the raw surface serves it: a `Range` that
    // is understood is a 206 with its `Content-Range`, and one that is not
    // understood is ignored. Today's `pip` requests ranges so as not to download
    // the same thing twice.
    let request = match crate::raw::range::the_range(
        ask.headers
            .get(header::RANGE)
            .and_then(|value| value.to_str().ok()),
        length,
    ) {
        Ok(request) => request,
        Err(crate::raw::range::RangeError::Unsatisfiable) => {
            return Err(RawApiError::RangeNotSatisfiable(length).into());
        }
    };
    let (body, status, content_range, response_length) = match request {
        crate::raw::range::RangeRequest::Whole => {
            if ask.head {
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
        crate::raw::range::RangeRequest::Single { first, last } => {
            let served = last - first + 1;
            let content_range = crate::raw::range::the_content_range(first, last, length);
            if ask.head {
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
    apply_common_headers(&mut response, &etag, ask.correlation);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response.headers_mut().insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    if let Some(content_range) = content_range
        && let Ok(value) = HeaderValue::from_str(&content_range)
    {
        response.headers_mut().insert(header::CONTENT_RANGE, value);
    }
    if ask.head
        && let Ok(value) = HeaderValue::from_str(&response_length.to_string())
    {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    Ok(response)
}

/// The asset of a distribution, wherever it comes from.
///
/// In a hosted one it is in the catalog. In a proxy it is fetched from the upstream via
/// the URL the upstream itself declares, and stored so as not to request
/// it again.
async fn the_distribution(
    state: &AppState,
    context: &VerifiedContext,
    owner: &RepositoryDescriptor,
    asset_path: &str,
    file: &FileView,
    correlation: &str,
) -> Result<hodei_catalog::AssetRecord, Failure> {
    if owner.mode == RepositoryMode::Proxy {
        let Some(source) = file.entry.source.as_deref() else {
            return Err(RawApiError::BadGateway(
                "the upstream does not say where the distribution is",
            )
            .into());
        };
        let url = Url::parse(source)
            .map_err(|_| RawApiError::BadGateway("the upstream declares an unusable URL"))?;
        let served = the_proxy_asset_at(
            state,
            context,
            owner,
            asset_path,
            Some(url),
            None,
            correlation,
        )
        .await?;
        // The trust gate goes here too: the software the upstream
        // brings is software, and what the channel does not admit is not
        // served even if it comes from outside.
        crate::trust::admit(
            state,
            context.account().as_str(),
            owner,
            &served.asset.digest,
        )
        .await
        .map_err(the_admission_error)?;
        return Ok(served.asset);
    }
    let asset = the_current_asset(state, context, &owner.repo_key, asset_path)
        .await?
        .ok_or(RawApiError::NotFound)?;
    // The trust gate goes before serving: what is not admitted is
    // a 404, like what does not exist.
    crate::trust::admit(state, context.account().as_str(), owner, &asset.digest)
        .await
        .map_err(the_admission_error)?;
    Ok(asset)
}

/// The upstream of a proxy repository.
fn the_upstream(repository: &RepositoryDescriptor) -> Result<UpstreamConfig, Failure> {
    UpstreamConfig::of(&repository.config)
        .ok()
        .flatten()
        .ok_or_else(|| Failure::plain(StatusCode::BAD_GATEWAY, "the proxy is not configured"))
}

/// A response with the body already ready.
fn the_page(
    body: Vec<u8>,
    content_type: &'static str,
    etag: Option<&str>,
    correlation: &str,
) -> Response {
    let length = body.len();
    let mut response = (StatusCode::OK, Body::from(body)).into_response();
    if let Some(etag) = etag {
        apply_common_headers(&mut response, etag, correlation);
    }
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response.headers_mut().insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    if let Ok(value) = HeaderValue::from_str(&length.to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    response
}

// ---------------------------------------------------------------------------
// Writes
// ---------------------------------------------------------------------------

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

/// What a document write left behind.
struct Written {
    /// The digest of the new document.
    digest: ContentDigest,
    /// Whether the document changed.
    changed: bool,
}

/// Applies a change to a project's document and publishes it.
///
/// The change is **re-evaluated** against what is there on each attempt: if another
/// writer moved the pointer between the read and the write, the catalog
/// rejects the precondition, it is re-read and applied again.
async fn update_document<F>(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    project: &ProjectName,
    create: bool,
    correlation: &str,
    mut change: F,
) -> Result<Written, Failure>
where
    F: FnMut(&mut ProjectDocument) -> Result<bool, Failure>,
{
    let (_, writer) = the_catalog(state)?;
    let store = the_content_store(state)?;
    for _ in 0..MAX_ATTEMPTS {
        let current =
            the_current_asset(state, context, &repository.repo_key, project.as_str()).await?;
        let (mut document, precondition) = match &current {
            Some(asset) => {
                let bytes = store
                    .get(&asset.digest)
                    .await
                    .map_err(|error| the_blob_error(&error))?;
                let value: Value =
                    serde_json::from_slice(&bytes).map_err(|_| RawApiError::Internal)?;
                let document =
                    pypi::ProjectDocument::from_value(&value).map_err(|_| RawApiError::Internal)?;
                (document, Precondition::Present(vec![asset.digest.clone()]))
            }
            None if create => (ProjectDocument::new(project.as_str()), Precondition::Absent),
            None => return Err(RawApiError::NotFound.into()),
        };
        let changed = change(&mut document)?;
        if !changed {
            return Ok(Written {
                digest: current
                    .map(|asset| asset.digest)
                    .unwrap_or(ContentDigest::sha256("")),
                changed: false,
            });
        }
        let bytes = serde_json::to_vec(&document.to_value(&the_files_base(&repository.repo_key)))
            .map_err(|_| RawApiError::Internal)?;
        let (digest, length) = write_the_content(
            state,
            context.account().as_str(),
            Body::from(bytes),
            None,
            None,
        )
        .await?;
        let publication = Publication {
            repository: repository.repo_key.clone(),
            asset_path: project.as_str().to_string(),
            // Each snapshot is a distinct version of the document: it is
            // identified by its content, and the previous one is still there.
            coordinates: PackageCoordinates {
                package_name: format!("{}{}", project.as_str(), pypi::PROJECT_COORDINATES_SUFFIX),
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
            Err(error) => return Err(the_publication_error(&error).into()),
        }
    }
    Err(Failure::plain(StatusCode::CONFLICT, CONTENDED))
}

/// Whether the project index can store this entry.
///
/// The project document has a cap: that of the content this service
/// accepts. If the entry does not fit, this is said **before** publishing the
/// distribution. The size is checked against the document as it is now, and the
/// document that gets written afterwards checks it again: between the two
/// things the world may have changed, and what rules is the second
/// check.
async fn the_index_fits(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    project: &ProjectName,
    entry: &FileEntry,
) -> Result<(), Failure> {
    let mut document =
        match the_current_asset(state, context, &repository.repo_key, project.as_str()).await? {
            Some(asset) => {
                let bytes = the_content_store(state)?
                    .get(&asset.digest)
                    .await
                    .map_err(|error| the_blob_error(&error))?;
                let value: Value =
                    serde_json::from_slice(&bytes).map_err(|_| RawApiError::Internal)?;
                pypi::ProjectDocument::from_value(&value).map_err(|_| RawApiError::Internal)?
            }
            None => pypi::ProjectDocument::new(project.as_str()),
        };
    if document.file(&entry.filename).is_some() {
        // It was already there: the document does not grow.
        return Ok(());
    }
    document
        .add(entry.clone())
        .map_err(|_| RawApiError::Internal)?;
    let bytes = serde_json::to_vec(&document.to_value(&the_files_base(&repository.repo_key)))
        .map_err(|_| RawApiError::Internal)?;
    if bytes.len() > state.max_body_size {
        return Err(RawApiError::PayloadTooLarge.into());
    }
    Ok(())
}

/// `POST /pypi/{repository}/`: the `twine` upload.
async fn upload(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    request: Request<Body>,
    correlation: &str,
) -> Result<Response, Failure> {
    the_write_gate(state, context, repository)?;
    let (fields, file) = the_multipart(state, request).await?;
    let (filename, bytes) = file.ok_or(pypi::PypiError::MissingFile)?;
    // The hash is computed here: what the client declares is checked
    // against the bytes that arrived.
    let sha256 = hex_of(&Sha256::digest(&bytes));
    let upload = pypi::parse_upload(&fields, &filename, &sha256)?;

    let asset_path = format!("{}/{}", upload.project.as_str(), upload.filename);
    let entry = FileEntry {
        filename: upload.filename.clone(),
        sha256: upload.sha256.clone(),
        size: Some(bytes.len() as u64),
        requires_python: upload.requires_python.clone(),
        yanked: None,
        source: None,
    };
    // The project index must be able to hold it: if it does not fit, this is said before
    // publishing the distribution. Publishing and then being unable to announce
    // leaves the content published and invisible.
    the_index_fits(state, context, repository, &upload.project, &entry).await?;

    let (digest, length) = write_the_content(
        state,
        context.account().as_str(),
        Body::from(bytes),
        None,
        None,
    )
    .await?;
    let (_, writer) = the_catalog(state)?;
    let publication = Publication {
        repository: repository.repo_key.clone(),
        asset_path: asset_path.clone(),
        coordinates: PackageCoordinates {
            // A PyPI version may have more than one file--the wheel
            // and the sdist-- and each is distinct content. That is why the
            // name in the coordinates is the file's: two
            // distributions of the same version are not the same published
            // version.
            package_name: asset_path.clone(),
            package_version: upload.version.clone(),
        },
        digest: digest.clone(),
        size_bytes: length,
        tags: Vec::new(),
        actor: context.principal().subject().as_str().to_string(),
        correlation_id: correlation.to_string(),
        // A distribution is immutable: publishing it again with other
        // bytes is a conflict, not a replacement.
        overwrite: false,
        precondition: None,
        subject: None,
    };
    match writer
        .publish(context.account().as_str(), publication)
        .await
    {
        Ok(_) => {}
        Err(CatalogError::Conflict { .. }) => {
            // The same content again is not a problem: it is the same.
            let existing =
                the_current_asset(state, context, &repository.repo_key, &asset_path).await?;
            if existing.is_none_or(|asset| asset.digest != digest) {
                return Err(pypi::PypiError::FileExists.into());
            }
        }
        Err(error) => return Err(the_publication_error(&error).into()),
    }

    let written = update_document(
        state,
        context,
        repository,
        &upload.project,
        true,
        correlation,
        |document| {
            if document.file(&entry.filename).is_some() {
                // It was already there: the document does not change.
                return Ok(false);
            }
            document.add(entry.clone())?;
            Ok(true)
        },
    )
    .await?;

    let body = serde_json::json!({
        "ok": true,
        "project": upload.project.as_str(),
        "version": upload.version,
        "filename": upload.filename,
        "sha256": upload.sha256,
        "changed": written.changed,
    });
    let mut response = (StatusCode::OK, axum::Json(body)).into_response();
    apply_common_headers(&mut response, &conditional::the_etag(&digest), correlation);
    Ok(response)
}

/// The fields and the file of an upload.
///
/// The limit is the content's: a distribution that does not fit is not
/// published, and this is said before spending it. The limit counts **all** the
/// bytes of the upload, not those of one field: `twine` sends a few text
/// fields and the file, and a client that sends a thousand empty
/// fields does not spend memory.
async fn the_multipart(
    state: &AppState,
    request: Request<Body>,
) -> Result<(Vec<(String, String)>, Option<(String, Vec<u8>)>), Failure> {
    let mut multipart = axum::extract::Multipart::from_request(request, &state)
        .await
        .map_err(|_| {
            Failure::plain(
                StatusCode::BAD_REQUEST,
                "the upload is not a multipart form",
            )
        })?;
    let limit = state.max_body_size;
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut file: Option<(String, Vec<u8>)> = None;
    let mut total = 0usize;
    let mut parts = 0usize;
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|error| the_upload_error(error.status()))?
    {
        parts += 1;
        if parts > MAX_PARTS {
            return Err(Failure::plain(
                StatusCode::BAD_REQUEST,
                "the upload carries too many fields",
            ));
        }
        let name = field.name().unwrap_or_default().to_string();
        let filename = field.file_name().map(str::to_string);
        let mut bytes: Vec<u8> = Vec::new();
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|error| the_upload_error(error.status()))?
        {
            total += chunk.len();
            if total > limit {
                return Err(RawApiError::PayloadTooLarge.into());
            }
            bytes.extend_from_slice(&chunk);
        }
        match filename {
            Some(filename) => {
                if name != "content" {
                    // A part with a file name that is not the
                    // distribution--the signature of `twine upload --sign`--
                    // is not a second distribution: it is ignored. This
                    // surface does not store signatures.
                    continue;
                }
                if file.is_some() {
                    // Two distributions in one upload: one is not chosen, it is
                    // said no.
                    return Err(Failure::plain(
                        StatusCode::BAD_REQUEST,
                        "the upload carries more than one distribution",
                    ));
                }
                file = Some((filename, bytes));
            }
            None => fields.push((name, String::from_utf8_lossy(&bytes).into_owned())),
        }
    }
    Ok((fields, file))
}

/// The error of an upload that cannot be read.
///
/// A body that exceeds the cap is not a bad form: it is a body
/// that is too large, and the `413` is stated by `axum` itself. It is the same
/// `413` the handler states when what exceeds the cap is the file's
/// bytes: what the client needs to know is that it does not fit, and
/// not which layer the rejection comes from.
fn the_upload_error(status: StatusCode) -> Failure {
    if status == StatusCode::PAYLOAD_TOO_LARGE {
        return RawApiError::PayloadTooLarge.into();
    }
    Failure::plain(StatusCode::BAD_REQUEST, "the upload cannot be read")
}

/// `POST /pypi/{repository}/yank`: withdraws a version.
///
/// It is an own extension: PyPI has no API for this, and a command-line
/// client does not have one. What it does is what PEP 592 says:
/// the version stops being eligible, and remains downloadable if requested
/// exactly.
async fn yank(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    request: Request<Body>,
    correlation: &str,
) -> Result<Response, Failure> {
    the_write_gate(state, context, repository)?;
    let body = axum::body::to_bytes(request.into_body(), 64 * 1024)
        .await
        .map_err(|_| RawApiError::BadRequest("the body cannot be read"))?;
    let value: Value = serde_json::from_slice(&body)
        .map_err(|_| RawApiError::BadRequest("the body is not a JSON object"))?;
    let project = value
        .get("project")
        .and_then(Value::as_str)
        .ok_or(RawApiError::BadRequest("the project is missing"))?;
    let project = ProjectName::parse(project)
        .map_err(|_| RawApiError::BadRequest("the project name is not valid"))?;
    let version = value
        .get("version")
        .and_then(Value::as_str)
        .ok_or(RawApiError::BadRequest("the version is missing"))?;
    // The version is canonicalized: the document stores versions as the
    // file names carry them, and those are canonical.
    let Some(version) = pypi::canonical_version(version) else {
        return Err(RawApiError::BadRequest("the version is not valid").into());
    };
    let yanked = value.get("yanked").and_then(Value::as_bool).unwrap_or(true);
    let reason = value
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let written = update_document(
        state,
        context,
        repository,
        &project,
        false,
        correlation,
        |document| {
            let yanked = if yanked { Some(reason.clone()) } else { None };
            Ok(document.set_yanked(&version, yanked))
        },
    )
    .await?;
    let body = serde_json::json!({
        "ok": true,
        "project": project.as_str(),
        "version": version,
        "yanked": yanked,
        "changed": written.changed,
    });
    let mut response = (StatusCode::OK, axum::Json(body)).into_response();
    apply_common_headers(
        &mut response,
        &conditional::the_etag(&written.digest),
        correlation,
    );
    Ok(response)
}
