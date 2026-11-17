//! La superficie **NuGet v3**: `dotnet restore` y `dotnet nuget push`
//! contra un repositorio de Hodei.
//!
//! Vive en `/nuget/{repositorio}/...` y habla la gramatica de la
//! resource hub: el service index, el flat container, la hoja de
//! registros y el endpoint de publicacion. La gramatica pura —rutas,
//! versiones, identidad del paquete y documentos— esta en
//! `hodei_catalog::nuget`; aqui solo hay lo que necesita efectos.
//!
//! Cada version publicada deja **un unico asset inmutable** en el
//! catalogo, con su direccion canonica de protocolo:
//! `flatcontainer/{id}/{version}/{id}.nupkg`. La lista de versiones y
//! la hoja de registros se ensamblan con lo almacenado, como el indice
//! sparse de cargo: no hay documento que reescribir, y una version
//! publicada es para siempre. La metadata del paquete no se duplica:
//! vive dentro del propio nupkg, y el servidor la lee al publicar en
//! vez de reescribirla.
//!
//! La modalidad **hosted** escribe por `PUT package/{id}/{version}` con
//! el nupkg como cuerpo; la identidad que manda la URL es una promesa,
//! y manda la del nuspec: si no coinciden, la publicacion se rechaza.
//! La modalidad **proxy** re-serva el flat container y los registros
//! byte a byte: un nupkg reescrito rompe su hash, y NuGet valida el
//! paquete contra el `contentHash` del registro.

use axum::{
    body::Body,
    extract::{OriginalUri, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::Engine as _;
use hodei_catalog::{
    CatalogError, PackageCoordinates, Publication, RepositoryDescriptor, RepositoryFormat,
    UpstreamConfig,
    nuget::{self, FlatRequest, NuGetRoute, NugetError},
};
use serde_json::{Value, json};

use super::raw::{
    RawApiError, apply_cache_headers, apply_common_headers, the_admission_error, the_blob_error,
    the_catalog, the_content_store, the_context, the_current_asset, the_decision,
    the_publication_error, the_repository_in, write_the_content,
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
    /// Un error de la gramatica NuGet.
    NuGet(NugetError),
    /// Un estado y un motivo.
    Plain(StatusCode, String),
}

impl From<RawApiError> for Failure {
    fn from(error: RawApiError) -> Self {
        Self::Raw(error)
    }
}

impl From<NugetError> for Failure {
    fn from(error: NugetError) -> Self {
        Self::NuGet(error)
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
            Self::NuGet(error) => (
                if error.is_path_error() {
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
            // El desafio es Basic, no Bearer: NuGet solo adjunta las
            // credenciales de la fuente cuando el 401 las pide con ese
            // esquema, y el dialecto del hub ya acepta Basic con la
            // clave como contrasena.
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                HeaderValue::from_static("Basic realm=\"hodei\""),
            );
        }
        response
    }
}

/// El resto de la ruta de `/nuget/{repositorio}/{resto}`, sin decodificar:
/// el identificador de un paquete puede llevar `%2e` y un `%2f` no debe
/// abrir caminos.
fn the_split(uri: &OriginalUri) -> Result<(String, String), Failure> {
    let rest = uri
        .path()
        .strip_prefix("/nuget/")
        .ok_or(RawApiError::NotFound)?;
    let (repository, rest) = match rest.split_once('/') {
        Some((repository, rest)) => (repository, rest),
        // `/nuget/{repositorio}` a secas tambien es el indice de servicios.
        None => (rest, ""),
    };
    if !crate::raw::path::valid_repository_name(repository) {
        return Err(RawApiError::NotFound.into());
    }
    Ok((repository.to_string(), rest.to_string()))
}

/// La URL base del feed tal y como la ve el cliente: el service index
/// declara direcciones absolutas, y absolutas por el host que pregunto.
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
    Ok(format!("{scheme}://{host}/nuget/{repository}"))
}

/// El cliente NuGet llega de dos maneras: con Basic desde sus
/// credenciales de fuente (la contrasena es la clave, el usuario es
/// informativo) o con la cabecera `X-NuGet-ApiKey` que manda
/// `dotnet nuget push`. La API key puede viajar base64 por contrato
/// del cliente; la superficie compartida solo habla Bearer, asi que
/// aqui se normaliza una y otra cabecera.
fn normalize_authorization(headers: &mut HeaderMap) {
    let basic = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .strip_prefix("Basic ")
                .or_else(|| value.strip_prefix("basic "))
                .map(str::to_string)
        })
        .and_then(|value| {
            base64::engine::general_purpose::STANDARD
                .decode(value.trim())
                .ok()
        })
        .and_then(|decoded| String::from_utf8(decoded).ok())
        .and_then(|decoded| {
            decoded
                .split_once(':')
                .filter(|(_, password)| !password.is_empty())
                .map(|(_, password)| password.to_string())
        });
    let api_key = headers
        .get("x-nuget-apikey")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            // El cliente admite la clave en base64: si decodifica a algo
            // legible, eso es lo que el servidor debe creer.
            match base64::engine::general_purpose::STANDARD.decode(value) {
                Ok(decoded) => String::from_utf8(decoded)
                    .map(|decoded| {
                        decoded
                            .split_once(':')
                            .map_or(value.to_string(), |(_, password)| password.to_string())
                    })
                    .unwrap_or_else(|_| value.to_string()),
                Err(_) => value.to_string(),
            }
        });
    let token = basic.or(api_key);
    if let Some(token) = token
        && let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}"))
    {
        headers.insert(header::AUTHORIZATION, value);
    }
}

/// La identidad que declaraba el nuspec de una publicacion exitosa,
/// para que la auditoria nombre el paquete aunque la URL no lo hiciera.
#[derive(Clone)]
struct PublishedId(String);

/// Cualquier peticion de `/nuget/{repositorio}/...`.
pub async fn handle(
    State(state): State<AppState>,
    method: Method,
    uri: OriginalUri,
    mut headers: HeaderMap,
    request: axum::extract::Request<Body>,
) -> Result<Response, Failure> {
    normalize_authorization(&mut headers);
    // El service index es anonimo por contrato del cliente: `dotnet
    // nuget push` lo carga **sin credenciales** antes de mandar la API
    // key al endpoint de publicacion. El documento no toca el catalogo:
    // dice donde vive cada protocolo del repositorio, y nada de
    // contenido. Cualquier otra peticion sin credenciales sigue
    // muriendo en el contexto, como siempre.
    if !headers.contains_key(header::AUTHORIZATION) && matches!(method, Method::GET | Method::HEAD)
    {
        let anonymous = (|| -> Result<Option<Response>, Failure> {
            let (repository_key, rest) = the_split(&uri)?;
            if matches!(nuget::parse_route(&rest)?, NuGetRoute::ServiceIndex) {
                Ok(Some(json_response(
                    StatusCode::OK,
                    &nuget::service_index(&the_base(&headers, &repository_key)?),
                    None,
                    &correlation_id(&headers, &state.identity),
                )))
            } else {
                Ok(None)
            }
        })();
        if let Some(response) = anonymous.unwrap_or_default() {
            return Ok(response);
        }
    }
    let context = the_context(&state, &headers).await?;
    let correlation = correlation_id(&headers, &state.identity);
    let (repository_key, rest) = the_split(&uri)?;
    let route = nuget::parse_route(&rest)?;
    let repository = the_repository_in(
        &state,
        &context,
        &repository_key,
        &[RepositoryFormat::NuGet],
    )
    .await?;

    let reading = method == Method::GET || method == Method::HEAD;
    let writing = method == Method::PUT;
    let outcome = match (&route, reading, writing) {
        (NuGetRoute::ServiceIndex, true, _) => Ok(json_response(
            StatusCode::OK,
            &nuget::service_index(&the_base(&headers, &repository_key)?),
            None,
            &correlation,
        )),
        (_, true, _) => {
            read(
                &state,
                &context,
                &repository,
                &route,
                method == Method::HEAD,
                &correlation,
            )
            .await
        }
        (NuGetRoute::Publish { .. }, false, true) => {
            publish(&state, &context, &repository, &route, request, &correlation).await
        }
        (_, false, true) => Err(RawApiError::MethodNotAllowed(
            "a nuget package is published with a PUT of its nupkg to the publish endpoint",
        )
        .into()),
        _ => Err(RawApiError::MethodNotAllowed("this NuGet operation is not supported").into()),
    };
    let audited = outcome
        .as_ref()
        .ok()
        .and_then(|response| response.extensions().get::<PublishedId>().cloned())
        .map(|published| published.0)
        .unwrap_or_else(|| route.package().unwrap_or("index").to_string());
    record_outcome(
        &state,
        &context,
        &repository_key,
        &audited,
        &outcome,
        &correlation,
    );
    outcome
}

fn record_outcome(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &str,
    package: &str,
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
        &format!("{repository}/{package}"),
        result,
        correlation,
    );
}

// ---------------------------------------------------------------------------
// Lectura
// ---------------------------------------------------------------------------

/// Una lectura es un salto al proxy o una visita al almacen propio, y la
/// modalidad del repositorio decide: un proxy no inventa versiones, y un
/// hosted no sale a la red.
async fn read(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    route: &NuGetRoute,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::download_asset(), repository)?;
    match repository.mode {
        hodei_catalog::RepositoryMode::Proxy => {
            return read_proxy(state, context, repository, route, head, correlation).await;
        }
        // Un grupo de nuget todavia no sabe ensamblar listas de versiones
        // entre sus miembros, y lo dice: no se convierte en un Raw que
        // sirve rutas sueltas y finge ser un feed.
        hodei_catalog::RepositoryMode::Group => {
            return Err(RawApiError::MethodNotAllowed(
                "this nuget repository is a group, and groups do not assemble package version lists yet",
            )
            .into());
        }
        _ => {}
    }
    match route {
        NuGetRoute::Flat(FlatRequest::Versions { id }) => {
            let versions = hosted_versions(state, context, repository, id).await?;
            if versions.is_empty() {
                return Err(Failure::plain(
                    StatusCode::NOT_FOUND,
                    "there is no package of that name in this repository",
                ));
            }
            Ok(json_response(
                StatusCode::OK,
                &nuget::version_index(&versions.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>()),
                None,
                correlation,
            ))
        }
        NuGetRoute::Registration { id } => {
            let versions = hosted_versions(state, context, repository, id).await?;
            if versions.is_empty() {
                return Err(Failure::plain(
                    StatusCode::NOT_FOUND,
                    "there is no package of that name in this repository",
                ));
            }
            Ok(json_response(
                StatusCode::OK,
                &nuget::registration_leaf(
                    id,
                    &versions.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>(),
                ),
                None,
                correlation,
            ))
        }
        NuGetRoute::Flat(FlatRequest::File { id, version, file }) => {
            // En el flat container solo hay lo que se publico: un
            // `PUT` deja el nupkg, y cualquier otro nombre bajo esa
            // version no nombra nada. El contrato admite dos formas:
            // la direccion canonica `{id}.nupkg` y la que de verdad
            // pide el cliente al descargar, `{id}.{version}.nupkg`.
            if *file != format!("{id}.nupkg") && *file != format!("{id}.{version}.nupkg") {
                return Err(Failure::plain(
                    StatusCode::NOT_FOUND,
                    "the flat container holds the nupkg of a published version, and nothing else",
                ));
            }
            read_asset_named(
                state,
                context,
                repository,
                &nuget::package_asset_path(id, version),
                "application/octet-stream",
                head,
                correlation,
            )
            .await
        }
        _ => Err(RawApiError::NotFound.into()),
    }
}

/// Las versiones almacenadas de un paquete, ordenadas por la regla de
/// NuGet: los numeros primero y los previos antes que su version base.
async fn hosted_versions(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    id: &str,
) -> Result<Vec<(nuget::NuGetVersion, String)>, Failure> {
    let (reader, _) = the_catalog(state)?;
    let records = reader
        .assets_under(
            context.account().as_str(),
            &repository.repo_key,
            &format!("flatcontainer/{id}/"),
        )
        .await
        .map_err(|error| Failure::from(the_publication_error(&error)))?;
    let mut versions: Vec<(nuget::NuGetVersion, String)> = Vec::new();
    for record in records {
        let Some(version) = record
            .asset_path
            .strip_prefix(&format!("flatcontainer/{id}/"))
            .and_then(|rest| rest.strip_suffix(&format!("/{id}.nupkg")))
        else {
            continue;
        };
        if let Some(parsed) = nuget::parse_version(version) {
            versions.push((parsed, version.to_string()));
        }
    }
    versions.sort_by(|left, right| left.0.compare(&right.0));
    versions.dedup_by(|left, right| left.1 == right.1);
    Ok(versions)
}

/// Los bytes de un asset publicado, verificados contra el catalogo, con
/// su tipo de contenido de protocolo.
async fn read_asset_named(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    asset_path: &str,
    content_type: &str,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    let resolved =
        super::raw::the_resolved_asset(state, context, repository.clone(), asset_path, correlation)
            .await?;
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
    let mut response = body_response(StatusCode::OK, bytes, content_type, &etag, correlation);
    apply_cache_headers(&mut response, &served);
    if head && let Ok(value) = HeaderValue::from_str(&length.to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    Ok(response)
}

// ---------------------------------------------------------------------------
// Proxy
// ---------------------------------------------------------------------------

/// Un proxy NuGet re-serva el documento del upstream tal cual: el
/// flat container y los registros se cachean bajo su ruta de protocolo
/// y un nupkg reescrito dejaria de casar con el `contentHash` que el
/// propio upstream declara.
async fn read_proxy(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    route: &NuGetRoute,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    let upstream = UpstreamConfig::of(&repository.config)
        .ok()
        .flatten()
        .ok_or_else(|| Failure::plain(StatusCode::BAD_GATEWAY, "the proxy is not configured"))?;
    let (asset_path, content_type) = match route {
        NuGetRoute::Flat(FlatRequest::Versions { id }) => {
            (format!("flatcontainer/{id}/index.json"), "application/json")
        }
        NuGetRoute::Flat(FlatRequest::File { id, version, file }) => (
            format!("flatcontainer/{id}/{version}/{file}"),
            "application/octet-stream",
        ),
        NuGetRoute::Registration { id } => {
            (format!("registrations/{id}/index.json"), "application/json")
        }
        _ => return Err(RawApiError::NotFound.into()),
    };
    let url = upstream.target(&asset_path).ok_or(RawApiError::NotFound)?;
    let served = super::raw::the_proxy_asset_at(
        state,
        context,
        repository,
        &asset_path,
        Some(url),
        Some("application/octet-stream"),
        correlation,
    )
    .await?;
    let bytes = if head {
        Vec::new()
    } else {
        the_content_store(state)?
            .get(&served.asset.digest)
            .await
            .map_err(|error| the_blob_error(&error))?
            .to_vec()
    };
    let etag = conditional::the_etag(&served.asset.digest);
    let mut response = body_response(StatusCode::OK, bytes, content_type, &etag, correlation);
    apply_cache_headers(&mut response, &Some((served.status, served.provenance)));
    Ok(response)
}

// ---------------------------------------------------------------------------
// Publicacion (hosted)
// ---------------------------------------------------------------------------

/// Publicar un nupkg. La identidad del nuspec manda sobre la URL, y la
/// inmutabilidad manda sobre todo lo demas: el mismo digest es un si,
/// un digest distinto es un 409 que explica por que.
async fn publish(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    route: &NuGetRoute,
    request: axum::extract::Request<Body>,
    correlation: &str,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::upload_asset(), repository)?;
    if !repository.mode.accepts_client_writes() {
        return Err(
            RawApiError::MethodNotAllowed("this repository does not accept uploads").into(),
        );
    }
    let NuGetRoute::Publish { id, version } = route else {
        return Err(RawApiError::NotFound.into());
    };
    let limit = state.max_body_size.saturating_add(1024 * 1024);
    let body = axum::body::to_bytes(request.into_body(), limit)
        .await
        .map_err(|_| Failure::Raw(RawApiError::PayloadTooLarge))?;

    // El cliente real manda el nupkg dentro de un formulario multipart
    // y sin identidad en la URL; el cuerpo crudo tambien es publicacion
    // valida. En ambos casos la identidad interior es la autoridad: la
    // URL es una promesa del cliente sobre el nuspec, y aqui se cumple
    // o se rechaza.
    let payload = if body.starts_with(b"PK") {
        body.to_vec()
    } else {
        nuget::multipart_package(&body)?
    };
    let declared = nuget::package_identity(&payload)?;
    if let Some(promise) = id
        && declared.id != *promise
    {
        return Err(NugetError::InvalidPackage(format!(
            "the nuspec declares {}, but the URL says {promise}",
            declared.id
        ))
        .into());
    }
    if let Some(promise) = version
        && declared.version != *promise
    {
        return Err(NugetError::InvalidPackage(format!(
            "the nuspec declares version {}, but the URL says {promise}",
            declared.version
        ))
        .into());
    }
    let (id, version) = (declared.id.clone(), declared.version.clone());

    let asset_path = nuget::package_asset_path(&id, &version);
    let (digest, length) = write_the_content(state, Body::from(payload), None, None).await?;

    // Una version ya publicada es para siempre: el mismo digest es el eco
    // de una publicacion interrumpida, un digest distinto es un 409 que
    // explica por que.
    let immutable =
        format!("version {version} of {id} is already published; a published version is immutable");
    if let Some(existing) =
        the_current_asset(state, context, &repository.repo_key, &asset_path).await?
        && existing.digest != digest
    {
        return Err(Failure::plain(StatusCode::CONFLICT, &immutable));
    }

    let (_, writer) = the_catalog(state)?;
    let publication = Publication {
        repository: repository.repo_key.clone(),
        asset_path,
        coordinates: PackageCoordinates {
            package_name: id.clone(),
            package_version: version.clone(),
        },
        digest,
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
        Ok(_) => {
            let mut response = json_response(
                StatusCode::CREATED,
                &json!({ "ok": true }),
                None,
                correlation,
            );
            // La auditoria nombra el paquete aunque la URL no lo hiciera.
            response.extensions_mut().insert(PublishedId(id));
            Ok(response)
        }
        Err(CatalogError::Conflict { .. }) => Err(Failure::plain(StatusCode::CONFLICT, &immutable)),
        Err(error) => Err(the_publication_error(&error).into()),
    }
}

// ---------------------------------------------------------------------------
// Respuestas
// ---------------------------------------------------------------------------

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
