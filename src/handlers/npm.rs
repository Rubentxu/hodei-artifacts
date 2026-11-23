//! La superficie **npm**: `npm publish`, `npm install`, `npm dist-tag` y
//! `npm deprecate` contra un repositorio de Hodei.
//!
//! Vive en `/npm/{repositorio}/...` y habla el protocolo del registro: la
//! gramatica de rutas y de documentos esta en `hodei_catalog::npm` (pura) y
//! aqui solo hay lo que necesita efectos.
//!
//! # Hosted
//!
//! El packument es una **instantanea**: cada publicacion, deprecacion o
//! movimiento de dist-tag escribe un documento nuevo en el almacen de
//! contenido y mueve el puntero de la ruta con una precondicion sobre el
//! digest anterior (`If-Match` del catalogo). Los bytes de las instantaneas
//! anteriores no se tocan, y dos escritores concurrentes no se pisan: el que
//! pierde relee y reintenta.
//!
//! Los tarballs son contenido inmutable en `{nombre}/-/{fichero}.tgz`:
//! publicar otra vez la misma version es un `403`, como en el registro de
//! npm (un `409` haria que `libnpmpublish` intente fusionar documentos).
//!
//! # Unpublish
//!
//! La politica es conservadora: **no se borra nada**. `npm unpublish` recibe
//! un `403` que dice que se use `npm deprecate`, y el intento queda en el
//! rastro de auditoria.
//!
//! # Proxy y grupo
//!
//! Reutilizan la misma resolucion que el resto de formatos: el packument se
//! trata como metadato revalidable (TTL, offline) y el tarball como
//! contenido inmutable. En un grupo gana el primer miembro que responde.

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

use super::raw::{
    RawApiError, Resolved, apply_cache_headers, apply_common_headers, apply_group_headers, hex_of,
    the_admission_error, the_blob_error, the_catalog, the_content_store, the_context,
    the_current_asset, the_decision, the_publication_error, the_repository_in, the_resolved_asset,
    write_the_content,
};
use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{correlation_id, record_audit};
use crate::public_url::TheOrigin;
use crate::raw::{conditional, path};

/// Cuantas veces se reintenta una escritura del packument que pierde la
/// carrera con otra.
const MAX_ATTEMPTS: usize = 6;

/// Lo que se le dice al cliente cuando se agotan los reintentos.
const CONTENDED: &str = "the package is being updated concurrently; retry";

/// Lo que pasa con una peticion que no se puede servir.
///
/// npm lee el cuerpo de un error como JSON: `{"error": "..."}`.
#[derive(Debug)]
pub enum Failure {
    /// Un error de la superficie compartida.
    Raw(RawApiError),
    /// Un error del protocolo npm.
    Npm(NpmError),
    /// Un estado y un motivo.
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
                RawApiError::Internal => {
                    (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
                }
            },
            Self::Npm(error) => {
                let status = match error {
                    // npm responde 403 al publicar sobre una version existente.
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
        // El consejo de cuando volver viaja con la negativa por
        // saturacion, tambien en esta superficie: el veredicto es uno
        // solo, y la cabecera no es un detalle del transporte crudo.
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

/// El repositorio y el resto de la ruta de `/npm/{repositorio}/{resto}`.
///
/// El resto se lee **sin decodificar**: el nombre de un paquete con alcance
/// llega como `@alcance%2fnombre` y decodificarlo antes de tiempo lo
/// confundiria con dos segmentos.
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

/// La URL base de este repositorio tal y como la ve el cliente.
///
/// El origen lo decide el despliegue y no la peticion: el `Host` y el
/// `X-Forwarded-Proto` de una peticion los puede escribir cualquiera, y
/// una URL que el cliente mueve es una URL que el cliente envenena.
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

/// Cualquier peticion de `/npm/{repositorio}/...`.
pub async fn handle(
    State(state): State<AppState>,
    origin: TheOrigin,
    method: Method,
    uri: OriginalUri,
    headers: HeaderMap,
    request: Request<Body>,
) -> Result<Response, Failure> {
    // La autenticacion es la primera puerta, como en el resto de superficies.
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
        // `npm unpublish <nombre>@<version>` hace `PUT .../-rev/{rev}` con el
        // documento sin esa version; el paquete entero, un `DELETE`.
        (Route::UnpublishPackage { name } | Route::UnpublishTarball { name, .. }, false)
            if method == Method::DELETE || method == Method::PUT =>
        {
            refuse_unpublish(&state, &context, &repository, name, &correlation)
        }
        _ => Err(RawApiError::MethodNotAllowed("this npm operation is not supported").into()),
    }
}

// ---------------------------------------------------------------------------
// Lectura
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
    the_decision(
        state,
        context,
        crate::actions::download_asset(),
        &repository,
    )?;
    let requested = repository.repo_key.clone();
    let asset_path = route.asset_path().ok_or(RawApiError::NotFound)?;
    let Resolved {
        asset,
        served,
        resolved_by,
        repository: backing,
    } = the_resolved_asset(state, context, repository, &asset_path, correlation).await?;
    // La compuerta de confianza va antes de las condiciones de lectura: un
    // 304 diria que el contenido existe.
    crate::trust::admit(state, context.account().as_str(), &backing, &asset.digest)
        .await
        .map_err(the_admission_error)?;

    let etag = conditional::the_etag(&asset.digest);
    match conditional::the_read_condition(
        conditional::the_if_match(headers).as_deref(),
        conditional::the_if_none_match(headers).as_deref(),
        Some(&etag),
    ) {
        conditional::ReadOutcome::Failed => return Err(RawApiError::PreconditionFailed.into()),
        conditional::ReadOutcome::NotModified => {
            let mut response = StatusCode::NOT_MODIFIED.into_response();
            apply_common_headers(&mut response, &etag, correlation);
            apply_cache_headers(&mut response, &served);
            apply_group_headers(&mut response, &resolved_by);
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
                    // `npm view` y los clientes que piden un tag leen la
                    // version como una cadena JSON.
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
    apply_cache_headers(&mut response, &served);
    apply_group_headers(&mut response, &resolved_by);
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response.headers_mut().insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    if head && let Ok(value) = HeaderValue::from_str(&length.to_string()) {
        // Un HEAD dice cuanto traeria el GET.
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    Ok(response)
}

// ---------------------------------------------------------------------------
// Escritura
// ---------------------------------------------------------------------------

/// Lo que dejo una escritura del packument.
struct Written {
    digest: ContentDigest,
    changed: bool,
}

/// El permiso de escribir: Cedar y el modo del repositorio.
fn the_write_gate(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
) -> Result<(), Failure> {
    the_decision(state, context, crate::actions::upload_asset(), repository)?;
    // Lo que dice un proxy sale de su upstream. Va despues de Cedar: quien
    // no puede subir recibe el mismo 403 en cualquier repositorio.
    if !repository.mode.accepts_client_writes() {
        return Err(
            RawApiError::MethodNotAllowed("this repository does not accept uploads").into(),
        );
    }
    Ok(())
}

/// Aplica un cambio al packument y lo publica como instantanea nueva.
///
/// El cambio se **reevalua** contra lo que hay en cada intento: si otro
/// escritor movio el puntero entre la lectura y la escritura, el catalogo
/// rechaza la precondicion, se relee y se vuelve a aplicar.
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
            asset_path: name.full().to_string(),
            // Cada instantanea es una version distinta del packument: la
            // identifica su contenido, y la anterior sigue estando.
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
            Err(error) => return Err(the_publication_error(&error).into()),
        }
    }
    Err(Failure::plain(StatusCode::CONFLICT, CONTENDED))
}

/// El `rev` que npm espera ver en las respuestas de escritura.
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

/// El cuerpo de una escritura, con tope.
///
/// Un `npm publish` lleva el tarball en base64 dentro del JSON: el tope es el
/// del contenido mas el 4/3 del base64 y un margen para el `package.json`.
async fn read_body(state: &AppState, request: Request<Body>) -> Result<bytes::Bytes, Failure> {
    let limit = state.max_body_size / 3 * 4 + 1024 * 1024 + 1024;
    axum::body::to_bytes(request.into_body(), limit)
        .await
        .map_err(|_| RawApiError::PayloadTooLarge.into())
}

/// La politica de alcances del repositorio: vale para toda escritura de un
/// paquete (publicar, deprecar, mover o quitar un dist-tag).
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

/// `PUT /npm/{repositorio}/{nombre}`: publicar una version o deprecar.
async fn put_packument(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    route_name: &PackageName,
    request: Request<Body>,
    correlation: &str,
) -> Result<Response, Failure> {
    the_write_gate(state, context, repository)?;
    // Publicar exige las reglas estrictas de nombres nuevos.
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
                    // Quitar una version en un PUT es un unpublish disfrazado.
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
    // Lo que el servicio sabe del tarball es lo que calcula, no lo que dice
    // el cliente: si el cliente declara otra cosa, se rechaza.
    let facts = npm::TarballFacts {
        shasum: hex_of(&sha1::Sha1::digest(bytes)),
        integrity: format!(
            "sha512-{}",
            base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes))
        ),
    };
    npm::check_declared_dist(&new.manifest, &facts)?;

    // Una version publicada no se reescribe: se dice antes de escribir bytes.
    if let Some(current) =
        the_current_asset(state, context, &repository.repo_key, name.full()).await?
    {
        let stored = the_content_store(state)?
            .get(&current.digest)
            .await
            .map_err(|error| the_blob_error(&error))?;
        let packument = npm::parse_packument(&stored).map_err(|_| RawApiError::Internal)?;
        if npm::has_version(&packument, &new.version) {
            return Err(NpmError::VersionExists(new.version.clone()).into());
        }
    }

    // 1. El tarball, como contenido inmutable en su propia ruta.
    let (digest, length) = write_the_content(
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
        // Otro contenido en la misma version: la version ya existe.
        Err(CatalogError::Conflict { .. }) => {
            return Err(NpmError::VersionExists(new.version.clone()).into());
        }
        Err(error) => return Err(the_publication_error(&error).into()),
    }

    // 2. El packument: una instantanea nueva que apunta a ese tarball.
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

/// `PUT /npm/{repositorio}/-/package/{nombre}/dist-tags/{tag}`
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
    // npm manda la version como una cadena JSON.
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

/// `DELETE /npm/{repositorio}/-/package/{nombre}/dist-tags/{tag}`
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

/// `DELETE .../-rev/{rev}`: no se borra nada, y el intento queda en el rastro.
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
