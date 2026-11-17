//! La superficie **go**: `go mod download` / `go build` con `GOPROXY`
//! apuntando a un repositorio de Hodei.
//!
//! Vive en `/go/{repositorio}/...` y habla GOPROXY: la gramatica de rutas
//! y de documentos esta en `hodei_catalog::gomod` (pura) y aqui solo hay
//! lo que necesita efectos.
//!
//! Cada version publicada deja **tres assets inmutables** en el catalogo,
//! con la misma ruta que el protocolo: `{modulo}/@v/{version}.info`,
//! `.mod` y `.zip`. La lista de versiones y `@latest` se ensamblan con lo
//! que hay almacenado, como el indice sparse de cargo: no hay documento
//! que reescribir, y una version publicada es para siempre.
//!
//! El hosted conoce dos modalidades. En **hosted**, el publicador deja
//! los tres artefactos con `PUT` (el protocolo `go` no sabe escribir; la
//! escritura es propia del registro y usa la misma autorizacion que el
//! resto de publicaciones). En **proxy**, cada lectura se trae del
//! upstream con la cache, los recibos y el single-flight de la
//! infraestructura compartida, y se sirve tal cual: GOPROXY es opaco por
//! contrato, y reescribir un zip romperia su checksum.

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
    gomod::{self, GoError, Route},
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
    /// Un error del protocolo GOPROXY.
    Go(GoError),
    /// Un estado y un motivo.
    Plain(StatusCode, String),
}

impl From<RawApiError> for Failure {
    fn from(error: RawApiError) -> Self {
        Self::Raw(error)
    }
}

impl From<GoError> for Failure {
    fn from(error: GoError) -> Self {
        Self::Go(error)
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
            Self::Go(error) => (
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
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
    }
}

/// El resto de la ruta de `/go/{repositorio}/{resto}`, sin decodificar: el
/// `go` escapa los nombres el solo, y un `%2f` no debe abrir caminos.
fn the_split(uri: &OriginalUri) -> Result<(String, String), Failure> {
    let rest = uri
        .path()
        .strip_prefix("/go/")
        .ok_or(RawApiError::NotFound)?;
    let (repository, rest) = rest.split_once('/').ok_or(RawApiError::NotFound)?;
    if !crate::raw::path::valid_repository_name(repository) || rest.is_empty() {
        return Err(RawApiError::NotFound.into());
    }
    Ok((repository.to_string(), rest.to_string()))
}

/// El cliente `go` autentica con Basic desde su `.netrc`, y la credencial
/// va en la contrasena: el usuario es informativo. La superficie
/// compartida solo habla Bearer, asi que aqui se normaliza la cabecera,
/// igual que hace el canal cargo con el token sin esquema.
fn normalize_authorization(headers: &mut HeaderMap) {
    let encoded = headers
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
    if let Some(token) = encoded
        && let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}"))
    {
        headers.insert(header::AUTHORIZATION, value);
    }
}

/// Cualquier peticion de `/go/{repositorio}/...`.
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
        the_repository_in(&state, &context, &repository_key, &[RepositoryFormat::Go]).await?;

    let reading = method == Method::GET || method == Method::HEAD;
    let writing = method == Method::PUT;
    let outcome = match (&route, reading, writing) {
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
        (Route::Info { .. } | Route::Mod { .. } | Route::Zip { .. }, false, true) => {
            publish(&state, &context, &repository, &route, request, &correlation).await
        }
        (_, false, true) => Err(RawApiError::MethodNotAllowed(
            "a go version is published with a PUT of its .info, .mod or .zip",
        )
        .into()),
        _ => Err(RawApiError::MethodNotAllowed("this GOPROXY operation is not supported").into()),
    };
    record_outcome(
        &state,
        &context,
        &repository_key,
        route.module(),
        &outcome,
        &correlation,
    );
    outcome
}

fn record_outcome(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &str,
    module: &str,
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
        &format!("{repository}/{module}"),
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
    route: &Route,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::download_asset(), repository)?;
    match repository.mode {
        hodei_catalog::RepositoryMode::Proxy => {
            return read_proxy(state, context, repository, route, head, correlation).await;
        }
        // Un grupo de go todavia no sabe ensamblar una lista de versiones
        // entre sus miembros, y lo dice: no se convierte en un Raw que
        // sirve rutas sueltas y finge ser un proxy.
        hodei_catalog::RepositoryMode::Group => {
            return Err(RawApiError::MethodNotAllowed(
                "this go repository is a group, and groups do not assemble module version lists yet",
            )
            .into());
        }
        _ => {}
    }
    match route {
        Route::List { module } => {
            let versions = hosted_versions(state, context, repository, module).await?;
            if versions.is_empty() {
                // El cliente distingue "modulo desconocido" de "modulo sin
                // versiones publicadas": el primero es un 404.
                return Err(Failure::plain(
                    StatusCode::NOT_FOUND,
                    "there is no module of that name in this repository",
                ));
            }
            let body = versions
                .iter()
                .map(|(_, version)| format!("{version}\n"))
                .collect::<String>();
            Ok(text_response(body, head, correlation))
        }
        Route::Latest { module } => {
            let versions = hosted_versions(state, context, repository, module).await?;
            let Some((_, latest)) = versions.last() else {
                return Err(Failure::plain(
                    StatusCode::NOT_FOUND,
                    "this module has no published versions",
                ));
            };
            read_asset_named(
                state,
                context,
                repository,
                &format!("{module}/@v/{latest}.info"),
                gomod::content_type("info"),
                head,
                correlation,
            )
            .await
        }
        Route::Info { .. } | Route::Mod { .. } | Route::Zip { .. } => {
            let path = route.asset_path().expect("a version route names its asset");
            let extension = path.rsplit('.').next().unwrap_or_default();
            read_asset_named(
                state,
                context,
                repository,
                &path,
                gomod::content_type(extension),
                head,
                correlation,
            )
            .await
        }
    }
}

/// Las versiones almacenadas de un modulo, ordenadas por semver.
async fn hosted_versions(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    module: &str,
) -> Result<Vec<(semver::Version, String)>, Failure> {
    let (reader, _) = the_catalog(state)?;
    let records = reader
        .assets_under(
            context.account().as_str(),
            &repository.repo_key,
            &format!("{module}/@v/"),
        )
        .await
        .map_err(|error| Failure::from(the_publication_error_ref(&error)))?;
    let mut versions: Vec<(semver::Version, String)> = Vec::new();
    for record in records {
        // Solo los .info cuentan como versiones: un .mod hueso sin .info
        // no es una version publicada para el protocolo.
        if let Some(version) = record
            .asset_path
            .strip_prefix(&format!("{module}/@v/"))
            .and_then(|file| file.strip_suffix(".info"))
            && let Some(parsed) = gomod::parse_version(version)
        {
            versions.push((parsed, version.to_string()));
        }
    }
    versions.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(versions)
}

/// Los bytes de un asset publicado, verificados contra el catalogo, con su
/// tipo de contenido de protocolo.
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

/// Un proxy GOPROXY sirve lo que dice el upstream, byte a byte: la lista,
/// el documento, el go.mod y el zip se cachean bajo su ruta de protocolo
/// y se re-servan sin reescribir nada.
async fn read_proxy(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    route: &Route,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    let upstream = UpstreamConfig::of(&repository.config)
        .ok()
        .flatten()
        .ok_or_else(|| Failure::plain(StatusCode::BAD_GATEWAY, "the proxy is not configured"))?;
    let (asset_path, content_type) = match route {
        Route::List { module } => (format!("{module}/@v/list"), gomod::content_type("list")),
        Route::Latest { module } => (format!("{module}/@latest"), gomod::content_type("info")),
        Route::Info { .. } | Route::Mod { .. } | Route::Zip { .. } => {
            let extension = match route {
                Route::Mod { .. } => "mod",
                Route::Zip { .. } => "zip",
                _ => "info",
            };
            (
                route.asset_path().expect("a version route names its asset"),
                gomod::content_type(extension),
            )
        }
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

/// Publicar un artefacto de version. La inmutabilidad manda: el mismo
/// digest es un si, un digest distinto es un 409 que explica por que.
async fn publish(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    route: &Route,
    request: axum::extract::Request<Body>,
    correlation: &str,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::upload_asset(), repository)?;
    if !repository.mode.accepts_client_writes() {
        return Err(
            RawApiError::MethodNotAllowed("this repository does not accept uploads").into(),
        );
    }
    let asset_path = route.asset_path().expect("a version route names its asset");
    let extension = asset_path.rsplit('.').next().unwrap_or_default();
    let limit = state.max_body_size.saturating_add(1024 * 1024);
    let body = axum::body::to_bytes(request.into_body(), limit)
        .await
        .map_err(|_| Failure::Raw(RawApiError::PayloadTooLarge))?;

    let (module, version) = match route {
        Route::Info { module, version }
        | Route::Mod { module, version }
        | Route::Zip { module, version } => (module.clone(), version.clone()),
        _ => return Err(RawApiError::NotFound.into()),
    };
    match extension {
        "info" => gomod::check_info(&body, &version)?,
        "mod" => gomod::check_mod(&body, &module)?,
        "zip" => {
            if body.is_empty() {
                return Err(GoError::InvalidDocument("the module zip is empty".into()).into());
            }
        }
        _ => return Err(RawApiError::NotFound.into()),
    }

    let (digest, length) = write_the_content(state, Body::from(body.to_vec()), None, None).await?;

    // Una version ya publicada es para siempre: el mismo digest es el eco
    // de una publicacion interrumpida, un digest distinto es un 409 que
    // explica por que.
    let immutable = format!(
        "version {version} of {module} is already published; a published version is immutable"
    );
    if let Some(existing) =
        the_current_asset(state, context, &repository.repo_key, &asset_path).await?
        && existing.digest != digest
    {
        return Err(Failure::plain(StatusCode::CONFLICT, &immutable));
    }

    let (_, writer) = the_catalog(state)?;
    // El zip es el contenido de la version; el `.info` y el `.mod` son su
    // metadata. El catalogo lleva una fila de version por coordenadas,
    // asi que la metadata se registra con un nombre propio, igual que
    // hace cargo con su indice: sin el, publicar el `.mod` seria
    // reescribir la version que el `.zip` acaba de fijar.
    let package_name = match extension {
        "zip" => module.clone(),
        _ => format!("{module}#{extension}"),
    };
    let publication = Publication {
        repository: repository.repo_key.clone(),
        asset_path: asset_path.clone(),
        coordinates: PackageCoordinates {
            package_name,
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
        Ok(_) => Ok(json_response(
            StatusCode::OK,
            &json!({ "ok": true }),
            None,
            correlation,
        )),
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

/// La lista y el `@latest` del hosted: texto con su tamano real, y el
/// `go` no pide ETag para la lista pero agradece el nosniff.
fn text_response(body: String, head: bool, correlation: &str) -> Response {
    let length = body.len();
    let mut response = body_response(
        StatusCode::OK,
        if head { Vec::new() } else { body.into_bytes() },
        gomod::content_type("list"),
        "-",
        correlation,
    );
    if head && let Ok(value) = HeaderValue::from_str(&length.to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
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

/// El error de catalogo de una lectura: la lectura no provoca conflictos,
/// pero el traductor sabe que significa cada variante.
fn the_publication_error_ref(error: &CatalogError) -> RawApiError {
    match error {
        CatalogError::Denied { .. } => RawApiError::Forbidden,
        CatalogError::Missing { .. } => RawApiError::NotFound,
        CatalogError::Unavailable { .. } | CatalogError::Corrupt { .. } => RawApiError::Internal,
        other => RawApiError::BadRequest(Box::leak(other.to_string().into_boxed_str())),
    }
}
