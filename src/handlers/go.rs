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
        // Un grupo de go ensambla la union de las versiones de sus miembros
        // y resuelve cada ruta por precedencia. La precedencia, el MISS
        // distinto del ERROR y la reserva de namespaces son los del motor de
        // grupo de la superficie cruda: aqui no se reimplementan.
        hodei_catalog::RepositoryMode::Group => {
            return read_group(state, context, repository, route, head, correlation).await;
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
// Grupo
// ---------------------------------------------------------------------------

/// Lo que un miembro del grupo ofrece de un modulo.
enum Offering {
    /// El miembro tiene el modulo, con estas versiones.
    Versions(Vec<(semver::Version, String)>),
    /// El miembro no lo tiene: un 404 del upstream, un MISS.
    Missing,
}

/// Las hojas que el grupo deja pasar para un modulo, con la reserva de
/// namespaces ya aplicada.
///
/// La reserva se consulta **antes** que los miembros: un modulo reservado no
/// se pide al upstream publico del grupo, y los miembros hosted son la
/// fuente interna y no la toca.
async fn the_governed(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    group: &RepositoryDescriptor,
    module: &str,
) -> Result<
    (
        Vec<RepositoryDescriptor>,
        Vec<String>,
        Option<hodei_catalog::Judgement>,
    ),
    Failure,
> {
    super::raw_group::the_governed_leaves(state, context, group, module)
        .await
        .map_err(Failure::from)
}

/// Los bytes que un miembro proxy trae de su upstream.
///
/// El camino es el de la superficie cruda: la cache, los recibos, el
/// single-flight y el modo offline son los mismos, y lo que baja se publica
/// en el namespace del miembro.
async fn the_proxy_bytes(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    member: &RepositoryDescriptor,
    asset_path: &str,
    correlation: &str,
) -> Result<Vec<u8>, Failure> {
    let served =
        super::raw::the_proxy_asset(state, context, member, asset_path, correlation).await?;
    let bytes = the_content_store(state)?
        .get(&served.asset.digest)
        .await
        .map_err(|error| the_blob_error(&error))?
        .to_vec();
    Ok(bytes)
}

/// Las versiones de un miembro, con la misma regla que un hosted: una
/// version es una version si tiene `.info`.
///
/// Un miembro proxy pregunta a su upstream por el **nombre del modulo**, no
/// por el nombre del grupo: el grupo no renombra a su miembro.
async fn the_member_versions(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    member: &RepositoryDescriptor,
    module: &str,
    correlation: &str,
) -> Result<Offering, Failure> {
    if member.mode == hodei_catalog::RepositoryMode::Proxy {
        let asset_path = format!("{module}/@v/list");
        let bytes = match the_proxy_bytes(state, context, member, &asset_path, correlation).await {
            Ok(bytes) => bytes,
            // Un 404 del upstream es un MISS: se sigue con el siguiente
            // miembro. Cualquier otro fallo es un ERROR y detiene la
            // resolucion: un fallo transitorio no puede cambiar en silencio
            // lo que se descarga.
            Err(Failure::Raw(RawApiError::NotFound)) => return Ok(Offering::Missing),
            Err(other) => return Err(other),
        };
        let text = String::from_utf8_lossy(&bytes);
        let mut versions = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if let Some(parsed) = gomod::parse_version(line) {
                versions.push((parsed, line.to_string()));
            }
        }
        return Ok(Offering::Versions(versions));
    }
    Ok(Offering::Versions(
        hosted_versions(state, context, member, module).await?,
    ))
}

/// La union de las versiones de los miembros del grupo, en orden semver.
///
/// La union no es destructiva: si dos miembros tienen la misma version, sale
/// **una** vez. Un upstream caido detiene la resolucion y no deja un listado
/// a medias.
async fn the_group_versions(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    leaves: &[RepositoryDescriptor],
    module: &str,
    correlation: &str,
) -> Result<Vec<(semver::Version, String)>, Failure> {
    let mut versions: Vec<(semver::Version, String)> = Vec::new();
    for member in leaves {
        match the_member_versions(state, context, member, module, correlation).await? {
            Offering::Versions(found) => {
                for entry in found {
                    if !versions.iter().any(|(_, known)| known == &entry.1) {
                        versions.push(entry);
                    }
                }
            }
            Offering::Missing => {}
        }
    }
    versions.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(versions)
}

/// El digest que el catalogo conoce de un artefacto de una version.
///
/// `None` es que ese miembro no tiene (o no ha descargado) el artefacto: no
/// se compara con nadie.
async fn the_known_digest(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    member: &RepositoryDescriptor,
    asset_path: &str,
) -> Result<Option<String>, Failure> {
    Ok(
        super::raw::the_current_asset(state, context, &member.repo_key, asset_path)
            .await?
            .map(|asset| asset.digest.to_string()),
    )
}

/// La divergencia entre miembros: la misma version con contenido distinto.
///
/// El protocolo go no tiene forma de decir "hay dos": el cliente no puede
/// saber cual de los dos es el bueno. Por eso el grupo **rechaza** con un
/// error definido en vez de elegir en silencio.
///
/// Se comparan los `.mod` y los `.zip`, que son los dos digests que el
/// cliente escribe en su `go.sum`. Solo se comparan los miembros cuyo
/// contenido el catalogo conoce; el que aun no ha descargado esa version no
/// se compara, y ahi la precedencia decide.
async fn the_divergence(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    leaves: &[RepositoryDescriptor],
    module: &str,
    version: &str,
) -> Result<Option<String>, Failure> {
    for extension in ["mod", "zip"] {
        let asset_path = format!("{module}/@v/{version}.{extension}");
        let mut seen: Option<(String, String)> = None;
        for member in leaves {
            let Some(digest) = the_known_digest(state, context, member, &asset_path).await? else {
                continue;
            };
            match &seen {
                None => seen = Some((member.repo_key.clone(), digest)),
                Some((known_key, known_digest)) if known_digest != &digest => {
                    return Ok(Some(format!(
                        "the version {version} of {module} has different content in the members {known_key} and {}",
                        member.repo_key
                    )));
                }
                Some(_) => {}
            }
        }
    }
    Ok(None)
}

/// El estado de un grupo go frente a un modulo.
///
/// Son las hojas que la reserva de namespaces deja pasar, los miembros que
/// dejo fuera y el veredicto de la reserva: la resolucion los consulta
/// siempre, y no se repregunta en cada artefacto.
struct TheGroup<'a> {
    /// El repositorio de grupo.
    group: &'a RepositoryDescriptor,
    /// Las hojas que se consultan, en el orden de precedencia.
    leaves: Vec<RepositoryDescriptor>,
    /// Los miembros proxy que la reserva de namespaces dejo fuera.
    denied: Vec<String>,
    /// El veredicto de la reserva, si la hubo.
    judgement: Option<hodei_catalog::Judgement>,
}

/// El miembro que duena una version: el primero, en el orden configurado, que
/// la tiene.
///
/// La version se reconoce por su `.info`, que es el mismo criterio que usa
/// `hosted_versions`: un `.info` con version parseable es una version.
///
/// El dueno decide **que** se sirve: los tres artefactos (`.info`, `.mod` y
/// `.zip`) salen de ese mismo miembro. Nunca se mezcla el `.mod` de un
/// miembro con el `.zip` de otro, porque un `go.sum` que no cuadra es un
/// cliente que falla con un error que no explica.
async fn the_owner_of(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    group: &TheGroup<'_>,
    module: &str,
    version: &str,
    correlation: &str,
) -> Result<RepositoryDescriptor, Failure> {
    for member in &group.leaves {
        let asset_path = format!("{module}/@v/{version}.info");
        let has = if member.mode == hodei_catalog::RepositoryMode::Proxy {
            // Un miembro proxy responde con su upstream: un 404 es un MISS
            // (el miembro no tiene esa version) y lo demas es un ERROR que
            // detiene la resolucion.
            match the_proxy_bytes(state, context, member, &asset_path, correlation).await {
                Ok(_) => true,
                Err(Failure::Raw(RawApiError::NotFound)) => false,
                Err(other) => return Err(other),
            }
        } else {
            super::raw::the_current_asset(state, context, &member.repo_key, &asset_path)
                .await?
                .is_some()
        };
        if has {
            return Ok(member.clone());
        }
    }
    // Un caso reservado no hace fallback: se explica por que no se sirve.
    if !group.denied.is_empty()
        && let Some(judgement) = &group.judgement
    {
        return Err(super::raw_group::the_group_reservation_explanation(
            group.group,
            judgement,
            &group.denied,
        )
        .into());
    }
    Err(Failure::plain(
        StatusCode::NOT_FOUND,
        "this module has no such version in this group",
    ))
}

/// Lo que un grupo go sirve en una ruta.
///
/// Un grupo no es un proxy con otro nombre: ensambla la **union** de las
/// versiones de sus miembros y resuelve cada version por precedencia.
async fn read_group(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    group: &RepositoryDescriptor,
    route: &Route,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    let module = route.module().to_string();
    let (leaves, denied, judgement) = the_governed(state, context, group, &module).await?;
    let governed = TheGroup {
        group,
        leaves,
        denied,
        judgement,
    };
    match route {
        Route::List { .. } => {
            let versions =
                the_group_versions(state, context, &governed.leaves, &module, correlation).await?;
            if versions.is_empty() {
                // Un caso reservado no hace fallback: se explica por que no
                // se sirve en vez de parecer un modulo que no existe.
                if !governed.denied.is_empty()
                    && let Some(judgement) = &governed.judgement
                {
                    return Err(super::raw_group::the_group_reservation_explanation(
                        governed.group,
                        judgement,
                        &governed.denied,
                    )
                    .into());
                }
                return Err(Failure::plain(
                    StatusCode::NOT_FOUND,
                    "there is no module of that name in this group",
                ));
            }
            let body = versions
                .iter()
                .map(|(_, version)| format!("{version}\n"))
                .collect::<String>();
            Ok(text_response(body, head, correlation))
        }
        Route::Latest { .. } => {
            // El "latest" de un grupo es el latest de la **union**, no el del
            // ultimo miembro que responda.
            let versions =
                the_group_versions(state, context, &governed.leaves, &module, correlation).await?;
            let Some((_, latest)) = versions.last() else {
                if !governed.denied.is_empty()
                    && let Some(judgement) = &governed.judgement
                {
                    return Err(super::raw_group::the_group_reservation_explanation(
                        governed.group,
                        judgement,
                        &governed.denied,
                    )
                    .into());
                }
                return Err(Failure::plain(
                    StatusCode::NOT_FOUND,
                    "this module has no published versions",
                ));
            };
            let owned = Route::Info {
                module: module.clone(),
                version: latest.clone(),
            };
            serve_owned(state, context, &governed, &owned, head, correlation).await
        }
        Route::Info { .. } | Route::Mod { .. } | Route::Zip { .. } => {
            serve_owned(state, context, &governed, route, head, correlation).await
        }
    }
}

/// Sirve un artefacto de version desde el miembro que la duena.
async fn serve_owned(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    group: &TheGroup<'_>,
    route: &Route,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    let (module, version) = match route {
        Route::Info { module, version }
        | Route::Mod { module, version }
        | Route::Zip { module, version } => (module.clone(), version.clone()),
        _ => return Err(RawApiError::NotFound.into()),
    };
    if let Some(conflict) = the_divergence(state, context, &group.leaves, &module, &version).await?
    {
        // La divergencia se rechaza con un error definido: el cliente puede
        // distinguir "no lo tengo" de "esto esta en conflicto".
        return Err(Failure::plain(StatusCode::CONFLICT, &conflict));
    }
    let owner = the_owner_of(state, context, group, &module, &version, correlation).await?;
    match owner.mode {
        hodei_catalog::RepositoryMode::Proxy => {
            read_proxy(state, context, &owner, route, head, correlation).await
        }
        _ => {
            let path = route.asset_path().expect("a version route names its asset");
            let extension = path.rsplit('.').next().unwrap_or_default();
            read_asset_named(
                state,
                context,
                &owner,
                &path,
                gomod::content_type(extension),
                head,
                correlation,
            )
            .await
        }
    }
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

    let (digest, length) = write_the_content(
        state,
        context.account().as_str(),
        Body::from(body.to_vec()),
        None,
        None,
    )
    .await?;

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
