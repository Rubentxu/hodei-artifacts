//! La superficie de contenido **crudo**: publicar, descargar y describir.
//!
//! Un repositorio crudo guarda bytes en una ruta. No hay paquetes, ni
//! versiones, ni formatos: hay una dirección y un contenido.
//!
//! Las tres peticiones hacen lo mismo en los tres primeros pasos, y por
//! eso lo hacen en el mismo sitio:
//!
//! 1. **Autenticar.** El token va primero, siempre: una petición sin
//!    credencial es un 401 antes de mirar la ruta, el repositorio o
//!    cualquier otra cosa.
//! 2. **Leer la ruta.** Con las reglas de [`crate::raw::path`], que
//!    rechazan el traversal, la codificación doble, los caracteres de
//!    control y las rutas ambiguas.
//! 3. **Buscar el repositorio** en la cuenta **verificada**. Un
//!    repositorio de otra cuenta no existe para esta cuenta, y se
//!    responde con el mismo 404 que un repositorio que no existe.
//!
//! Y después, la decisión de Cedar con la acción que corresponde:
//! `UploadAsset` para publicar, `DownloadAsset` para descargar. Sin
//! permiso no hay contenido, y sin contenido no se dice que lo hay.

use axum::{
    body::Body,
    extract::{OriginalUri, State},
    http::{HeaderMap, HeaderName, HeaderValue, Request, StatusCode, header},
    response::{IntoResponse, Response},
};
use hodei_authz::VerifiedContext;
use hodei_catalog::{
    AssetDisposition, CatalogError, CatalogWriter, PackageCoordinates, Publication,
    RepositoryDescriptor, RepositoryFormat, RepositoryMode,
};
use hodei_evidence::domain::content::ContentDigest;

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{correlation_id, record_audit};
use crate::raw::{conditional, path, policy, range};

const CONTENT_TYPE: &str = "application/octet-stream";

/// La cabecera donde el cliente declara el digest del contenido.
///
/// No es obligatorio: si no viene, el digest se calcula sobre lo que
/// llega. Si viene, se **comprueba**: un cliente que declara un digest
/// y envía otra cosa está roto, y un servidor que lo acepta sin
/// comprobar está mintiendo sobre lo que tiene.
const CHECKSUM_HEADER: &str = "x-checksum-sha256";

pub use crate::resolution::ResolutionError as RawApiError;
#[allow(unused_imports)]
pub(super) use crate::resolution::{
    Resolved, apply_cache_headers, apply_common_headers, apply_group_headers, hex_of,
    the_admission_error, the_admitted, the_blob_error, the_catalog, the_content_store, the_context,
    the_current_asset, the_decision, the_proxy_asset, the_proxy_asset_at, the_publication_error,
    the_repository_in, the_resolved_asset, write_the_content,
};

/// La ruta pedida, con sus reglas.
fn the_target(uri: &OriginalUri) -> Result<path::RawTarget, RawApiError> {
    path::the_target(uri.path()).map_err(|error| RawApiError::BadRequest(error.message()))
}

/// El repositorio, en la cuenta verificada.
///
/// La cuenta viene del principal: un repositorio de otra cuenta no
/// existe para esta, y se responde con el mismo 404 que uno que no
/// existe nunca tuvo.
async fn the_repository(
    state: &AppState,
    context: &VerifiedContext,
    repository: &str,
) -> Result<RepositoryDescriptor, RawApiError> {
    the_repository_in(
        state,
        context,
        repository,
        &[RepositoryFormat::Raw, RepositoryFormat::Maven2],
    )
    .await
}

/// `PUT /repository/{repository}/{*path}`
///
/// Publica el contenido del cuerpo en esa dirección.
///
/// El contenido se escribe **por trozos** en el almacén de contenido: un
/// fichero de un gigabyte no cabe en memoria, y un servidor que lo
/// guarda en memoria no puede servir un artefacto de un gigabyte.
///
/// Y el asset y su evento de auditoría se escriben en la **misma**
/// transacción del catalogo: si el evento no se puede escribir, el asset
/// no se publica. Lo que si queda es el blob sin referenciar, que es
/// reconciliable.
pub async fn publish(
    State(state): State<AppState>,
    uri: OriginalUri,
    headers: HeaderMap,
    request: Request<Body>,
) -> Result<Response, RawApiError> {
    let context = the_context(&state, &headers).await?;
    let target = the_target(&uri)?;
    let repository = the_repository(&state, &context, &target.repository).await?;
    the_decision(
        &state,
        &context,
        crate::actions::upload_asset(),
        &repository,
    )?;
    // Lo que dice un proxy sale de su upstream, no de quien pregunta. Va
    // después de Cedar: quien no puede subir recibe el mismo 403 que en
    // cualquier otro repositorio, y no averigua el modo.
    if !repository.mode.accepts_client_writes() {
        return Err(RawApiError::MethodNotAllowed(
            "this repository does not accept uploads",
        ));
    }

    let correlation = correlation_id(&headers, &state.identity);
    // Un repositorio Maven 2 decide aqui que se guarda y que solo se valida.
    let maven_file = if repository.format == RepositoryFormat::Maven2 {
        match super::maven::the_write(&repository, &target.asset_path)? {
            super::maven::Write::Absorbed(absorbed) => {
                return super::maven::absorb(
                    &state,
                    &context,
                    &repository,
                    &target.asset_path,
                    absorbed,
                    request.into_body(),
                    &correlation,
                )
                .await;
            }
            super::maven::Write::File { file, replace } => Some((file, replace)),
        }
    } else {
        None
    };
    let current =
        the_current_asset(&state, &context, &target.repository, &target.asset_path).await?;
    let current_etag = current
        .as_ref()
        .map(|asset| conditional::the_etag(&asset.digest));
    let current_digest = current.as_ref().map(|asset| asset.digest.clone());

    // La condición se comprueba **antes** de escribir los bytes: una
    // petición que no se va a hacer no deja bytes en el almacén.
    let outcome = conditional::the_write_condition(
        conditional::the_if_match(&headers).as_deref(),
        conditional::the_if_none_match(&headers).as_deref(),
        current_etag.as_deref(),
    );
    if matches!(outcome, conditional::WriteOutcome::Failed) {
        return Err(RawApiError::PreconditionFailed);
    }

    // Si la dirección publica una vez, publicar otro contenido es un
    // conflicto. Y se dice **antes** de leer el cuerpo cuando el cliente
    // ha declarado el digest: un cliente que ya sabe que no puede
    // publicar no tiene por que enviar el contenido.
    // En Maven la sustitucion la decide la version del fichero, no `raw.replace`.
    let replace = match &maven_file {
        Some((_, replace)) => *replace,
        None => policy::the_replace_policy(&repository.config.config_settings),
    };
    let declared = declared_digest(&headers)?;
    if replace == policy::ReplacePolicy::Deny
        && let Some(current) = current_digest.as_ref()
        && let Some(declared) = declared.as_ref()
        && declared != current
    {
        return Err(RawApiError::Conflict(
            "the path already holds another content",
        ));
    }

    // Cuando la dirección publica una vez, el contenido que llega tiene
    // que ser **el mismo** que ya hay. Si no lo es, la escritura se
    // aborta y no queda ni un byte en el almacén.
    let refuse_replacement = match (replace, current_digest.as_ref()) {
        (policy::ReplacePolicy::Deny, Some(current)) => Some(current),
        _ => None,
    };
    let body = match &maven_file {
        Some((file, _)) => super::maven::the_file_body(file, request.into_body()).await?,
        None => request.into_body(),
    };
    let (digest, length) = write_the_content(
        &state,
        context.account().as_str(),
        body,
        declared,
        refuse_replacement,
    )
    .await?;

    let publication = Publication {
        repository: target.repository.clone(),
        asset_path: target.asset_path.clone(),
        // La versión de un contenido crudo es el **contenido**: una
        // versión publicada no cambia, y si el contenido cambia, es otra
        // versión. La ruta es el nombre del paquete, porque en un
        // repositorio crudo la ruta es lo que nombra el contenido.
        coordinates: PackageCoordinates {
            package_name: target.asset_path.clone(),
            package_version: digest.value.clone(),
        },
        digest: digest.clone(),
        size_bytes: length,
        tags: Vec::new(),
        actor: context.principal().subject().as_str().to_string(),
        correlation_id: correlation.clone(),
        // La sustitución la decide el **repositorio**, no el cliente: una
        // dirección publica una vez salvo que la configuración del
        // repositorio diga que se puede reponer. Lo que no se reescribe
        // es la versión anterior, que sigue estando.
        overwrite: replace == policy::ReplacePolicy::Allow,
        // La condición del cliente viaja al catálogo, que la comprueba
        // **dentro** de la transacción que escribe. La comprobación de
        // arriba es una cortesía: dice pronto lo que va a pasar, pero la
        // que vale es la del catálogo, porque allí no hay ventana entre
        // el comprobar y el escribir.
        precondition: conditional::the_precondition(
            conditional::the_if_match(&headers).as_deref(),
            conditional::the_if_none_match(&headers).as_deref(),
        ),
        // Un contenido crudo no cuelga de nada: el `subject` es una
        // relacion de los manifiestos de OCI, y aqui no hay manifiesto.
        subject: None,
    };

    let (_, writer) = the_catalog(&state)?;
    let outcome = writer
        .publish(context.account().as_str(), publication)
        .await
        .map_err(|error| the_publication_error(&error))?;

    let etag = conditional::the_etag(&outcome.reference.digest);
    // El estado lo dice el **catálogo**, no una lectura que se hizo antes
    // de escribir: lo que hay ahora es la verdad, y una lectura anterior
    // puede estar caduca. El cuerpo dice lo mismo, y no se contradice
    // consigo mismo.
    let (status, created, replaced) = match outcome.disposition {
        AssetDisposition::Created => (StatusCode::CREATED, true, false),
        AssetDisposition::Replaced => (StatusCode::OK, false, true),
        AssetDisposition::Unchanged => (StatusCode::OK, false, false),
    };
    let body = serde_json::json!({
        "repository": target.repository,
        "path": target.asset_path,
        "digest": outcome.reference.digest.to_string(),
        "size_bytes": outcome.reference.size_bytes,
        "etag": etag,
        "created": created,
        "replaced": replaced,
    });
    let mut response = (status, axum::Json(body)).into_response();
    apply_common_headers(&mut response, &etag, &correlation);
    // La `Location` es una **referencia URI**: la ruta que se acaba de
    // publicar, tal y como se puede escribir en una URI. Los segmentos
    // admitidos son los de RFC 3986, así que la referencia es válida sin
    // codificar nada.
    if let Ok(location) = HeaderValue::from_str(&format!(
        "/repository/{}/{}",
        target.repository, target.asset_path
    )) {
        response.headers_mut().insert(header::LOCATION, location);
    }
    Ok(response)
}

/// Lo que pide la lectura de procedencia.
#[derive(Debug, serde::Deserialize)]
pub struct ProvenanceQuery {
    /// La clave del repositorio.
    pub repository: String,
    /// La ruta del contenido.
    pub path: String,
}

/// `GET /api/v1/provenance?repository=&path=`
///
/// Los recibos de procedencia de una ruta de un proxy, el más reciente
/// primero. Lo autoriza la misma acción que leer el contenido
/// (`DownloadAsset`): quien puede bajarlo puede saber de dónde vino.
///
/// La respuesta no lleva credenciales ni consultas: el recibo ya las
/// excluye al escribirse.
pub async fn provenance(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<ProvenanceQuery>,
    headers: HeaderMap,
) -> Result<Response, RawApiError> {
    let context = the_context(&state, &headers).await?;
    // La ruta se valida con las mismas reglas que la del contenido.
    let target = path::the_target(&format!("/repository/{}/{}", query.repository, query.path))
        .map_err(|error| RawApiError::BadRequest(error.message()))?;
    let repository = the_repository(&state, &context, &target.repository).await?;
    the_decision(
        &state,
        &context,
        crate::actions::download_asset(),
        &repository,
    )?;
    let plane = state
        .proxy_plane
        .as_ref()
        .ok_or(RawApiError::NotImplemented(
            "this service keeps no provenance ledger",
        ))?;
    let receipts = plane
        .ledger
        .receipts_of(
            context.account().as_str(),
            &target.repository,
            &target.asset_path,
        )
        .await
        .map_err(|error| the_publication_error(&error))?;
    // Un recibo habla de un digest: si el canal no deja servir ese digest,
    // tampoco dice que existe. Es la misma compuerta que la lectura.
    let mut admitted = Vec::with_capacity(receipts.len());
    for receipt in receipts {
        match crate::trust::admit(
            &state,
            context.account().as_str(),
            &repository,
            &receipt.digest,
        )
        .await
        {
            Ok(_) => admitted.push(receipt),
            Err(crate::trust::Admission::Hidden) => {}
            Err(other) => return Err(the_admission_error(other)),
        }
    }
    let receipts: Vec<serde_json::Value> = admitted
        .iter()
        .map(|receipt| {
            serde_json::json!({
                "id": receipt.id,
                "repository": receipt.repository,
                "path": receipt.asset_path,
                "upstream": receipt.upstream_id,
                "source_url": receipt.source_url,
                "config_version": receipt.config_version,
                "digest": receipt.digest.to_string(),
                "size_bytes": receipt.size_bytes,
                "checksum": receipt.checksum,
                "status": receipt.status,
                "redirects": receipt.redirects,
                "etag": receipt.etag,
                "last_modified": receipt.last_modified,
                "outcome": receipt.outcome.as_str(),
                "actor": receipt.actor,
                "correlation_id": receipt.correlation_id,
                "fetched_at": receipt.fetched_at,
            })
        })
        .collect();
    Ok(axum::Json(serde_json::json!({ "receipts": receipts })).into_response())
}

/// `GET /repository/{repository}/{*path}`
///
/// Devuelve el contenido de esa dirección.
///
/// Admite `Range` y las condiciones `If-Match` e `If-None-Match`, según
/// RFC 9110. Y dice que admite rangos con `Accept-Ranges`, que es lo
/// que un cliente lee para saber si puede pedir un tramo.
pub async fn download(
    State(state): State<AppState>,
    uri: OriginalUri,
    headers: HeaderMap,
) -> Result<Response, RawApiError> {
    let (response, _) = the_content(state, uri, headers, false).await?;
    Ok(response)
}

/// `HEAD /repository/{repository}/{*path}`
///
/// Lo mismo que `GET`, sin el cuerpo. Las cabeceras son las mismas, y
/// `Content-Length` es el de la respuesta que habría dado el `GET`: el
/// del contenido entero, o el del tramo cuando la petición pide un
/// tramo. Un `HEAD` que dice una longitud y después un `GET` dice otra
/// es un servidor que no sabe lo que tiene.
pub async fn describe(
    State(state): State<AppState>,
    uri: OriginalUri,
    headers: HeaderMap,
) -> Result<Response, RawApiError> {
    let (response, _) = the_content(state, uri, headers, true).await?;
    Ok(response)
}

/// El contenido de una dirección, con sus condiciones y su rango.
///
/// El parámetro `head` dice si la respuesta lleva el cuerpo: `HEAD` trae
/// las mismas cabeceras y ningún byte.
async fn the_content(
    state: AppState,
    uri: OriginalUri,
    headers: HeaderMap,
    head: bool,
) -> Result<(Response, String), RawApiError> {
    let context = the_context(&state, &headers).await?;
    let correlation = correlation_id(&headers, &state.identity);
    // El rastro dice lo que pasó, **también** cuando lo que pasó es que
    // no se pudo servir: un registro que no dice quién intentó leer y
    // fue rechazado es un registro con un agujero.
    let resource = match the_target(&uri) {
        Ok(target) => format!("{}/{}", target.repository, target.asset_path),
        // Una ruta que no se puede leer no tiene nombre: se dice la que
        // llegó, que es lo que el cliente mandó.
        Err(_) => uri.path().to_string(),
    };
    let outcome = the_content_of(&state, &context, &uri, &headers, head, &correlation).await;
    let result = match &outcome {
        Ok(_) => AuditResult::Allowed,
        Err(RawApiError::Forbidden) => AuditResult::Denied,
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

/// El contenido, una vez que se sabe quién lo pide.
async fn the_content_of(
    state: &AppState,
    context: &VerifiedContext,
    uri: &OriginalUri,
    headers: &HeaderMap,
    head: bool,
    correlation: &str,
) -> Result<(Response, String), RawApiError> {
    let target = the_target(uri)?;
    let repository = the_repository(state, context, &target.repository).await?;
    the_decision(
        state,
        context,
        crate::actions::download_asset(),
        &repository,
    )?;

    let correlation = correlation.to_string();
    // En Maven, metadatos y checksums no se guardan en un hosted: se derivan
    // de lo publicado. Un grupo los compone con lo que aportan sus miembros
    // (la union de versiones, el checksum del fichero que gana). Un proxy
    // sirve los del upstream tal cual, como cualquier otro contenido. El
    // resto de rutas sigue el flujo normal.
    if repository.format == RepositoryFormat::Maven2 {
        let derived = match repository.mode {
            RepositoryMode::Hosted => {
                super::maven::the_derived(
                    state,
                    context,
                    &repository,
                    &target.asset_path,
                    headers,
                    head,
                    &correlation,
                )
                .await?
            }
            RepositoryMode::Group => {
                super::maven::the_group_derived(
                    state,
                    context,
                    &repository,
                    &target.asset_path,
                    headers,
                    head,
                    &correlation,
                )
                .await?
            }
            _ => None,
        };
        if let Some(response) = derived {
            return Ok((response, correlation));
        }
    }
    let Resolved {
        asset,
        served,
        resolved_by,
        repository,
    } = the_resolved_asset(state, context, repository, &target.asset_path, &correlation).await?;

    // La compuerta de confianza: Cedar ya dijo que esta persona puede
    // leer el repositorio; falta decir si este **digest** puede servirse
    // por su canal. Va antes de las condiciones de lectura: un 304 o un
    // 412 diria que el contenido existe.
    crate::trust::admit(
        state,
        context.account().as_str(),
        &repository,
        &asset.digest,
    )
    .await
    .map_err(the_admission_error)?;

    let etag = conditional::the_etag(&asset.digest);
    let outcome = conditional::the_read_condition(
        conditional::the_if_match(headers).as_deref(),
        conditional::the_if_none_match(headers).as_deref(),
        Some(&etag),
    );
    match outcome {
        conditional::ReadOutcome::Failed => return Err(RawApiError::PreconditionFailed),
        conditional::ReadOutcome::NotModified => {
            // El cliente ya tiene este contenido: 304, con la etiqueta
            // para que sepa cuál tiene.
            let mut response = StatusCode::NOT_MODIFIED.into_response();
            apply_common_headers(&mut response, &etag, &correlation);
            apply_cache_headers(&mut response, &served);
            apply_group_headers(&mut response, &resolved_by);
            return Ok((response, correlation));
        }
        conditional::ReadOutcome::Serve => {}
    }

    let store = the_content_store(state)?;
    let length = asset.size_bytes;
    let request = match range::the_range(
        headers
            .get(header::RANGE)
            .and_then(|value| value.to_str().ok()),
        length,
    ) {
        Ok(request) => request,
        Err(range::RangeError::Unsatisfiable) => {
            return Err(RawApiError::RangeNotSatisfiable(length));
        }
    };

    // El `HEAD` no trae el cuerpo, pero trae **las mismas cabeceras** que
    // traería el `GET`: la longitud que dice es la de la respuesta, que
    // con un tramo es la del tramo, y no la del contenido entero. Un
    // `HEAD` que dice `Content-Length: 100` y un `GET` que devuelve diez
    // bytes son dos servidores que no se conocen.
    let (body, status, content_range, response_length) = match request {
        range::RangeRequest::Whole => {
            if head {
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
        range::RangeRequest::Single { first, last } => {
            let served = last - first + 1;
            let content_range = range::the_content_range(first, last, length);
            if head {
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
    apply_common_headers(&mut response, &etag, &correlation);
    apply_cache_headers(&mut response, &served);
    apply_group_headers(&mut response, &resolved_by);
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(CONTENT_TYPE));
    response
        .headers_mut()
        .insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    // El contenido es binario y no se renderiza: se dice, para que el
    // navegador no lo invente.
    response.headers_mut().insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    if let Some(content_range) = content_range
        && let Ok(value) = HeaderValue::from_str(&content_range)
    {
        response.headers_mut().insert(header::CONTENT_RANGE, value);
    }
    if head {
        // Un HEAD dice cuanto trae la respuesta, y la respuesta no trae
        // nada: lo que dice es la longitud que tendría el `GET`.
        if let Ok(value) = HeaderValue::from_str(&response_length.to_string()) {
            response.headers_mut().insert(header::CONTENT_LENGTH, value);
        }
    }
    Ok((response, correlation))
}

/// El digest que declara el cliente, si declara alguno.
///
/// Una cabecera que no es un digest es una cabecera **mal formada**, no una
/// cabecera ausente: el cliente ha dicho algo, y lo que ha dicho no tiene
/// sentido. Aceptarla en silencio sería aceptar una promesa que no se
/// puede comprobar.
fn declared_digest(headers: &HeaderMap) -> Result<Option<ContentDigest>, RawApiError> {
    let Some(value) = headers.get(CHECKSUM_HEADER) else {
        return Ok(None);
    };
    let value = value
        .to_str()
        .map_err(|_| RawApiError::BadRequest("the declared digest is not valid"))?
        .trim();
    // El nombre del algoritmo no distingue mayúsculas: `SHA256:` dice lo
    // mismo que `sha256:`. Un prefijo que no es el de este algoritmo es
    // una promesa que no se puede comprobar.
    let value = match value.split_once(':') {
        Some((algorithm, value)) if algorithm.eq_ignore_ascii_case("sha256") => value,
        Some(_) => {
            return Err(RawApiError::BadRequest(
                "the declared digest is not a sha256",
            ));
        }
        None => value,
    };
    let digest = ContentDigest::sha256(value);
    if !digest.is_well_formed() {
        return Err(RawApiError::BadRequest(
            "the declared digest is not a valid sha256",
        ));
    }
    Ok(Some(digest))
}

// ---------------------------------------------------------------------------
// Fonteria comun de las superficies de sistema operativo (apt, yum)
// ---------------------------------------------------------------------------

/// La respuesta comun de un documento: estado, cuerpo, tipo, etag,
/// cabeceras comunes y `nosniff`. La comparten apt y yum porque el
/// contrato HTTP es el mismo: bytes deterministas con condicionales.
pub(super) fn system_body_response(
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
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    response
}

/// `If-None-Match`/`If-Match` contra el etag de un documento: un 304
/// dice que no cambio; `None` dice que quien llama debe servir el
/// cuerpo. Con HEAD no se envia cuerpo, solo el tamano.
pub(super) fn system_conditional_read(
    etag: &str,
    headers: &HeaderMap,
    correlation: &str,
    head: bool,
    body: &[u8],
) -> Result<Option<Response>, RawApiError> {
    match conditional::the_read_condition(
        conditional::the_if_match(headers).as_deref(),
        conditional::the_if_none_match(headers).as_deref(),
        Some(etag),
    ) {
        conditional::ReadOutcome::Failed => Err(RawApiError::PreconditionFailed),
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

/// Sirve un documento derivado (indice, cabecera, firma): su etag es el
/// digest de los bytes que se responden, y las condicionales valen
/// sobre ese mismo documento.
pub(super) fn serve_system_bytes(
    bytes: Vec<u8>,
    content_type: &str,
    headers: &HeaderMap,
    head: bool,
    correlation: &str,
) -> Result<Response, RawApiError> {
    let digest = ContentDigest::of_bytes(&bytes);
    let etag = conditional::the_etag(&digest);
    if let Some(response) = system_conditional_read(&etag, headers, correlation, head, &bytes)? {
        return Ok(response);
    }
    Ok(system_body_response(
        StatusCode::OK,
        bytes,
        content_type,
        &etag,
        correlation,
    ))
}

/// Escribe un asset inmutable en el catalogo. El conflicto con el mismo
/// digest es el eco de una publicacion interrumpida, no una
/// reescritura; con otros bytes es un 409 con el motivo dado.
#[allow(clippy::too_many_arguments)]
pub(super) async fn publish_system_asset(
    context: &VerifiedContext,
    writer: &dyn CatalogWriter,
    repository: &RepositoryDescriptor,
    asset_path: &str,
    coordinates: PackageCoordinates,
    digest: ContentDigest,
    size_bytes: u64,
    conflict_message: &str,
    correlation: &str,
) -> Result<(), RawApiError> {
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
        Err(CatalogError::Conflict { .. }) => Err(RawApiError::Conflict(Box::leak(
            conflict_message.to_string().into_boxed_str(),
        ))),
        Err(error) => Err(the_publication_error(&error)),
    }
}

/// El momento cero: sin publicaciones, la fecha de un indice es la del
/// epoch para que el documento sea determinista.
pub(super) fn epoch_moment() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(0, 0).expect("the epoch is a valid moment")
}
