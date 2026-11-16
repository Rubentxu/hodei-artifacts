//! La superficie **PyPI**: `twine upload` y `pip install` contra un
//! repositorio de Hodei.
//!
//! Vive en `/pypi/{repositorio}/...` y habla la Simple API: la gramatica
//! de nombres, de ficheros y de documentos esta en `hodei_catalog::pypi`
//! (pura) y aqui solo hay lo que necesita efectos.
//!
//! # Las rutas
//!
//! - `GET /pypi/{repositorio}/simple/` — el indice de los proyectos.
//! - `GET /pypi/{repositorio}/simple/{proyecto}/` — la pagina de un
//!   proyecto.
//! - `GET /pypi/{repositorio}/files/{proyecto}/{fichero}` — la distribucion.
//! - `POST /pypi/{repositorio}/` — la subida (`twine upload`).
//! - `POST /pypi/{repositorio}/yank` — retirar una version.
//!
//! # Como se autentica
//!
//! `pip` y `twine` no saben mandar un `Bearer`: los dos hacen
//! autenticacion basica. Por eso esta superficie acepta las dos formas: la
//! cabecera `Bearer` con la credencial de la plataforma, y la basica, cuya
//! contrasena **es** la credencial de la plataforma (el usuario es
//! informativo), como en el endpoint de token de OCI.
//!
//! Lo que se puede hacer lo decide Cedar en cada peticion, como en el resto
//! de superficies: la credencial lleva una identidad, no unos permisos.
//!
//! # El indice lo escribe esta pagina
//!
//! El HTML que sale de aqui lo pinta este servicio, con los valores
//! escapados. Un proxy no devuelve el HTML del upstream: lo vuelve a
//! pintar con lo que el upstream declaro en su JSON. Un upstream puede
//! publicar lo que quiera en su HTML; lo que el cliente recibe de aqui no
//! lo puede inyectar.
//!
//! # El documento de un proyecto
//!
//! Es una **instantanea**: cada publicacion y cada retiro escribe un
//! documento nuevo en el almacen de contenido y mueve el puntero de la
//! ruta con una precondicion sobre el digest anterior. Las distribuciones
//! son contenido inmutable en su propia ruta, y no se tocan al reescribir
//! el documento.
//!
//! # Lo que se comprueba de una subida
//!
//! El hash que declara el cliente tiene que ser el de los bytes que han
//! llegado, y el nombre y la version tienen que ser los del nombre del
//! fichero. Un cliente que miente no publica: el indice no puede prometer
//! un hash que no es el del contenido que se descarga.

use axum::{
    body::Body,
    extract::{FromRequest, OriginalUri, State},
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::Engine as _;
use hodei_authz::{Credentials, VerifiedContext, verify_principal};
use hodei_catalog::{
    CatalogError, PackageCoordinates, Precondition, Publication, RepositoryDescriptor,
    RepositoryFormat, RepositoryMode, UpstreamConfig,
    pypi::{self, FileEntry, ProjectDocument, ProjectName, Route},
};
use hodei_evidence::domain::content::ContentDigest;
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;

use super::raw::{
    RawApiError, apply_common_headers, hex_of, the_admission_error, the_blob_error, the_catalog,
    the_content_store, the_current_asset, the_decision, the_publication_error, the_repository_in,
    write_the_content,
};
use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{bearer_token, correlation_id, record_audit};
use crate::raw::conditional;

/// Cuantas veces se reintenta una escritura del documento que pierde la
/// carrera con otra.
const MAX_ATTEMPTS: usize = 6;

/// Cuantos campos puede traer una subida.
///
/// El `twine` manda unos cuantos: la accion, la version del protocolo, el
/// nombre, la version, el hash, el `requires-python` y el fichero. Un
/// limite alto de sobra y uno bajo delatan a un cliente que no es un
/// cliente.
const MAX_PARTS: usize = 64;

/// Lo que se le dice al cliente cuando se agotan los reintentos.
const CONTENDED: &str = "the project is being updated concurrently; retry it";

/// Lo que pasa con una peticion que no se puede servir.
#[derive(Debug)]
pub enum Failure {
    /// Un error de la superficie compartida.
    Raw(RawApiError),
    /// Un error del protocolo PyPI.
    Pypi(pypi::PypiError),
    /// Un estado y un motivo.
    Plain(StatusCode, String),
}

impl From<RawApiError> for Failure {
    fn from(error: RawApiError) -> Self {
        Self::Raw(error)
    }
}

impl From<pypi::PypiError> for Failure {
    fn from(error: pypi::PypiError) -> Self {
        Self::Pypi(error)
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
                RawApiError::Conflict(reason) => (StatusCode::CONFLICT, (*reason).into()),
                RawApiError::Unavailable => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "the PyPI surface is not available".into(),
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
                    "the upstream did not answer".into(),
                ),
                RawApiError::Offline(reason) => (StatusCode::SERVICE_UNAVAILABLE, (*reason).into()),
                RawApiError::Internal => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "the service failed to answer".into(),
                ),
            },
            Self::Pypi(error) => match error {
                pypi::PypiError::InvalidName(_)
                | pypi::PypiError::InvalidFileName(_)
                | pypi::PypiError::InvalidVersion(_)
                | pypi::PypiError::MissingField(_)
                | pypi::PypiError::MissingFile
                | pypi::PypiError::Mismatch(_)
                | pypi::PypiError::DigestMismatch
                | pypi::PypiError::InvalidPath(_) => (StatusCode::BAD_REQUEST, error.message()),
                pypi::PypiError::FileExists => (
                    StatusCode::CONFLICT,
                    "the distribution is already published in this project".into(),
                ),
                pypi::PypiError::UnknownFile(_) => (StatusCode::NOT_FOUND, "not found".into()),
                pypi::PypiError::InvalidDocument(_) => (
                    StatusCode::BAD_GATEWAY,
                    "the stored project document is not valid".into(),
                ),
            },
            Self::Plain(status, reason) => (*status, reason.clone()),
        }
    }

    /// La longitud del contenido cuando la respuesta es un `416`.
    ///
    /// RFC 9110 pide que la respuesta a un rango que no se puede servir
    /// diga en `Content-Range` cual es la longitud de lo que hay: sin esa
    /// cabecera el cliente no sabe ni cuanto tiene.
    fn unsatisfiable_length(&self) -> Option<u64> {
        match self {
            Self::Raw(RawApiError::RangeNotSatisfiable(length)) => Some(*length),
            _ => None,
        }
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let (status, message) = self.status_and_message();
        let mut response = (status, message).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        );
        if status == StatusCode::UNAUTHORIZED {
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                HeaderValue::from_static("Basic realm=\"hodei\", Bearer"),
            );
        }
        if let Some(length) = self.unsatisfiable_length()
            && let Ok(value) =
                HeaderValue::from_str(&crate::raw::range::the_unsatisfiable_range(length))
        {
            response.headers_mut().insert(header::CONTENT_RANGE, value);
        }
        response
    }
}

/// El repositorio y el resto de la ruta de `/pypi/{repositorio}/{resto}`.
fn the_split(uri: &OriginalUri) -> Result<(String, String), Failure> {
    let rest = uri
        .path()
        .strip_prefix("/pypi/")
        .ok_or(RawApiError::NotFound)?;
    let (repository, rest) = match rest.split_once('/') {
        Some((repository, rest)) => (repository, rest),
        // `/pypi/{repositorio}` sin mas: la subida de `twine`, que publica
        // en la raiz del repositorio.
        None => (rest, ""),
    };
    if repository.is_empty() || !crate::raw::path::valid_repository_name(repository) {
        return Err(RawApiError::NotFound.into());
    }
    Ok((repository.to_string(), rest.to_string()))
}

/// La URL base de este repositorio tal y como la ve el cliente.
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
    Ok(format!("{scheme}://{host}/pypi/{repository}"))
}

/// El prefijo donde viven los ficheros, tal y como lo guarda el documento.
fn the_files_base(repository: &str) -> String {
    format!("/pypi/{}/files", repository)
}

/// La credencial de la peticion.
///
/// La cabecera `Bearer` lleva la credencial de la plataforma. Las
/// credenciales basicas tambien, en la **contrasena**: el usuario es
/// informativo, y es como las lleva el protocolo de Docker.
fn the_credential(headers: &HeaderMap) -> Option<String> {
    if let Some(token) = bearer_token(headers) {
        return Some(token);
    }
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let encoded = value
        .strip_prefix("Basic ")
        .or_else(|| value.strip_prefix("basic "))?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (_user, password) = decoded.split_once(':')?;
    if password.is_empty() {
        return None;
    }
    Some(password.to_string())
}

/// La identidad verificada de quien pide.
async fn the_pypi_context(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<VerifiedContext, Failure> {
    let credential = the_credential(headers).ok_or(RawApiError::Unauthorized)?;
    let principal = match verify_principal(
        state.authenticator.as_ref(),
        &Credentials::bearer(credential),
    )
    .await
    {
        Ok(principal) => principal,
        Err(hodei_authz::AuthenticationError::ProviderUnavailable(_)) => {
            return Err(RawApiError::Unavailable.into());
        }
        Err(_) => return Err(RawApiError::Unauthorized.into()),
    };
    Ok(VerifiedContext::from_principal(principal))
}

/// Lo que la peticion necesita saber de si misma.
struct Ask<'a> {
    /// Si lo que se pide es solo la cabeza de la respuesta.
    head: bool,
    /// Las cabeceras de la peticion.
    headers: &'a HeaderMap,
    /// La correlacion de la peticion.
    correlation: &'a str,
}

/// Un fichero de un proyecto y el repositorio del que sale.
struct FileView {
    /// Lo que el indice sabe del fichero.
    entry: FileEntry,
    /// El repositorio que lo tiene.
    owner: RepositoryDescriptor,
}

/// El documento de un proyecto, con la procedencia de cada fichero.
struct ProjectView {
    /// El nombre normalizado del proyecto.
    name: String,
    /// Los ficheros, en el orden en que se publican.
    files: Vec<FileView>,
}

/// Cualquier peticion de `/pypi/{repositorio}/...`.
pub async fn handle(
    State(state): State<AppState>,
    method: Method,
    uri: OriginalUri,
    headers: HeaderMap,
    request: Request<Body>,
) -> Result<Response, Failure> {
    // La autenticacion es la primera puerta, como en el resto de superficies.
    let context = the_pypi_context(&state, &headers).await?;
    let correlation = correlation_id(&headers, &state.identity);
    let (repository_key, rest) = the_split(&uri)?;
    let route = pypi::route(&rest)?;
    let repository =
        the_repository_in(&state, &context, &repository_key, &[RepositoryFormat::PyPi]).await?;

    let reading = method == Method::GET || method == Method::HEAD;
    match (&route, reading) {
        (Route::Root, true) => {
            let resource = format!("{repository_key}/simple/");
            let outcome = the_root(&state, &context, &repository, &headers, &correlation).await;
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
        (Route::Project { project }, true) => {
            let resource = format!("{repository_key}/simple/{}/", project.as_str());
            let outcome = the_project_page(
                &state,
                &context,
                &repository,
                project,
                &Ask {
                    head: method == Method::HEAD,
                    headers: &headers,
                    correlation: &correlation,
                },
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
        (Route::File { project, filename }, true) => {
            let resource = format!("{repository_key}/files/{project}/{filename}");
            let outcome = the_file(
                &state,
                &context,
                &repository,
                project,
                filename,
                &Ask {
                    head: method == Method::HEAD,
                    headers: &headers,
                    correlation: &correlation,
                },
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
        (Route::Upload, false) if method == Method::POST => {
            let resource = format!("{repository_key}/");
            let outcome = upload(&state, &context, &repository, request, &correlation).await;
            let result = match &outcome {
                Ok(_) => AuditResult::Allowed,
                Err(Failure::Raw(RawApiError::Forbidden)) => AuditResult::Denied,
                Err(_) => AuditResult::Failed,
            };
            record_audit(
                &state,
                context.principal().subject().as_str(),
                context.account().as_str(),
                "upload",
                &resource,
                result,
                &correlation,
            );
            outcome
        }
        (Route::Yank, false) if method == Method::POST => {
            let resource = format!("{repository_key}/yank");
            let outcome = yank(&state, &context, &repository, request, &correlation).await;
            let result = match &outcome {
                Ok(_) => AuditResult::Allowed,
                Err(Failure::Raw(RawApiError::Forbidden)) => AuditResult::Denied,
                Err(_) => AuditResult::Failed,
            };
            record_audit(
                &state,
                context.principal().subject().as_str(),
                context.account().as_str(),
                "yank",
                &resource,
                result,
                &correlation,
            );
            outcome
        }
        (Route::Upload, _) | (Route::Yank, _) => Err(Failure::plain(
            StatusCode::METHOD_NOT_ALLOWED,
            "this PyPI operation is not supported",
        )),
        _ => Err(RawApiError::MethodNotAllowed("this PyPI operation is not supported").into()),
    }
}

// ---------------------------------------------------------------------------
// Lectura
// ---------------------------------------------------------------------------

/// El indice de los proyectos del repositorio.
///
/// Un proxy no puede enumerar lo que no conoce: su indice va vacio, y el
/// cliente pregunta por el proyecto que quiere. Un hosted y un grupo si
/// pueden: el primero tiene sus assets, y el segundo los de sus miembros.
async fn the_root(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    headers: &HeaderMap,
    correlation: &str,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::download_asset(), repository)?;
    let projects = the_projects(state, context, repository, correlation).await?;
    // Los proyectos del repositorio, en orden.
    let base = the_base(headers, &repository.repo_key)?;
    let representation = pypi::negotiate(headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()));
    let (body, content_type) = match representation {
        pypi::Representation::Json => {
            let value = pypi::render_root_json(&projects);
            let bytes = serde_json::to_vec(&value).map_err(|_| RawApiError::Internal)?;
            (bytes, pypi::JSON_CONTENT_TYPE)
        }
        pypi::Representation::Html => (
            pypi::render_root_html(&projects, &base).into_bytes(),
            "text/html; charset=utf-8",
        ),
    };
    Ok(the_page(body, content_type, None, correlation))
}

/// Los proyectos del repositorio, en orden.
async fn the_projects(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    correlation: &str,
) -> Result<Vec<String>, Failure> {
    match repository.mode {
        RepositoryMode::Group => {
            let mut projects: Vec<String> = Vec::new();
            // Los miembros de un grupo son hojas: ninguno es otro grupo.
            for member in super::raw_group::the_leaves(state, context, repository).await? {
                for project in the_leaf_projects(state, context, &member, correlation).await? {
                    if !projects.contains(&project) {
                        projects.push(project);
                    }
                }
            }
            projects.sort();
            Ok(projects)
        }
        _ => the_leaf_projects(state, context, repository, correlation).await,
    }
}

/// Los proyectos de un repositorio que no es un grupo.
async fn the_leaf_projects(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    correlation: &str,
) -> Result<Vec<String>, Failure> {
    if repository.mode == RepositoryMode::Proxy {
        // Lo que hay en el upstream no se puede enumerar sin pedirlo
        // proyecto a proyecto.
        let _ = correlation;
        return Ok(Vec::new());
    }
    let (reader, _) = the_catalog(state)?;
    let assets = reader
        .assets_under(context.account().as_str(), &repository.repo_key, "")
        .await
        .map_err(|error| the_publication_error(&error))?;
    let mut projects: Vec<String> = assets
        .iter()
        .map(|asset| asset.asset_path.clone())
        // Un documento de proyecto no tiene separador; las
        // distribuciones si.
        .filter(|path| !path.contains('/'))
        .collect();
    projects.sort();
    projects.dedup();
    Ok(projects)
}

/// La pagina de un proyecto.
async fn the_project_page(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    project: &ProjectName,
    ask: &Ask<'_>,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::download_asset(), repository)?;
    let view = the_project(state, context, repository, project, ask.correlation).await?;
    let document = ProjectDocument {
        name: view.name.clone(),
        files: view.files.iter().map(|file| file.entry.clone()).collect(),
    };
    let base = the_base(ask.headers, &repository.repo_key)?;
    let representation = pypi::negotiate(
        ask.headers
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok()),
    );
    let (body, content_type) = match representation {
        pypi::Representation::Json => {
            let value = pypi::render_project_json(&document, &base);
            let bytes = serde_json::to_vec(&value).map_err(|_| RawApiError::Internal)?;
            (bytes, pypi::JSON_CONTENT_TYPE)
        }
        pypi::Representation::Html => (
            pypi::render_project_html(&document, &base).into_bytes(),
            "text/html; charset=utf-8",
        ),
    };
    let digest = ContentDigest::of_bytes(&body);
    let etag = conditional::the_etag(&digest);
    match conditional::the_read_condition(
        conditional::the_if_match(ask.headers).as_deref(),
        conditional::the_if_none_match(ask.headers).as_deref(),
        Some(&etag),
    ) {
        conditional::ReadOutcome::Failed => return Err(RawApiError::PreconditionFailed.into()),
        conditional::ReadOutcome::NotModified => {
            let mut response = StatusCode::NOT_MODIFIED.into_response();
            apply_common_headers(&mut response, &etag, ask.correlation);
            return Ok(response);
        }
        conditional::ReadOutcome::Serve => {}
    }
    let body = if ask.head { Vec::new() } else { body };
    Ok(the_page(body, content_type, Some(&etag), ask.correlation))
}

/// El documento de un proyecto, con la procedencia de cada fichero.
async fn the_project(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    project: &ProjectName,
    correlation: &str,
) -> Result<ProjectView, Failure> {
    match repository.mode {
        RepositoryMode::Group => {
            let mut files: Vec<FileView> = Vec::new();
            for member in super::raw_group::the_leaves(state, context, repository).await? {
                // Un miembro que no tiene el proyecto no es un fallo: se
                // sigue con el siguiente.
                let view =
                    match the_leaf_project(state, context, &member, project, correlation).await {
                        Ok(view) => view,
                        Err(Failure::Raw(RawApiError::NotFound)) => continue,
                        Err(other) => return Err(other),
                    };
                for file in view.files {
                    // Gana el primer miembro que tiene el fichero.
                    if !files
                        .iter()
                        .any(|known| known.entry.filename == file.entry.filename)
                    {
                        files.push(file);
                    }
                }
            }
            if files.is_empty() {
                return Err(RawApiError::NotFound.into());
            }
            Ok(ProjectView {
                name: project.as_str().to_string(),
                files,
            })
        }
        _ => the_leaf_project(state, context, repository, project, correlation).await,
    }
}

/// El documento de un repositorio que no es un grupo.
async fn the_leaf_project(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    project: &ProjectName,
    correlation: &str,
) -> Result<ProjectView, Failure> {
    if repository.mode == RepositoryMode::Proxy {
        return the_proxy_project(state, context, repository, project, correlation).await;
    }
    let asset_path = project.as_str();
    let asset = the_current_asset(state, context, &repository.repo_key, asset_path)
        .await?
        .ok_or(RawApiError::NotFound)?;
    let bytes = the_content_store(state)?
        .get(&asset.digest)
        .await
        .map_err(|error| the_blob_error(&error))?;
    let document = the_document(&bytes)?;
    let files = the_admitted_files(state, context, repository, document.files).await?;
    Ok(ProjectView {
        name: document.name,
        files: files
            .into_iter()
            .map(|entry| FileView {
                entry,
                owner: repository.clone(),
            })
            .collect(),
    })
}

/// Los ficheros que el canal de confianza deja servir.
///
/// El canal abierto no filtra nada. El canal `trusted` deja servir solo los
/// digests con concesion activa, y el indice no anuncia lo que no se puede
/// descargar: es lo mismo que hace la superficie OCI con sus listados.
async fn the_admitted_files(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    files: Vec<FileEntry>,
) -> Result<Vec<FileEntry>, Failure> {
    match crate::trust::servable(state, context.account().as_str(), repository).await {
        Ok(None) => Ok(files),
        Ok(Some(allowed)) => Ok(files
            .into_iter()
            .filter(|file| allowed.contains(&file.sha256))
            .collect()),
        Err(error) => Err(the_admission_error(error).into()),
    }
}

/// El documento de un proyecto tal y como lo publico el upstream.
async fn the_proxy_project(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    project: &ProjectName,
    correlation: &str,
) -> Result<ProjectView, Failure> {
    let upstream = the_upstream(repository)?;
    let asset_path = project.as_str();
    let url = upstream.target(asset_path).ok_or(RawApiError::NotFound)?;
    let served = super::raw::the_proxy_asset_at(
        state,
        context,
        repository,
        asset_path,
        Some(url),
        Some(pypi::JSON_CONTENT_TYPE),
        correlation,
    )
    .await?;
    let bytes = the_content_store(state)?
        .get(&served.asset.digest)
        .await
        .map_err(|error| the_blob_error(&error))?;
    let document = the_document(&bytes)?;
    let files = the_admitted_files(state, context, repository, document.files).await?;
    let files = files
        .into_iter()
        .filter_map(|entry| {
            // Un upstream puede declarar un nombre que no es una
            // distribucion. No se anuncia: un enlace que no se puede
            // descargar es un enlace roto, y el indice no los publica.
            pypi::Distribution::parse(&entry.filename).ok()?;
            // La URL del upstream puede ser relativa: es relativa a su base.
            let entry = match entry.source.as_deref() {
                Some(source) if !source.contains("://") => {
                    let source = upstream
                        .base
                        .join(source)
                        .map(|url| url.to_string())
                        .unwrap_or_else(|_| source.to_string());
                    FileEntry {
                        source: Some(source),
                        ..entry
                    }
                }
                _ => entry,
            };
            Some(FileView {
                entry,
                owner: repository.clone(),
            })
        })
        .collect();
    Ok(ProjectView {
        // El nombre del upstream puede venir sin normalizar: el indice
        // lo dice como lo dice PEP 503, o no lo dice.
        name: ProjectName::parse(&document.name)
            .map(|name| name.as_str().to_string())
            .unwrap_or_else(|_| project.as_str().to_string()),
        files,
    })
}

/// Lee un documento de proyecto.
fn the_document(bytes: &[u8]) -> Result<ProjectDocument, Failure> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| RawApiError::BadGateway("the stored project document is not valid"))?;
    pypi::ProjectDocument::from_value(&value)
        .map_err(|_| RawApiError::BadGateway("the stored project document is not valid"))
        .map_err(Failure::Raw)
}

/// La distribucion de un proyecto.
async fn the_file(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    project: &ProjectName,
    filename: &str,
    ask: &Ask<'_>,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::download_asset(), repository)?;
    let view = the_project(state, context, repository, project, ask.correlation).await?;
    let file = view
        .files
        .iter()
        .find(|file| file.entry.filename == filename)
        .ok_or(RawApiError::NotFound)?;
    let asset_path = format!("{project}/{filename}");
    let asset = the_distribution(
        state,
        context,
        &file.owner,
        &asset_path,
        file,
        ask.correlation,
    )
    .await?;
    // Lo que se sirve tiene que ser lo que el indice promised.
    if asset.digest.value != file.entry.sha256 {
        return Err(RawApiError::BadGateway(
            "the distribution does not match the hash the index published",
        )
        .into());
    }
    let etag = conditional::the_etag(&asset.digest);
    match conditional::the_read_condition(
        conditional::the_if_match(ask.headers).as_deref(),
        conditional::the_if_none_match(ask.headers).as_deref(),
        Some(&etag),
    ) {
        conditional::ReadOutcome::Failed => return Err(RawApiError::PreconditionFailed.into()),
        conditional::ReadOutcome::NotModified => {
            let mut response = StatusCode::NOT_MODIFIED.into_response();
            apply_common_headers(&mut response, &etag, ask.correlation);
            return Ok(response);
        }
        conditional::ReadOutcome::Serve => {}
    }
    let store = the_content_store(state)?;
    let length = asset.size_bytes;
    // El tramo se sirve como lo sirve la superficie cruda: un `Range` que
    // se entiende es un 206 con su `Content-Range`, y uno que no se
    // entiende se ignora. El `pip` de hoy pide tramos para no bajar dos
    // veces lo mismo.
    let request = match crate::raw::range::the_range(
        ask.headers
            .get(header::RANGE)
            .and_then(|value| value.to_str().ok()),
        length,
    ) {
        Ok(request) => request,
        Err(crate::raw::range::RangeError::Unsatisfiable) => {
            return Err(RawApiError::RangeNotSatisfiable(length).into());
        }
    };
    let (body, status, content_range, response_length) = match request {
        crate::raw::range::RangeRequest::Whole => {
            if ask.head {
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
        crate::raw::range::RangeRequest::Single { first, last } => {
            let served = last - first + 1;
            let content_range = crate::raw::range::the_content_range(first, last, length);
            if ask.head {
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
    apply_common_headers(&mut response, &etag, ask.correlation);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response.headers_mut().insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    if let Some(content_range) = content_range
        && let Ok(value) = HeaderValue::from_str(&content_range)
    {
        response.headers_mut().insert(header::CONTENT_RANGE, value);
    }
    if ask.head
        && let Ok(value) = HeaderValue::from_str(&response_length.to_string())
    {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    Ok(response)
}

/// El asset de una distribucion, de donde venga.
///
/// En un hosted esta en el catalogo. En un proxy se trae del upstream por
/// la URL que el propio upstream declara, y se guarda para no volver a
/// pedirlo.
async fn the_distribution(
    state: &AppState,
    context: &VerifiedContext,
    owner: &RepositoryDescriptor,
    asset_path: &str,
    file: &FileView,
    correlation: &str,
) -> Result<hodei_catalog::AssetRecord, Failure> {
    if owner.mode == RepositoryMode::Proxy {
        let Some(source) = file.entry.source.as_deref() else {
            return Err(RawApiError::BadGateway(
                "the upstream does not say where the distribution is",
            )
            .into());
        };
        let url = Url::parse(source)
            .map_err(|_| RawApiError::BadGateway("the upstream declares an unusable URL"))?;
        let served = super::raw::the_proxy_asset_at(
            state,
            context,
            owner,
            asset_path,
            Some(url),
            None,
            correlation,
        )
        .await?;
        // La compuerta de confianza va tambien aqui: el software que
        // trae el upstream es software, y lo que el canal no admite no se
        // sirve ni porque venga de fuera.
        crate::trust::admit(
            state,
            context.account().as_str(),
            owner,
            &served.asset.digest,
        )
        .await
        .map_err(the_admission_error)?;
        return Ok(served.asset);
    }
    let asset = the_current_asset(state, context, &owner.repo_key, asset_path)
        .await?
        .ok_or(RawApiError::NotFound)?;
    // La compuerta de confianza va antes de servir: lo que no se admite es
    // un 404, como lo que no existe.
    crate::trust::admit(state, context.account().as_str(), owner, &asset.digest)
        .await
        .map_err(the_admission_error)?;
    Ok(asset)
}

/// El upstream de un repositorio proxy.
fn the_upstream(repository: &RepositoryDescriptor) -> Result<UpstreamConfig, Failure> {
    UpstreamConfig::of(&repository.config)
        .ok()
        .flatten()
        .ok_or_else(|| Failure::plain(StatusCode::BAD_GATEWAY, "the proxy is not configured"))
}

/// Una respuesta con el cuerpo ya listo.
fn the_page(
    body: Vec<u8>,
    content_type: &'static str,
    etag: Option<&str>,
    correlation: &str,
) -> Response {
    let length = body.len();
    let mut response = (StatusCode::OK, Body::from(body)).into_response();
    if let Some(etag) = etag {
        apply_common_headers(&mut response, etag, correlation);
    }
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response.headers_mut().insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    if let Ok(value) = HeaderValue::from_str(&length.to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    response
}

// ---------------------------------------------------------------------------
// Escritura
// ---------------------------------------------------------------------------

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

/// Lo que dejo una escritura del documento.
struct Written {
    /// El digest del documento nuevo.
    digest: ContentDigest,
    /// Si el documento cambio.
    changed: bool,
}

/// Aplica un cambio al documento de un proyecto y lo publica.
///
/// El cambio se **reevalua** contra lo que hay en cada intento: si otro
/// escritor movio el puntero entre la lectura y la escritura, el catalogo
/// rechaza la precondicion, se relee y se vuelve a aplicar.
async fn update_document<F>(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    project: &ProjectName,
    create: bool,
    correlation: &str,
    mut change: F,
) -> Result<Written, Failure>
where
    F: FnMut(&mut ProjectDocument) -> Result<bool, Failure>,
{
    let (_, writer) = the_catalog(state)?;
    let store = the_content_store(state)?;
    for _ in 0..MAX_ATTEMPTS {
        let current =
            the_current_asset(state, context, &repository.repo_key, project.as_str()).await?;
        let (mut document, precondition) = match &current {
            Some(asset) => {
                let bytes = store
                    .get(&asset.digest)
                    .await
                    .map_err(|error| the_blob_error(&error))?;
                let value: Value =
                    serde_json::from_slice(&bytes).map_err(|_| RawApiError::Internal)?;
                let document =
                    pypi::ProjectDocument::from_value(&value).map_err(|_| RawApiError::Internal)?;
                (document, Precondition::Present(vec![asset.digest.clone()]))
            }
            None if create => (ProjectDocument::new(project.as_str()), Precondition::Absent),
            None => return Err(RawApiError::NotFound.into()),
        };
        let changed = change(&mut document)?;
        if !changed {
            return Ok(Written {
                digest: current
                    .map(|asset| asset.digest)
                    .unwrap_or(ContentDigest::sha256("")),
                changed: false,
            });
        }
        let bytes = serde_json::to_vec(&document.to_value(&the_files_base(&repository.repo_key)))
            .map_err(|_| RawApiError::Internal)?;
        let (digest, length) = write_the_content(state, Body::from(bytes), None, None).await?;
        let publication = Publication {
            repository: repository.repo_key.clone(),
            asset_path: project.as_str().to_string(),
            // Cada instantanea es una version distinta del documento: la
            // identifica su contenido, y la anterior sigue estando.
            coordinates: PackageCoordinates {
                package_name: format!("{}{}", project.as_str(), pypi::PROJECT_COORDINATES_SUFFIX),
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

/// Que el indice del proyecto pueda guardar esta entrada.
///
/// El documento del proyecto tiene un tope: el del contenido que acepta
/// este servicio. Si la entrada no cabe, se dice **antes** de publicar la
/// distribucion. El tamano se mira con el documento que hay ahora, y el
/// documento que se escriba despues lo comprueba otra vez: entre las dos
/// cosas puede haber cambiado el mundo, y lo que manda es el segundo
/// control.
async fn the_index_fits(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    project: &ProjectName,
    entry: &FileEntry,
) -> Result<(), Failure> {
    let mut document =
        match the_current_asset(state, context, &repository.repo_key, project.as_str()).await? {
            Some(asset) => {
                let bytes = the_content_store(state)?
                    .get(&asset.digest)
                    .await
                    .map_err(|error| the_blob_error(&error))?;
                let value: Value =
                    serde_json::from_slice(&bytes).map_err(|_| RawApiError::Internal)?;
                pypi::ProjectDocument::from_value(&value).map_err(|_| RawApiError::Internal)?
            }
            None => pypi::ProjectDocument::new(project.as_str()),
        };
    if document.file(&entry.filename).is_some() {
        // Ya estaba: el documento no crece.
        return Ok(());
    }
    document
        .add(entry.clone())
        .map_err(|_| RawApiError::Internal)?;
    let bytes = serde_json::to_vec(&document.to_value(&the_files_base(&repository.repo_key)))
        .map_err(|_| RawApiError::Internal)?;
    if bytes.len() > state.max_body_size {
        return Err(RawApiError::PayloadTooLarge.into());
    }
    Ok(())
}

/// `POST /pypi/{repositorio}/`: la subida de `twine`.
async fn upload(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    request: Request<Body>,
    correlation: &str,
) -> Result<Response, Failure> {
    the_write_gate(state, context, repository)?;
    let (fields, file) = the_multipart(state, request).await?;
    let (filename, bytes) = file.ok_or(pypi::PypiError::MissingFile)?;
    // El hash se calcula aqui: lo que declara el cliente se comprueba
    // contra los bytes que han llegado.
    let sha256 = hex_of(&Sha256::digest(&bytes));
    let upload = pypi::parse_upload(&fields, &filename, &sha256)?;

    let asset_path = format!("{}/{}", upload.project.as_str(), upload.filename);
    let entry = FileEntry {
        filename: upload.filename.clone(),
        sha256: upload.sha256.clone(),
        size: Some(bytes.len() as u64),
        requires_python: upload.requires_python.clone(),
        yanked: None,
        source: None,
    };
    // El indice del proyecto tiene que caber: si no cabe, se dice antes
    // de publicar la distribucion. Publicar y luego no poder anunciar
    // deja el contenido publicado y invisible.
    the_index_fits(state, context, repository, &upload.project, &entry).await?;

    let (digest, length) = write_the_content(state, Body::from(bytes), None, None).await?;
    let (_, writer) = the_catalog(state)?;
    let publication = Publication {
        repository: repository.repo_key.clone(),
        asset_path: asset_path.clone(),
        coordinates: PackageCoordinates {
            // Una version de PyPI puede tener mas de un fichero--el wheel
            // y el sdist-- y cada uno es contenido distinto. Por eso el
            // nombre de las coordenadas es el del fichero: dos
            // distribuciones de la misma version no son la misma version
            // publicada.
            package_name: asset_path.clone(),
            package_version: upload.version.clone(),
        },
        digest: digest.clone(),
        size_bytes: length,
        tags: Vec::new(),
        actor: context.principal().subject().as_str().to_string(),
        correlation_id: correlation.to_string(),
        // Una distribucion es inmutable: publicarla otra vez con otros
        // bytes es un conflicto, no una sustitucion.
        overwrite: false,
        precondition: None,
        subject: None,
    };
    match writer
        .publish(context.account().as_str(), publication)
        .await
    {
        Ok(_) => {}
        Err(CatalogError::Conflict { .. }) => {
            // El mismo contenido otra vez no es un problema: es lo mismo.
            let existing =
                the_current_asset(state, context, &repository.repo_key, &asset_path).await?;
            if existing.is_none_or(|asset| asset.digest != digest) {
                return Err(pypi::PypiError::FileExists.into());
            }
        }
        Err(error) => return Err(the_publication_error(&error).into()),
    }

    let written = update_document(
        state,
        context,
        repository,
        &upload.project,
        true,
        correlation,
        |document| {
            if document.file(&entry.filename).is_some() {
                // Ya estaba: el documento no cambia.
                return Ok(false);
            }
            document.add(entry.clone())?;
            Ok(true)
        },
    )
    .await?;

    let body = serde_json::json!({
        "ok": true,
        "project": upload.project.as_str(),
        "version": upload.version,
        "filename": upload.filename,
        "sha256": upload.sha256,
        "changed": written.changed,
    });
    let mut response = (StatusCode::OK, axum::Json(body)).into_response();
    apply_common_headers(&mut response, &conditional::the_etag(&digest), correlation);
    Ok(response)
}

/// Los campos y el fichero de una subida.
///
/// El limite es el del contenido: una distribucion que no cabe no se
/// publica, y se dice antes de gastarla. El limite cuenta **todos** los
/// bytes de la subida, no los de un campo: el `twine` manda unos cuantos
/// campos de texto y el fichero, y un cliente que manda mil campos
/// vacios no gasta memoria.
async fn the_multipart(
    state: &AppState,
    request: Request<Body>,
) -> Result<(Vec<(String, String)>, Option<(String, Vec<u8>)>), Failure> {
    let mut multipart = axum::extract::Multipart::from_request(request, &state)
        .await
        .map_err(|_| {
            Failure::plain(
                StatusCode::BAD_REQUEST,
                "the upload is not a multipart form",
            )
        })?;
    let limit = state.max_body_size;
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut file: Option<(String, Vec<u8>)> = None;
    let mut total = 0usize;
    let mut parts = 0usize;
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|error| the_upload_error(error.status()))?
    {
        parts += 1;
        if parts > MAX_PARTS {
            return Err(Failure::plain(
                StatusCode::BAD_REQUEST,
                "the upload carries too many fields",
            ));
        }
        let name = field.name().unwrap_or_default().to_string();
        let filename = field.file_name().map(str::to_string);
        let mut bytes: Vec<u8> = Vec::new();
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|error| the_upload_error(error.status()))?
        {
            total += chunk.len();
            if total > limit {
                return Err(RawApiError::PayloadTooLarge.into());
            }
            bytes.extend_from_slice(&chunk);
        }
        match filename {
            Some(filename) => {
                if name != "content" {
                    // Una parte con nombre de fichero que no es la
                    // distribucion--la firma del `twine upload --sign`--
                    // no es una segunda distribucion: se ignora. Esta
                    // superficie no guarda firmas.
                    continue;
                }
                if file.is_some() {
                    // Dos distribuciones en una subida: no se elige una, se
                    // dice que no.
                    return Err(Failure::plain(
                        StatusCode::BAD_REQUEST,
                        "the upload carries more than one distribution",
                    ));
                }
                file = Some((filename, bytes));
            }
            None => fields.push((name, String::from_utf8_lossy(&bytes).into_owned())),
        }
    }
    Ok((fields, file))
}

/// El error de una subida que no se puede leer.
///
/// Un cuerpo que pasa del tope no es un formulario malo: es un cuerpo
/// demasiado grande, y el `413` lo dice el propio `axum`. Es el mismo
/// `413` que dice el manejador cuando lo que se pasa del tope son los
/// bytes del fichero: lo que el cliente necesita saber es que no cabe, y
/// no de que capa viene el rechazo.
fn the_upload_error(status: StatusCode) -> Failure {
    if status == StatusCode::PAYLOAD_TOO_LARGE {
        return RawApiError::PayloadTooLarge.into();
    }
    Failure::plain(StatusCode::BAD_REQUEST, "the upload cannot be read")
}

/// `POST /pypi/{repositorio}/yank`: retira una version.
///
/// Es una extension propia: PyPI no tiene una API para esto, y un cliente
/// de linea de comandos no la tiene. Lo que hace es lo que dice PEP 592:
/// la version deja de ser elegible, y sigue downloadable si se pide
/// exactamente.
async fn yank(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    request: Request<Body>,
    correlation: &str,
) -> Result<Response, Failure> {
    the_write_gate(state, context, repository)?;
    let body = axum::body::to_bytes(request.into_body(), 64 * 1024)
        .await
        .map_err(|_| RawApiError::BadRequest("the body cannot be read"))?;
    let value: Value = serde_json::from_slice(&body)
        .map_err(|_| RawApiError::BadRequest("the body is not a JSON object"))?;
    let project = value
        .get("project")
        .and_then(Value::as_str)
        .ok_or(RawApiError::BadRequest("the project is missing"))?;
    let project = ProjectName::parse(project)
        .map_err(|_| RawApiError::BadRequest("the project name is not valid"))?;
    let version = value
        .get("version")
        .and_then(Value::as_str)
        .ok_or(RawApiError::BadRequest("the version is missing"))?;
    // La version se canoniza: el documento guarda las versiones como las
    // llevan los nombres de los ficheros, y esas son canonicas.
    let Some(version) = pypi::canonical_version(version) else {
        return Err(RawApiError::BadRequest("the version is not valid").into());
    };
    let yanked = value.get("yanked").and_then(Value::as_bool).unwrap_or(true);
    let reason = value
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let written = update_document(
        state,
        context,
        repository,
        &project,
        false,
        correlation,
        |document| {
            let yanked = if yanked { Some(reason.clone()) } else { None };
            Ok(document.set_yanked(&version, yanked))
        },
    )
    .await?;
    let body = serde_json::json!({
        "ok": true,
        "project": project.as_str(),
        "version": version,
        "yanked": yanked,
        "changed": written.changed,
    });
    let mut response = (StatusCode::OK, axum::Json(body)).into_response();
    apply_common_headers(
        &mut response,
        &conditional::the_etag(&written.digest),
        correlation,
    );
    Ok(response)
}
