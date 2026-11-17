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

use std::sync::Arc;

use axum::{
    body::Body,
    extract::{OriginalUri, State},
    http::{HeaderMap, HeaderName, HeaderValue, Request, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures_util::StreamExt;
use hodei_authz::{Credentials, VerifiedContext, verify_principal};
use hodei_catalog::{
    AssetDisposition, AssetRecord, CatalogError, CatalogReader, CatalogWriter, DigestScope,
    PackageCoordinates, Publication, RepositoryDescriptor, RepositoryFormat, RepositoryMode,
};
use hodei_evidence::domain::content::ContentDigest;
use hodei_hub::blob::{BlobError, BlobStore, CommitExpectation};
use sha2::{Digest, Sha256};
use url::Url;

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{bearer_token, correlation_id, record_audit};
use crate::raw::{conditional, path, policy, range};

/// El tipo de contenido que se sirve.
///
/// El catalogo no modela el tipo de contenido de un asset, así que se
/// sirve como lo que es: bytes. Inventar un tipo de contenido aquí
/// sería mentir sobre lo que el catalogo sabe.
/// Cuantos recibos recientes se leen para decidir una lectura: el libro de
/// una ruta crece con cada revalidacion y la decision solo mira los ultimos.
const RECEIPTS_READ_ON_THE_HOT_PATH: usize = 32;

const CONTENT_TYPE: &str = "application/octet-stream";

/// La cabecera donde el cliente declara el digest del contenido.
///
/// No es obligatorio: si no viene, el digest se calcula sobre lo que
/// llega. Si viene, se **comprueba**: un cliente que declara un digest
/// y envía otra cosa está roto, y un servidor que lo acepta sin
/// comprobar está mintiendo sobre lo que tiene.
const CHECKSUM_HEADER: &str = "x-checksum-sha256";

/// La cabecera de la correlación.
const CORRELATION_HEADER: &str = "x-request-id";

/// Los errores de la superficie cruda, y lo que se le dice al cliente.
#[derive(Debug)]
pub enum RawApiError {
    /// No hay credencial, o no es válida.
    Unauthorized,
    /// La decisión de Cedar no es permiso.
    Forbidden,
    /// No hay nada en esa dirección.
    NotFound,
    /// La petición no tiene sentido: la ruta, el repositorio o el
    /// contenido.
    BadRequest(&'static str),
    /// La condición pedida no se cumple.
    PreconditionFailed,
    /// El tramo pedido no existe.
    RangeNotSatisfiable(u64),
    /// El contenido es más grande de lo que se admite.
    PayloadTooLarge,
    /// Hay algo en esa dirección y no se puede sustituir.
    Conflict(&'static str),
    /// La superficie no está montada: no hay catalogo ni almacén.
    Unavailable,
    /// Esto todavía no lo sabe hacer, y se dice.
    NotImplemented(&'static str),
    /// El método no se admite en este repositorio (por ejemplo, subir a
    /// un proxy).
    MethodNotAllowed(&'static str),
    /// El upstream de un proxy no entregó un contenido utilizable.
    ///
    /// El motivo es genérico a propósito: el cliente no tiene por qué ver
    /// la red interna del servicio. El detalle queda en el rastro.
    BadGateway(&'static str),
    /// El upstream de un proxy no respondió a tiempo.
    GatewayTimeout,
    /// El proxy no sale a la red y lo pedido no está en su caché (o no se
    /// puede demostrar íntegro): un 503 explícito, no un 404.
    Offline(&'static str),
    /// Algo fallo por dentro.
    Internal,
}

impl RawApiError {
    /// El estado y el motivo de este error.
    ///
    /// Las superficies de protocolo (cargo, go, pypi) responden en JSON
    /// pero no pueden **rebajar** el estado: un `405` que llega como `400`
    /// es un cliente que no sabe qué volvió a intentar. El veredicto es
    /// uno solo para todas las superficies.
    pub(crate) fn the_verdict(&self) -> (StatusCode, String) {
        match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized".to_string()),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden".to_string()),
            Self::NotFound => (StatusCode::NOT_FOUND, "not found".to_string()),
            Self::BadRequest(reason) => (StatusCode::BAD_REQUEST, reason.to_string()),
            Self::PreconditionFailed => (
                StatusCode::PRECONDITION_FAILED,
                "precondition failed".to_string(),
            ),
            Self::RangeNotSatisfiable(_) => (
                StatusCode::RANGE_NOT_SATISFIABLE,
                "the range is not satisfiable".to_string(),
            ),
            Self::PayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "the content is larger than this service accepts".to_string(),
            ),
            Self::Conflict(reason) => (StatusCode::CONFLICT, reason.to_string()),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "the surface is not available".to_string(),
            ),
            Self::NotImplemented(reason) => (StatusCode::NOT_IMPLEMENTED, reason.to_string()),
            Self::MethodNotAllowed(reason) => (StatusCode::METHOD_NOT_ALLOWED, reason.to_string()),
            Self::BadGateway(reason) => (StatusCode::BAD_GATEWAY, reason.to_string()),
            Self::GatewayTimeout => (
                StatusCode::GATEWAY_TIMEOUT,
                "the upstream did not answer in time".to_string(),
            ),
            Self::Offline(reason) => (StatusCode::SERVICE_UNAVAILABLE, reason.to_string()),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal error".to_string(),
            ),
        }
    }
}

impl IntoResponse for RawApiError {
    fn into_response(self) -> Response {
        // La longitud del contenido se mira antes de gastar el error: la
        // respuesta a un rango que no se puede servir la lleva en una
        // cabecera, no en el cuerpo.
        let unsatisfiable = match &self {
            Self::RangeNotSatisfiable(length) => Some(*length),
            _ => None,
        };
        let (status, body) = self.the_verdict();
        let mut response = (status, body).into_response();
        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        if let Some(length) = unsatisfiable
            && let Ok(value) = HeaderValue::from_str(&range::the_unsatisfiable_range(length))
        {
            // RFC 9110: la respuesta a un rango que no se puede servir
            // dice en `Content-Range` cuál es la longitud de lo que hay.
            // Sin esa cabecera el cliente no sabe ni cuanto tiene.
            response.headers_mut().insert(header::CONTENT_RANGE, value);
        }
        response
    }
}

/// El catalogo de la instancia, para leer y para escribir.
type TheCatalog<'a> = (&'a Arc<dyn CatalogReader>, &'a Arc<dyn CatalogWriter>);

/// El catalogo de la instancia.
///
/// Es opcional: una instancia sin catalogo no sirve contenido crudo, y lo
/// dice con un 503 en vez de fingir que el contenido no existe.
pub(super) fn the_catalog(state: &AppState) -> Result<TheCatalog<'_>, RawApiError> {
    let reader = state
        .catalog_reader
        .as_ref()
        .ok_or(RawApiError::Unavailable)?;
    let writer = state
        .catalog_writer
        .as_ref()
        .ok_or(RawApiError::Unavailable)?;
    Ok((reader, writer))
}

/// El almacén de contenido.
pub(super) fn the_content_store(state: &AppState) -> Result<&Arc<dyn BlobStore>, RawApiError> {
    state.content_store.as_ref().ok_or(RawApiError::Unavailable)
}

/// El principal verificado de la petición.
///
/// La autenticación es la primera puerta: una petición sin credencial no
/// llega a mirar la ruta, y por eso no puede convertirse en un 400 que
/// diga algo de la ruta que el cliente no tiene permiso de conocer.
pub(super) async fn the_context(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<VerifiedContext, RawApiError> {
    let token = bearer_token(headers).ok_or(RawApiError::Unauthorized)?;
    let principal =
        match verify_principal(state.authenticator.as_ref(), &Credentials::bearer(token)).await {
            Ok(principal) => principal,
            Err(hodei_authz::AuthenticationError::ProviderUnavailable(_)) => {
                return Err(RawApiError::Unavailable);
            }
            Err(_) => return Err(RawApiError::Unauthorized),
        };
    Ok(VerifiedContext::from_principal(principal))
}

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

/// El repositorio, si es de uno de los formatos que sirve la superficie que
/// pregunta. Raw/Maven y npm comparten reglas (cuenta, modo, ambito) y solo
/// difieren en los formatos admitidos.
pub(super) async fn the_repository_in(
    state: &AppState,
    context: &VerifiedContext,
    repository: &str,
    served: &[RepositoryFormat],
) -> Result<RepositoryDescriptor, RawApiError> {
    let (reader, _) = the_catalog(state)?;
    let descriptor = match reader
        .repository(context.account().as_str(), repository)
        .await
    {
        Ok(descriptor) => descriptor,
        // Un repositorio que no está en esta cuenta no existe para esta
        // cuenta: se responde con el mismo 404 que uno que no existe
        // nunca tuvo, para que la respuesta no diga si existe en otra.
        //
        // Y un `Denied` también: el catálogo decide sobre la cuenta, y
        // un `403` diría que el repositorio existe en algún sitio. Si el
        // principal no puede usarlo, lo dice Cedar, con el recurso a la
        // vista.
        Err(CatalogError::Missing { .. }) | Err(CatalogError::Denied { .. }) => {
            return Err(RawApiError::NotFound);
        }
        Err(_) => return Err(RawApiError::Unavailable),
    };
    // Un repositorio que no es de este formato, o que no se sirve en esta
    // release, no es un repositorio de esta superficie: se responde con
    // el mismo 404 que uno que no existe.
    // Maven 2 comparte esta superficie en sus tres modos: hosted, proxy y
    // grupo. Los metadatos y checksums que cambian segun el modo se
    // resuelven en `maven`.
    let served_format = served.contains(&descriptor.format);
    if !served_format || !descriptor.mode.is_served_by_this_release() {
        return Err(RawApiError::NotFound);
    }
    // El ambito del contenido decide dónde viven sus bytes, y esta
    // release solo sabe leer el ambito global. Un repositorio que pide el
    // particionado se dice que no se sirve, y se dice claro: un 404
    // aquí sería mentir.
    if descriptor.config.digest_scope != DigestScope::Global {
        return Err(RawApiError::NotImplemented(
            "this service serves the content of a repository only in the global scope",
        ));
    }
    Ok(descriptor)
}

/// La decisión de Cedar, con la acción que corresponde.
///
/// El recurso es el **repositorio**: la decisión es sobre el
/// repositorio, no sobre la ruta. Una ruta concreta no es un recurso
/// con identidad propia en el catalogo, y una acción sobre un recurso
/// que no existe no se puede evaluar.
pub(super) fn the_decision(
    state: &AppState,
    context: &VerifiedContext,
    action: hodei_authz::model::ActionId,
    repository: &RepositoryDescriptor,
) -> Result<(), RawApiError> {
    let outcome = state
        .authorize_resource
        .execute_canonical(context.clone(), action, &repository.hrn)
        .map_err(|_| RawApiError::Internal)?;
    if !outcome.allows_access() {
        return Err(RawApiError::Forbidden);
    }
    Ok(())
}

/// Lo que se responde cuando la compuerta de confianza no admite.
///
/// Lo no admitido es un 404, igual que lo que no existe: un 403 diria
/// que el contenido esta ahi. Y un libro que no responde es un 503: lo
/// desconocido no se sirve, ni se confunde con la ausencia.
pub(super) fn the_admission_error(admission: crate::trust::Admission) -> RawApiError {
    match admission {
        crate::trust::Admission::Hidden => RawApiError::NotFound,
        crate::trust::Admission::Unavailable => RawApiError::Unavailable,
    }
}

/// El contenido que hay ahora en esa dirección.
pub(super) async fn the_current_asset(
    state: &AppState,
    context: &VerifiedContext,
    repository: &str,
    asset_path: &str,
) -> Result<Option<AssetRecord>, RawApiError> {
    let (reader, _) = the_catalog(state)?;
    match reader
        .asset(context.account().as_str(), repository, asset_path)
        .await
    {
        Ok(asset) => Ok(Some(asset)),
        // Un asset que no esta no es un error: es lo que pasa antes de
        // la primera publicación.
        Err(CatalogError::Missing { .. }) => Ok(None),
        Err(_) => Err(RawApiError::Unavailable),
    }
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
    let (digest, length) = write_the_content(&state, body, declared, refuse_replacement).await?;

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

/// Cómo se sirvió un contenido de un proxy. Va en `X-Hodei-Cache`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CacheStatus {
    /// Estaba en la caché y vigente: no se salió.
    Hit,
    /// No estaba: se trajo del upstream.
    Miss,
    /// Estaba caducado y el upstream confirmó los mismos bytes.
    Revalidated,
    /// Estaba caducado y el upstream entregó bytes nuevos.
    Refreshed,
    /// El upstream falló y se sirvió lo que había, dentro de la ventana.
    Stale,
    /// El servicio no sale a la red: se sirvió lo que había con recibo.
    Offline,
}

impl CacheStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Miss => "miss",
            Self::Revalidated => "revalidated",
            Self::Refreshed => "refreshed",
            Self::Stale => "stale",
            Self::Offline => "offline",
        }
    }
}

/// Un contenido de un proxy, listo para pasar por la compuerta de confianza.
pub(super) struct ProxyServed {
    pub(super) asset: AssetRecord,
    pub(super) status: CacheStatus,
    /// El upstream que respalda el contenido y cuándo lo confirmó, cuando
    /// la respuesta no sale de una comprobación reciente (obsoleto, offline).
    pub(super) provenance: Option<(String, String)>,
}

impl ProxyServed {
    fn new(asset: AssetRecord, status: CacheStatus) -> Self {
        Self {
            asset,
            status,
            provenance: None,
        }
    }
}

/// El contenido de una ruta de un proxy: de la caché, revalidado o traído.
///
/// La decisión (¿está vigente? ¿hay que preguntar? ¿se puede salir?) es
/// pura y vive en `hodei_catalog::proxy_cache`; aquí se junta con lo que
/// necesita efectos: el catálogo, el libro de recibos, el almacén, el
/// cliente de salida y el estado de proceso (una sola descarga por ruta, las
/// ausencias recordadas y los contadores).
///
/// Lo que se publique pasa después por la misma compuerta de confianza que
/// cualquier otro contenido, incluido lo servido obsoleto o sin red: una
/// revocación se respeta siempre.
pub(super) async fn the_proxy_asset(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    asset_path: &str,
    correlation: &str,
) -> Result<ProxyServed, RawApiError> {
    the_proxy_asset_at(
        state,
        context,
        repository,
        asset_path,
        None,
        None,
        correlation,
    )
    .await
}

/// Como [`the_proxy_asset`], pero con la URL exacta de donde se trae el
/// contenido y el `Accept` que se pide.
///
/// La Simple API de PyPI no tiene la forma de las demas superficies: la
/// pagina de un proyecto y cada distribucion se piden por la URL que
/// declara el upstream, que no es la ruta del asset. La cache, los recibos
/// y la publicacion son los mismos.
pub(super) async fn the_proxy_asset_at(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    asset_path: &str,
    url: Option<Url>,
    accept: Option<&str>,
    correlation: &str,
) -> Result<ProxyServed, RawApiError> {
    use hodei_catalog::proxy_cache::{CacheDecision, CacheEntry, Context, decide, stale_allowed};

    use crate::proxy::cache::PathKey;

    let plane = state
        .proxy_plane
        .as_ref()
        .ok_or(RawApiError::NotImplemented(
            "this service has no egress for remote proxies",
        ))?;
    // Un proxy Maven aplica la politica de versiones del repositorio: una
    // version que no admite no se pide al upstream.
    if repository.format == RepositoryFormat::Maven2
        && !super::maven::admits_path(repository, asset_path)?
    {
        return Err(RawApiError::NotFound);
    }
    let store = the_content_store(state)?;
    let account = context.account().as_str();
    let current = the_current_asset(state, context, &repository.repo_key, asset_path).await?;

    // La configuración se valida al crear el repositorio. Si aun así no se
    // entiende, el proxy no sale: lo que ya tiene se sirve como antes y lo
    // que no, falla cerrado.
    let upstream = match hodei_catalog::UpstreamConfig::of(&repository.config) {
        Ok(Some(upstream)) => upstream,
        _ => {
            return match current {
                Some(asset) => Ok(ProxyServed::new(asset, CacheStatus::Hit)),
                None => Err(RawApiError::BadGateway("the proxy is not configured")),
            };
        }
    };

    let receipts = match &current {
        Some(_) => plane
            .ledger
            .latest_receipts(
                account,
                &repository.repo_key,
                asset_path,
                RECEIPTS_READ_ON_THE_HOT_PATH,
            )
            .await
            .map_err(|error| the_publication_error(&error))?,
        None => Vec::new(),
    };
    let entry = current
        .as_ref()
        .map(|asset| CacheEntry::of(&asset.digest, asset.size_bytes, &receipts));
    let config_version = repository.config.version;
    let decision_context = Context {
        format: repository.format,
        policy: &upstream.cache,
        asset_path,
        upstream_id: &upstream.id,
        in_scope: upstream.in_scope(asset_path),
        config_version,
        offline: upstream.cache.offline || plane.is_offline(),
        now: chrono::Utc::now(),
    };
    let decision = decide(&decision_context, entry.as_ref());
    tracing::debug!(
        target: "hodei_artifacts::proxy",
        repository = %repository.repo_key,
        path = asset_path,
        decision = ?std::mem::discriminant(&decision),
        "the proxy decided what to do with the path"
    );
    let confirmed = |entry: Option<&CacheEntry>| {
        entry
            .and_then(|entry| entry.confirmation.as_ref())
            .map(|confirmation| {
                (
                    confirmation.upstream_id.clone(),
                    confirmation.at.to_rfc3339(),
                )
            })
    };

    let known = match decision {
        CacheDecision::OutOfScope => return Err(RawApiError::NotFound),
        CacheDecision::Fresh => {
            plane.metrics.record_hit();
            let asset = current.ok_or(RawApiError::Internal)?;
            return Ok(ProxyServed::new(asset, CacheStatus::Hit));
        }
        CacheDecision::OfflineServe => {
            let asset = current.ok_or(RawApiError::Internal)?;
            // Sin red no hay quien corrija un blob dañado: se comprueba
            // antes de servirlo.
            if let Err(error) = store.verify(&asset.digest).await {
                tracing::error!(
                    target: "hodei_artifacts::proxy",
                    repository = %repository.repo_key,
                    error = %error,
                    "the cached content failed its integrity check"
                );
                plane.metrics.record_offline_miss();
                return Err(RawApiError::Offline(
                    "the cached content failed its integrity check",
                ));
            }
            plane.metrics.record_offline_hit();
            return Ok(ProxyServed {
                provenance: confirmed(entry.as_ref()),
                ..ProxyServed::new(asset, CacheStatus::Offline)
            });
        }
        CacheDecision::OfflineMiss => {
            plane.metrics.record_offline_miss();
            return Err(RawApiError::Offline(
                "the proxy is offline and does not hold a verified copy of that content",
            ));
        }
        CacheDecision::Quarantined => {
            plane.metrics.record_quarantined();
            return Err(RawApiError::BadGateway(
                "the upstream content is withheld pending review",
            ));
        }
        CacheDecision::Fetch => None,
        CacheDecision::Revalidate(known) => Some(known),
    };

    let key = PathKey::of(account, &repository.repo_key, asset_path);
    if current.is_none()
        && plane
            .negatives
            .contains(&key, config_version, std::time::Instant::now())
    {
        plane.metrics.record_negative_hit();
        return Err(RawApiError::NotFound);
    }

    let (_, writer) = the_catalog(state)?;
    let actor = context.principal().subject().as_str();
    let overwrite = current.is_some();
    let flight = plane
        .flights
        .run((key.clone(), config_version), || {
            fetch_and_publish(
                plane,
                store.as_ref(),
                writer.as_ref(),
                &upstream,
                repository,
                asset_path,
                account,
                actor,
                correlation,
                known.as_ref(),
                overwrite,
                url,
                accept,
            )
        })
        .await;
    if flight.is_followed() {
        plane.metrics.record_coalesced();
    }

    use crate::proxy::resolution::Outcome;
    let status = match flight.into_inner() {
        Ok(Outcome::Stored(_)) if known.is_none() => {
            plane.metrics.record_miss();
            plane.negatives.forget(&key);
            CacheStatus::Miss
        }
        Ok(Outcome::Stored(_)) => {
            plane.metrics.record_refreshed();
            CacheStatus::Refreshed
        }
        Ok(Outcome::Unchanged(_)) => {
            plane.metrics.record_revalidated();
            CacheStatus::Revalidated
        }
        Ok(Outcome::Quarantined(receipt)) => {
            plane.metrics.record_quarantined();
            tracing::warn!(
                target: "hodei_artifacts::proxy",
                repository = %repository.repo_key,
                path = asset_path,
                receipt = %receipt.id,
                "the upstream offered other bytes for immutable content: withheld"
            );
            return Err(RawApiError::BadGateway(
                "the upstream content is withheld pending review",
            ));
        }
        Err(failure) => {
            if failure == crate::proxy::cache::ProxyFailure::NotFound && current.is_none() {
                plane.negatives.remember(
                    &key,
                    config_version,
                    std::time::Duration::from_secs(upstream.cache.negative_secs),
                    std::time::Instant::now(),
                );
            }
            // El contenido caducado se sirve solo ante un fallo del
            // upstream y dentro de la ventana que la política declara.
            if failure.is_upstream_error()
                && let Some(asset) = current
                && entry
                    .as_ref()
                    .is_some_and(|entry| stale_allowed(&decision_context, entry))
            {
                plane.metrics.record_stale();
                tracing::warn!(
                    target: "hodei_artifacts::proxy",
                    repository = %repository.repo_key,
                    path = asset_path,
                    error = %failure,
                    "the upstream failed: serving the stale content"
                );
                return Ok(ProxyServed {
                    provenance: confirmed(entry.as_ref()),
                    ..ProxyServed::new(asset, CacheStatus::Stale)
                });
            }
            return Err(the_proxy_error(&failure));
        }
    };

    let asset = the_current_asset(state, context, &repository.repo_key, asset_path)
        .await?
        .ok_or(RawApiError::Unavailable)?;
    Ok(ProxyServed::new(asset, status))
}

/// El trabajo de la descarga compartida: sale, verifica, guarda y publica.
///
/// Lo ejecuta una sola petición por ruta (la líder); las demás reciben su
/// resultado. El recibo queda a nombre de quien lideró.
#[allow(clippy::too_many_arguments)]
async fn fetch_and_publish(
    plane: &crate::proxy::ProxyPlane,
    store: &dyn BlobStore,
    writer: &dyn CatalogWriter,
    upstream: &hodei_catalog::UpstreamConfig,
    repository: &RepositoryDescriptor,
    asset_path: &str,
    account: &str,
    actor: &str,
    correlation: &str,
    known: Option<&hodei_catalog::proxy_cache::Known>,
    overwrite: bool,
    url: Option<Url>,
    accept: Option<&str>,
) -> crate::proxy::FlightResult {
    use crate::proxy::resolution::{self, Outcome};

    plane.metrics.record_upstream_request();
    let request = resolution::Resolution {
        account,
        repository: &repository.repo_key,
        asset_path,
        upstream,
        config_version: repository.config.version,
        actor,
        correlation_id: correlation,
        accept,
    };
    let outcome = match url {
        Some(url) => {
            resolution::resolve_url(
                &plane.fetcher,
                store,
                plane.ledger.as_ref(),
                &request,
                known,
                url,
            )
            .await
        }
        None => {
            resolution::resolve(
                &plane.fetcher,
                store,
                plane.ledger.as_ref(),
                &request,
                known,
            )
            .await
        }
    }
    .map_err(|error| {
        // El rastro lleva el motivo; la respuesta, uno genérico.
        tracing::warn!(
            target: "hodei_artifacts::proxy",
            repository = %repository.repo_key,
            error = %error,
            "the proxy could not resolve the content"
        );
        let failure = the_proxy_failure(error);
        if failure.is_upstream_error() {
            plane.metrics.record_upstream_error();
        }
        failure
    })?;

    if let Outcome::Stored(resolved) = &outcome {
        let publication = Publication {
            repository: repository.repo_key.clone(),
            asset_path: asset_path.to_string(),
            coordinates: PackageCoordinates {
                package_name: asset_path.to_string(),
                package_version: resolved.digest.value.clone(),
            },
            digest: resolved.digest.clone(),
            size_bytes: resolved.size_bytes,
            tags: Vec::new(),
            actor: actor.to_string(),
            correlation_id: correlation.to_string(),
            // Una actualización de metadata mueve el puntero de la ruta al
            // digest nuevo; la primera resolución no pisa nada.
            overwrite,
            precondition: None,
            subject: None,
        };
        match writer.publish(account, publication).await {
            Ok(_) => {}
            // Otra instancia resolvió la misma ruta antes: lo que hay es lo
            // que se sirve, y se relee.
            Err(CatalogError::Conflict { .. }) if !overwrite => {}
            Err(error) => {
                return Err(crate::proxy::cache::ProxyFailure::Local(the_local_failure(
                    &the_publication_error(&error),
                )));
            }
        }
    }
    Ok(outcome)
}

/// Un fallo de resolución, en la forma que comparten las peticiones.
fn the_proxy_failure(
    error: crate::proxy::resolution::ResolveError,
) -> crate::proxy::cache::ProxyFailure {
    use crate::proxy::cache::ProxyFailure;
    use crate::proxy::fetch::FetchError;
    use crate::proxy::resolution::ResolveError;

    match error {
        ResolveError::Fetch(FetchError::NotFound) => ProxyFailure::NotFound,
        ResolveError::Fetch(FetchError::Busy) => ProxyFailure::Busy,
        ResolveError::Fetch(FetchError::Timeout) => ProxyFailure::Timeout,
        ResolveError::Fetch(FetchError::OutOfScope) => ProxyFailure::OutOfScope,
        ResolveError::Fetch(FetchError::Offline) => ProxyFailure::Offline,
        ResolveError::Fetch(FetchError::Egress(_)) => ProxyFailure::NotReachable,
        ResolveError::Fetch(FetchError::ChecksumMismatch | FetchError::ChecksumMissing) => {
            ProxyFailure::Unverified
        }
        ResolveError::Fetch(FetchError::Truncated | FetchError::TooLarge) => {
            ProxyFailure::Incomplete
        }
        ResolveError::Fetch(_) => ProxyFailure::UpstreamFailed,
        ResolveError::Blob(error) => {
            ProxyFailure::Local(the_local_failure(&the_blob_error(&error)))
        }
        ResolveError::Ledger(error) => {
            ProxyFailure::Local(the_local_failure(&the_publication_error(&error)))
        }
    }
}

/// Un error HTTP propio, sin el detalle, para compartirlo entre peticiones.
fn the_local_failure(error: &RawApiError) -> crate::proxy::cache::LocalFailure {
    use crate::proxy::cache::LocalFailure;
    match error {
        RawApiError::Unavailable => LocalFailure::Unavailable,
        RawApiError::Conflict(_)
        | RawApiError::PreconditionFailed
        | RawApiError::Forbidden
        | RawApiError::BadRequest(_) => LocalFailure::Rejected,
        _ => LocalFailure::Internal,
    }
}

/// Un fallo de resolución, traducido a HTTP.
fn the_proxy_error(failure: &crate::proxy::cache::ProxyFailure) -> RawApiError {
    use crate::proxy::cache::{LocalFailure, ProxyFailure};
    match failure {
        ProxyFailure::NotFound | ProxyFailure::OutOfScope => RawApiError::NotFound,
        ProxyFailure::Busy => RawApiError::Unavailable,
        ProxyFailure::Timeout => RawApiError::GatewayTimeout,
        ProxyFailure::NotReachable => {
            RawApiError::BadGateway("the upstream is not reachable from this service")
        }
        ProxyFailure::Unverified => {
            RawApiError::BadGateway("the upstream content could not be verified")
        }
        ProxyFailure::Incomplete => {
            RawApiError::BadGateway("the upstream content is incomplete or too large")
        }
        ProxyFailure::UpstreamFailed => RawApiError::BadGateway("the upstream did not deliver"),
        ProxyFailure::Offline => RawApiError::Offline(
            "the proxy is offline and does not hold a verified copy of that content",
        ),
        ProxyFailure::Local(LocalFailure::Unavailable) => RawApiError::Unavailable,
        ProxyFailure::Local(LocalFailure::Internal) => RawApiError::Internal,
        ProxyFailure::Local(LocalFailure::Rejected) => {
            RawApiError::Conflict("the publication of the upstream content was rejected")
        }
    }
}

/// Pone en la respuesta cómo se sirvió el contenido de un proxy.
pub(super) fn apply_cache_headers(
    response: &mut Response,
    served: &Option<(CacheStatus, Option<(String, String)>)>,
) {
    let Some((status, provenance)) = served else {
        return;
    };
    response.headers_mut().insert(
        header::HeaderName::from_static("x-hodei-cache"),
        HeaderValue::from_static(status.as_str()),
    );
    if let Some((upstream, confirmed_at)) = provenance {
        if let Ok(value) = HeaderValue::from_str(upstream) {
            response
                .headers_mut()
                .insert(header::HeaderName::from_static("x-hodei-upstream"), value);
        }
        if let Ok(value) = HeaderValue::from_str(confirmed_at) {
            response.headers_mut().insert(
                header::HeaderName::from_static("x-hodei-confirmed-at"),
                value,
            );
        }
    }
}

/// Que miembro de un grupo sirvio el contenido, y en que version de la
/// configuracion del grupo. Va en `X-Hodei-Resolved-From` y
/// `X-Hodei-Group-Version`: quien descarga sabe de que repositorio salio
/// lo que tiene, y una resolucion distinta tras reconfigurar el grupo es
/// visible.
pub(super) fn apply_group_headers(response: &mut Response, resolved_by: &Option<(String, u64)>) {
    let Some((member, version)) = resolved_by else {
        return;
    };
    if let Ok(value) = HeaderValue::from_str(member) {
        response
            .headers_mut()
            .insert(HeaderName::from_static("x-hodei-resolved-from"), value);
    }
    response.headers_mut().insert(
        HeaderName::from_static("x-hodei-group-version"),
        HeaderValue::from(*version),
    );
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

/// Un asset ya resuelto segun el modo del repositorio pedido.
pub(super) struct Resolved {
    pub(super) asset: AssetRecord,
    /// Como se sirvio, si salio de un proxy.
    pub(super) served: Option<(CacheStatus, Option<(String, String)>)>,
    /// Que miembro de un grupo lo sirvio y en que version de su configuracion.
    pub(super) resolved_by: Option<(String, u64)>,
    /// El repositorio que respalda el contenido: el pedido o el miembro que
    /// gano. La confianza se decide sobre este.
    pub(super) repository: RepositoryDescriptor,
}

/// Resuelve una ruta segun el modo del repositorio.
///
/// Un hosted sirve lo que tiene. Un proxy decide cada vez si lo que tiene
/// vale, hay que revalidarlo o hay que traerlo. Un grupo recorre sus
/// miembros en orden y sirve la ruta del primero que la tiene; lo que pasa
/// despues (confianza, condiciones, rango) es del miembro que gano, no del
/// grupo.
pub(super) async fn the_resolved_asset(
    state: &AppState,
    context: &VerifiedContext,
    repository: RepositoryDescriptor,
    asset_path: &str,
    correlation: &str,
) -> Result<Resolved, RawApiError> {
    if repository.mode == RepositoryMode::Group {
        let version = repository.config.version;
        let resolution =
            super::raw_group::the_group_asset(state, context, &repository, asset_path, correlation)
                .await;
        // El rastro de la resolucion: que grupo, en que version de su
        // configuracion, que miembros se consultaron y quien gano.
        super::raw_group::record_resolution(
            state,
            context,
            &repository,
            asset_path,
            &resolution.trail_line(),
            resolution.outcome.is_ok(),
            correlation,
        );
        let won = resolution.outcome?;
        Ok(Resolved {
            asset: won.asset,
            served: won.served,
            resolved_by: Some((won.member.repo_key.clone(), version)),
            repository: won.member,
        })
    } else if repository.mode == RepositoryMode::Proxy {
        let served = the_proxy_asset(state, context, &repository, asset_path, correlation).await?;
        Ok(Resolved {
            asset: served.asset,
            served: Some((served.status, served.provenance)),
            resolved_by: None,
            repository,
        })
    } else {
        let asset = the_current_asset(state, context, &repository.repo_key, asset_path)
            .await?
            .ok_or(RawApiError::NotFound)?;
        Ok(Resolved {
            asset,
            served: None,
            resolved_by: None,
            repository,
        })
    }
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

/// Escribe el contenido del cuerpo en el almacén de contenido.
///
/// Los bytes van **por trozos**, y el digest se calcula mientras pasan:
/// el cliente puede declarar cuál va a ser, y si no declara ninguno, el
/// servidor lo calcula sobre lo que llega. En los dos casos, el commit
/// comprueba que lo publicado es exactamente lo declarado.
///
/// `refuse_replacement` es el digest que hay **ahora** en la dirección
/// cuando esa dirección publica una vez. Si lo que llega no es eso, la
/// escritura se aborta: no queda ni un byte en el almacén, y el cliente
/// recibe un conflicto en vez de un contenido que no se publico.
pub(super) async fn write_the_content(
    state: &AppState,
    body: Body,
    declared: Option<ContentDigest>,
    refuse_replacement: Option<&ContentDigest>,
) -> Result<(ContentDigest, u64), RawApiError> {
    let store = the_content_store(state)?;
    let mut session = store
        .begin()
        .await
        .map_err(|error| the_blob_error(&error))?;
    let mut hasher = Sha256::new();
    let mut length: u64 = 0;
    let limit = state.max_body_size as u64;

    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| RawApiError::BadRequest("the body could not be read"))?;
        length = length.saturating_add(chunk.len() as u64);
        if length > limit {
            // El contenido es más grande de lo que se admite: se
            // aborta la escritura y se dice, en vez de acceptarlo y
            // fallar más tarde por disco.
            let _ = session.abort().await;
            return Err(RawApiError::PayloadTooLarge);
        }
        hasher.update(&chunk);
        session
            .append(&chunk)
            .await
            .map_err(|error| the_blob_error(&error))?;
    }

    let found = ContentDigest::sha256(hex_of(&hasher.finalize()));
    if let Some(declared) = declared
        && declared != found
    {
        // El cliente dijo que mandaba otra cosa. Se aborta la escritura:
        // un blob con un digest que no es el suyo no es un blob.
        let _ = session.abort().await;
        return Err(RawApiError::BadRequest(
            "the content does not match the declared digest",
        ));
    }

    if let Some(current) = refuse_replacement
        && &found != current
    {
        // La dirección publica una vez y lo que llega no es lo que hay.
        // Se aborta la escritura: un contenido que no se publica no
        // puede quedarse en el almacén.
        let _ = session.abort().await;
        return Err(RawApiError::Conflict(
            "the path already holds another content",
        ));
    }

    let digest = session
        .commit(CommitExpectation {
            digest: found.clone(),
            length,
        })
        .await
        .map_err(|error| the_blob_error(&error))?;
    Ok((digest, length))
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

/// Un error del almacén de contenido, traducido a HTTP.
///
/// Un contenido que **no esta** es un 404. Un contenido que esta pero no
/// es el que dice su digest es un 500: el almacén esta roto, y no es un
/// fallo del cliente. Confundir los dos sería decirle a un cliente que
/// su fichero no existe cuando lo que pasa es que el servidor lo tiene
/// roto.
pub(super) fn the_blob_error(error: &BlobError) -> RawApiError {
    match error {
        BlobError::NotFound => RawApiError::NotFound,
        BlobError::Corrupted { .. }
        | BlobError::LengthMismatch { .. }
        | BlobError::DigestMismatch { .. }
        | BlobError::UnsupportedAlgorithm(_) => RawApiError::Internal,
        BlobError::SessionClosed => RawApiError::Internal,
        BlobError::Unavailable(_) => RawApiError::Unavailable,
    }
}

/// Un error del catalogo, traducido a HTTP.
pub(super) fn the_publication_error(error: &CatalogError) -> RawApiError {
    match error {
        CatalogError::Missing { .. } => RawApiError::NotFound,
        CatalogError::Denied { .. } => RawApiError::Forbidden,
        CatalogError::Conflict { .. } => {
            RawApiError::Conflict("the path already holds another content")
        }
        // La condición que el cliente exigía no se cumple: no es un
        // conflicto, es una precondición, y el cliente las trata distinto.
        CatalogError::PreconditionFailed { .. } => RawApiError::PreconditionFailed,
        CatalogError::InvalidRequest { .. } => {
            RawApiError::BadRequest("the publication is not well formed")
        }
        CatalogError::ModeNotImplemented { .. } => RawApiError::NotFound,
        CatalogError::SchemaMismatch { .. } | CatalogError::Corrupt { .. } => RawApiError::Internal,
        CatalogError::Unavailable { .. } => RawApiError::Unavailable,
    }
}

/// Las cabeceras que todas las respuestas llevan.
pub(super) fn apply_common_headers(response: &mut Response, etag: &str, correlation: &str) {
    if let Ok(value) = HeaderValue::from_str(etag) {
        response.headers_mut().insert(header::ETAG, value);
    }
    if let Ok(value) = HeaderValue::from_str(correlation) {
        response
            .headers_mut()
            .insert(HeaderName::from_static(CORRELATION_HEADER), value);
    }
}

/// El hexadecimal de los bytes.
pub(super) fn hex_of(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}
