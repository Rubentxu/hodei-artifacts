//! The **terraform** surface: the *network mirror protocol* of
//! Terraform/OpenTofu over a Hodei repository.
//!
//! Lives at `/terraform/{repository}/...` and speaks the provider mirror
//! protocol: the grammar of paths and documents is in
//! `hodei_catalog::terraform` (pure) and here there is only what needs
//! effects.
//!
//! The mode served is **mirror**, and the mirror is a proxy: each
//! read is fetched from the upstream with the cache, the receipts and the
//! single-flight of the shared infrastructure, and served **as
//! is**. Provider zips are immutable content and are stored in
//! the CAS with the digest of the bytes the upstream delivered, so
//! that the SHA256 the client verifies against its lockfile is the upstream's,
//! never a recalculation. The protocol's JSON documents are
//! revalidatable: the upstream publishes new versions.
//!
//! It is not an origin registry nor a module registry: nothing is published here,
//! and a mirror neither knows nor wants to know about `terraform publish`.
//!
//! **The zip and the signed URL.** Terraform downloads the archive from a network
//! mirror without sending any credential, so a hub that authenticates
//! every read could not serve it. The version document the mirror
//! delivers to an authenticated reader therefore declares, for each platform,
//! a short-lived signed URL of the zip (`signed_url`): scoped to that
//! zip, that repository and that account. Redeeming it needs no `Authorization`
//! and is good for nothing else. Authorization (Cedar) is evaluated on issue;
//! the trust gate is additionally applied on redeeming, by digest. Without
//! signing keys the version document fails closed and the mirror does not
//! issue URLs.

use axum::{
    body::Body,
    extract::{OriginalUri, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use hodei_catalog::{
    RepositoryDescriptor, RepositoryFormat,
    terraform::{Route, TerraformError},
};

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{correlation_id, record_audit};
use crate::resolution::{
    ResolutionError as RawApiError, apply_cache_headers, apply_common_headers, the_admission_error,
    the_blob_error, the_content_store, the_context, the_decision, the_proxy_asset_as,
    the_proxy_asset_at, the_repository_in, the_repository_of,
};
use crate::signed_url::{Claims, Scope};

/// What happens to a request that cannot be served.
#[derive(Debug)]
pub enum Failure {
    /// An error from the shared surface.
    Raw(RawApiError),
    /// An error from the mirror protocol.
    Terraform(TerraformError),
    /// The mirror cannot issue archive URLs: there is no signing key.
    SigningUnavailable,
}

impl From<RawApiError> for Failure {
    fn from(error: RawApiError) -> Self {
        Self::Raw(error)
    }
}

impl From<TerraformError> for Failure {
    fn from(error: TerraformError) -> Self {
        Self::Terraform(error)
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Raw(error) => error.the_verdict(),
            // A path the grammar rejects is a resource that does not exist:
            // the mirror does not explain why a path is bad, it says it does not
            // exist. Nothing leaks to whoever probes the service.
            Self::Terraform(_) => (
                StatusCode::NOT_FOUND,
                "there is nothing at that path".to_string(),
            ),
            // It is stated plainly: it is an operator decision, not a client
            // failure, and whoever reads the `terraform init` log must know it.
            Self::SigningUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "this mirror cannot issue archive URLs because no signing key is configured; the operator must add a key to [signing]".to_string(),
            ),
        };
        let mut response =
            (status, axum::Json(serde_json::json!({ "error": message }))).into_response();
        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
    }
}

/// The rest of the path of `/terraform/{repository}/{rest}`, without
/// decoding: the protocol rules are the only ones that may
/// interpret a `%2f`.
fn the_split(uri: &OriginalUri) -> Result<(String, String), Failure> {
    let rest = uri
        .path()
        .strip_prefix("/terraform/")
        .ok_or(RawApiError::NotFound)?;
    let (repository, rest) = rest.split_once('/').ok_or(RawApiError::NotFound)?;
    if !crate::raw::path::valid_repository_name(repository) || rest.is_empty() {
        return Err(RawApiError::NotFound.into());
    }
    Ok((repository.to_string(), rest.to_string()))
}

/// Any request to `/terraform/{repository}/...`.
///
/// The mode decides the dialect: a **proxy** repository speaks the
/// mirror protocol (here) and a **hosted** one speaks the module
/// registry protocol (delegated to `terraform_modules`). The mirror only
/// reads: a `PUT` or a `POST` is not a publication, it is a method the
/// protocol does not have.
pub async fn handle(
    State(state): State<AppState>,
    method: Method,
    uri: OriginalUri,
    headers: HeaderMap,
    request: axum::extract::Request<Body>,
) -> Result<Response, Failure> {
    let (repository_key, rest) = the_split(&uri)?;
    if hodei_catalog::terraform_modules::is_registry_path(&rest) {
        // The registry dialect answers with its own error shape:
        // its surface answers in full, and here it is only delivered.
        return Ok(crate::handlers::terraform_modules::handle(
            State(state),
            method,
            headers,
            request,
            repository_key,
            rest,
        )
        .await);
    }
    // A signed archive URL carries its own authority: it is the one request
    // that arrives without a credential, and it is judged before anything
    // else is looked at.
    if let Some(query) = uri.query()
        && Claims::is_present(query)
    {
        let correlation = correlation_id(&headers, &state.identity);
        return redeem(
            &state,
            &method,
            uri.path(),
            query,
            &repository_key,
            &rest,
            &correlation,
        )
        .await;
    }
    let context = the_context(&state, &headers).await?;
    let correlation = correlation_id(&headers, &state.identity);
    let route = Route::parse(&rest)?;
    let repository = the_repository_in(
        &state,
        &context,
        &repository_key,
        &[RepositoryFormat::Terraform],
    )
    .await?;
    // A hosted repository speaks the module registry protocol, not
    // this one: its mirror paths do not exist.
    if repository.mode != hodei_catalog::RepositoryMode::Proxy {
        return Err(RawApiError::NotFound.into());
    }

    let reading = method == Method::GET || method == Method::HEAD;
    let outcome = if reading {
        read(
            &state,
            &context,
            &repository,
            &route,
            method == Method::HEAD,
            &correlation,
        )
        .await
    } else {
        Err(RawApiError::MethodNotAllowed(
            "a mirror does not publish: the provider network mirror protocol is read-only",
        )
        .into())
    };
    let provider_label = route
        .provider_coordinates()
        .map(|(host, namespace, provider)| format!("{host}/{namespace}/{provider}"))
        .unwrap_or_else(|| "index".to_string());
    record_outcome(
        &state,
        &context,
        &repository_key,
        &provider_label,
        &outcome,
        &correlation,
    );
    outcome
}

fn record_outcome(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &str,
    provider: &str,
    outcome: &Result<Response, Failure>,
    correlation: &str,
) {
    record_outcome_of(
        state,
        context.principal().subject().as_str(),
        context.account().as_str(),
        repository,
        provider,
        outcome,
        correlation,
    );
}

fn record_outcome_of(
    state: &AppState,
    actor: &str,
    account: &str,
    repository: &str,
    provider: &str,
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
        actor,
        account,
        "download",
        &format!("{repository}/{provider}"),
        result,
        correlation,
    );
}

// ---------------------------------------------------------------------------
// Reads (mirror)
// ---------------------------------------------------------------------------

/// A mirror read: the protocol path IS the asset's path,
/// and the upstream serves by the same path. The document arrives from the
/// upstream and is served as is: rewriting an index or a zip would break
/// the checksums the client compares against its lockfile.
async fn read(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    route: &Route,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::download_asset(), repository)?;
    let asset_path = route.asset_path();
    let content_type = match route {
        Route::RootIndex | Route::ProviderIndex { .. } | Route::VersionDoc { .. } => {
            "application/json"
        }
        Route::ProviderFile { .. } => "application/zip",
    };
    let served = the_proxy_asset_at(
        state,
        context,
        repository,
        &asset_path,
        None,
        Some(content_type),
        correlation,
    )
    .await?;
    // The trust gate: a mirror is a repository too, and its channel decides
    // whether this digest may be served.
    crate::trust::admit(
        state,
        context.account().as_str(),
        repository,
        &served.asset.digest,
    )
    .await
    .map_err(the_admission_error)?;
    // The version document is rewritten per request, so it has to be read
    // even for a HEAD: its length and its etag are those of what is sent.
    let issues_urls = matches!(route, Route::VersionDoc { .. });
    let stored = if head && !issues_urls {
        Vec::new()
    } else {
        the_content_store(state)?
            .get(&served.asset.digest)
            .await
            .map_err(|error| the_blob_error(&error))?
            .to_vec()
    };
    let (bytes, etag, no_store) = if issues_urls {
        let document = sign_archives(state, context, repository, route, &stored)?;
        let etag = format!("\"sha256-{}\"", hex_of_digest(&document));
        (document, etag, true)
    } else {
        (
            stored,
            crate::raw::conditional::the_etag(&served.asset.digest),
            false,
        )
    };
    let length = if head && !issues_urls {
        served.asset.size_bytes as usize
    } else {
        bytes.len()
    };
    let bytes = if head { Vec::new() } else { bytes };
    let mut response = body_response(StatusCode::OK, bytes, content_type, &etag, correlation);
    apply_cache_headers(&mut response, &Some((served.status, served.provenance)));
    if no_store {
        mark_no_store(&mut response);
    }
    if head && let Ok(value) = HeaderValue::from_str(&length.to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    Ok(response)
}

/// A response that carries a capability must not be kept by any cache.
fn mark_no_store(response: &mut Response) {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
}

fn hex_of_digest(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    super::raw::hex_of(&sha2::Sha256::digest(bytes))
}

/// The version document, with every archive URL replaced by a signed one.
///
/// The upstream names each archive by a bare file name next to the document,
/// which the protocol resolves against the URL of the document. Each of them
/// becomes the same name with a query that grants that one read: this
/// account, this repository, this path, for a few minutes. Anything that is
/// not a bare archive name (an absolute URL, a path, a traversal) is not an
/// archive of this mirror and the document is refused: serving it would
/// send the client around the mirror, and its trust gate.
///
/// Without a signing key nothing is issued and the answer says why.
fn sign_archives(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    route: &Route,
    document: &[u8],
) -> Result<Vec<u8>, Failure> {
    if !state.signed_urls.is_configured() {
        return Err(Failure::SigningUnavailable);
    }
    let Some((hostname, namespace, provider)) = route.provider_coordinates() else {
        return Err(RawApiError::NotFound.into());
    };
    let mut value: serde_json::Value = serde_json::from_slice(document)
        .map_err(|_| RawApiError::BadGateway("the upstream version document is not valid JSON"))?;
    let now = chrono::Utc::now().timestamp();
    if let Some(archives) = value
        .get_mut("archives")
        .and_then(serde_json::Value::as_object_mut)
    {
        for archive in archives.values_mut() {
            let Some(url) = archive.get("url").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let relative = format!("{hostname}/{namespace}/{provider}/{url}");
            let Ok(Route::ProviderFile { filename, .. }) = Route::parse(&relative) else {
                return Err(RawApiError::BadGateway(
                    "the upstream version document names an archive that is not a file of this mirror",
                )
                .into());
            };
            let path = format!("/terraform/{}/{relative}", repository.repo_key);
            let claims = state
                .signed_urls
                .issue(
                    &Scope {
                        method: "GET",
                        path: &path,
                        repository: &repository.repo_key,
                        account: context.account().as_str(),
                        subject: context.principal().subject().as_str(),
                    },
                    now,
                )
                .map_err(|_| Failure::SigningUnavailable)?;
            archive["url"] = serde_json::Value::String(format!("{filename}?{}", claims.to_query()));
        }
    }
    serde_json::to_vec(&value)
        .map_err(|_| RawApiError::BadGateway("the version document cannot be written").into())
}

/// A read with a signed archive URL, and no credential.
///
/// What is checked, in this order, before anything is read from the catalog
/// or the store: the method, the signature over method, path, repository,
/// account, principal, expiry and key, and the expiry. Every refusal is the
/// same 403: a probe does not learn which part was wrong.
///
/// The authority of the request is only what the signature says: this
/// account may read this archive of this repository now. Cedar was asked
/// when the URL was issued; the trust gate is asked again here, by digest,
/// so a channel that stops admitting an archive stops serving it at once.
async fn redeem(
    state: &AppState,
    method: &Method,
    path: &str,
    query: &str,
    repository_key: &str,
    rest: &str,
    correlation: &str,
) -> Result<Response, Failure> {
    if method != Method::GET && method != Method::HEAD {
        return Err(RawApiError::MethodNotAllowed(
            "a signed archive URL only reads: the provider network mirror protocol is read-only",
        )
        .into());
    }
    let refuse = |reason: &dyn std::fmt::Display| {
        // Never the query: it holds the signature.
        tracing::info!(
            target: "hodei_artifacts::signed_url",
            repository = repository_key,
            %reason,
            "a signed archive URL was refused"
        );
        Failure::Raw(RawApiError::Forbidden)
    };
    let claims = Claims::parse(query).map_err(|error| refuse(&error))?;
    state
        .signed_urls
        .verify(
            method.as_str(),
            path,
            repository_key,
            &claims,
            chrono::Utc::now().timestamp(),
        )
        .map_err(|error| refuse(&error))?;
    // From here on the account and the principal are the signed ones.
    let route = Route::parse(rest)?;
    let Route::ProviderFile { .. } = &route else {
        // A signature is only ever issued for an archive.
        return Err(refuse(&"the path is not an archive"));
    };
    let outcome = serve_archive(
        state,
        &claims,
        repository_key,
        &route,
        method == Method::HEAD,
        correlation,
    )
    .await;
    let provider_label = route
        .provider_coordinates()
        .map(|(host, namespace, provider)| format!("{host}/{namespace}/{provider}"))
        .unwrap_or_default();
    record_outcome_of(
        state,
        &claims.subject,
        &claims.account,
        repository_key,
        &provider_label,
        &outcome,
        correlation,
    );
    outcome
}

async fn serve_archive(
    state: &AppState,
    claims: &Claims,
    repository_key: &str,
    route: &Route,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    let repository = the_repository_of(
        state,
        &claims.account,
        repository_key,
        &[RepositoryFormat::Terraform],
    )
    .await?;
    if repository.mode != hodei_catalog::RepositoryMode::Proxy {
        return Err(RawApiError::NotFound.into());
    }
    let served = the_proxy_asset_as(
        state,
        &claims.account,
        &claims.subject,
        &repository,
        &route.asset_path(),
        None,
        Some("application/zip"),
        correlation,
    )
    .await?;
    crate::trust::admit(state, &claims.account, &repository, &served.asset.digest)
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
    let etag = crate::raw::conditional::the_etag(&served.asset.digest);
    let length = if head {
        served.asset.size_bytes as usize
    } else {
        bytes.len()
    };
    let mut response = body_response(StatusCode::OK, bytes, "application/zip", &etag, correlation);
    // The provenance of the bytes is not for a holder of a capability.
    apply_cache_headers(&mut response, &Some((served.status, None)));
    mark_no_store(&mut response);
    if head && let Ok(value) = HeaderValue::from_str(&length.to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    Ok(response)
}

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

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
