//! La superficie **apt**: el repositorio debian que un `apt-get` real
//! puede leer y escribir contra un repositorio hosted de Hodei.
//!
//! Vive en `/apt/{repositorio}/...` y habla el formato de `apt-ftparchive`:
//! la gramatica de contenedores, indices y rutas esta en
//! `hodei_catalog::apt` (pura) y aqui solo hay lo que necesita efectos:
//! el almacen, el catalogo y la firma OpenPGP de `Release`.
//!
//! Cada `.deb` publicado deja **dos assets inmutables**: los bytes en
//! `pool/main/{package}/{archivo}.deb` y su registro de indice en esa
//! misma ruta con sufijo `.record`. El indice `Packages` se ensambla
//! listando registros, nunca reescribiendo un documento compartido: una
//! version publicada es para siempre y dos publicadores concurrentes no
//! pueden pisarse. Si una caida deja el `.deb` sin registro, el paquete
//! es invisible para apt, nunca corrupto.

use axum::{
    body::Body,
    extract::{OriginalUri, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::Engine as _;
use hodei_catalog::{
    CatalogError, PackageCoordinates, Publication, RepositoryDescriptor, RepositoryFormat,
    apt::{self, AptError, Route},
};
use hodei_evidence::ContentDigest;
use serde_json::json;

use super::raw::{
    RawApiError, apply_cache_headers, apply_common_headers, the_admission_error, the_blob_error,
    the_catalog, the_content_store, the_context, the_current_asset, the_decision,
    the_publication_error, the_repository_in, the_resolved_asset, write_the_content,
};
use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{correlation_id, record_audit};
use crate::raw::conditional;

/// Lo que pasa con una peticion apt que no se puede servir.
#[derive(Debug)]
pub enum Failure {
    /// Un error de la superficie compartida.
    Raw(RawApiError),
    /// Un error del formato debian.
    Apt(AptError),
    /// Un estado y un motivo.
    Plain(StatusCode, String),
}

impl From<RawApiError> for Failure {
    fn from(error: RawApiError) -> Self {
        Self::Raw(error)
    }
}

impl From<AptError> for Failure {
    fn from(error: AptError) -> Self {
        Self::Apt(error)
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
            Self::Apt(error) => (
                if matches!(error, AptError::BadIdentity(_)) {
                    StatusCode::NOT_FOUND
                } else {
                    StatusCode::BAD_REQUEST
                },
                error.to_string(),
            ),
            Self::Plain(status, message) => (status, message),
        };
        (status, axum::Json(json!({ "error": message }))).into_response()
    }
}

/// El resto de la ruta de `/apt/{repositorio}/{resto}`, sin decodificar:
/// apt no escapa nombres y la gramatica es la unica que interpreta.
fn the_split(uri: &OriginalUri) -> Result<(String, String), Failure> {
    let rest = uri
        .path()
        .strip_prefix("/apt/")
        .ok_or(RawApiError::NotFound)?;
    let (repository, rest) = rest.split_once('/').ok_or(RawApiError::NotFound)?;
    if !crate::raw::path::valid_repository_name(repository) || rest.is_empty() {
        return Err(RawApiError::NotFound.into());
    }
    Ok((repository.to_string(), rest.to_string()))
}

/// apt lleva la credencial en el userinfo de la URI: `Basic`, con el
/// token como contrasena y el usuario informativo. La superficie
/// compartida solo habla Bearer, asi que aqui se normaliza la cabecera.
fn normalize_authorization(headers: &mut HeaderMap) {
    if crate::handlers::bearer_token(headers).is_some() {
        return;
    }
    let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    else {
        return;
    };
    let Some(encoded) = value
        .strip_prefix("Basic ")
        .or_else(|| value.strip_prefix("basic "))
    else {
        return;
    };
    let decoded = match base64::engine::general_purpose::STANDARD.decode(encoded.trim()) {
        Ok(decoded) => decoded,
        Err(_) => return,
    };
    let decoded = match String::from_utf8(decoded) {
        Ok(decoded) => decoded,
        Err(_) => return,
    };
    let Some((_user, password)) = decoded.split_once(':') else {
        return;
    };
    if password.is_empty() {
        return;
    }
    if let Ok(value) = HeaderValue::from_str(&format!("Bearer {password}")) {
        headers.insert(header::AUTHORIZATION, value);
    }
}

/// Cualquier peticion de `/apt/{repositorio}/...`.
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
        the_repository_in(&state, &context, &repository_key, &[RepositoryFormat::Apt]).await?;

    let reading = method == Method::GET || method == Method::HEAD;
    let publishing = method == Method::PUT;
    match (&route, reading, publishing) {
        (Route::Key, true, _) => {
            serve_document(
                &state,
                &context,
                &repository,
                || state.metadata_signer.key_asc().map(String::into_bytes),
                "application/pgp-keys",
                &headers,
                method == Method::HEAD,
                &correlation,
            )
            .await
        }
        (Route::Packages { architecture, .. }, true, _) => {
            serve_index(
                &state,
                &context,
                &repository,
                architecture,
                method == Method::HEAD,
                &headers,
                &correlation,
            )
            .await
        }
        (Route::Release { dist }, true, _) => {
            serve_release(
                &state,
                &context,
                &repository,
                dist,
                method == Method::HEAD,
                &headers,
                &correlation,
            )
            .await
        }
        (Route::ReleaseSignature { dist }, true, _) => {
            serve_release_signature(
                &state,
                &context,
                &repository,
                dist,
                method == Method::HEAD,
                &headers,
                &correlation,
            )
            .await
        }
        (Route::Pool { path }, true, _) => {
            let outcome = read_deb(
                &state,
                &context,
                &repository,
                path,
                method == Method::HEAD,
                &correlation,
            )
            .await;
            record_read(
                &state,
                &context,
                &repository_key,
                path,
                &outcome,
                &correlation,
            );
            outcome
        }
        (Route::Pool { path }, false, true) => {
            upload(&state, &context, &repository, path, request, &correlation).await
        }
        (_, false, true) => Err(RawApiError::MethodNotAllowed(
            "only .deb files under pool/ can be uploaded to an apt repository",
        )
        .into()),
        _ => Err(RawApiError::MethodNotAllowed("this apt operation is not supported").into()),
    }
}

// ---------------------------------------------------------------------------
// Lectura
// ---------------------------------------------------------------------------

/// Un registro de indice guardado junto a su binario: la ruta del `.deb`
/// del que habla y el cuerpo `Packages` tal cual se publico.
struct StoredRecord {
    architecture: String,
    updated_at: chrono::DateTime<chrono::Utc>,
    body: String,
}

/// Los registros de indice del repositorio, en el orden de ruta que
/// mantiene el catalogo: la lista nunca reescribe nada, solo lee fichas
/// inmutables ya publicadas.
async fn records(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
) -> Result<Vec<StoredRecord>, Failure> {
    let (reader, _) = the_catalog(state)?;
    let store = the_content_store(state)?;
    let found = reader
        .assets_under(context.account().as_str(), &repository.repo_key, "pool/")
        .await
        .map_err(|error| Failure::from(the_publication_error_ref(&error)))?;
    let mut out = Vec::new();
    for asset in found
        .into_iter()
        .filter(|asset| asset.asset_path.ends_with(".record"))
    {
        let bytes = store
            .get(&asset.digest)
            .await
            .map_err(|error| the_blob_error(&error))?;
        let body = String::from_utf8(bytes.to_vec()).map_err(|_| {
            Failure::plain(
                StatusCode::BAD_GATEWAY,
                "a stored index record is not valid text",
            )
        })?;
        let architecture = record_architecture(&body)
            .ok_or_else(|| {
                Failure::plain(
                    StatusCode::BAD_GATEWAY,
                    "a stored index record has no architecture",
                )
            })?
            .to_string();
        out.push(StoredRecord {
            architecture,
            updated_at: asset.updated_at,
            body,
        });
    }
    Ok(out)
}

/// El campo `Architecture:` de un registro ya pintado.
fn record_architecture(body: &str) -> Option<&str> {
    body.lines()
        .find_map(|line| line.strip_prefix("Architecture: "))
        .map(str::trim)
}

/// El cuerpo `Packages` de una arquitectura: sus registros y los `all`,
/// en el mismo orden de ruta para que dos lecturas del mismo estado den
/// los mismos bytes.
fn index_body(all: &[StoredRecord], architecture: &str) -> String {
    let mut body = String::new();
    for record in all {
        if record.architecture == architecture || record.architecture == "all" {
            body.push_str(&record.body);
        }
    }
    body
}

async fn serve_index(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    architecture: &str,
    head: bool,
    headers: &HeaderMap,
    correlation: &str,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::download_asset(), repository)?;
    let all = records(state, context, repository).await?;
    let body = index_body(&all, architecture);
    serve_bytes(body.into_bytes(), "text/plain", headers, head, correlation)
}

/// El aggregate `Release` del dist: las cabeceras del archive y la lista
/// SHA256 de los indices. La fecha sale del estado, no del reloj: dos
/// lecturas sin publicaciones intermedias son byte a byte la misma.
async fn release_bytes(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    dist: &str,
) -> Result<Vec<u8>, Failure> {
    let all = records(state, context, repository).await?;
    let newest = all
        .iter()
        .map(|record| record.updated_at)
        .max()
        .unwrap_or_else(epoch);
    let date = Route::release_date(&newest);
    let indexes: Vec<(String, Vec<u8>)> = apt::ARCHITECTURES
        .iter()
        .map(|architecture| {
            (
                format!("main/binary-{architecture}/Packages"),
                index_body(&all, architecture).into_bytes(),
            )
        })
        .collect();
    let view: Vec<(&str, &[u8])> = indexes
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    Ok(apt::release_header(dist, &date, &view).into_bytes())
}

fn epoch() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(0, 0).expect("the epoch is a valid moment")
}

async fn serve_release(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    dist: &str,
    head: bool,
    headers: &HeaderMap,
    correlation: &str,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::download_asset(), repository)?;
    let bytes = release_bytes(state, context, repository, dist).await?;
    serve_bytes(bytes, "text/plain", headers, head, correlation)
}

async fn serve_release_signature(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    dist: &str,
    head: bool,
    headers: &HeaderMap,
    correlation: &str,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::download_asset(), repository)?;
    let release = release_bytes(state, context, repository, dist).await?;
    let signature = state
        .metadata_signer
        .sign_detached(&release)
        .map_err(|error| {
            Failure::plain(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("the Release could not be signed: {error}"),
            )
        })?;
    serve_bytes(
        signature,
        "application/pgp-signature",
        headers,
        head,
        correlation,
    )
}

/// Sirve un documento derivado: se decide el permiso, se calculan los
/// bytes y se responde con su etag, con condicionales y HEAD incluidos.
fn serve_bytes(
    bytes: Vec<u8>,
    content_type: &str,
    headers: &HeaderMap,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    let digest = ContentDigest::of_bytes(&bytes);
    let etag = conditional::the_etag(&digest);
    if let Some(response) = conditional_read(&etag, headers, correlation, head, &bytes)? {
        return Ok(response);
    }
    Ok(body_response(
        StatusCode::OK,
        bytes,
        content_type,
        &etag,
        correlation,
    ))
}

/// Un documento que sale de la clave del hub: la parte que puede fallar
/// se evalua solo cuando el permiso ya es permiso.
async fn serve_document<T, E>(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    render: T,
    content_type: &str,
    headers: &HeaderMap,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure>
where
    T: FnOnce() -> Result<Vec<u8>, E>,
    E: std::fmt::Display,
{
    the_decision(state, context, crate::actions::download_asset(), repository)?;
    let bytes = render().map_err(|error| {
        Failure::plain(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("the apt document could not be produced: {error}"),
        )
    })?;
    serve_bytes(bytes, content_type, headers, head, correlation)
}

/// Los bytes de un `.deb`, verificados contra su asset y la compuerta de
/// confianza del repositorio.
async fn read_deb(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    asset_path: &str,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::download_asset(), repository)?;
    let resolved =
        the_resolved_asset(state, context, repository.clone(), asset_path, correlation).await?;
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
        "application/vnd.debian.binary-package",
        &etag,
        correlation,
    );
    apply_cache_headers(&mut response, &served);
    if head && let Ok(value) = HeaderValue::from_str(&length.to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    Ok(response)
}

fn record_read(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &str,
    path: &str,
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
        &format!("{repository}/{path}"),
        result,
        correlation,
    );
}

// ---------------------------------------------------------------------------
// Escritura
// ---------------------------------------------------------------------------

/// Subir un `.deb`: el control dicta la ruta, y la ruta dicta los dos
/// assets. Primero se conmutan los bytes, luego se publica el binario y
/// por ultimo su ficha: interrumpida a la mitad, la ficha no existe y el
/// paquete queda invisible, nunca mentiroso.
async fn upload(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    asset_path: &str,
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
    let control = apt::read_deb_control(&body)?;

    let expected = apt::pool_path_for(&control.package, &control.version, &control.architecture)?;
    if expected != asset_path {
        return Err(Failure::plain(
            StatusCode::CONFLICT,
            &format!("the pool path for this package must be {expected}"),
        ));
    }

    let (_, writer) = the_catalog(state)?;
    let record_path = apt::record_path_for(asset_path);

    // 1. Los bytes del .deb, conmutados antes del catalogo.
    let (deb_digest, deb_size) =
        write_the_content(state, Body::from(body.to_vec()), None, None).await?;
    let record = apt::packages_record(&control, asset_path, deb_size, &deb_digest.value);
    let record_bytes = record.into_bytes();
    // 2. Los bytes de la ficha.
    let (record_digest, record_size) =
        write_the_content(state, Body::from(record_bytes.clone()), None, None).await?;

    let existing_deb = the_current_asset(state, context, &repository.repo_key, asset_path).await?;
    let existing_record =
        the_current_asset(state, context, &repository.repo_key, &record_path).await?;

    // La version es para siempre: identica es el eco o la reanudacion de
    // una publicacion interrumpida, distinta es un no con explicacion.
    let deb_already = existing_deb
        .as_ref()
        .map(|asset| asset.digest == deb_digest)
        .unwrap_or(false);
    let record_already = existing_record
        .as_ref()
        .map(|asset| asset.digest == record_digest)
        .unwrap_or(false);
    if existing_deb.is_some() && !deb_already {
        return Err(Failure::plain(
            StatusCode::CONFLICT,
            &format!(
                "{} is already published with other bytes; a published version is immutable",
                control.package
            ),
        ));
    }
    if existing_record.is_some() && !record_already {
        return Err(Failure::plain(
            StatusCode::CONFLICT,
            &format!(
                "the index record of {} is already published with other bytes; it is immutable",
                control.package
            ),
        ));
    }
    if deb_already && record_already {
        return Ok(axum::Json(json!({ "ok": true })).into_response());
    }

    if !deb_already {
        publish_asset(
            context,
            writer.as_ref(),
            repository,
            asset_path,
            PackageCoordinates {
                package_name: control.package.clone(),
                package_version: control.version.clone(),
            },
            deb_digest.clone(),
            deb_size,
            &format!(
                "{} is already published; a published version is immutable",
                control.package
            ),
            correlation,
        )
        .await?;
    }
    if !record_already {
        publish_asset(
            context,
            writer.as_ref(),
            repository,
            &record_path,
            PackageCoordinates {
                package_name: format!("{}#record", control.package),
                package_version: control.version.clone(),
            },
            record_digest,
            record_size,
            &format!(
                "the index record of {} is already published; it is immutable",
                control.package
            ),
            correlation,
        )
        .await?;
    }

    Ok(axum::Json(json!({ "ok": true })).into_response())
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

// ---------------------------------------------------------------------------
// Respuesta comun
// ---------------------------------------------------------------------------

/// `If-None-Match` contra el etag del documento: un 304 dice que no cambio.
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
