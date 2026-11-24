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
//!
//! En modo **proxy** el repositorio no guarda lo que se publica en el:
//! trae el repodata de su upstream y lo sirve **tal cual**, con la firma
//! y la clave del upstream, y comprueba el checksum que cada metadata
//! declara antes de servirlo. Un proxy no reconstruye el repodata ajeno:
//! lo que se firma es lo que se sirve.

use axum::{
    body::Body,
    extract::{OriginalUri, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use hodei_catalog::{
    PackageCoordinates, RepositoryDescriptor, RepositoryFormat, RepositoryMode, UpstreamConfig,
    yum::{self, Route, UpstreamRoute, YumError},
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
    let repository =
        the_repository_in(&state, &context, &repository_key, &[RepositoryFormat::Rpm]).await?;
    // El modo decide la gramatica: un proxy no reconstruye el repodata
    // ajeno, y un repositorio alojado no sirve el layout de un upstream.
    if repository.mode == RepositoryMode::Proxy {
        return proxy(
            &state,
            &context,
            &repository,
            &rest,
            &method,
            &headers,
            &correlation,
        )
        .await;
    }
    let route = Route::parse(&rest)?;

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
// Proxy
// ---------------------------------------------------------------------------

/// La rama **proxy**: el repositorio no guarda lo que se publica en el,
/// lo trae de su upstream y lo sirve tal cual.
///
/// Un proxy no reconstruye el repodata ajeno: el `repomd.xml` que se
/// sirve es el del upstream, con la firma del upstream y con la clave del
/// upstream. Reconstruirlo produciria un documento que la firma que lo
/// cubre no firma.
async fn proxy(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    rest: &str,
    method: &Method,
    headers: &HeaderMap,
    correlation: &str,
) -> Result<Response, Failure> {
    let route = UpstreamRoute::parse(rest)?;
    let head = method == Method::HEAD;
    if !head && method != Method::GET {
        return Err(
            RawApiError::MethodNotAllowed("a proxy repository does not accept uploads").into(),
        );
    }
    match route {
        UpstreamRoute::Key => {
            the_decision(state, context, crate::actions::download_asset(), repository)?;
            let upstream = the_upstream(repository)?;
            let Some(armor) = upstream.gpg_key_armor else {
                // La clave del hub no sirve aqui: haria fallar la
                // verificacion de la firma del upstream con un error que
                // no dice nada. Sin la clave del upstream no hay nada que
                // servir, y se dice por que.
                return Err(Failure::plain(
                    StatusCode::NOT_FOUND,
                    "the upstream key is not configured",
                ));
            };
            Ok(serve_system_bytes(
                armor.into_bytes(),
                "application/pgp-keys",
                headers,
                head,
                correlation,
            )?)
        }
        UpstreamRoute::Repomd => {
            proxy_asset(
                state,
                context,
                repository,
                rest,
                "application/xml",
                head,
                correlation,
            )
            .await
        }
        UpstreamRoute::RepomdSignature => {
            proxy_asset(
                state,
                context,
                repository,
                rest,
                "application/pgp-signature",
                head,
                correlation,
            )
            .await
        }
        UpstreamRoute::Asset { path } => {
            proxy_asset(
                state,
                context,
                repository,
                &path,
                content_type_of(&path),
                head,
                correlation,
            )
            .await
        }
    }
}

/// El upstream que declara la configuracion del repositorio.
///
/// Una configuracion que no se entiende no degrada a "sin upstream": un
/// proxy sin upstream no tiene nada que servir, y se dice claro.
fn the_upstream(repository: &RepositoryDescriptor) -> Result<UpstreamConfig, Failure> {
    UpstreamConfig::of(&repository.config)
        .ok()
        .flatten()
        .ok_or_else(|| Failure::plain(StatusCode::BAD_GATEWAY, "the proxy is not configured"))
}

/// El tipo de contenido de una ruta del upstream.
///
/// El cliente real mira el tipo: un `.gz` que se sirve como texto plano no
/// es lo que `dnf` espera de un indice comprimido.
fn content_type_of(path: &str) -> &'static str {
    if path.ends_with(".rpm") {
        "application/x-rpm"
    } else if path.ends_with(".gz") {
        "application/x-gzip"
    } else if path.ends_with(".xz") {
        "application/x-xz"
    } else if path.ends_with(".xml") {
        "application/xml"
    } else if path.ends_with(".asc") {
        "application/pgp-signature"
    } else {
        "application/octet-stream"
    }
}

/// El contenido del upstream que se sirve tal cual, con su procedencia.
///
/// El checksum se comprueba **antes** de servir: un metadata que no
/// cumple el que su nombre (o el `repomd` que lo declara) anuncia no se
/// sirve, porque un cliente que lo recibiera veria un metadata corrupto
/// como si fuera bueno.
async fn proxy_asset(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    asset_path: &str,
    content_type: &str,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::download_asset(), repository)?;
    let upstream = the_upstream(repository)?;
    // Una ruta fuera de los scopes del upstream no existe para este
    // proxy: no se pregunta y no se sirve.
    let url = upstream.target(asset_path).ok_or(RawApiError::NotFound)?;
    let expected =
        the_expected_checksum(state, context, repository, asset_path, correlation).await?;
    let served = super::raw::the_proxy_asset_at(
        state,
        context,
        repository,
        asset_path,
        Some(url),
        Some(content_type),
        correlation,
    )
    .await?;
    // La compuerta de confianza: un proxy tambien es un repositorio, y su
    // canal decide si ese digest puede servirse. Sin esta pregunta, un
    // proxy en el canal trusted serviria contenido sin concesion.
    crate::trust::admit(
        state,
        context.account().as_str(),
        repository,
        &served.asset.digest,
    )
    .await
    .map_err(the_admission_error)?;
    let bytes = the_content_store(state)?
        .get(&served.asset.digest)
        .await
        .map_err(|error| the_blob_error(&error))?
        .to_vec();
    if let Some(expected) = expected
        && expected != hex_of(sha2::Sha256::digest(&bytes).as_slice())
    {
        return Err(Failure::plain(
            StatusCode::BAD_GATEWAY,
            "the upstream content does not match the checksum it declares",
        ));
    }
    let length = if head {
        served.asset.size_bytes as usize
    } else {
        bytes.len()
    };
    let body = if head { Vec::new() } else { bytes };
    let etag = conditional::the_etag(&served.asset.digest);
    let mut response = system_body_response(StatusCode::OK, body, content_type, &etag, correlation);
    apply_cache_headers(&mut response, &Some((served.status, served.provenance)));
    if head && let Ok(value) = HeaderValue::from_str(&length.to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    Ok(response)
}

/// El checksum que los bytes de una ruta tienen que cumplir.
///
/// `None` es "esta ruta no es metadata": no hay nada que comprobar. El
/// nombre de un metadata de `createrepo` es su propio SHA256, y ese
/// nombre es una promesa: un proxy que sirve unos bytes que no la
/// cumplen esta sirviendo metadata corrupto como si fuera bueno.
///
/// Los metadata cuyo nombre no se verifica solos solo se sirven si el
/// `repomd.xml` que el servicio tiene cacheado los declara. Metadata que
/// nadie ha declarado no se sirve, y ni se pregunta al upstream por ella.
async fn the_expected_checksum(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    asset_path: &str,
    correlation: &str,
) -> Result<Option<String>, Failure> {
    // El `repomd` y su firma se sirven tal cual: los verifica el cliente,
    // con la clave del upstream.
    if asset_path == "repodata/repomd.xml" || asset_path == "repodata/repomd.xml.asc" {
        return Ok(None);
    }
    if !asset_path.starts_with("repodata/") {
        return Ok(None);
    }
    if let Some(declared) = yum::checksum_of_name(asset_path) {
        return Ok(Some(declared));
    }
    let repomd = the_cached_repomd(state, context, repository, correlation).await?;
    match repomd
        .as_deref()
        .and_then(|repomd| yum::declared_checksum(repomd, asset_path))
    {
        Some(declared) => Ok(Some(declared)),
        None => Err(RawApiError::NotFound.into()),
    }
}

/// El `repomd.xml` que el servicio tiene, con lo que declara.
///
/// El documento es de un upstream: se lee del almacen de contenido, por
/// el digest que el catalogo tiene para esa ruta. Si el servicio no lo
/// tiene todavia se trae: el cliente lo pide primero, y sin el no hay
/// metadata que se pueda declarar.
async fn the_cached_repomd(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    correlation: &str,
) -> Result<Option<String>, Failure> {
    const REPOMD: &str = "repodata/repomd.xml";
    let cached = the_current_asset(state, context, &repository.repo_key, REPOMD).await?;
    let digest = match cached {
        Some(asset) => asset.digest,
        None => {
            let upstream = the_upstream(repository)?;
            let Some(url) = upstream.target(REPOMD) else {
                return Ok(None);
            };
            let served = super::raw::the_proxy_asset_at(
                state,
                context,
                repository,
                REPOMD,
                Some(url),
                Some("application/xml"),
                correlation,
            )
            .await?;
            served.asset.digest
        }
    };
    let bytes = the_content_store(state)?
        .get(&digest)
        .await
        .map_err(|error| the_blob_error(&error))?;
    Ok(String::from_utf8(bytes.to_vec()).ok())
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
    let (rpm_digest, rpm_size) = write_the_content(
        state,
        context.account().as_str(),
        Body::from(body.to_vec()),
        None,
        None,
    )
    .await?;
    let record = yum::package_record(&metadata, asset_path, rpm_size, &rpm_digest.value);
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
