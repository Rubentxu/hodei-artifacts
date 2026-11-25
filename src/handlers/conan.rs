//! La superficie **conan**: `conan upload` y la resolución de
//! `conan install` de un cliente Conan 2 contra un repositorio de Hodei
//! usado como remote.
//!
//! Vive en `/conan/{repositorio}/...` y habla el dialecto del remote de
//! Conan 2 con revisiones: la tabla de rutas y los documentos los fija
//! `hodei_catalog::conan` (pura, contrastada con el cliente 2.20.1), y
//! aquí solo hay efectos.
//!
//! Cada fichero que sube el cliente deja **un asset inmutable** cuya ruta
//! lleva su recipe revision, su `package_id` y su package revision:
//! `recipes/{n}/{v}/{u}/{c}/{rrev}/{file}` y
//! `packages/{n}/{v}/{u}/{c}/{rrev}/{pid}/{prev}/{file}`. Los binarios de
//! dos `package_id` del mismo recipe viven bajo prefijos distintos, y las
//! revisiones son identificadores versionados que el servidor conserva
//! opacos: nada deriva `package_id` del nombre ni iguala versión con
//! binario.
//!
//! La modalidad servida hoy es **hosted**: el proxy de conan necesita un
//! upstream validado con cliente real antes de poder declararse, y un
//! repositorio cuyo modo no se sirve no se puede ni crear.
//!
//! La autenticación es la del hub: el cliente pide token con Basic en
//! `/v2/users/authenticate`, y el hub verifica la contraseña **como
//! credencial bearer** y la devuelve como token. El token que el cliente
//! guarda y manda como `Bearer` es una credencial verificable del
//! principal, así que el scoping por cuenta es el del hub, no uno propio.

use axum::{
    body::Body,
    extract::{OriginalUri, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::Engine;
use chrono::{DateTime, Utc};
use hodei_authz::{Credentials, VerifiedContext, verify_principal};
use hodei_catalog::{
    AssetRecord, CatalogError, PackageCoordinates, Publication, RepositoryDescriptor,
    RepositoryFormat,
    conan::{self, RecipeRef, Route},
};
use serde_json::{Value, json};

use super::raw::{
    RawApiError, Resolved, apply_cache_headers, apply_common_headers, the_admission_error,
    the_blob_error, the_content_store, the_context, the_decision, the_publication_error,
    the_repository_in, the_resolved_asset, write_the_content,
};
use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{correlation_id, record_audit};

/// Lo que pasa con una petición que no se puede servir.
#[derive(Debug)]
pub enum Failure {
    /// Un error de la superficie compartida.
    Raw(RawApiError),
    /// Un estado y un motivo.
    Plain(StatusCode, String),
}

impl From<RawApiError> for Failure {
    fn from(error: RawApiError) -> Self {
        Self::Raw(error)
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Raw(error) => error.the_verdict(),
            Self::Plain(status, message) => (status, message),
        };
        let mut response = (status, axum::Json(json!({ "error": message }))).into_response();
        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Basic"));
        }
        response
    }
}

/// La cabecera de capacidades: lo único que el cliente lee del ping.
///
/// El dialecto lo fija el cliente: sin la cabecera, el remote "no parece
/// un Conan remote" y el cliente se niega. `checksum_deploy` no se
/// promete: la deduplicación de Artifactory es otro dialecto de subida
/// que aquí no se habla.
const CAPABILITIES: &str = "revisions";

/// El resto de la ruta de `/conan/{repositorio}/{resto}`, sin decodificar:
/// las reglas del protocolo son las únicas que pueden interpretar un `%2f`.
fn the_split(uri: &OriginalUri) -> Result<(String, String), Failure> {
    let rest = uri
        .path()
        .strip_prefix("/conan/")
        .ok_or(RawApiError::NotFound)?;
    let (repository, rest) = rest.split_once('/').ok_or(RawApiError::NotFound)?;
    if !crate::raw::path::valid_repository_name(repository) || rest.is_empty() {
        return Err(RawApiError::NotFound.into());
    }
    Ok((repository.to_string(), rest.to_string()))
}

/// Cualquier petición de `/conan/{repositorio}/...`.
pub async fn handle(
    State(state): State<AppState>,
    method: Method,
    uri: OriginalUri,
    headers: HeaderMap,
    request: axum::extract::Request<Body>,
) -> Result<Response, Failure> {
    let correlation = correlation_id(&headers, &state.identity);
    let (repository_key, rest) = the_split(&uri)?;
    let Some(route) = conan::parse(&rest) else {
        return Err(RawApiError::NotFound.into());
    };

    // El ping no tiene recurso ni cuenta: es la presentación del remote, y
    // el cliente la pide antes de tener token. Sin él no hay remote.
    if route == Route::Ping {
        return Ok(ping_response(&correlation));
    }

    // La autenticación es del remote, no de un repositorio: el cliente
    // cambia su contraseña por un token antes de saber qué repositorios
    // existen, y el check_credentials comprueba un token, no un recurso.
    match route {
        Route::Authenticate => return the_token(&state, &headers, &correlation).await,
        Route::CheckCredentials => {
            the_context(&state, &headers).await?;
            return Ok(text_response(StatusCode::OK, "", &correlation));
        }
        _ => {}
    }

    let context = the_context(&state, &headers).await?;
    let repository = the_repository_in(
        &state,
        &context,
        &repository_key,
        &[RepositoryFormat::Conan],
    )
    .await?;

    let reading = method == Method::GET || method == Method::HEAD;
    let publishing = method == Method::PUT;
    match (&route, reading, publishing) {
        (_, true, _) => {
            the_decision(
                &state,
                &context,
                crate::actions::download_asset(),
                &repository,
            )?;
            read_route(
                &state,
                &context,
                &repository,
                route,
                uri.query().unwrap_or_default(),
                method == Method::HEAD,
                &correlation,
            )
            .await
        }
        (Route::RecipeFile(_, _, _) | Route::PackageFile(_, _, _, _, _), false, true) => {
            let outcome = async {
                the_decision(
                    &state,
                    &context,
                    crate::actions::upload_asset(),
                    &repository,
                )?;
                if !repository.mode.accepts_client_writes() {
                    return Err(RawApiError::MethodNotAllowed(
                        "this repository does not accept uploads",
                    )
                    .into());
                }
                put_file(&state, &context, &repository, route, request, &correlation).await
            }
            .await;
            // Every write leaves a trace, accepted or not.
            let result = match &outcome {
                Ok(_) => AuditResult::Allowed,
                Err(Failure::Raw(RawApiError::Forbidden)) => AuditResult::Denied,
                Err(_) => AuditResult::Failed,
            };
            record_audit(
                &state,
                context.principal().subject().as_str(),
                context.account().as_str(),
                "publish",
                &format!("{repository_key}/{rest}"),
                result,
                &correlation,
            );
            outcome
        }
        _ => Err(RawApiError::MethodNotAllowed("this conan operation is not supported").into()),
    }
}

/// El ping: el cliente lee las capacidades de la cabecera y no otra cosa.
fn ping_response(correlation: &str) -> Response {
    let mut response = (StatusCode::OK, Body::from("ping")).into_response();
    response.headers_mut().insert(
        header::HeaderName::from_static("x-conan-server-capabilities"),
        HeaderValue::from_static(CAPABILITIES),
    );
    apply_common_headers(&mut response, "\"ping\"", correlation);
    response
}

/// El cambio de contraseña por token.
///
/// El cliente manda Basic `{user}:{password}`; el hub verifica la
/// contraseña **como credencial bearer** del principal y la devuelve como
/// el token que el cliente guardará y mandará como `Bearer`. Un token que
/// el hub puede verificar es una credencial verificable: el scoping por
/// cuenta no se inventa aquí, lo pone el principal dentro del token. El
/// usuario del Basic es decorativo: la identidad vive en el token, y una
/// identidad que se declara sin verificarla no es una identidad.
async fn the_token(
    state: &AppState,
    headers: &HeaderMap,
    correlation: &str,
) -> Result<Response, Failure> {
    let raw = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Basic "))
        .ok_or(Failure::Plain(
            StatusCode::UNAUTHORIZED,
            "the conan authentication needs Basic credentials".to_string(),
        ))?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(raw.trim())
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .ok_or(Failure::Plain(
            StatusCode::UNAUTHORIZED,
            "the Basic credentials are not decodable".to_string(),
        ))?;
    let password = decoded
        .split_once(':')
        .map(|(_, password)| password)
        .ok_or(Failure::Plain(
            StatusCode::UNAUTHORIZED,
            "the Basic credentials carry no password".to_string(),
        ))?;
    match verify_principal(state.authenticator.as_ref(), &Credentials::bearer(password)).await {
        Ok(_) => Ok(text_response(StatusCode::OK, password, correlation)),
        Err(_) => Err(Failure::Plain(
            StatusCode::UNAUTHORIZED,
            "wrong user or password".to_string(),
        )),
    }
}

/// Una respuesta de texto plano: el dialecto del authenticate y del
/// check_credentials es texto, no JSON.
fn text_response(status: StatusCode, body: &str, correlation: &str) -> Response {
    let mut response = (status, Body::from(body.to_string())).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    apply_common_headers(&mut response, "\"text\"", correlation);
    response
}

fn json_response(status: StatusCode, value: &Value, correlation: &str) -> Response {
    let mut response = (status, axum::Json(value.clone())).into_response();
    apply_common_headers(&mut response, "\"json\"", correlation);
    response
}

// ---------------------------------------------------------------------------
// Lectura
// ---------------------------------------------------------------------------

async fn read_route(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    route: Route,
    query: &str,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    match route {
        Route::Search => {
            let records = assets_under(state, context, repository, "recipes/").await?;
            let mut references: Vec<RecipeRef> = records
                .iter()
                .filter(|record| record.asset_path.ends_with("/conanfile.py"))
                .filter_map(|record| the_recipe_of(&record.asset_path))
                .collect();
            references.sort_by_key(RecipeRef::render);
            references.dedup_by_key(|reference| reference.render());
            let pattern = conan::the_query_of(query);
            let matched: Vec<RecipeRef> = references
                .into_iter()
                .filter(|reference| {
                    pattern
                        .as_deref()
                        .map(|pattern| conan::the_search_matches(pattern, reference))
                        .unwrap_or(true)
                })
                .collect();
            Ok(json_response(
                StatusCode::OK,
                &conan::search_document(&matched),
                correlation,
            ))
        }
        Route::RecipeLatest(reference) => {
            let revisions = recipe_revisions(state, context, repository, &reference).await?;
            let (revision, time) = revisions.last().ok_or(RawApiError::NotFound)?.clone();
            Ok(json_response(
                StatusCode::OK,
                &conan::revision_document(&revision, time),
                correlation,
            ))
        }
        Route::RecipeRevisions(reference) => {
            let revisions = recipe_revisions(state, context, repository, &reference).await?;
            if revisions.is_empty() {
                return Err(RawApiError::NotFound.into());
            }
            Ok(json_response(
                StatusCode::OK,
                &conan::revisions_document(&revisions),
                correlation,
            ))
        }
        Route::RecipeSnapshot(reference, revision) => {
            let files = recipe_files(state, context, repository, &reference, &revision).await?;
            Ok(json_response(
                StatusCode::OK,
                &conan::snapshot_document(&files),
                correlation,
            ))
        }
        Route::RecipeFile(reference, revision, file) => {
            let path = reference.recipe_file_path(&revision, &file);
            read_asset(state, context, repository, &path, head, correlation).await
        }
        Route::PackageIds(reference, revision) | Route::PackageSearch(reference, revision) => {
            let prefix = reference.packages_prefix(&revision);
            let records = assets_under(state, context, repository, &prefix).await?;
            let mut ids: Vec<String> = records
                .iter()
                .filter_map(|record| the_segment(&record.asset_path, 6))
                .collect();
            ids.sort();
            ids.dedup();
            let document: Value = ids
                .into_iter()
                .map(|id| (id, json!({})))
                .collect::<serde_json::Map<_, _>>()
                .into();
            Ok(json_response(StatusCode::OK, &document, correlation))
        }
        Route::PackageLatest(reference, revision, package_id) => {
            let revisions = package_revisions(
                state,
                context,
                repository,
                &reference,
                &revision,
                &package_id,
            )
            .await?;
            let (package_revision, time) = revisions.last().ok_or(RawApiError::NotFound)?.clone();
            Ok(json_response(
                StatusCode::OK,
                &conan::revision_document(&package_revision, time),
                correlation,
            ))
        }
        Route::PackageRevisions(reference, revision, package_id) => {
            let revisions = package_revisions(
                state,
                context,
                repository,
                &reference,
                &revision,
                &package_id,
            )
            .await?;
            if revisions.is_empty() {
                return Err(RawApiError::NotFound.into());
            }
            Ok(json_response(
                StatusCode::OK,
                &conan::revisions_document(&revisions),
                correlation,
            ))
        }
        Route::PackageSnapshot(reference, revision, package_id, package_revision) => {
            let files = package_files(
                state,
                context,
                repository,
                &reference,
                &revision,
                &package_id,
                &package_revision,
            )
            .await?;
            Ok(json_response(
                StatusCode::OK,
                &conan::snapshot_document(&files),
                correlation,
            ))
        }
        Route::PackageFile(reference, revision, package_id, package_revision, file) => {
            let path =
                reference.package_file_path(&revision, &package_id, &package_revision, &file);
            read_asset(state, context, repository, &path, head, correlation).await
        }
        Route::Ping | Route::Authenticate | Route::CheckCredentials => {
            Err(RawApiError::MethodNotAllowed("this conan operation is not supported").into())
        }
    }
}

/// El segmento `index` de una ruta de asset, si es de la forma que el
/// protocolo escribe.
///
/// `recipes/{n}/{v}/{u}/{c}/{rrev}/{file}` y
/// `packages/{n}/{v}/{u}/{c}/{rrev}/{pid}/{prev}/{file}`: la posición es
/// del protocolo, no de la cadena, y un path que no la tiene no es una
/// referencia de este dialecto.
fn the_segment(path: &str, index: usize) -> Option<String> {
    path.split('/').nth(index).map(str::to_string)
}

/// La referencia de un recipe desde su ruta de asset.
fn the_recipe_of(path: &str) -> Option<RecipeRef> {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 6 || parts[0] != "recipes" {
        return None;
    }
    Some(RecipeRef {
        name: parts[1].to_string(),
        version: parts[2].to_string(),
        user: parts[3].to_string(),
        channel: parts[4].to_string(),
    })
}

/// Los assets bajo un prefijo, en la cuenta del principal.
async fn assets_under(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    prefix: &str,
) -> Result<Vec<AssetRecord>, Failure> {
    let (reader, _) = super::raw::the_catalog(state)?;
    // A scan stops at the segment boundary: `c` must not match `c2`.
    let prefix = if prefix.ends_with('/') {
        prefix.to_string()
    } else {
        format!("{prefix}/")
    };
    let records = reader
        .assets_under(context.account().as_str(), &repository.repo_key, &prefix)
        .await
        .map_err(|error| Failure::from(the_publication_error(&error)))?;
    // A listing exposes digests and existence: what the channel does not
    // admit is not in it.
    super::raw::the_admitted(state, context.account().as_str(), repository, records)
        .await
        .map_err(Failure::from)
}

/// Las recipe revisions de un recipe, con el tiempo de su primera subida.
///
/// Una revisión es la suma de sus ficheros, y el tiempo que la identifica
/// es el del primero que llegó: un listado de revisiones no reescribe la
/// historia, la ordena.
async fn recipe_revisions(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    reference: &RecipeRef,
) -> Result<Vec<(String, DateTime<Utc>)>, Failure> {
    let records = assets_under(state, context, repository, &reference.recipe_prefix()).await?;
    Ok(the_revisions_of(&records, 5))
}

/// Las package revisions de un `package_id`, con el tiempo de su primera
/// subida.
async fn package_revisions(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    reference: &RecipeRef,
    revision: &str,
    package_id: &str,
) -> Result<Vec<(String, DateTime<Utc>)>, Failure> {
    let records = assets_under(
        state,
        context,
        repository,
        &reference.package_prefix(revision, package_id),
    )
    .await?;
    Ok(the_revisions_of(&records, 7))
}

/// Agrupa los assets por su segmento de revisión y ordena por tiempo.
fn the_revisions_of(
    records: &[AssetRecord],
    revision_index: usize,
) -> Vec<(String, DateTime<Utc>)> {
    let mut revisions: Vec<(String, DateTime<Utc>)> = Vec::new();
    for record in records {
        let Some(revision) = the_segment(&record.asset_path, revision_index) else {
            continue;
        };
        match revisions.iter_mut().find(|(known, _)| *known == revision) {
            Some((_, time)) => *time = (*time).min(record.updated_at),
            None => revisions.push((revision, record.updated_at)),
        }
    }
    revisions.sort_by_key(|(_, time)| *time);
    revisions
}

/// Los ficheros de una revisión del recipe: la ruta relativa y su tamaño.
async fn recipe_files(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    reference: &RecipeRef,
    revision: &str,
) -> Result<Vec<(String, u64)>, Failure> {
    let prefix = reference.revision_prefix(revision);
    the_files_under(state, context, repository, &prefix).await
}

/// Los ficheros de una revisión de package: la ruta relativa y su tamaño.
async fn package_files(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    reference: &RecipeRef,
    revision: &str,
    package_id: &str,
    package_revision: &str,
) -> Result<Vec<(String, u64)>, Failure> {
    let prefix = reference.package_revision_prefix(revision, package_id, package_revision);
    the_files_under(state, context, repository, &prefix).await
}

/// Los ficheros bajo un prefijo, con su ruta relativa al prefijo.
async fn the_files_under(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    prefix: &str,
) -> Result<Vec<(String, u64)>, Failure> {
    let records = assets_under(state, context, repository, prefix).await?;
    let boundary = format!("{prefix}/");
    Ok(records
        .into_iter()
        .filter_map(|record| {
            let file = record.asset_path.strip_prefix(&boundary)?.to_string();
            Some((file, record.size_bytes))
        })
        .collect())
}

/// Los bytes de un asset, verificados contra su fila del catálogo.
async fn read_asset(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    asset_path: &str,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    let resolved =
        the_resolved_asset(state, context, repository.clone(), asset_path, correlation).await?;
    let Resolved {
        asset,
        served,
        resolved_by: _,
        repository: backing,
    } = resolved;
    crate::trust::admit(state, context.account().as_str(), &backing, &asset.digest)
        .await
        .map_err(the_admission_error)?;
    let etag = crate::raw::conditional::the_etag(&asset.digest);
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
    let mut response = body_response(StatusCode::OK, bytes, &etag, correlation);
    apply_cache_headers(&mut response, &served);
    apply_common_headers(&mut response, &etag, correlation);
    if head && let Ok(value) = HeaderValue::from_str(&length.to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    Ok(response)
}

fn body_response(status: StatusCode, body: Vec<u8>, etag: &str, correlation: &str) -> Response {
    let mut response = (status, Body::from(body)).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response.headers_mut().insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    apply_common_headers(&mut response, etag, correlation);
    response
}

// ---------------------------------------------------------------------------
// Escritura
// ---------------------------------------------------------------------------

/// Las coordenadas del catalogo para UN fichero conan.
///
/// El catálogo identifica el contenido por (nombre, versión) y esa
/// identidad es inmutable: cada fichero del dialecto es su propio
/// contenido, así que su versión lleva toda su identidad conan —la
/// revisión, el `package_id` y la revisión de package cuando la
/// lleva—. El nombre sigue siendo el del recipe conan: es lo que el
/// formato llama paquete.
fn the_file_coordinates(
    reference: &RecipeRef,
    revision: &str,
    package_id: Option<&str>,
    package_revision: Option<&str>,
    _sources: Option<&str>,
    file: &str,
) -> PackageCoordinates {
    let mut version = format!(
        "{}@{}/{}#{}",
        reference.version, reference.user, reference.channel, revision
    );
    if let Some(package_id) = package_id {
        version.push_str(&format!("#{package_id}"));
    }
    if let Some(package_revision) = package_revision {
        version.push_str(&format!("#{package_revision}"));
    }
    version.push_str(&format!("#{file}"));
    PackageCoordinates {
        package_name: reference.name.clone(),
        package_version: version,
    }
}

/// La subida de un fichero: la ruta la determina la revisión que el
/// cliente declara, y el contenido es inmutable.
///
/// Reenviar lo mismo es un eco de una subida interrumpida, no una
/// reescritura: el mismo digest en la misma dirección es un sí. Otro
/// contenido en la misma dirección es un no, porque las revisiones son
/// identificadores versionados: un `rrev`/`prev` que cambia de bytes no
/// es una revisión, es otra.
async fn put_file(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    route: Route,
    request: axum::extract::Request<Body>,
    correlation: &str,
) -> Result<Response, Failure> {
    let (asset_path, coordinates, what) = match route {
        Route::RecipeFile(reference, revision, file) => {
            let coordinates = the_file_coordinates(&reference, &revision, None, None, None, &file);
            (
                reference.recipe_file_path(&revision, &file),
                coordinates,
                format!("recipe {} rrev {revision}", reference.render()),
            )
        }
        Route::PackageFile(reference, revision, package_id, package_revision, file) => {
            let coordinates = the_file_coordinates(
                &reference,
                &revision,
                Some(&package_id),
                Some(&package_revision),
                None,
                &file,
            );
            (
                reference.package_file_path(&revision, &package_id, &package_revision, &file),
                coordinates,
                format!(
                    "package {} rrev {revision} pid {package_id} prev {package_revision}",
                    reference.render()
                ),
            )
        }
        _ => return Err(RawApiError::MethodNotAllowed("only conan files can be uploaded").into()),
    };

    let limit = state.max_body_size.saturating_add(1024 * 1024);
    let body = axum::body::to_bytes(request.into_body(), limit)
        .await
        .map_err(|_| Failure::Raw(RawApiError::PayloadTooLarge))?;

    let (digest, length) = write_the_content(
        state,
        context.account().as_str(),
        Body::from(body.to_vec()),
        None,
        None,
    )
    .await?;

    let (_, writer) = super::raw::the_catalog(state)?;
    let publication = Publication {
        repository: repository.repo_key.clone(),
        asset_path,
        coordinates,
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
        Ok(_) => Ok(text_response(StatusCode::OK, "", correlation)),
        Err(CatalogError::Conflict { .. }) => Err(Failure::Plain(
            StatusCode::CONFLICT,
            format!("{what} is already published with another content"),
        )),
        Err(error) => Err(Failure::from(the_publication_error(&error))),
    }
}
