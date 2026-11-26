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
    AptCasReference, AptGroupSnapshot, AptSnapshotKey, AptSnapshotMember, BlobLocation,
    DigestScope, PackageCoordinates, RepositoryDescriptor, RepositoryFormat, RepositoryMode,
    apt::{self, AptError, AptProxyConfig, PackagesCompression, ParsedRelease, Route},
};
use hodei_evidence::{ContentDigest, DigestAlgorithm};
use serde_json::json;
use sha2::Digest as _;

use super::raw::{
    RawApiError, apply_cache_headers, epoch_moment, publish_system_asset, serve_system_bytes,
    system_body_response, the_admission_error, the_blob_error, the_catalog, the_content_store,
    the_context, the_current_asset, the_decision, the_publication_error, the_repository_in,
    the_resolved_asset, write_the_content,
};
use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{correlation_id, record_audit};
use crate::openpgp::MetadataSigner;
use crate::raw::conditional;
use crate::resolution::{ProxyServed, the_proxy_asset_verified};

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
pub(crate) fn normalize_authorization(headers: &mut HeaderMap) {
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

    if repository.mode == RepositoryMode::Proxy {
        if !reading {
            return Err(RawApiError::MethodNotAllowed(
                "a proxy apt repository does not accept uploads",
            )
            .into());
        }
        let outcome = handle_proxy(
            &state,
            &context,
            &repository,
            &route,
            method == Method::HEAD,
            &headers,
            &correlation,
        )
        .await;
        if matches!(route, Route::Pool { .. }) {
            record_read(
                &state,
                &context,
                &repository_key,
                &route.asset_path(),
                &outcome,
                &correlation,
            );
        }
        return outcome;
    }

    if repository.mode == RepositoryMode::Group {
        if !reading {
            return Err(RawApiError::MethodNotAllowed(
                "a group apt repository does not accept uploads",
            )
            .into());
        }
        let outcome = handle_group(
            &state,
            &context,
            &repository,
            &route,
            method == Method::HEAD,
            &headers,
            &correlation,
        )
        .await;
        if matches!(route, Route::Pool { .. }) {
            record_read(
                &state,
                &context,
                &repository_key,
                &route.asset_path(),
                &outcome,
                &correlation,
            );
        }
        return outcome;
    }

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
        (
            Route::Packages {
                component,
                architecture,
                compression,
                ..
            },
            true,
            _,
        ) => {
            if component.as_deref().is_some_and(|c| c != "main")
                || *compression != PackagesCompression::Plain
            {
                return Err(RawApiError::NotFound.into());
            }
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
        (Route::InRelease { .. } | Route::PackagesByHash { .. }, true, _) => {
            Err(RawApiError::NotFound.into())
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
        .map_err(|error| Failure::from(the_publication_error(&error)))?;
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
    Ok(serve_system_bytes(
        body.into_bytes(),
        "text/plain",
        headers,
        head,
        correlation,
    )?)
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
        .unwrap_or_else(epoch_moment);
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
    Ok(serve_system_bytes(
        bytes,
        "text/plain",
        headers,
        head,
        correlation,
    )?)
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
    Ok(serve_system_bytes(
        signature,
        "application/pgp-signature",
        headers,
        head,
        correlation,
    )?)
}

/// Un documento que sale de la clave del hub: la parte que puede fallar
/// se evalua solo cuando el permiso ya es permiso.
#[allow(clippy::too_many_arguments)]
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
    Ok(serve_system_bytes(
        bytes,
        content_type,
        headers,
        head,
        correlation,
    )?)
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
    let mut response = system_body_response(
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
    let (deb_digest, deb_size) = write_the_content(
        state,
        context.account().as_str(),
        Body::from(body.to_vec()),
        None,
        None,
    )
    .await?;
    let record = apt::packages_record(&control, asset_path, deb_size, &deb_digest.value);
    let record_bytes = record.into_bytes();
    // 2. Los bytes de la ficha.
    let (record_digest, record_size) = write_the_content(
        state,
        context.account().as_str(),
        Body::from(record_bytes.clone()),
        None,
        None,
    )
    .await?;

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
        publish_system_asset(
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
        publish_system_asset(
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

// ---------------------------------------------------------------------------
// Proxy
// ---------------------------------------------------------------------------

async fn handle_proxy(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    route: &Route,
    head: bool,
    _headers: &HeaderMap,
    correlation: &str,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::download_asset(), repository)?;
    let apt_config = AptProxyConfig::of(&repository.config).map_err(|error| {
        tracing::warn!(error = %error, "invalid apt proxy configuration");
        Failure::plain(
            StatusCode::BAD_GATEWAY,
            "the apt proxy configuration is invalid",
        )
    })?;

    match route {
        Route::Key => Err(RawApiError::NotFound.into()),
        Route::InRelease { dist } | Route::Release { dist } => {
            if !apt_config.allows_dist(dist) {
                return Err(RawApiError::NotFound.into());
            }
            let is_inrelease = matches!(route, Route::InRelease { .. });
            let (served, _parsed) =
                resolve_proxy_release(state, context, repository, dist, is_inrelease, correlation)
                    .await?;
            serve_proxy_response(
                state,
                context,
                repository,
                &served,
                "text/plain",
                head,
                correlation,
            )
            .await
        }
        Route::ReleaseSignature { dist } => {
            if !apt_config.allows_dist(dist) {
                return Err(RawApiError::NotFound.into());
            }
            let release_path = format!("dists/{dist}/Release");
            let sig_path = format!("dists/{dist}/Release.gpg");
            let release_asset =
                the_current_asset(state, context, &repository.repo_key, &release_path).await?;
            if let Some(ref rel) = release_asset {
                let bytes = the_content_store(state)?
                    .get(&rel.digest)
                    .await
                    .map_err(|error| the_blob_error(&error))?;
                let parsed = parse_and_check_release(&bytes, false)?;
                if parsed.is_expired(chrono::Utc::now()) {
                    return Err(Failure::plain(
                        StatusCode::BAD_GATEWAY,
                        "upstream Release has expired (Valid-Until)",
                    ));
                }
            }
            let sig_asset =
                the_current_asset(state, context, &repository.repo_key, &sig_path).await?;
            // If Release was refreshed more recently than Release.gpg, force-refresh
            // Release.gpg so signature and Release never come from different generations.
            let force_fetch = match (&release_asset, &sig_asset) {
                (Some(rel), Some(sig)) => rel.updated_at > sig.updated_at,
                _ => false,
            };
            let served = the_proxy_asset_verified(
                state,
                context,
                repository,
                &sig_path,
                None,
                force_fetch,
                correlation,
            )
            .await?;
            serve_proxy_response(
                state,
                context,
                repository,
                &served,
                "application/pgp-signature",
                head,
                correlation,
            )
            .await
        }
        Route::Packages {
            dist,
            component,
            architecture,
            compression,
        } => {
            if !apt_config.allows_dist(dist)
                || !apt_config.allows_component(component.as_deref())
                || !apt_config.allows_architecture(architecture)
            {
                return Err(RawApiError::NotFound.into());
            }
            let parsed =
                active_parsed_release(state, context, repository, dist, correlation).await?;
            let asset_path = route.asset_path();
            let rel_in_dist = asset_path
                .strip_prefix(&format!("dists/{dist}/"))
                .ok_or(RawApiError::NotFound)?;
            let (expected_sha256, expected_size) = parsed
                .sha256_entries
                .get(rel_in_dist)
                .ok_or(RawApiError::NotFound)?;

            let served = the_proxy_asset_verified(
                state,
                context,
                repository,
                &asset_path,
                Some(expected_sha256),
                false,
                correlation,
            )
            .await?;
            if served.asset.size_bytes != *expected_size {
                return Err(Failure::plain(
                    StatusCode::BAD_GATEWAY,
                    "upstream Packages size does not match Release",
                ));
            }
            serve_proxy_response(
                state,
                context,
                repository,
                &served,
                compression.content_type(),
                head,
                correlation,
            )
            .await
        }
        Route::PackagesByHash {
            dist,
            component,
            architecture,
            algorithm: _,
            hash,
        } => {
            if !apt_config.allows_dist(dist)
                || !apt_config.allows_component(component.as_deref())
                || !apt_config.allows_architecture(architecture)
            {
                return Err(RawApiError::NotFound.into());
            }
            let parsed =
                active_parsed_release(state, context, repository, dist, correlation).await?;
            let prefix = match component {
                Some(comp) => format!("{comp}/binary-{architecture}/Packages"),
                None => format!("binary-{architecture}/Packages"),
            };
            let matched = parsed
                .sha256_entries
                .iter()
                .find(|(path, (sha, _size))| path.starts_with(&prefix) && sha == hash);
            let Some((_, (expected_sha256, expected_size))) = matched else {
                return Err(RawApiError::NotFound.into());
            };
            let asset_path = route.asset_path();
            let served = the_proxy_asset_verified(
                state,
                context,
                repository,
                &asset_path,
                Some(expected_sha256),
                false,
                correlation,
            )
            .await?;
            if served.asset.size_bytes != *expected_size {
                return Err(Failure::plain(
                    StatusCode::BAD_GATEWAY,
                    "upstream by-hash Packages size does not match Release",
                ));
            }
            serve_proxy_response(
                state,
                context,
                repository,
                &served,
                "application/octet-stream",
                head,
                correlation,
            )
            .await
        }
        Route::Pool { path } => {
            if !apt_config.allows_pool_path(path) {
                return Err(RawApiError::NotFound.into());
            }
            let entry =
                find_deb_in_cached_snapshot(state, context, repository, &apt_config, path).await?;
            let served = the_proxy_asset_verified(
                state,
                context,
                repository,
                path,
                Some(&entry.sha256),
                false,
                correlation,
            )
            .await?;
            if served.asset.size_bytes != entry.size {
                return Err(Failure::plain(
                    StatusCode::BAD_GATEWAY,
                    "upstream .deb size does not match signed Packages index",
                ));
            }
            serve_proxy_response(
                state,
                context,
                repository,
                &served,
                "application/vnd.debian.binary-package",
                head,
                correlation,
            )
            .await
        }
    }
}

fn parse_and_check_release(bytes: &[u8], is_inrelease: bool) -> Result<ParsedRelease, Failure> {
    let body = apt::extract_release_body(bytes, is_inrelease)
        .map_err(|error| Failure::plain(StatusCode::BAD_GATEWAY, &error.to_string()))?;
    let parsed = apt::parse_release(&body)
        .map_err(|error| Failure::plain(StatusCode::BAD_GATEWAY, &error.to_string()))?;
    if parsed.is_expired(chrono::Utc::now()) {
        return Err(Failure::plain(
            StatusCode::BAD_GATEWAY,
            "upstream Release has expired (Valid-Until)",
        ));
    }
    Ok(parsed)
}

async fn resolve_proxy_release(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    dist: &str,
    is_inrelease: bool,
    correlation: &str,
) -> Result<(ProxyServed, ParsedRelease), Failure> {
    let asset_path = if is_inrelease {
        format!("dists/{dist}/InRelease")
    } else {
        format!("dists/{dist}/Release")
    };
    let served = match the_proxy_asset_verified(
        state,
        context,
        repository,
        &asset_path,
        None,
        false,
        correlation,
    )
    .await
    {
        Ok(served) => served,
        Err(RawApiError::Offline(_)) if is_inrelease => {
            let release_path = format!("dists/{dist}/Release");
            if the_current_asset(state, context, &repository.repo_key, &release_path)
                .await?
                .is_some()
            {
                return Err(RawApiError::NotFound.into());
            }
            return Err(RawApiError::Offline(
                "the proxy is offline and does not hold a verified copy of that content",
            )
            .into());
        }
        Err(error) => return Err(error.into()),
    };
    let bytes = the_content_store(state)?
        .get(&served.asset.digest)
        .await
        .map_err(|error| the_blob_error(&error))?;
    let parsed = parse_and_check_release(&bytes, is_inrelease)?;
    Ok((served, parsed))
}

async fn active_parsed_release(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    dist: &str,
    correlation: &str,
) -> Result<ParsedRelease, Failure> {
    let apt_config = AptProxyConfig::of(&repository.config).map_err(|error| {
        tracing::warn!(error = %error, member = %repository.repo_key, "invalid apt proxy configuration");
        Failure::plain(StatusCode::BAD_GATEWAY, "the apt proxy configuration is invalid")
    })?;
    let trusted_key = apt_config.trusted_key_asc.as_deref().ok_or_else(|| {
        Failure::plain(
            StatusCode::BAD_GATEWAY,
            "the APT proxy group member has no operator-pinned signing key",
        )
    })?;

    match resolve_proxy_release(state, context, repository, dist, true, correlation).await {
        Ok((served, parsed)) => {
            let bytes = the_content_store(state)?
                .get(&served.asset.digest)
                .await
                .map_err(|error| the_blob_error(&error))?;
            MetadataSigner::verify_cleartext(&bytes, trusted_key).map_err(|_| {
                Failure::plain(
                    StatusCode::BAD_GATEWAY,
                    "upstream InRelease signature is invalid",
                )
            })?;
            Ok(parsed)
        }
        Err(Failure::Raw(RawApiError::NotFound)) => {
            let (release, parsed) =
                resolve_proxy_release(state, context, repository, dist, false, correlation).await?;
            let release_bytes = the_content_store(state)?
                .get(&release.asset.digest)
                .await
                .map_err(|error| the_blob_error(&error))?;
            let signature_path = format!("dists/{dist}/Release.gpg");
            let signature = the_proxy_asset_verified(
                state,
                context,
                repository,
                &signature_path,
                None,
                false,
                correlation,
            )
            .await?;
            let signature_bytes = the_content_store(state)?
                .get(&signature.asset.digest)
                .await
                .map_err(|error| the_blob_error(&error))?;
            MetadataSigner::verify_detached(&release_bytes, &signature_bytes, trusted_key)
                .map_err(|_| {
                    Failure::plain(
                        StatusCode::BAD_GATEWAY,
                        "upstream Release signature is invalid",
                    )
                })?;
            Ok(parsed)
        }
        Err(other) => Err(other),
    }
}

async fn find_deb_in_cached_snapshot(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    apt_config: &AptProxyConfig,
    pool_path: &str,
) -> Result<apt::DebIndexEntry, Failure> {
    let (reader, _) = the_catalog(state)?;
    let store = the_content_store(state)?;
    let assets = reader
        .assets_under(context.account().as_str(), &repository.repo_key, "dists/")
        .await
        .map_err(|error| Failure::from(the_publication_error(&error)))?;

    let mut parsed_releases: std::collections::BTreeMap<String, ParsedRelease> =
        std::collections::BTreeMap::new();

    for asset in &assets {
        let Ok(route) = Route::parse(&asset.asset_path) else {
            continue;
        };
        let (dist, component, architecture, by_hash_compression) = match &route {
            Route::Packages {
                dist,
                component,
                architecture,
                compression,
            } => (
                dist.clone(),
                component.clone(),
                architecture.clone(),
                Some(*compression),
            ),
            Route::PackagesByHash {
                dist,
                component,
                architecture,
                ..
            } => (dist.clone(), component.clone(), architecture.clone(), None),
            _ => continue,
        };
        if !apt_config.allows_dist(&dist)
            || !apt_config.allows_component(component.as_deref())
            || !apt_config.allows_architecture(&architecture)
        {
            continue;
        }
        if !parsed_releases.contains_key(&dist) {
            let inrel = format!("dists/{dist}/InRelease");
            let rel = format!("dists/{dist}/Release");
            let loaded = if let Some(r_asset) = assets.iter().find(|a| a.asset_path == inrel) {
                let bytes = store
                    .get(&r_asset.digest)
                    .await
                    .map_err(|error| the_blob_error(&error))?;
                parse_and_check_release(&bytes, true)?
            } else if let Some(r_asset) = assets.iter().find(|a| a.asset_path == rel) {
                let bytes = store
                    .get(&r_asset.digest)
                    .await
                    .map_err(|error| the_blob_error(&error))?;
                parse_and_check_release(&bytes, false)?
            } else {
                continue;
            };
            parsed_releases.insert(dist.clone(), loaded);
        }
        let Some(parsed) = parsed_releases.get(&dist) else {
            continue;
        };
        let compression = match by_hash_compression {
            Some(comp) => {
                let Some(rel_in_dist) = asset.asset_path.strip_prefix(&format!("dists/{dist}/"))
                else {
                    continue;
                };
                let Some((expected_sha, expected_size)) = parsed.sha256_entries.get(rel_in_dist)
                else {
                    continue;
                };
                if asset.digest.value != *expected_sha || asset.size_bytes != *expected_size {
                    continue;
                }
                comp
            }
            None => {
                let prefix = match &component {
                    Some(comp) => format!("{comp}/binary-{architecture}/Packages"),
                    None => format!("binary-{architecture}/Packages"),
                };
                let Some((rel_path, _)) = parsed.sha256_entries.iter().find(|(p, (sha, sz))| {
                    p.starts_with(&prefix) && *sha == asset.digest.value && *sz == asset.size_bytes
                }) else {
                    continue;
                };
                if rel_path.ends_with(".xz") {
                    PackagesCompression::Xz
                } else if rel_path.ends_with(".gz") {
                    PackagesCompression::Gz
                } else {
                    PackagesCompression::Plain
                }
            }
        };
        let raw_bytes = store
            .get(&asset.digest)
            .await
            .map_err(|error| the_blob_error(&error))?;
        let plain = apt::decompress_packages(&raw_bytes, compression)
            .map_err(|error| Failure::plain(StatusCode::BAD_GATEWAY, &error.to_string()))?;
        let text = String::from_utf8_lossy(&plain);
        if let Some(entry) = apt::find_deb_in_packages(&text, pool_path) {
            return Ok(entry);
        }
    }

    Err(RawApiError::NotFound.into())
}

async fn serve_proxy_response(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    served: &ProxyServed,
    content_type: &str,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    crate::trust::admit(
        state,
        context.account().as_str(),
        repository,
        &served.asset.digest,
    )
    .await
    .map_err(the_admission_error)?;
    let etag = conditional::the_etag(&served.asset.digest);
    let bytes = if head {
        Vec::new()
    } else {
        the_content_store(state)?
            .get(&served.asset.digest)
            .await
            .map_err(|error| the_blob_error(&error))?
            .to_vec()
    };
    let mut response =
        system_body_response(StatusCode::OK, bytes, content_type, &etag, correlation);
    let cache_headers = Some((served.status, served.provenance.clone()));
    apply_cache_headers(&mut response, &cache_headers);
    if head && let Ok(value) = HeaderValue::from_str(&served.asset.size_bytes.to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    Ok(response)
}

// ---------------------------------------------------------------------------
// Group
// ---------------------------------------------------------------------------

fn group_snapshot_key(
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    dist: &str,
) -> AptSnapshotKey {
    AptSnapshotKey {
        account: context.account().as_str().to_owned(),
        group: repository.repo_key.clone(),
        distribution: dist.to_owned(),
        principal_subject: context.principal().subject().as_str().to_owned(),
    }
}

fn apt_snapshots(state: &AppState) -> Result<&dyn hodei_catalog::AptSnapshotStore, Failure> {
    state.apt_snapshot_store.as_deref().ok_or_else(|| {
        Failure::plain(
            StatusCode::SERVICE_UNAVAILABLE,
            "APT snapshot storage is unavailable",
        )
    })
}

async fn write_apt_reference(
    state: &AppState,
    account: &str,
    scope: DigestScope,
    bytes: &[u8],
) -> Result<AptCasReference, Failure> {
    let (digest, size_bytes) =
        write_the_content(state, account, Body::from(bytes.to_vec()), None, None).await?;
    Ok(AptCasReference {
        location: BlobLocation::new(account, scope, digest),
        size_bytes,
    })
}

async fn read_apt_reference(
    state: &AppState,
    reference: &AptCasReference,
) -> Result<Vec<u8>, Failure> {
    let bytes = the_content_store(state)?
        .get(&reference.location.digest)
        .await
        .map_err(|error| the_blob_error(&error))?;
    if u64::try_from(bytes.len()).ok() != Some(reference.size_bytes) {
        return Err(Failure::plain(
            StatusCode::BAD_GATEWAY,
            "an APT snapshot blob has an inconsistent length",
        ));
    }
    Ok(bytes.to_vec())
}

fn same_apt_generation(left: &AptGroupSnapshot, right: &AptGroupSnapshot) -> bool {
    left.digest == right.digest
        && left.release == right.release
        && left.packages_by_arch == right.packages_by_arch
        && left.pool_by_path == right.pool_by_path
        && left.members == right.members
}

async fn publish_group_snapshot(
    state: &AppState,
    key: &AptSnapshotKey,
    mut candidate: AptGroupSnapshot,
) -> Result<AptGroupSnapshot, Failure> {
    let store = apt_snapshots(state)?;
    let mut current = store.current_apt_snapshot(key).await.map_err(|_| {
        Failure::plain(
            StatusCode::SERVICE_UNAVAILABLE,
            "APT snapshot storage failed",
        )
    })?;
    for _ in 0..3 {
        let expected = current.as_ref().map(|snapshot| snapshot.revision);
        if let Some(snapshot) = &current {
            candidate.revision = snapshot.revision;
            if same_apt_generation(&candidate, snapshot) {
                return Ok(snapshot.clone());
            }
            candidate.revision = snapshot.revision.checked_add(1).ok_or_else(|| {
                Failure::plain(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "APT snapshot revision exhausted",
                )
            })?;
        } else {
            candidate.revision = 1;
        }
        match store
            .publish_apt_snapshot(key, expected, candidate.clone())
            .await
        {
            Ok(()) => return Ok(candidate),
            Err(hodei_catalog::CatalogError::PreconditionFailed { .. }) => {
                current = store.current_apt_snapshot(key).await.map_err(|_| {
                    Failure::plain(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "APT snapshot storage failed",
                    )
                })?;
            }
            Err(_) => {
                return Err(Failure::plain(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "APT snapshot publication failed",
                ));
            }
        }
    }
    Err(Failure::plain(
        StatusCode::SERVICE_UNAVAILABLE,
        "APT snapshot changed during publication",
    ))
}

async fn require_current_members(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    snapshot: &AptGroupSnapshot,
) -> Result<Vec<RepositoryDescriptor>, Failure> {
    let leaves = super::raw_group::the_leaves(state, context, repository).await?;
    if snapshot
        .members
        .iter()
        .any(|member| !leaves.iter().any(|leaf| leaf.repo_key == member.repository))
    {
        return Err(RawApiError::NotFound.into());
    }
    Ok(leaves)
}

async fn durable_group_snapshot(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    dist: &str,
    correlation: &str,
) -> Result<AptGroupSnapshot, Failure> {
    let _ = apt_snapshots(state)?;
    let key = group_snapshot_key(context, repository, dist);
    let candidate = build_group_snapshot(state, context, repository, dist, correlation).await?;
    let snapshot = publish_group_snapshot(state, &key, candidate).await?;

    // Pool URLs do not carry a distribution. Keep a durable, group-scoped
    // index of pool references contributed by the latest materialized dist
    // generations; each candidate is still membership- and trust-checked on read.
    let pool_key = group_snapshot_key(context, repository, "__pool__");
    let store = apt_snapshots(state)?;
    let existing_pool = store.current_apt_snapshot(&pool_key).await.map_err(|_| {
        Failure::plain(
            StatusCode::SERVICE_UNAVAILABLE,
            "APT snapshot storage failed",
        )
    })?;
    let mut pool_snapshot = snapshot.clone();
    pool_snapshot.packages_by_arch.clear();
    if let Some(existing) = existing_pool {
        let mut pool = existing.pool_by_path;
        pool.extend(pool_snapshot.pool_by_path);
        pool_snapshot.pool_by_path = pool;
        let mut members = existing.members;
        for member in pool_snapshot.members {
            if !members.contains(&member) {
                members.push(member);
            }
        }
        pool_snapshot.members = members;
    }
    let mut pool_identity = String::new();
    for (path, reference) in &pool_snapshot.pool_by_path {
        pool_identity.push_str(path);
        pool_identity.push(':');
        pool_identity.push_str(&reference.location.digest.to_string());
        pool_identity.push(':');
        pool_identity.push_str(&reference.size_bytes.to_string());
        pool_identity.push('\n');
    }
    use sha2::Digest as _;
    pool_snapshot.digest = ContentDigest {
        algorithm: DigestAlgorithm::Sha256,
        value: format!("{:x}", sha2::Sha256::digest(pool_identity.as_bytes())),
    };
    let _ = publish_group_snapshot(state, &pool_key, pool_snapshot).await?;
    Ok(snapshot)
}

async fn handle_group(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    route: &Route,
    head: bool,
    headers: &HeaderMap,
    correlation: &str,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::download_asset(), repository)?;

    match route {
        Route::Key => {
            serve_document(
                state,
                context,
                repository,
                || state.metadata_signer.key_asc().map(String::into_bytes),
                "application/pgp-keys",
                headers,
                head,
                correlation,
            )
            .await
        }
        Route::InRelease { dist } => {
            let snapshot =
                durable_group_snapshot(state, context, repository, dist, correlation).await?;
            require_current_members(state, context, repository, &snapshot).await?;
            let release = read_apt_reference(state, &snapshot.release).await?;
            let release = std::str::from_utf8(&release).map_err(|_| {
                Failure::plain(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Release is not valid UTF-8",
                )
            })?;
            let signed = state.metadata_signer.sign_cleartext(release).map_err(|_| {
                Failure::plain(StatusCode::INTERNAL_SERVER_ERROR, "Release signing failed")
            })?;
            Ok(serve_system_bytes(
                signed,
                "text/plain",
                headers,
                head,
                correlation,
            )?)
        }
        Route::Release { dist } => {
            let snapshot =
                durable_group_snapshot(state, context, repository, dist, correlation).await?;
            require_current_members(state, context, repository, &snapshot).await?;
            let bytes = read_apt_reference(state, &snapshot.release).await?;
            Ok(serve_system_bytes(
                bytes,
                "text/plain",
                headers,
                head,
                correlation,
            )?)
        }
        Route::ReleaseSignature { dist } => {
            let snapshot =
                durable_group_snapshot(state, context, repository, dist, correlation).await?;
            require_current_members(state, context, repository, &snapshot).await?;
            let bytes = read_apt_reference(state, &snapshot.signature).await?;
            Ok(serve_system_bytes(
                bytes,
                "application/pgp-signature",
                headers,
                head,
                correlation,
            )?)
        }
        Route::Packages {
            dist,
            component,
            architecture,
            compression,
        } => {
            if component.as_deref().is_some_and(|c| c != "main")
                || *compression != PackagesCompression::Plain
            {
                return Err(RawApiError::NotFound.into());
            }
            let snapshot =
                durable_group_snapshot(state, context, repository, dist, correlation).await?;
            require_current_members(state, context, repository, &snapshot).await?;
            let reference = snapshot
                .packages_by_arch
                .get(architecture)
                .ok_or(RawApiError::NotFound)?;
            let bytes = read_apt_reference(state, reference).await?;
            Ok(serve_system_bytes(
                bytes,
                "text/plain",
                headers,
                head,
                correlation,
            )?)
        }
        Route::PackagesByHash {
            dist,
            component,
            architecture,
            algorithm,
            hash,
        } => {
            if component.as_deref().is_some_and(|c| c != "main")
                || algorithm != "SHA256"
                || hash.len() != 64
                || !hash.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(RawApiError::NotFound.into());
            }
            let key = group_snapshot_key(context, repository, dist);
            let digest = ContentDigest::sha256(hash.to_ascii_lowercase());
            let snapshot = apt_snapshots(state)?
                .apt_snapshot_by_packages_hash(&key, architecture, &digest)
                .await
                .map_err(|_| {
                    Failure::plain(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "APT snapshot lookup failed",
                    )
                })?
                .ok_or(RawApiError::NotFound)?;
            require_current_members(state, context, repository, &snapshot).await?;
            let reference = snapshot
                .packages_by_arch
                .get(architecture)
                .ok_or(RawApiError::NotFound)?;
            if reference.location.digest != digest {
                return Err(RawApiError::NotFound.into());
            }
            let bytes = read_apt_reference(state, reference).await?;
            if apt::parse_packages_stanzas(
                std::str::from_utf8(&bytes).map_err(|_| RawApiError::NotFound)?,
            )
            .is_err()
            {
                return Err(RawApiError::NotFound.into());
            }
            Ok(serve_system_bytes(
                bytes,
                "application/octet-stream",
                headers,
                head,
                correlation,
            )?)
        }
        Route::Pool { path } => {
            serve_group_pool(state, context, repository, path, head, correlation).await
        }
    }
}

async fn collect_proxy_member_stanzas(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    member: &RepositoryDescriptor,
    dist: &str,
    architecture: &str,
    correlation: &str,
) -> Result<
    (
        Vec<apt::PackageStanza>,
        Option<chrono::DateTime<chrono::Utc>>,
    ),
    Failure,
> {
    let apt_config = AptProxyConfig::of(&member.config).map_err(|error| {
        tracing::warn!(error = %error, member = %member.repo_key, "invalid apt proxy configuration");
        Failure::plain(
            StatusCode::BAD_GATEWAY,
            "the apt proxy configuration is invalid",
        )
    })?;
    if !apt_config.allows_dist(dist)
        || !apt_config.allows_component(Some("main"))
        || !apt_config.allows_architecture(architecture)
    {
        return Ok((Vec::new(), None));
    }

    let parsed = match active_parsed_release(state, context, member, dist, correlation).await {
        Ok(parsed) => parsed,
        Err(Failure::Raw(RawApiError::NotFound)) => return Ok((Vec::new(), None)),
        Err(other) => return Err(other),
    };

    let mut member_updated = None;
    for rel_name in [
        format!("dists/{dist}/InRelease"),
        format!("dists/{dist}/Release"),
    ] {
        if let Some(asset) = the_current_asset(state, context, &member.repo_key, &rel_name).await? {
            member_updated = Some(asset.updated_at);
            break;
        }
    }

    let candidates = [
        (
            format!("main/binary-{architecture}/Packages"),
            PackagesCompression::Plain,
        ),
        (
            format!("main/binary-{architecture}/Packages.gz"),
            PackagesCompression::Gz,
        ),
        (
            format!("main/binary-{architecture}/Packages.xz"),
            PackagesCompression::Xz,
        ),
    ];
    let Some((rel_in_dist, compression, (expected_sha256, expected_size))) =
        candidates.into_iter().find_map(|(rel, comp)| {
            parsed
                .sha256_entries
                .get(&rel)
                .map(|entry| (rel, comp, entry))
        })
    else {
        return Ok((Vec::new(), member_updated));
    };

    let asset_path = format!("dists/{dist}/{rel_in_dist}");
    let served = the_proxy_asset_verified(
        state,
        context,
        member,
        &asset_path,
        Some(expected_sha256),
        false,
        correlation,
    )
    .await?;
    if served.asset.size_bytes != *expected_size {
        return Err(Failure::plain(
            StatusCode::BAD_GATEWAY,
            "upstream Packages size does not match Release",
        ));
    }
    let raw_bytes = the_content_store(state)?
        .get(&served.asset.digest)
        .await
        .map_err(|error| the_blob_error(&error))?;
    let plain = apt::decompress_packages(&raw_bytes, compression)
        .map_err(|error| Failure::plain(StatusCode::BAD_GATEWAY, &error.to_string()))?;
    let text = String::from_utf8(plain).map_err(|_| {
        Failure::plain(
            StatusCode::BAD_GATEWAY,
            "upstream Packages index is not valid UTF-8",
        )
    })?;
    let stanzas = apt::parse_packages_stanzas(&text)
        .map_err(|error| Failure::plain(StatusCode::BAD_GATEWAY, &error.to_string()))?
        .into_iter()
        .filter(|stanza| apt_config.allows_pool_path(&stanza.filename))
        .collect();
    Ok((stanzas, member_updated))
}

async fn build_group_snapshot(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    dist: &str,
    correlation: &str,
) -> Result<AptGroupSnapshot, Failure> {
    let leaves = super::raw_group::the_leaves(state, context, repository).await?;
    let mut newest: Option<chrono::DateTime<chrono::Utc>> = None;
    let mut winners: std::collections::BTreeMap<
        (String, String, String),
        (RepositoryDescriptor, apt::PackageStanza),
    > = std::collections::BTreeMap::new();

    for member in &leaves {
        let stanzas = match member.mode {
            RepositoryMode::Hosted => {
                let records = records(state, context, member).await?;
                let mut stanzas = Vec::new();
                for record in records {
                    newest = Some(
                        newest.map_or(record.updated_at, |current| current.max(record.updated_at)),
                    );
                    stanzas.extend(apt::parse_packages_stanzas(&record.body).map_err(|error| {
                        Failure::plain(StatusCode::BAD_GATEWAY, &error.to_string())
                    })?);
                }
                stanzas
            }
            RepositoryMode::Proxy => {
                let mut stanzas = Vec::new();
                for architecture in apt::ARCHITECTURES {
                    let (found, updated) = collect_proxy_member_stanzas(
                        state,
                        context,
                        member,
                        dist,
                        architecture,
                        correlation,
                    )
                    .await?;
                    if let Some(updated) = updated {
                        newest = Some(newest.map_or(updated, |current| current.max(updated)));
                    }
                    stanzas.extend(found);
                }
                stanzas
            }
            RepositoryMode::Group => Vec::new(),
        };

        for stanza in stanzas {
            let key = (
                stanza.package.clone(),
                stanza.version.clone(),
                stanza.architecture.clone(),
            );
            // Configured leaf order is the package-precedence policy: the first
            // member offering a package/version/architecture wins, and its repo
            // remains the recorded source for the snapshot and pool bytes.
            winners
                .entry(key)
                .or_insert_with(|| (member.clone(), stanza));
        }
    }

    let mut indexes = Vec::with_capacity(apt::ARCHITECTURES.len());
    for architecture in apt::ARCHITECTURES {
        let body = winners
            .values()
            .filter(|(_, stanza)| {
                stanza.architecture == *architecture || stanza.architecture == "all"
            })
            .map(|(_, stanza)| stanza.raw.as_str())
            .collect::<String>();
        indexes.push((
            architecture.to_owned(),
            format!("main/binary-{architecture}/Packages"),
            body.into_bytes(),
        ));
    }

    let date = Route::release_date(&newest.unwrap_or_else(epoch_moment));
    let release_view: Vec<(&str, &[u8])> = indexes
        .iter()
        .map(|(_, name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    let release_bytes = apt::release_header_by_hash(dist, &date, &release_view).into_bytes();
    let mut snapshot_identity = release_bytes.clone();
    snapshot_identity.extend_from_slice(state.metadata_signer.fingerprint_hex().as_bytes());
    let digest = ContentDigest {
        algorithm: DigestAlgorithm::Sha256,
        value: format!("{:x}", sha2::Sha256::digest(&snapshot_identity)),
    };

    let account = context.account().as_str();
    let mut packages_by_arch = std::collections::BTreeMap::new();
    for (architecture, _, bytes) in &indexes {
        let reference =
            write_apt_reference(state, account, repository.config.digest_scope, bytes).await?;
        packages_by_arch.insert(architecture.to_string(), reference);
    }
    let release = write_apt_reference(
        state,
        account,
        repository.config.digest_scope,
        &release_bytes,
    )
    .await?;

    let mut pool_by_path = std::collections::BTreeMap::new();
    let mut members = Vec::new();
    for (member, stanza) in winners.values() {
        if pool_by_path.contains_key(&stanza.filename) {
            continue;
        }
        let entry = apt::find_deb_in_packages(&stanza.raw, &stanza.filename).ok_or_else(|| {
            Failure::plain(
                StatusCode::BAD_GATEWAY,
                "a Packages stanza has no valid SHA256 or size",
            )
        })?;
        let (digest, size_bytes) = match member.mode {
            RepositoryMode::Hosted => {
                let asset = the_current_asset(state, context, &member.repo_key, &stanza.filename)
                    .await?
                    .ok_or(RawApiError::NotFound)?;
                if !asset.digest.value.eq_ignore_ascii_case(&entry.sha256)
                    || asset.size_bytes != entry.size
                {
                    return Err(Failure::plain(
                        StatusCode::BAD_GATEWAY,
                        "a hosted package does not match its Packages stanza",
                    ));
                }
                (asset.digest, asset.size_bytes)
            }
            RepositoryMode::Proxy => (ContentDigest::sha256(entry.sha256.clone()), entry.size),
            RepositoryMode::Group => continue,
        };
        let content = AptCasReference {
            location: BlobLocation::new(account, member.config.digest_scope, digest),
            size_bytes,
        };
        pool_by_path.insert(stanza.filename.clone(), content.clone());
        members.push(AptSnapshotMember {
            repository: member.repo_key.clone(),
            package: stanza.package.clone(),
            version: stanza.version.clone(),
            content,
        });
        if members.len() > 10_000 || pool_by_path.len() > 50_000 {
            return Err(Failure::plain(
                StatusCode::PAYLOAD_TOO_LARGE,
                "APT group snapshot exceeds its package limit",
            ));
        }
    }

    let key = group_snapshot_key(context, repository, dist);
    let current = apt_snapshots(state)?
        .current_apt_snapshot(&key)
        .await
        .map_err(|_| {
            Failure::plain(
                StatusCode::SERVICE_UNAVAILABLE,
                "APT snapshot lookup failed",
            )
        })?;
    let signature = if let Some(current) = current.filter(|current| {
        current.digest == digest
            && current.release == release
            && current.packages_by_arch == packages_by_arch
            && current.pool_by_path == pool_by_path
            && current.members == members
    }) {
        current.signature
    } else {
        let signature_bytes = state
            .metadata_signer
            .sign_detached(&release_bytes)
            .map_err(|error| {
                Failure::plain(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("the Release could not be signed: {error}"),
                )
            })?;
        write_apt_reference(
            state,
            account,
            repository.config.digest_scope,
            &signature_bytes,
        )
        .await?
    };
    Ok(AptGroupSnapshot {
        revision: 0,
        digest,
        release,
        signature,
        packages_by_arch,
        pool_by_path,
        members,
    })
}

async fn serve_group_pool(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    asset_path: &str,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    let key = group_snapshot_key(context, repository, "__pool__");
    let snapshot = apt_snapshots(state)?
        .current_apt_snapshot(&key)
        .await
        .map_err(|_| {
            Failure::plain(
                StatusCode::SERVICE_UNAVAILABLE,
                "APT snapshot lookup failed",
            )
        })?
        .ok_or(RawApiError::NotFound)?;
    let leaves = super::raw_group::the_leaves(state, context, repository).await?;
    let Some(reference) = snapshot.pool_by_path.get(asset_path) else {
        return Err(RawApiError::NotFound.into());
    };

    let matching: Vec<_> = snapshot
        .members
        .iter()
        .filter(|member| member.content == *reference)
        .collect();
    let repositories: std::collections::BTreeSet<_> = matching
        .iter()
        .map(|member| member.repository.as_str())
        .collect();
    if repositories.len() != 1 {
        return Err(RawApiError::NotFound.into());
    }
    let source_key = *repositories.iter().next().ok_or(RawApiError::NotFound)?;
    let source = leaves
        .iter()
        .find(|leaf| leaf.repo_key == source_key)
        .ok_or(RawApiError::NotFound)?;
    match source.mode {
        RepositoryMode::Proxy => {
            let served = the_proxy_asset_verified(
                state,
                context,
                source,
                asset_path,
                Some(&reference.location.digest.value),
                false,
                correlation,
            )
            .await?;
            if served.asset.digest != reference.location.digest
                || served.asset.size_bytes != reference.size_bytes
            {
                return Err(Failure::plain(
                    StatusCode::BAD_GATEWAY,
                    "proxy pool content changed from the committed APT snapshot",
                ));
            }
            serve_proxy_response(
                state,
                context,
                source,
                &served,
                "application/vnd.debian.binary-package",
                head,
                correlation,
            )
            .await
        }
        RepositoryMode::Hosted => {
            let current = the_current_asset(state, context, &source.repo_key, asset_path)
                .await?
                .ok_or(RawApiError::NotFound)?;
            if current.digest != reference.location.digest
                || current.size_bytes != reference.size_bytes
            {
                return Err(RawApiError::NotFound.into());
            }
            crate::trust::admit(
                state,
                context.account().as_str(),
                source,
                &reference.location.digest,
            )
            .await
            .map_err(the_admission_error)?;
            let bytes = read_apt_reference(state, reference).await?;
            let response_body = if head { Vec::new() } else { bytes };
            let mut response = system_body_response(
                StatusCode::OK,
                response_body,
                "application/vnd.debian.binary-package",
                &conditional::the_etag(&reference.location.digest),
                correlation,
            );
            if head && let Ok(value) = HeaderValue::from_str(&reference.size_bytes.to_string()) {
                response.headers_mut().insert(header::CONTENT_LENGTH, value);
            }
            Ok(response)
        }
        RepositoryMode::Group => Err(RawApiError::NotFound.into()),
    }
}
