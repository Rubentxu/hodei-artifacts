//! La superficie **cargo**: `cargo publish` y la resolucion sparse de
//! `cargo add` / `cargo build` contra un repositorio de Hodei.
//!
//! Vive en `/cargo/{repositorio}/...` y habla el protocolo del registro en
//! su variante sparse: la gramatica de rutas y de documentos esta en
//! `hodei_catalog::cargo` (pura) y aqui solo hay lo que necesita efectos.
//!
//! Cada version publicada deja **dos assets inmutables**: los bytes del
//! .crate en `{nombre}/-/{nombre}-{version}.crate` y su linea de indice en
//! `{nombre}/-/index/{version}.json`. El indice se ensambla listando esos
//! documentos, nunca reescribiendolos: una version publicada es para
//! siempre, y dos publicadores concurrentes no pueden pisarse porque cada
//! uno escribe su propia version.
//!
//! La modalidad servida hoy es **hosted**: el proxy de cargo necesita un
//! upstream validado con cliente real antes de poder declararse, y un
//! repositorio cuyo modo no se sirve no se puede ni crear.

use axum::{
    body::Body,
    extract::{OriginalUri, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use hodei_catalog::{
    CatalogError, PackageCoordinates, Publication, RepositoryDescriptor, RepositoryFormat,
    cargo::{self, CargoError, Route},
};
use hodei_evidence::ContentDigest;
use serde_json::{Value, json};

use super::raw::{
    RawApiError, apply_cache_headers, apply_common_headers, the_admission_error, the_blob_error,
    the_catalog, the_content_store, the_context, the_decision, the_publication_error,
    the_repository_in, write_the_content,
};
use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{correlation_id, record_audit};
use crate::raw::conditional;

/// Lo que pasa con una peticion que no se puede servir.
#[derive(Debug)]
pub enum Failure {
    /// Un error de la superficie compartida.
    Raw(RawApiError),
    /// Un error del protocolo cargo.
    Cargo(CargoError),
    /// Un estado y un motivo.
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

/// El resto de la ruta de `/cargo/{repositorio}/{resto}`, sin decodificar:
/// las reglas del protocolo son las unicas que pueden interpretar un `%2f`.
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

/// La URL base del indice tal y como la ve el cliente.
fn the_base(headers: &HeaderMap, repository: &str) -> Result<String, Failure> {
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .filter(|host| {
            !host.is_empty()
                && host.len() <= 255
                && host.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':' | b'[' | b']')
                })
        })
        .ok_or(RawApiError::BadRequest(
            "the Host header is missing or invalid",
        ))?;
    let scheme = match headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
    {
        Some("https") => "https",
        _ => "http",
    };
    Ok(format!("{scheme}://{host}/cargo/{repository}"))
}

/// Cargo manda el token sin esquema: `Authorization: <jwt>`. La superficie
/// compartida solo habla Bearer, asi que aqui se normaliza la cabecera.
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

/// Cualquier peticion de `/cargo/{repositorio}/...`.
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
            let body = cargo::config_json(&the_base(&headers, &repository_key)?);
            Ok(json_response(StatusCode::OK, &body, None, &correlation))
        }
        (Route::Index { name }, true, _) => {
            let outcome = read_index(
                &state,
                &context,
                &repository,
                name,
                method == Method::HEAD,
                &headers,
                &correlation,
            )
            .await;
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
            let asset_path = format!("{name}/-/{name}-{version}.crate");
            let outcome = read_crate(
                &state,
                &context,
                repository.clone(),
                &asset_path,
                method == Method::HEAD,
                &correlation,
            )
            .await;
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
// Lectura
// ---------------------------------------------------------------------------

/// El indice sparse de un crate: la concatenacion NDJSON de sus lineas,
/// ordenada por version. No hay documento que reescribir: se ensambla.
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
        // Cargo espera un 404 que diga que el crate no existe aqui.
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

/// Los bytes de un .crate, verificados contra su asset.
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

/// `If-None-Match` contra el etag del indice: un 304 dice que no cambio.
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

/// Publicar: la version es inmutable, y eso se dice antes de escribir nada.
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

    // Una version ya publicada es para siempre: idéntica es un si,
    // distinta es un no que explica por que.
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

    // 1. Los bytes del .crate, conmutados antes del catalogo.
    let (crate_digest, crate_length) =
        write_the_content(state, Body::from(new.file.to_vec()), None, None).await?;
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

    // 2. La linea de indice, inmutable como su version.
    let (doc_digest, doc_length) =
        write_the_content(state, Body::from(line_bytes), None, None).await?;
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

/// Escribe un asset inmutable; el conflicto con el mismo digest es el eco
/// de una publicacion interrumpida, no una reescritura.
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

/// El error de publicacion de una lectura de lista: la lista no la provoca
/// un conflicto, pero el mismo traductor sabe que significa cada variante.
fn the_publication_error_ref(error: &CatalogError) -> RawApiError {
    match error {
        CatalogError::Denied { .. } => RawApiError::Forbidden,
        CatalogError::Missing { .. } => RawApiError::NotFound,
        CatalogError::Unavailable { .. } | CatalogError::Corrupt { .. } => RawApiError::Internal,
        other => RawApiError::BadRequest(Box::leak(other.to_string().into_boxed_str())),
    }
}
