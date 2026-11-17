//! La superficie **yum**: el repositorio RPM que un `dnf` real puede
//! leer y escribir contra un repositorio hosted de Hodei.
//!
//! Vive en `/yum/{repositorio}/...` y habla el formato de
//! `createrepo_c`: la gramatica de contenedores, indices y rutas esta
//! en `hodei_catalog::yum` (pura) y aqui solo hay lo que necesita
//! efectos: el almacen, el catalogo y la firma OpenPGP de `repomd.xml`.
//!
//! Cada `.rpm` publicado deja **dos assets inmutables**: los bytes en
//! `pool/{archivo}.rpm` y su ficha `<package>` en esa misma ruta con
//! sufijo `.record`. El indice `primary` se ensambla listando fichas,
//! nunca reescribiendo un documento compartido: una version publicada
//! es para siempre y dos publicadores concurrentes no pueden pisarse.
//! Si una caida deja el `.rpm` sin ficha, el paquete es invisible para
//! dnf, nunca corrupto.

use axum::{
    body::Body,
    extract::{OriginalUri, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use hodei_catalog::{
    PackageCoordinates, RepositoryDescriptor, RepositoryFormat,
    yum::{self, Route, YumError},
};
use serde_json::json;
use sha2::Digest as _;

use super::raw::{
    RawApiError, apply_cache_headers, epoch_moment, hex_of, publish_system_asset,
    serve_system_bytes, system_body_response, the_admission_error, the_blob_error, the_catalog,
    the_content_store, the_context, the_current_asset, the_decision, the_publication_error,
    the_repository_in, the_resolved_asset, write_the_content,
};
use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::apt::normalize_authorization;
use crate::handlers::{correlation_id, record_audit};
use crate::raw::conditional;

/// Lo que pasa con una peticion yum que no se puede servir.
#[derive(Debug)]
pub enum Failure {
    /// Un error de la superficie compartida.
    Raw(RawApiError),
    /// Un error del formato rpm.
    Yum(YumError),
    /// Un estado y un motivo.
    Plain(StatusCode, String),
}

impl From<RawApiError> for Failure {
    fn from(error: RawApiError) -> Self {
        Self::Raw(error)
    }
}

impl From<YumError> for Failure {
    fn from(error: YumError) -> Self {
        Self::Yum(error)
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
            Self::Yum(error) => (
                if matches!(error, YumError::BadIdentity(_)) {
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

/// El resto de la ruta de `/yum/{repositorio}/{resto}`, sin decodificar:
/// dnf no escapa nombres y la gramatica es la unica que interpreta.
fn the_split(uri: &OriginalUri) -> Result<(String, String), Failure> {
    let rest = uri
        .path()
        .strip_prefix("/yum/")
        .ok_or(RawApiError::NotFound)?;
    let (repository, rest) = rest.split_once('/').ok_or(RawApiError::NotFound)?;
    if !crate::raw::path::valid_repository_name(repository) || rest.is_empty() {
        return Err(RawApiError::NotFound.into());
    }
    Ok((repository.to_string(), rest.to_string()))
}

/// Cualquier peticion de `/yum/{repositorio}/...`.
pub async fn handle(
    State(state): State<AppState>,
    method: Method,
    uri: OriginalUri,
    mut headers: HeaderMap,
    request: axum::extract::Request<Body>,
) -> Result<Response, Failure> {
    // dnf admite la credencial en el userinfo de la URL, igual que apt:
    // aqui tambien se normaliza `Basic` a `Bearer`.
    normalize_authorization(&mut headers);
    let context = the_context(&state, &headers).await?;
    let correlation = correlation_id(&headers, &state.identity);
    let (repository_key, rest) = the_split(&uri)?;
    let route = Route::parse(&rest)?;
    let repository =
        the_repository_in(&state, &context, &repository_key, &[RepositoryFormat::Rpm]).await?;

    let reading = method == Method::GET || method == Method::HEAD;
    let publishing = method == Method::PUT;
    match (&route, reading, publishing) {
        (Route::Key, true, _) => {
            the_decision(
                &state,
                &context,
                crate::actions::download_asset(),
                &repository,
            )?;
            let document = state.metadata_signer.key_asc().map_err(|error| {
                Failure::plain(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("the yum key document could not be produced: {error}"),
                )
            })?;
            Ok(serve_system_bytes(
                document.into_bytes(),
                "application/pgp-keys",
                &headers,
                head_of(&method),
                &correlation,
            )?)
        }
        (Route::Repomd, true, _) => {
            let bytes = repomd_bytes(&state, &context, &repository).await?;
            serve(
                &state,
                &context,
                &repository,
                bytes,
                "application/xml",
                &headers,
                head_of(&method),
                &correlation,
            )
            .await
        }
        (Route::RepomdSignature, true, _) => {
            let repomd = repomd_bytes(&state, &context, &repository).await?;
            let signature = state
                .metadata_signer
                .sign_detached(&repomd)
                .map_err(|error| {
                    Failure::plain(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        &format!("the repomd could not be signed: {error}"),
                    )
                })?;
            serve(
                &state,
                &context,
                &repository,
                signature,
                "application/pgp-signature",
                &headers,
                head_of(&method),
                &correlation,
            )
            .await
        }
        (Route::Primary { checksum }, true, _) => {
            let (gz, _, _) = primary_bytes(&state, &context, &repository).await?;
            // El nombre es el digest de lo que se sirve: si no
            // coincide, ese fichero no existe en este repositorio.
            if checksum != &hex_of(sha2::Sha256::digest(&gz).as_slice()) {
                return Err(RawApiError::NotFound.into());
            }
            serve(
                &state,
                &context,
                &repository,
                gz,
                "application/x-gzip",
                &headers,
                head_of(&method),
                &correlation,
            )
            .await
        }
        (Route::Pool { path }, true, _) => {
            let outcome = read_rpm(
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
            "only .rpm files under pool/ can be uploaded to a yum repository",
        )
        .into()),
        _ => Err(RawApiError::MethodNotAllowed("this yum operation is not supported").into()),
    }
}

fn head_of(method: &Method) -> bool {
    method == Method::HEAD
}

/// El permiso de lectura primero y luego el documento ya producido.
#[allow(clippy::too_many_arguments)]
async fn serve(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    bytes: Vec<u8>,
    content_type: &str,
    headers: &HeaderMap,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::download_asset(), repository)?;
    Ok(serve_system_bytes(
        bytes,
        content_type,
        headers,
        head,
        correlation,
    )?)
}

// ---------------------------------------------------------------------------
// Lectura
// ---------------------------------------------------------------------------

/// Un registro de indice guardado junto a su binario: la ficha
/// `<package>` y el momento de su publicacion.
struct StoredRecord {
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
        out.push(StoredRecord {
            updated_at: asset.updated_at,
            body,
        });
    }
    Ok(out)
}

/// El `primary.xml` y su gzip, del estado actual. La suma del gz es el
/// nombre con el que se sirve: dos lecturas del mismo estado dan el
/// mismo documento, la misma suma y la misma ruta. La revision sale
/// del ultimo momento publicado, no del reloj.
async fn primary_bytes(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
) -> Result<(Vec<u8>, Vec<u8>, u64), Failure> {
    use std::io::Write as _;
    let all = records(state, context, repository).await?;
    let bodies: Vec<&str> = all.iter().map(|record| record.body.as_str()).collect();
    let document = yum::primary_document(&bodies).into_bytes();
    let moment = all
        .iter()
        .map(|record| record.updated_at)
        .max()
        .unwrap_or_else(epoch_moment)
        .timestamp() as u64;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(&document)
        .map_err(|_| Failure::plain(StatusCode::INTERNAL_SERVER_ERROR, "the gzip failed"))?;
    let gz = encoder
        .finish()
        .map_err(|_| Failure::plain(StatusCode::INTERNAL_SERVER_ERROR, "the gzip failed"))?;
    Ok((gz, document, moment))
}

/// El documento `repomd.xml` del repositorio: fija la suma exacta del
/// gz que se sirve, y su revision es la del estado.
async fn repomd_bytes(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
) -> Result<Vec<u8>, Failure> {
    let (gz, document, moment) = primary_bytes(state, context, repository).await?;
    let gz_sha = hex_of(&sha2::Sha256::digest(&gz));
    let xml_sha = hex_of(&sha2::Sha256::digest(&document));
    let entry = yum::RepodataEntry {
        kind: "primary",
        location: &yum::primary_location(&gz_sha),
        checksum: &gz_sha,
        size: gz.len() as u64,
        open_checksum: &xml_sha,
        open_size: document.len() as u64,
        timestamp: moment,
    };
    Ok(yum::repomd_document(moment, &[entry]).into_bytes())
}

/// Los bytes de un `.rpm`, verificados contra su asset y la compuerta
/// de confianza del repositorio.
async fn read_rpm(
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
        "application/x-rpm",
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

/// Subir un `.rpm`: el header dicta la ruta, y la ruta dicta los dos
/// assets. Primero se conmutan los bytes, luego se publica el binario y
/// por ultimo su ficha: interrumpida a la mitad, la ficha no existe y
/// el paquete queda invisible, nunca mentiroso.
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
    let metadata = yum::parse_rpm(&body)?;

    let expected = yum::pool_path_for(&metadata)?;
    if expected != asset_path {
        return Err(Failure::plain(
            StatusCode::CONFLICT,
            &format!("the pool path for this package must be {expected}"),
        ));
    }

    let (_, writer) = the_catalog(state)?;
    let record_path = yum::record_path_for(asset_path);

    // 1. Los bytes del .rpm, conmutados antes del catalogo.
    let (rpm_digest, rpm_size) =
        write_the_content(state, Body::from(body.to_vec()), None, None).await?;
    let record = yum::package_record(&metadata, asset_path, rpm_size, &rpm_digest.value);
    let record_bytes = record.into_bytes();
    // 2. Los bytes de la ficha.
    let (record_digest, record_size) =
        write_the_content(state, Body::from(record_bytes.clone()), None, None).await?;

    let existing_rpm = the_current_asset(state, context, &repository.repo_key, asset_path).await?;
    let existing_record =
        the_current_asset(state, context, &repository.repo_key, &record_path).await?;

    // La version es para siempre: identica es el eco o la reanudacion
    // de una publicacion interrumpida, distinta es un no con
    // explicacion.
    let rpm_already = existing_rpm
        .as_ref()
        .map(|asset| asset.digest == rpm_digest)
        .unwrap_or(false);
    let record_already = existing_record
        .as_ref()
        .map(|asset| asset.digest == record_digest)
        .unwrap_or(false);
    if existing_rpm.is_some() && !rpm_already {
        return Err(Failure::plain(
            StatusCode::CONFLICT,
            &format!(
                "{} is already published with other bytes; a published version is immutable",
                metadata.name
            ),
        ));
    }
    if existing_record.is_some() && !record_already {
        return Err(Failure::plain(
            StatusCode::CONFLICT,
            &format!(
                "the index record of {} is already published with other bytes; it is immutable",
                metadata.name
            ),
        ));
    }
    if rpm_already && record_already {
        return Ok(axum::Json(json!({ "ok": true })).into_response());
    }

    if !rpm_already {
        publish_system_asset(
            context,
            writer.as_ref(),
            repository,
            asset_path,
            PackageCoordinates {
                package_name: metadata.name.clone(),
                package_version: format!("{}-{}", metadata.version, metadata.release),
            },
            rpm_digest.clone(),
            rpm_size,
            &format!(
                "{} is already published; a published version is immutable",
                metadata.name
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
                package_name: format!("{}#record", metadata.name),
                package_version: format!("{}-{}", metadata.version, metadata.release),
            },
            record_digest,
            record_size,
            &format!(
                "the index record of {} is already published; it is immutable",
                metadata.name
            ),
            correlation,
        )
        .await?;
    }

    Ok(axum::Json(json!({ "ok": true })).into_response())
}
