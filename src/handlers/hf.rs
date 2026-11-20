//! La superficie **hf**: el dialecto de lectura del Hugging Face Hub
//! (`hf download` de `huggingface_hub` 1.x, que habla httpx) sobre un
//! repositorio **hosted** de Hodei.
//!
//! Vive en `/hf/{repository}/...` y habla el dialecto que fija el
//! cliente, verificado en el spike de HSP-169 contra huggingface.co y
//! contra un fixture local (FMT-07 de `uat/UAT-FORMATS.md`): la tabla de
//! rutas, el documento de revision JSON y las cabeceras del resolve son
//! los que `hodei_catalog::hf` (puro) sirve, y aqui solo hay efectos.
//!
//! El flujo del cliente es el del hub clasico:
//!
//! 1. `GET /api/models/{repo_id}/revision/{rev}`: el documento de
//!    revision; el `sha` del documento es el commit que el cliente usa
//!    despues en los resolve.
//! 2. `HEAD /{repo_id}/resolve/{rev}/{rfilename}`: una `302` con
//!    `X-Repo-Commit`, `X-Linked-Etag` (el sha256), `X-Linked-Size` y una
//!    `Location` **relativa** (una ruta, sin scheme ni host) hacia el
//!    blob. El cliente solo sigue los redirects relativos.
//! 3. `HEAD`/`GET {Location}`: la metadata se lee de la respuesta final y
//!    los bytes van **en streaming**, a tramos del CAS, sin buffer
//!    completo en memoria: los pesos de un modelo son el artefacto
//!    grande tipico.
//!
//! La identidad del blob es su sha256: es el etag que el cliente usa para
//! nombrar su cache y el digest que el CAS garantiza al servir. El
//! cliente clasico SOLO verifica el tamaño —el sha256 declarado en
//! `X-Linked-Etag` no lo comprueba—: la integridad la garantiza el CAS
//! del servidor (conexion con HSP-105), que no entrega bytes que no son
//! su digest.
//!
//! El servidor no anuncia `X-Xet-*`: el cliente cae a la ruta HTTP
//! clasica, el dialecto que este release sirve (decision del fixture del
//! spike, documentada). La escritura no se declara: el upload clasico y
//! el xet quedan fuera, y el contenido entra por la siembra del hub.
//!
//! La autenticacion es la del hub, mapeada al dialecto: el cliente lleva
//! su token como `Authorization: Bearer <key>`, y el hub verifica la
//! credencial como bearer del principal. El scoping por cuenta es el del
//! hub, no el del formato.

use axum::{
    body::{Body, Bytes},
    extract::{OriginalUri, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use hodei_authz::{Credentials, VerifiedContext, verify_principal};
use hodei_catalog::{
    AssetRecord, RepositoryDescriptor, RepositoryFormat, RepositoryMode,
    hf::{self, BlobMeta, ModelRef, Route},
};
use hodei_evidence::domain::content::ContentDigest;
use hodei_hub::BlobStore;
use serde_json::{Value, json};
use std::sync::Arc;

use super::raw::{
    RawApiError, Resolved, apply_cache_headers, apply_common_headers, the_admission_error,
    the_catalog, the_content_store, the_decision, the_publication_error, the_repository_in,
    the_resolved_asset,
};
use crate::app_state::AppState;
use crate::handlers::{bearer_token, correlation_id};

/// El tramo que el CAS entrega por lectura: el mismo tamaño con el que
/// el fixture del spike sirvio sus 256 MiB, y el que deja el RSS del
/// servidor acotado mientras el cliente escribe a disco.
const THE_CHUNK: u64 = 64 * 1024;

/// Lo que pasa con una peticion que no se puede servir.
#[derive(Debug)]
pub enum Failure {
    /// Un error de la superficie compartida.
    Raw(RawApiError),
    /// Un estado y una razon.
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
        // El dialecto del hub lleva sus errores como JSON plano: el CLI
        // imprime el estado y la razon, y no hay una forma del protocolo
        // que el cliente convierta en otra cosa.
        let body = json!({ "error": message });
        let mut response = (status, axum::Json(body)).into_response();
        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
    }
}

impl Failure {
    /// Un error con un estado y una razon.
    fn the_failure(status: StatusCode, detail: &str) -> Self {
        Self::Plain(status, detail.to_string())
    }

    /// Lo que no esta no dice que estuvo: un `404` sin explicacion de si
    /// existio en otro sitio.
    fn the_absence() -> Self {
        Self::Raw(RawApiError::NotFound)
    }
}

/// El resto de la ruta de `/hf/{repository}/{rest}`, sin decodificar: las
/// reglas de la gramatica del protocolo son las unicas que pueden mirar
/// un `%2f`.
fn the_split(uri: &OriginalUri) -> Result<(String, String), Failure> {
    let rest = uri
        .path()
        .strip_prefix("/hf/")
        .ok_or_else(Failure::the_absence)?;
    let (repository, rest) = rest.split_once('/').ok_or_else(Failure::the_absence)?;
    if !crate::raw::path::valid_repository_name(repository) || rest.is_empty() {
        return Err(Failure::the_absence());
    }
    Ok((repository.to_string(), rest.to_string()))
}

/// Cualquier peticion de `/hf/{repository}/...`.
pub async fn handle(
    State(state): State<AppState>,
    method: Method,
    uri: OriginalUri,
    headers: HeaderMap,
) -> Result<Response, Failure> {
    let correlation = correlation_id(&headers, &state.identity);
    let (repository_key, rest) = the_split(&uri)?;
    let Some(route) = hf::parse(&rest) else {
        return Err(Failure::the_absence());
    };

    let context = the_context(&state, &headers).await?;
    let repository =
        the_repository_in(&state, &context, &repository_key, &[RepositoryFormat::Hf]).await?;
    // La modalidad servida es la hosted: un proxy del hub real es otro
    // dialecto (el upstream anuncia xet) que este release no ha
    // verificado, y no se declara.
    if repository.mode != RepositoryMode::Hosted {
        return Err(Failure::the_absence());
    }
    let reading = method == Method::GET || method == Method::HEAD;
    if !reading {
        return Err(Failure::the_failure(
            StatusCode::METHOD_NOT_ALLOWED,
            "this hf operation is not supported",
        ));
    }
    the_decision(
        &state,
        &context,
        crate::actions::download_asset(),
        &repository,
    )?;
    read(
        &state,
        &context,
        &repository,
        &repository_key,
        route,
        method == Method::HEAD,
        &correlation,
    )
    .await
}

/// El principal verificado de las credenciales del cliente.
///
/// El CLI lleva el token como `Authorization: Bearer <key>`: el hub
/// verifica la credencial como bearer del principal, y el scoping por
/// cuenta es la que el token lleva. Una credencial que el hub no puede
/// verificar no es una identidad.
async fn the_context(state: &AppState, headers: &HeaderMap) -> Result<VerifiedContext, Failure> {
    let token = bearer_token(headers).ok_or(Failure::the_failure(
        StatusCode::UNAUTHORIZED,
        "the hf surface needs an API key as a Bearer credential",
    ))?;
    match verify_principal(state.authenticator.as_ref(), &Credentials::bearer(&token)).await {
        Ok(principal) => Ok(VerifiedContext::from_principal(principal)),
        Err(hodei_authz::AuthenticationError::ProviderUnavailable(_)) => {
            Err(RawApiError::Unavailable.into())
        }
        Err(_) => Err(Failure::the_failure(
            StatusCode::UNAUTHORIZED,
            "the credential is not one this hub can verify",
        )),
    }
}

fn json_response(status: StatusCode, value: &Value, correlation: &str) -> Response {
    let mut response = (status, axum::Json(value.clone())).into_response();
    apply_common_headers(&mut response, "\"json\"", correlation);
    response
}

// ---------------------------------------------------------------------------
// Lecturas
// ---------------------------------------------------------------------------

/// Una lectura de la superficie: el documento de revision, la `302` del
/// resolve y los bytes del blob.
#[allow(clippy::too_many_arguments)]
async fn read(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    repository_key: &str,
    route: Route,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    match route {
        Route::ApiRevision(model, rev) => {
            let snapshot = the_snapshot(state, context, repository, &model).await?;
            // Una revision que no es la rama `main` ni el commit del
            // snapshot es una revision que el hub no llego a servir: el
            // catalogo guarda el estado actual, no una historia de ramas.
            if rev != "main" && rev != snapshot.commit {
                return Err(Failure::the_absence());
            }
            let document = hf::revision_document(
                &model,
                &snapshot.commit,
                snapshot.last_modified,
                &snapshot.the_filenames(),
            );
            Ok(json_response(StatusCode::OK, &document, correlation))
        }
        Route::Resolve(model, rev, filename) => {
            let snapshot = the_snapshot(state, context, repository, &model).await?;
            if rev != "main" && rev != snapshot.commit {
                return Err(Failure::the_absence());
            }
            let Some(record) = snapshot.the_record_of(&filename) else {
                return Err(Failure::the_absence());
            };
            let meta = BlobMeta {
                commit: snapshot.commit.clone(),
                sha256: record.digest.value.clone(),
                size: record.size_bytes,
            };
            if head {
                Ok(the_redirect(
                    repository_key,
                    &model,
                    &meta,
                    &filename,
                    correlation,
                ))
            } else {
                // Un `GET` del resolve entrega los mismos bytes que la
                // `Location`: el redirect es una forma de nombrar el
                // blob, no una manera de cambiar lo que se sirve.
                let asset_path = model.asset_path(&filename);
                the_blob(
                    state,
                    context,
                    repository,
                    &asset_path,
                    &meta,
                    false,
                    correlation,
                )
                .await
            }
        }
        Route::Blob(model, digest, filename) => {
            // El digest de la ruta nombra el contenido: una URL que
            // nadie construyo con el sha256 de lo que hay es una URL que
            // no se sirve.
            let snapshot = the_snapshot(state, context, repository, &model).await?;
            let Some(record) = snapshot.the_record_of(&filename) else {
                return Err(Failure::the_absence());
            };
            if record.digest.value != digest {
                return Err(Failure::the_absence());
            }
            let meta = BlobMeta {
                commit: snapshot.commit.clone(),
                sha256: record.digest.value.clone(),
                size: record.size_bytes,
            };
            let asset_path = model.asset_path(&filename);
            the_blob(
                state,
                context,
                repository,
                &asset_path,
                &meta,
                head,
                correlation,
            )
            .await
        }
    }
}

/// El snapshot de un modelo: los ficheros que el catalogo guarda hoy.
///
/// El commit es el digest del listado que lo forma: dos snapshots con el
/// mismo contenido son el mismo commit, y el commit solo cambia cuando
/// cambia el contenido que declara.
struct Snapshot {
    /// El modelo del snapshot.
    model: ModelRef,
    /// El commit del snapshot.
    commit: String,
    /// El momento en que el contenido se movio por ultima vez.
    last_modified: DateTime<Utc>,
    /// Los assets del catalogo del modelo, tal cual llegaron.
    records: Vec<AssetRecord>,
}

impl Snapshot {
    /// El asset de un fichero del snapshot, si esta.
    fn the_record_of(&self, filename: &str) -> Option<&AssetRecord> {
        let prefix = format!("{}/", self.model.asset_prefix());
        self.records
            .iter()
            .find(|record| record.asset_path.strip_prefix(&prefix) == Some(filename))
    }

    /// Los `rfilename` del snapshot, en el orden determinista del
    /// listado.
    fn the_filenames(&self) -> Vec<String> {
        let prefix = format!("{}/", self.model.asset_prefix());
        let mut names: Vec<String> = self
            .records
            .iter()
            .filter_map(|record| record.asset_path.strip_prefix(&prefix).map(str::to_string))
            .collect();
        names.sort();
        names
    }
}

/// El snapshot de un modelo, leido del catalogo.
async fn the_snapshot(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    model: &ModelRef,
) -> Result<Snapshot, Failure> {
    let records = assets_under(state, context, repository, &model.asset_prefix()).await?;
    if records.is_empty() {
        return Err(Failure::the_absence());
    }
    let entries: Vec<(&str, &str)> = records
        .iter()
        .filter_map(|record| {
            let prefix = format!("{}/", model.asset_prefix());
            let filename = record.asset_path.strip_prefix(&prefix)?;
            Some((filename, record.digest.value.as_str()))
        })
        .collect();
    let commit = hf::snapshot_commit(&entries);
    let last_modified = records
        .iter()
        .map(|record| record.updated_at)
        .max()
        .unwrap_or_else(Utc::now);
    Ok(Snapshot {
        model: model.clone(),
        commit,
        last_modified,
        records,
    })
}

/// Los assets de un modelo, en la cuenta del principal.
async fn assets_under(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    prefix: &str,
) -> Result<Vec<AssetRecord>, Failure> {
    let (reader, _) = the_catalog(state)?;
    reader
        .assets_under(context.account().as_str(), &repository.repo_key, prefix)
        .await
        .map_err(|error| Failure::from(the_publication_error(&error)))
}

/// La `302` del resolve: la metadata viaja en cabeceras y la `Location`
/// es una ruta relativa hacia el blob. El cuerpo no hay: el cliente solo
/// lee cabeceras aqui.
fn the_redirect(
    repository_key: &str,
    model: &ModelRef,
    meta: &BlobMeta,
    filename: &str,
    correlation: &str,
) -> Response {
    let location =
        hf::the_download_location(&format!("/hf/{repository_key}"), model, meta, filename);
    let mut response = (StatusCode::FOUND, Body::empty()).into_response();
    for (name, value) in meta.the_linked_headers() {
        if let Ok(value) = HeaderValue::from_str(&value) {
            response
                .headers_mut()
                .insert(header::HeaderName::from_static(name), value);
        }
    }
    if let Ok(value) = HeaderValue::from_str(&location) {
        response.headers_mut().insert(header::LOCATION, value);
    }
    if let Ok(value) = HeaderValue::from_str("0") {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    apply_common_headers(&mut response, &meta.sha256, correlation);
    response
}

/// Los bytes del blob, servidos desde el CAS.
///
/// El contenido va en **streaming**: el CAS se lee a tramos de 64 KiB y
/// cada tramo sale tal cual, sin buffer completo en memoria. La longitud
/// declarada es la del asset del catalogo, la misma que el
/// `Content-Length` anuncia al cliente; un tramo que no llega es un
/// error de transporte, no un contenido corto. Un `HEAD` responde la
/// metadata final del redirect (la que el cliente lee) sin los bytes.
async fn the_blob(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    asset_path: &str,
    meta: &BlobMeta,
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
    // El contenido que se sirve es el que la URL nombra: el etag del
    // asset y el sha256 de la ruta son el mismo digest, y si no lo son,
    // la URL era de otro contenido que ya no esta.
    if asset.digest.value != meta.sha256 {
        return Err(Failure::the_absence());
    }
    crate::trust::admit(state, context.account().as_str(), &backing, &asset.digest)
        .await
        .map_err(the_admission_error)?;
    let etag = crate::raw::conditional::the_etag(&asset.digest);
    let length = asset.size_bytes;
    let body = if head {
        Body::empty()
    } else {
        let store = Arc::clone(the_content_store(state)?);
        let digest = asset.digest.clone();
        the_streamed_body(store, digest, length)
    };
    let mut response = (StatusCode::OK, body).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response.headers_mut().insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    // La metadata del dialecto: el `HEAD` final del redirect es donde el
    // cliente la lee, y la repite el `GET` por la misma honestidad.
    for (name, value) in meta.the_linked_headers() {
        if let Ok(value) = HeaderValue::from_str(&value) {
            response
                .headers_mut()
                .insert(header::HeaderName::from_static(name), value);
        }
    }
    apply_cache_headers(&mut response, &served);
    apply_common_headers(&mut response, &etag, correlation);
    // El `ETag` del dialecto es el sha256 entre comillas, la forma del
    // hub clasico (la del fixture del spike): el etag condicional del hub
    // viaja en el `X-Linked-Etag` que ya va puesto.
    if let Ok(value) = HeaderValue::from_str(&format!("\"{}\"", meta.sha256)) {
        response.headers_mut().insert(header::ETAG, value);
    }
    if let Ok(value) = HeaderValue::from_str(&length.to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    Ok(response)
}

/// Los bytes del blob como un cuerpo en streaming: tramos del CAS que
/// salen tal cual llegan.
fn the_streamed_body(store: Arc<dyn BlobStore>, digest: ContentDigest, size: u64) -> Body {
    let stream = futures_util::stream::try_unfold(
        (store, digest, size, 0u64),
        |(store, digest, size, offset)| async move {
            if offset >= size {
                return Ok::<
                    Option<(Bytes, (Arc<dyn BlobStore>, ContentDigest, u64, u64))>,
                    std::io::Error,
                >(None);
            }
            let length = THE_CHUNK.min(size - offset);
            let bytes = store
                .read_range(&digest, offset, length)
                .await
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            Ok(Some((
                Bytes::from(bytes),
                (store, digest, size, offset + length),
            )))
        },
    );
    Body::from_stream(stream)
}
