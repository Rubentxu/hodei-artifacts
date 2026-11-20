//! The **terraform module registry** surface: the private module registry
//! protocol v1 of Terraform/OpenTofu over a **hosted** Terraform
//! repository of Hodei.
//!
//! It lives under `/terraform/{repository}/...` — the same prefix the
//! provider network mirror speaks, with the modality deciding the
//! dialect: a **proxy** repository speaks the mirror (its handler lives
//! in `handlers::terraform`) and a **hosted** one speaks the registry
//! protocol handled here.
//!
//! The dialect is the one the client fixes (terraform 1.10.1,
//! `internal/registry/client.go` + `terraform-svchost/disco` +
//! `go-getter` v1.7):
//!
//! - Service discovery at the host root: `/.well-known/terraform.json`
//!   answers `{"modules.v1": "<base>"}`. The CLI always fetches that
//!   document over **https**, so a plain-http hub declares the service
//!   directly in the CLI config (`host` block + `services`), which is
//!   `ForceHostServices` of the discovery package. This release serves
//!   only `modules.v1`: a provider origin registry is **not implemented**
//!   (HSP-158 AC3 conditional) and nothing unimplemented is announced.
//! - `GET {base}/{namespace}/{name}/{system}/versions`: the version
//!   index; the client reads `modules[0].versions[].version`.
//! - `GET {base}/{namespace}/{name}/{system}/{version}/download`: a body
//!   less answer whose `X-Terraform-Get` is a go-getter source. A value
//!   starting with `/` is resolved by the client against the URL it
//!   asked, so the location is the artifact path of this surface with a
//!   `?checksum=sha256:...` query that go-getter verifies (the archive
//!   extension makes go-getter treat the download as a file it checks).
//! - The artifact bytes: go-getter carries **no** registry credentials,
//!   so the artifact read accepts the hub Bearer and the netrc `Basic`
//!   the client sends (`machine ... password <key>`: the password is the
//!   key, the login is ignored).
//!
//! One published module version is **one immutable asset**:
//! `artifacts/{namespace}/{name}/{system}/{version}.tar.gz`. The
//! publication is the hub's own (`POST publish/...`), because the CLI
//! cannot publish modules: the tarball arrives whole, the hub computes
//! its sha256 at the commit, and the receipt hands it back. Republishing
//! a version — same bytes or not — is a `409`: a registry version is
//! immutable by contract.
//!
//! The authentication is the one of the hub, mapped to the dialect, and
//! the scoping by account is the one of the hub, not one of the format.

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::Engine;
use futures_util::StreamExt as _;
use hodei_authz::{Credentials, VerifiedContext, verify_principal};
use hodei_catalog::{
    AssetRecord, CatalogError, PackageCoordinates, Publication, RepositoryDescriptor,
    RepositoryFormat, RepositoryMode,
    terraform_modules::{self, ModuleRef, ModuleRegistryError, Route},
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
use crate::handlers::{bearer_token, correlation_id, record_audit};

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

impl From<ModuleRegistryError> for Failure {
    fn from(error: ModuleRegistryError) -> Self {
        // A path the grammar refuses is a resource that does not exist:
        // the registry does not explain why a path is bad, it says there
        // is nothing there. Nothing leaks to whoever probes the service.
        if error.is_path_error() {
            Self::Raw(RawApiError::NotFound)
        } else {
            Self::Plain(StatusCode::NOT_FOUND, error.message())
        }
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Raw(error) => error.the_verdict(),
            Self::Plain(status, message) => (status, message),
        };
        // The registry dialect carries its errors as `errors[]`, the shape
        // the client prints when a listing or a download fails.
        let body = json!({ "errors": [message] });
        let mut response = (status, axum::Json(body)).into_response();
        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
    }
}

/// The principal verified out of the client credentials.
///
/// The requests the CLI makes carry `Authorization: Bearer` — the token
/// the CLI config declares as `credentials` for the host. Both are
/// verified the same way, as a bearer credential of the principal, and
/// the scoping by account is the one the token carries.
async fn the_registry_context(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<VerifiedContext, Failure> {
    let token = bearer_token(headers).ok_or(Failure::Plain(
        StatusCode::UNAUTHORIZED,
        "the terraform registry needs an API key as a Bearer credential".to_string(),
    ))?;
    the_verified(state, &Credentials::bearer(token)).await
}

/// The principal of an artifact read: Bearer, or the netrc `Basic` whose
/// password is the key.
///
/// go-getter downloads the artifact of a registry module and it carries
/// no registry credential: its HTTP getter reads `~/.netrc` (or `$NETRC`)
/// and sends the entry as Basic auth. The password of the netrc entry is
/// the API key and the login is decoration, so the Basic is unwrapped and
/// the password is verified as the bearer credential it stands for.
async fn the_artifact_context(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<VerifiedContext, Failure> {
    let value = headers
        .get(header::AUTHORIZATION)
        .and_then(|header| header.to_str().ok())
        .unwrap_or_default();
    let credential = if let Some(token) = bearer_token(headers) {
        Credentials::bearer(token)
    } else if let Some((scheme, encoded)) = value.split_once(' ')
        && scheme.eq_ignore_ascii_case("Basic")
    {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded.trim())
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .ok_or(Failure::Plain(
                StatusCode::UNAUTHORIZED,
                "the Basic credential of the artifact read is not decodable".to_string(),
            ))?;
        // The netrc convention: the password carries the credential, the
        // login does not matter. What the client wrote before the colon
        // stays out of the key.
        let password = decoded.rsplit(':').next().unwrap_or_default().to_string();
        Credentials::bearer(password)
    } else {
        return Err(Failure::Plain(
            StatusCode::UNAUTHORIZED,
            "the artifact read needs a Bearer or a netrc Basic credential".to_string(),
        ));
    };
    the_verified(state, &credential).await
}

async fn the_verified(
    state: &AppState,
    credential: &Credentials,
) -> Result<VerifiedContext, Failure> {
    match verify_principal(state.authenticator.as_ref(), credential).await {
        Ok(principal) => Ok(VerifiedContext::from_principal(principal)),
        Err(hodei_authz::AuthenticationError::ProviderUnavailable(_)) => {
            Err(RawApiError::Unavailable.into())
        }
        Err(_) => Err(Failure::Plain(
            StatusCode::UNAUTHORIZED,
            "the credential is not one this hub can verify".to_string(),
        )),
    }
}

// ---------------------------------------------------------------------------
// Service discovery
// ---------------------------------------------------------------------------

/// `GET /.well-known/terraform.json`: the service discovery of the host.
///
/// The discovery is host scoped and the hub is multi tenant: the account
/// of the credential decides the repository, and the answer names the one
/// Terraform **hosted** repository of that account. An account with none
/// gets a `404` the CLI turns into "host does not provide a module
/// registry", and an account with several gets a `409`: with several
/// registries the CLI config (`host` + `services`) names the repository
/// explicitly.
pub async fn discovery(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, Failure> {
    let context = the_registry_context(&state, &headers).await?;
    let correlation = correlation_id(&headers, &state.identity);
    let searcher = state
        .catalog_search
        .as_ref()
        .ok_or(RawApiError::Unavailable)?;
    let repositories = searcher
        .repositories(context.account().as_str())
        .await
        .map_err(|error| Failure::from(the_publication_error(&error)))?;
    let modules: Vec<&RepositoryDescriptor> = repositories
        .iter()
        .filter(|repository| {
            repository.format == RepositoryFormat::Terraform
                && repository.mode == RepositoryMode::Hosted
        })
        .collect();
    let base = match modules.len() {
        1 => format!("/terraform/{}/v1/modules/", modules[0].repo_key),
        0 => {
            return Err(Failure::Plain(
                StatusCode::NOT_FOUND,
                "the account has no terraform module registry: discovery names nothing".to_string(),
            ));
        }
        _ => {
            return Err(Failure::Plain(
                StatusCode::CONFLICT,
                "the account has several terraform module registries: name one in the CLI config services"
                    .to_string(),
            ));
        }
    };
    let mut response = (
        StatusCode::OK,
        axum::Json(terraform_modules::discovery_document(&base)),
    )
        .into_response();
    apply_common_headers(&mut response, "\"json\"", &correlation);
    Ok(response)
}

// ---------------------------------------------------------------------------
// The registry surface of a hosted repository
// ---------------------------------------------------------------------------

/// Any request of `/terraform/{repository}/{registry path}`.
///
/// The mirror handler delegates here when the path belongs to the
/// registry protocol; the repository is resolved with the credential of
/// the request, and a repository that is not **hosted** Terraform does
/// not exist for this surface.
pub async fn handle(
    State(state): State<AppState>,
    method: axum::http::Method,
    headers: HeaderMap,
    request: axum::extract::Request<Body>,
    repository_key: String,
    rest: String,
) -> Response {
    // The registry dialect answers with its own error shape: the surface
    // answers whole, and the mirror only hands what it answered.
    match the_registry(state, method, headers, request, repository_key, rest).await {
        Ok(response) => response,
        Err(failure) => failure.into_response(),
    }
}

async fn the_registry(
    state: AppState,
    method: axum::http::Method,
    headers: HeaderMap,
    request: axum::extract::Request<Body>,
    repository_key: String,
    rest: String,
) -> Result<Response, Failure> {
    let correlation = correlation_id(&headers, &state.identity);
    let route = Route::parse(&rest)?;
    let subject = the_audit_subject(&route);
    let action = the_audit_action(&route);

    // The artifact read is the request go-getter makes: its credential is
    // the netrc one, not the registry one, and it decides who is asking.
    let context = match &route {
        Route::Artifact { .. } => the_artifact_context(&state, &headers).await?,
        _ => the_registry_context(&state, &headers).await?,
    };
    let repository = the_repository_in(
        &state,
        &context,
        &repository_key,
        &[RepositoryFormat::Terraform],
    )
    .await?;
    if repository.mode != RepositoryMode::Hosted {
        return Err(RawApiError::NotFound.into());
    }

    let reading = method == axum::http::Method::GET || method == axum::http::Method::HEAD;
    let outcome = match (&route, reading, method == axum::http::Method::POST) {
        (_, true, _) => {
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
                method == axum::http::Method::HEAD,
                &correlation,
            )
            .await
        }
        (Route::Publish { .. }, false, true) => {
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
            publish(
                &state,
                &context,
                &repository,
                route,
                request,
                &headers,
                &correlation,
            )
            .await
        }
        _ => Err(RawApiError::MethodNotAllowed(
            "this terraform module registry operation is not supported",
        )
        .into()),
    };
    record_outcome(
        &state,
        &context,
        &repository_key,
        &subject,
        action,
        &outcome,
        &correlation,
    );
    outcome
}

/// The action the audit trail names: a publish is an upload, every other
/// read is a download.
fn the_audit_action(route: &Route) -> &'static str {
    match route {
        Route::Publish { .. } => "upload",
        _ => "download",
    }
}

/// The subject the audit trail names: the module coordinates the route
/// carries.
fn the_audit_subject(route: &Route) -> String {
    match route {
        Route::Versions { reference } | Route::Download { reference, .. } => reference.source(),
        Route::Artifact { reference, .. } | Route::Publish { reference, .. } => reference.source(),
    }
}

fn record_outcome(
    state: &AppState,
    context: &VerifiedContext,
    repository: &str,
    subject: &str,
    action: &'static str,
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
        action,
        &format!("{repository}/{subject}"),
        result,
        correlation,
    );
}

/// A read of the registry: the version index, the download redirect and
/// the artifact bytes.
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
        Route::Versions { reference } => {
            let prefix = format!(
                "artifacts/{}/{}/{}/",
                reference.namespace, reference.name, reference.system
            );
            let records = assets_under(state, context, repository, &prefix).await?;
            let mut versions: Vec<&str> = records.iter().filter_map(the_version_of).collect();
            versions.sort();
            versions.dedup();
            Ok(json_response(
                StatusCode::OK,
                &terraform_modules::versions_document(&reference, &versions),
                correlation,
            ))
        }
        Route::Download { reference, version } => {
            let asset_path = reference.artifact_path(&version);
            let record = the_version_asset(state, context, repository, &asset_path).await?;
            let Some(record) = record else {
                return Err(Failure::Plain(
                    StatusCode::NOT_FOUND,
                    format!(
                        "module \"{}\" version \"{}\" not found",
                        reference.source(),
                        version
                    ),
                ));
            };
            // The location names the artifact of this surface and the
            // sha256 the CAS holds: the client verifies the bytes it
            // downloads against the digest of the very content stored.
            let location = terraform_modules::download_location(
                repository_key,
                &reference,
                &version,
                &record.digest.value,
            );
            let mut response = (StatusCode::NO_CONTENT, Body::empty()).into_response();
            response.headers_mut().insert(
                header::HeaderName::from_static("x-terraform-get"),
                HeaderValue::from_str(&location).map_err(|_| RawApiError::NotFound)?,
            );
            apply_common_headers(
                &mut response,
                &crate::raw::conditional::the_etag(&record.digest),
                correlation,
            );
            Ok(response)
        }
        Route::Artifact { reference, version } => {
            the_artifact(
                state,
                context,
                repository,
                &reference,
                &version,
                head,
                correlation,
            )
            .await
        }
        Route::Publish { .. } => {
            Err(RawApiError::MethodNotAllowed("a publish is not a read").into())
        }
    }
}

/// The artifact bytes of a version, served from the CAS.
///
/// The bytes come out exactly as they went in: the sha256 the client
/// verifies against `?checksum=` is the digest of the asset, the one the
/// publication computed at the commit.
async fn the_artifact(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    reference: &ModuleRef,
    version: &str,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    let asset_path = reference.artifact_path(version);
    let resolved =
        the_resolved_asset(state, context, repository.clone(), &asset_path, correlation).await?;
    let Resolved {
        asset,
        served,
        resolved_by: _,
        repository: backing,
    } = resolved;
    crate::trust::admit(state, context.account().as_str(), &backing, &asset.digest)
        .await
        .map_err(the_admission_error)?;
    let etag = crate::raw::conditional::the_etag(&asset.digest);
    let bytes = if head {
        Vec::new()
    } else {
        the_content_store(state)?
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
    let mut response = (StatusCode::OK, Body::from(bytes)).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/gzip"),
    );
    response.headers_mut().insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    apply_common_headers(&mut response, &etag, correlation);
    apply_cache_headers(&mut response, &served);
    if head && let Ok(value) = HeaderValue::from_str(&length.to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    Ok(response)
}

// ---------------------------------------------------------------------------
// Publish
// ---------------------------------------------------------------------------

/// The publication the hub defines: the tarball arrives whole for one new
/// version, the hub computes its sha256 at the commit and the receipt
/// hands it back. A version already there is a `409`, whatever the bytes:
/// a registry version is immutable by contract, so a republish is never a
/// rewrite — not even an echo of the same content.
#[allow(clippy::too_many_arguments)]
async fn publish(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    route: Route,
    request: axum::extract::Request<Body>,
    headers: &HeaderMap,
    correlation: &str,
) -> Result<Response, Failure> {
    let Route::Publish { reference, version } = route else {
        return Err(RawApiError::NotFound.into());
    };

    // The immutability is decided before anything is stored: a version
    // that exists is refused whatever the bytes, because a registry
    // version is immutable by contract. The catalog refuses a different
    // content at the commit (the race of two publishes), and the check
    // here refuses even the echo of the same bytes.
    let asset_path = reference.artifact_path(&version);
    if the_version_asset(state, context, repository, &asset_path)
        .await?
        .is_some()
    {
        return Err(Failure::Plain(
            StatusCode::CONFLICT,
            format!(
                "the version {version} of the module {} is already published: a registry version is immutable",
                reference.source()
            ),
        ));
    }

    let bytes = read_the_whole_body(state, request).await?;
    // A module tarball is a gzip archive: an upload that is not one is not
    // a tarball nobody named, it is an upload that is not what it says.
    if bytes.first() != Some(&0x1f) || bytes.get(1) != Some(&0x8b) {
        return Err(Failure::Plain(
            StatusCode::BAD_REQUEST,
            "the module artifact is not a gzip archive".to_string(),
        ));
    }

    // The write lands in the store when the bytes are the ones the
    // publish declared, the same promise the download hands back later.
    let declared = match declared_from_headers(headers) {
        Some(digest) => Some(digest),
        None if headers.get("x-hodei-sha256").is_some() => {
            return Err(Failure::Plain(
                StatusCode::BAD_REQUEST,
                "the declared sha256 of the publish is not a sha256".to_string(),
            ));
        }
        None => None,
    };
    let (digest, length) = write_the_content(
        state,
        context.account().as_str(),
        Body::from(bytes),
        declared,
        None,
    )
    .await?;

    let asset_path = reference.artifact_path(&version);
    let coordinates = PackageCoordinates {
        package_name: reference.package_name(),
        package_version: version.clone(),
    };
    let (_, writer) = the_catalog(state)?;
    let publication = Publication {
        repository: repository.repo_key.clone(),
        asset_path,
        coordinates,
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
        Ok(_) => Ok(json_response(
            StatusCode::CREATED,
            &terraform_modules::publish_receipt(&reference, &version, &digest.value, length),
            correlation,
        )),
        Err(CatalogError::Conflict { .. }) => Err(Failure::Plain(
            StatusCode::CONFLICT,
            format!(
                "the version {version} of the module {} is already published: a registry version is immutable",
                reference.source()
            ),
        )),
        Err(error) => Err(Failure::from(the_publication_error(&error))),
    }
}

/// The declared digest, read from the publish headers: `X-Hodei-Sha256`.
/// A publish that declares one asks the commit to check the bytes against
/// it; a publish that does not is checked by its own computed digest.
fn declared_from_headers(headers: &HeaderMap) -> Option<ContentDigest> {
    let declared = headers.get("x-hodei-sha256")?.to_str().ok()?;
    let declared = ContentDigest::sha256(declared.trim());
    if !declared.is_well_formed() {
        return None;
    }
    Some(declared)
}

/// The whole body of the publish, with the limit the hub fixes.
async fn read_the_whole_body(
    state: &AppState,
    request: axum::extract::Request<Body>,
) -> Result<Vec<u8>, Failure> {
    let limit = state.max_body_size;
    let mut bytes = Vec::new();
    let mut stream = request.into_body().into_data_stream();
    while let Some(chunk) = stream
        .next()
        .await
        .transpose()
        .map_err(|_| RawApiError::BadRequest("the publish body could not be read"))?
    {
        if bytes.len() + chunk.len() > limit {
            return Err(RawApiError::PayloadTooLarge.into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

// ---------------------------------------------------------------------------
// Catalog reads
// ---------------------------------------------------------------------------

/// The version a versioned artifact names, out of its path.
fn the_version_of(record: &AssetRecord) -> Option<&str> {
    record
        .asset_path
        .rsplit('/')
        .next()?
        .strip_suffix(".tar.gz")
}

/// The assets under a prefix, in the account of the principal.
async fn assets_under(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    prefix: &str,
) -> Result<Vec<AssetRecord>, Failure> {
    let (reader, _) = the_catalog(state)?;
    reader
        .assets_under(context.account().as_str(), &repository.repo_key, prefix)
        .await
        .map_err(|error| Failure::from(the_publication_error(&error)))
}

/// The asset of one version, if it is there.
async fn the_version_asset(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    asset_path: &str,
) -> Result<Option<AssetRecord>, Failure> {
    let (reader, _) = the_catalog(state)?;
    match reader
        .asset(context.account().as_str(), &repository.repo_key, asset_path)
        .await
    {
        Ok(record) => Ok(Some(record)),
        Err(CatalogError::Missing { .. }) => Ok(None),
        Err(error) => Err(Failure::from(the_publication_error(&error))),
    }
}

fn json_response(status: StatusCode, value: &Value, correlation: &str) -> Response {
    let mut response = (status, axum::Json(value.clone())).into_response();
    apply_common_headers(&mut response, "\"json\"", correlation);
    response
}
