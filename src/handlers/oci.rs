//! The Hodei OCI Distribution surface, on the wire.
//!
//! `hodei-hub::oci` reads manifests as **types**; this is the
//! **protocol**: the `/v2` endpoints of the distribution, served from
//! the catalog and the byte store that already exist. There is no
//! image storage of its own: a manifest is an asset of the
//! repository, a blob is an asset the repository references, and a
//! tag is the mutable reference that the catalog resolves.
//!
//! The **pull** slice is the reads: `GET /v2/`, blobs and manifests
//! by digest and by tag, with type negotiation. The **push**
//! slice --upload sessions and the manifest-- is in
//! [`crate::oci::push`], and what both share --the error
//! dialect, the name, the digest, the path split-- is in
//! [`crate::oci`].
//!
//! The content's identity is the digest, and the tag never
//! becomes content: it is resolved and forgotten, and the response states
//! the resolved digest, so that a client that asked by tag knows
//! that what lies behind it is immutable.
//!
//! Errors speak **OCI**: an envelope with `code` and `message`, and the
//! codes are those of the dialect (BLOB_UNKNOWN, MANIFEST_UNKNOWN,
//! NAME_UNKNOWN, NAME_INVALID, DIGEST_INVALID). A repository of another
//! account is `NAME_UNKNOWN`, which does not say whether it exists elsewhere.

use axum::body::Body;
use axum::extract::{OriginalUri, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::app_state::AppState;
use crate::oci::failure::OciFailure;
use crate::oci::target::{Target, the_digest_of_reference, the_target};
use crate::oci::token::the_realm;
use crate::oci::{the_catalog, the_content_store, the_context, the_decision, the_repository};
use crate::public_url::TheOrigin;
use hodei_catalog::RepositoryMode;

/// The manifest media types this registry serves.
///
/// They are those the OCI subset declares in code: the OCI image
/// manifest and the Docker one, the multi-platform index and the Docker
/// list, and the artifact manifest with its `subject`.
const SUPPORTED_MANIFESTS: &[&str] = hodei_hub::oci::SUPPORTED_MANIFESTS;

/// The header announcing the distribution API version.
const API_VERSION_HEADER: &str = "Docker-Distribution-API-Version";

/// The protocol ping: `GET /v2/`.
///
/// Authenticated like the rest of the surface; it does not look at repositories.
pub async fn version_check(
    State(state): State<AppState>,
    origin: TheOrigin,
    headers: HeaderMap,
) -> Result<Response, OciFailure> {
    let _ = the_context(&state, &headers)
        .await
        .map_err(|failure| failure.with_the_realm(the_realm(&origin)))?;
    let mut response = (StatusCode::OK, "{}".to_string()).into_response();
    response
        .headers_mut()
        .insert(API_VERSION_HEADER, HeaderValue::from_static("registry/2.0"));
    Ok(response)
}

/// The entry point of the surface.
///
/// The method and the path say which operation it is. The `GET` of an upload
/// session is the upload's **status**, not the read of a blob, and
/// so it is decided before reading.
pub async fn dispatch(
    State(state): State<AppState>,
    origin: TheOrigin,
    uri: OriginalUri,
    method: Method,
    headers: HeaderMap,
    request: axum::http::Request<Body>,
) -> Result<Response, OciFailure> {
    // The authentication challenge is set by the edge: the edge knows where
    // the deployment lives, and a `401` without a challenge does not tell the client how to
    // authenticate.
    let answer = match method {
        Method::GET | Method::HEAD => {
            if let Ok(Target::UploadSession { .. }) = the_target(uri.path()) {
                crate::oci::push::upload_status(state, uri, headers).await
            } else {
                content(state, uri, headers, method).await
            }
        }
        _ => crate::oci::push::mutate(state, uri, method, headers, request).await,
    };
    answer.map_err(|failure| failure.with_the_realm(the_realm(&origin)))
}

/// The distribution content: blobs and manifests.
///
/// GET and HEAD share a handler: HEAD gives the same headers
/// and brings no body, which is what the spec asks for and what a pull client
/// needs to know the size without downloading it.
pub async fn content(
    state: AppState,
    uri: OriginalUri,
    headers: HeaderMap,
    method: Method,
) -> Result<Response, OciFailure> {
    let context = the_context(&state, &headers).await?;
    let head = method == Method::HEAD;

    // The name, the resource kind and the reference come from the raw
    // path: `/<name>/blobs/<digest>` or `/<name>/manifests/<reference>`,
    // where `<name>` may have levels (`team/demo`).
    let target = the_target(uri.path())?;
    match target {
        Target::Blob { name, reference } => {
            blob(&state, &context, &name, &reference, &headers, head).await
        }
        Target::Manifest { name, reference } => {
            manifest(&state, &context, &name, &reference, &headers, head).await
        }
        // The tag list and the referrers are not content: they are
        // questions about the repository, and the queries surface
        // answers them.
        Target::Tags { name } => {
            crate::oci::query::the_tags(
                &state,
                &context,
                &name,
                uri.query().unwrap_or(""),
                &headers,
            )
            .await
        }
        Target::Referrers { name, digest } => {
            crate::oci::query::the_referrers(
                &state,
                &context,
                &name,
                &digest,
                uri.query().unwrap_or(""),
            )
            .await
        }
        // The start of an upload and its session are not content: they are the
        // state of an upload, and are handled in the push surface.
        Target::UploadStart { .. } | Target::UploadSession { .. } => Err(OciFailure::unsupported(
            "an upload is not content of the repository",
        )),
    }
}

/// A blob of the repository, served by its digest.
///
/// The catalog reference is what authorizes serving: bytes that
/// are in the CAS but that the repository does not reference are not the
/// repository's content, and are answered as BLOB_UNKNOWN.
async fn blob(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    name: &str,
    reference: &str,
    headers: &HeaderMap,
    head: bool,
) -> Result<Response, OciFailure> {
    let digest = the_digest_of_reference(reference)?;

    let repository = the_repository(state, context, name).await?;
    the_decision(
        state,
        context,
        crate::actions::download_asset(),
        &repository,
    )?;

    // A proxy repository does not store what was published in it: it fetches it from
    // an upstream, with the distribution rules in front.
    if repository.mode == RepositoryMode::Proxy {
        return crate::oci::proxy::the_blob(
            state,
            context,
            &repository,
            name,
            &digest,
            headers,
            head,
        )
        .await;
    }

    // A group does not store what was published in it: it resolves it among its
    // members, in the order they are configured, and says which
    // member what is served came from.
    if repository.mode == RepositoryMode::Group {
        let resolution =
            crate::oci::group::the_blob(state, context, &repository, &digest, headers).await;
        let won = resolution.outcome?;
        let mut response = oci_bytes_response(
            won.bytes,
            "application/octet-stream",
            &won.digest.value,
            head,
        );
        the_group_headers(&mut response, &repository, &won.member.repo_key);
        return Ok(response);
    }

    // The asset row is the reference: without it, the blob is not
    // content of this repository, wherever its bytes are.
    let (reader, _) = the_catalog(state)?;
    let asset = reader
        .asset(context.account().as_str(), name, &format!("blobs/{digest}"))
        .await
        .map_err(|error| match error {
            hodei_catalog::CatalogError::Missing { .. } => {
                OciFailure::blob_unknown(format!("the repository does not reference {digest}"))
            }
            _other => OciFailure::unavailable("the catalog is not available"),
        })?;

    // The bytes served are those the **row** references, not those
    // the URL names. The URL's digest says **what** is requested; the row
    // says **what** there is. If those of the URL's digest were served, a
    // row that only names a path would serve another blob's content,
    // which may belong to another account: the CAS is a shared tree.
    //
    // That is why the response carries the digest of what is served, and why
    // a requested digest that is not the referenced one is a BLOB_UNKNOWN, not
    // bytes.
    if asset.digest.value != digest {
        return Err(OciFailure::blob_unknown(format!(
            "the repository does not reference {digest}"
        )));
    }
    crate::trust::admit(
        state,
        context.account().as_str(),
        &repository,
        &asset.digest,
    )
    .await
    .map_err(|admission| {
        the_admission_failure(
            admission,
            |digest| {
                OciFailure::blob_unknown(format!("the repository does not reference {digest}"))
            },
            &digest,
        )
    })?;
    let store = the_content_store(state)?;
    let bytes = store
        .get(&asset.digest)
        .await
        .map_err(|_| OciFailure::blob_unknown(format!("the blob {digest} is not in the store")))?;
    debug_assert_eq!(
        bytes.len() as u64,
        asset.size_bytes,
        "the catalog and the store disagree about the size of the blob"
    );

    Ok(oci_bytes_response(
        bytes,
        "application/octet-stream",
        &digest,
        head,
    ))
}

/// A manifest of the repository, by digest or by tag.
///
/// The tag is the mutable reference that the catalog resolves to the
/// coordinates; the content's identity remains the manifest's
/// digest, and the response states it in `Docker-Content-Digest`.
async fn manifest(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    name: &str,
    reference: &str,
    headers: &HeaderMap,
    head: bool,
) -> Result<Response, OciFailure> {
    let repository = the_repository(state, context, name).await?;
    the_decision(
        state,
        context,
        crate::actions::download_asset(),
        &repository,
    )?;

    // A proxy repository does not store what was published in it: it fetches it from
    // an upstream, with the distribution rules in front.
    if repository.mode == RepositoryMode::Proxy {
        return crate::oci::proxy::the_manifest(
            state,
            context,
            &repository,
            name,
            reference,
            headers,
            head,
        )
        .await;
    }

    // A group does not store what was published in it: it resolves it among its
    // members, in the order they are configured. The manifest is
    // served **whole** from a single member, and the response says which
    // member it came from.
    if repository.mode == RepositoryMode::Group {
        let resolution =
            crate::oci::group::the_manifest(state, context, &repository, reference, headers).await;
        let won = resolution.outcome?;
        let media_type = the_declared_media_type(&won.bytes)?;
        the_negotiation(headers, &media_type)?;
        let mut response = oci_bytes_response(won.bytes, &media_type, &won.digest.value, head);
        the_group_headers(&mut response, &repository, &won.member.repo_key);
        return Ok(response);
    }

    let (reader, _) = the_catalog(state)?;

    // The manifest's digest: the one the client names, or the one the
    // tag resolves to. The tag never becomes content: it is
    // resolved and forgotten, and the response states the resolved digest.
    let digest = if reference.contains(':') {
        the_digest_of_reference(reference)?
    } else {
        let tag = reader
            .tag(context.account().as_str(), name, reference)
            .await
            .map_err(|error| match error {
                hodei_catalog::CatalogError::Missing { .. } => the_unknown_manifest(reference),
                _other => OciFailure::unavailable("the catalog is not available"),
            })?
            .coordinates
            .package_version;
        the_digest_of_reference(&format!("sha256:{tag}"))?
    };

    let asset = reader
        .asset(
            context.account().as_str(),
            name,
            &format!("manifests/{digest}"),
        )
        .await
        .map_err(|error| match error {
            hodei_catalog::CatalogError::Missing { .. } => the_unknown_manifest(reference),
            _other => OciFailure::unavailable("the catalog is not available"),
        })?;

    // The bytes served are those the row references, not those the
    // URL names: as with blobs, the requested digest says **what** is
    // requested and the row says **what** there is.
    if asset.digest.value != digest {
        return Err(the_unknown_manifest(reference));
    }

    // The tag was already resolved to its target digest: what is admitted is
    // the digest, not the name. A tag that moved to another digest does not
    // inherit the previous one's grant.
    crate::trust::admit(
        state,
        context.account().as_str(),
        &repository,
        &asset.digest,
    )
    .await
    .map_err(|admission| {
        the_admission_failure(admission, |_| the_unknown_manifest(reference), &digest)
    })?;

    let store = the_content_store(state)?;
    let bytes = store.get(&asset.digest).await.map_err(|_| {
        OciFailure::manifest_unknown(format!("the manifest {digest} is not in the store"))
    })?;

    // The media type is stated by the manifest itself, not the client: the
    // manifest is the truth of what it is, and the client requests it with
    // `Accept`, it does not decide it.
    //
    // It is read from the JSON and not from `hodei_hub::oci::OciManifest::read`, which
    // only admits the OCI type: a Docker schema2 manifest is a
    // valid manifest, and the registry serves it with the type the
    // manifest declares.
    let media_type = the_declared_media_type(&bytes)?;

    // Negotiation, with the type this manifest declares: if the
    // client does not accept it, the error is unknown manifest and not
    // a 200 with a type it did not ask for.
    the_negotiation(headers, &media_type)?;
    debug_assert_eq!(
        bytes.len() as u64,
        asset.size_bytes,
        "the catalog and the store disagree about the size of the manifest"
    );

    Ok(oci_bytes_response(bytes, &media_type, &digest, head))
}

/// The 404 of a manifest, with the reference the client wrote.
///
/// It is the same for what does not exist, what is in quarantine and a
/// tag that points to nothing: the message never names the resolved
/// digest, which would say the content is there.
fn the_unknown_manifest(reference: &str) -> OciFailure {
    OciFailure::manifest_unknown(format!("the manifest {reference:?} is unknown"))
}

/// What is answered when the trust gate does not admit.
///
/// What is not admitted answers with the same OCI error as what does not exist
/// (`unknown`), so that the response does not say the content is there.
/// A ledger that does not respond is a 503: the unknown is not served.
fn the_admission_failure(
    admission: crate::trust::Admission,
    unknown: impl FnOnce(&str) -> OciFailure,
    digest: &str,
) -> OciFailure {
    match admission {
        crate::trust::Admission::Hidden => unknown(digest),
        crate::trust::Admission::Unavailable => {
            OciFailure::unavailable("the trust ledger is not available")
        }
    }
}

/// The media type the manifest declares.
///
/// The manifest is the truth of what it is: if it declares a type this
/// registry does not serve, the request fails with the OCI unknown-manifest
/// error, and not with a 200 pretending to be what the client asked for.
pub(crate) fn the_declared_media_type(bytes: &[u8]) -> Result<String, OciFailure> {
    let declared = serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|manifest| {
            manifest
                .get("mediaType")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        });
    match declared {
        Some(media_type) if SUPPORTED_MANIFESTS.contains(&media_type.as_str()) => Ok(media_type),
        // A type this registry does NOT serve is not relabeled with another:
        // serving it with a different type is a 200 pretending to be what the
        // client asked for, and the client that asked for it did not receive it.
        Some(media_type) if hodei_hub::oci::UNSUPPORTED.contains(&media_type.as_str()) => Err(
            OciFailure::manifest_unknown(format!("the registry does not serve {media_type}")),
        ),
        // A type this registry does not know: it is not served either, for the
        // same reason. An arbitrary `Content-Type` in the response is
        // also a security problem: a browser navigating to the
        // URL would interpret the manifest as whatever it declares.
        Some(media_type) => Err(OciFailure::manifest_unknown(format!(
            "the registry does not serve {media_type}"
        ))),
        // A manifest may **omit** its `mediaType`: the media-spec
        // says that it is then inferred, and `helm push` omits it. The
        // inference is structural and only uses types this registry
        // serves: a JSON with `manifests` is an index, any other is
        // an image manifest. There is no relabeling of a declared
        // type: here there is nothing declared to contradict.
        None => {
            let kind = serde_json::from_slice::<serde_json::Value>(bytes)
                .ok()
                .and_then(|manifest| {
                    manifest
                        .get("manifests")
                        .and_then(|value| value.as_array())
                        .map(|_| "application/vnd.oci.image.index.v1+json")
                })
                .unwrap_or("application/vnd.oci.image.manifest.v1+json");
            debug_assert!(
                SUPPORTED_MANIFESTS.contains(&kind),
                "the inferred types must stay in the subset this registry serves"
            );
            Ok(kind.to_string())
        }
    }
}

/// Which member of a group served the content, and in which version of the
/// group's configuration it was resolved.
///
/// It goes in `X-Hodei-Resolved-From` and `X-Hodei-Group-Version`: whoever downloads
/// knows which repository what they have came from, and a different resolution
/// after reconfiguring the group is visible. It is the same convention the
/// raw surface uses.
fn the_group_headers(
    response: &mut Response,
    group: &hodei_catalog::RepositoryDescriptor,
    member: &str,
) {
    if let Ok(value) = HeaderValue::from_str(member) {
        response.headers_mut().insert(
            header::HeaderName::from_static("x-hodei-resolved-from"),
            value,
        );
    }
    if let Ok(value) = HeaderValue::from_str(&group.config.version.to_string()) {
        response.headers_mut().insert(
            header::HeaderName::from_static("x-hodei-group-version"),
            value,
        );
    }
}

/// The distribution's bytes response: the OCI headers and, on a
/// HEAD, no body.
fn oci_bytes_response(bytes: Vec<u8>, media_type: &str, digest: &str, head: bool) -> Response {
    let length = bytes.len().to_string();
    let mut response = if head {
        StatusCode::OK.into_response()
    } else {
        (StatusCode::OK, bytes).into_response()
    };
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(media_type).expect("the media type is a header value"),
    );
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&length).expect("the length is a header value"),
    );
    headers.insert(
        header::ETAG,
        HeaderValue::from_str(&format!("\"sha256:{digest}\""))
            .expect("the digest is a header value"),
    );
    headers.insert(
        "Docker-Content-Digest",
        HeaderValue::from_str(&format!("sha256:{digest}")).expect("the digest is a header value"),
    );
    // A manifest is JSON. Without this, a browser navigating to the
    // URL would interpret the response as whatever the manifest declares.
    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    response
}

/// The negotiation of `Accept`.
///
/// A client that names no types (`*/*`, `application/*` or nothing) accepts
/// whatever there is. One that only names types this registry does not serve
/// receives the OCI unknown-manifest error, not a response that
/// pretends to be what it asked for.
///
/// The `Accept` is read **whole**: a client may send the type it
/// accepts in **several headers**, and each header may carry a list
/// separated by commas. Reading only the first header--which is what
/// a `get` does--leaves out the types the client does accept, and a real client
/// (the Docker CLI sends two: the index one and the image one)
/// is left without a manifest.
///
/// And the weight is respected: a `q=0` says the type is **not** accepted
/// (RFC 9110, section 12.5.1), and a rejected type is a type that cannot
/// be served. Without this, an `Accept: */*;q=0`--which is a "I accept
/// nothing"--would serve any manifest.
pub(crate) fn the_negotiation(headers: &HeaderMap, served: &str) -> Result<(), OciFailure> {
    let mut there_is_an_accept = false;
    let mut asked: Vec<&str> = Vec::new();
    for value in headers.get_all(header::ACCEPT).iter() {
        let Ok(value) = value.to_str() else {
            continue;
        };
        there_is_an_accept = true;
        for part in value.split(',') {
            let mut pieces = part.split(';');
            let media = pieces.next().unwrap_or("").trim();
            if media.is_empty() {
                continue;
            }
            // A `q=0` weight is a rejection: the type is in the list, but
            // the client says it does not want it.
            if pieces.any(the_weight_is_zero) {
                continue;
            }
            asked.push(media);
        }
    }
    if !there_is_an_accept {
        // Without `Accept` the client accepts whatever there is: that is what the
        // protocol says, and the type served is declared by the manifest.
        return Ok(());
    }
    if asked.is_empty() {
        // The client has named types and has rejected **all** of them: it
        // accepts none. Serving it ours would be answering with something
        // it said it does not want.
        return Err(OciFailure::manifest_unknown(format!(
            "the client accepts none of the media types this registry serves: {served}"
        )));
    }
    // The client accepts anything (`*/*`, `application/*`) or accepts
    // the type about to be served. What is NOT accepted is a client that
    // only names other types: serving it ours would be answering with
    // something it did not ask for.
    let accepts = asked
        .iter()
        .any(|media| *media == "*/*" || *media == "application/*" || *media == served);
    if accepts {
        return Ok(());
    }
    Err(OciFailure::manifest_unknown(format!(
        "the registry does not serve any of the media types the client accepts: {}",
        asked.join(", ")
    )))
}

/// Whether an `Accept` parameter is a `q=0` weight.
///
/// The weight is written `q=0` or `q=0.0`: both forms say the same thing, and
/// a weight that cannot be read is not a rejection.
fn the_weight_is_zero(parameter: &str) -> bool {
    let parameter = parameter.trim();
    let Some(weight) = parameter.strip_prefix("q=") else {
        return false;
    };
    weight
        .trim()
        .parse::<f64>()
        .is_ok_and(|weight| weight <= 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const THE_SERVED: &str = "application/vnd.oci.image.manifest.v1+json";

    /// The headers of a request with an `Accept`.
    fn with_accept(values: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for value in values {
            headers.append(
                header::ACCEPT,
                HeaderValue::from_str(value).expect("a header value"),
            );
        }
        headers
    }

    /// Without `Accept`, whatever there is is served.
    #[test]
    fn without_accept_the_registry_serves_what_it_has() {
        assert!(the_negotiation(&HeaderMap::new(), THE_SERVED).is_ok());
    }

    /// A client that accepts anything receives whatever there is.
    #[test]
    fn a_client_that_accepts_anything_is_served() {
        assert!(the_negotiation(&with_accept(&["*/*"]), THE_SERVED).is_ok());
        assert!(the_negotiation(&with_accept(&["application/*"]), THE_SERVED).is_ok());
    }

    /// A client that names the type being served receives the manifest.
    #[test]
    fn a_client_that_accepts_the_served_type_is_served() {
        assert!(the_negotiation(&with_accept(&[THE_SERVED]), THE_SERVED).is_ok());
    }

    /// A client that only names other types receives the protocol
    /// error, not a manifest it did not ask for.
    #[test]
    fn a_client_that_accepts_another_type_is_refused() {
        let headers = with_accept(&["application/vnd.oci.image.index.v1+json"]);
        assert!(the_negotiation(&headers, THE_SERVED).is_err());
    }

    /// The comma-separated list is read whole.
    #[test]
    fn a_comma_separated_list_is_read_whole() {
        let headers = with_accept(&["application/vnd.oci.image.index.v1+json, image/png"]);
        assert!(the_negotiation(&headers, "image/png").is_ok());
    }

    /// **Several** headers are read whole: the Docker CLI sends the index
    /// one and the image one, and with a single header the client is
    /// left without a manifest.
    #[test]
    fn several_accept_headers_are_read_whole() {
        let headers = with_accept(&["application/vnd.oci.image.index.v1+json", THE_SERVED]);
        assert!(the_negotiation(&headers, THE_SERVED).is_ok());
    }

    /// A `q=0` weight is a rejection: the type is not accepted.
    #[test]
    fn a_zero_weight_is_a_rejection() {
        let headers = with_accept(&["application/vnd.oci.image.index.v1+json;q=0"]);
        assert!(the_negotiation(&headers, THE_SERVED).is_err());
    }

    /// And a `*/*` with zero weight does not accept anything.
    #[test]
    fn a_zero_weight_on_everything_refuses_everything() {
        let headers = with_accept(&["*/*;q=0"]);
        assert!(the_negotiation(&headers, THE_SERVED).is_err());
    }

    /// A weight that is not zero is a preference, not a rejection.
    #[test]
    fn a_positive_weight_is_a_preference() {
        let headers = with_accept(&["application/vnd.oci.image.index.v1+json;q=0.8", THE_SERVED]);
        assert!(the_negotiation(&headers, THE_SERVED).is_ok());
    }

    /// Parameters other than the weight do not weigh: the type remains the
    /// one that is compared.
    #[test]
    fn a_parameter_is_not_a_weight() {
        let headers = with_accept(&["application/vnd.oci.image.manifest.v1+json;charset=utf-8"]);
        assert!(the_negotiation(&headers, THE_SERVED).is_ok());
    }
}
