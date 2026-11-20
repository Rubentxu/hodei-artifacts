//! La superficie **terraform**: el *network mirror protocol* de
//! Terraform/OpenTofu sobre un repositorio de Hodei.
//!
//! Vive en `/terraform/{repositorio}/...` y habla el protocolo de mirror
//! de providers: la gramatica de rutas y de documentos esta en
//! `hodei_catalog::terraform` (pura) y aqui solo hay lo que necesita
//! efectos.
//!
//! La modalidad servida es **mirror**, y el mirror es un proxy: cada
//! lectura se trae del upstream con la cache, los recibos y el
//! single-flight de la infraestructura compartida, y se sirve **tal
//! cual**. Los zips de providers son contenido inmutable y se guardan en
//! el CAS con el digest de los bytes que el upstream entrego, de manera
//! que el SHA256 que el cliente verifica contra su lockfile es el del
//! upstream, nunca un recalculo. Los documentos JSON del protocolo son
//! revalidables: el upstream publica versiones nuevas.
//!
//! No es un registry de origen ni un module registry: aqui no se publica,
//! y un mirror no sabe ni quiere saber de `terraform publish`.

use axum::{
    body::Body,
    extract::{OriginalUri, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use hodei_catalog::{
    RepositoryDescriptor, RepositoryFormat,
    terraform::{Route, TerraformError},
};

use super::raw::{
    RawApiError, apply_cache_headers, apply_common_headers, the_blob_error, the_content_store,
    the_context, the_decision, the_proxy_asset_at, the_repository_in,
};
use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{correlation_id, record_audit};

/// Lo que pasa con una peticion que no se puede servir.
#[derive(Debug)]
pub enum Failure {
    /// Un error de la superficie compartida.
    Raw(RawApiError),
    /// Un error del protocolo del mirror.
    Terraform(TerraformError),
}

impl From<RawApiError> for Failure {
    fn from(error: RawApiError) -> Self {
        Self::Raw(error)
    }
}

impl From<TerraformError> for Failure {
    fn from(error: TerraformError) -> Self {
        Self::Terraform(error)
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Raw(error) => error.the_verdict(),
            // La ruta que la gramatica rechaza es un recurso que no hay:
            // el mirror no explica por que una ruta es mala, dice que no
            // existe. Nada se filtra a quien tantea el servicio.
            Self::Terraform(_) => (
                StatusCode::NOT_FOUND,
                "there is nothing at that path".to_string(),
            ),
        };
        let mut response =
            (status, axum::Json(serde_json::json!({ "error": message }))).into_response();
        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
    }
}

/// El resto de la ruta de `/terraform/{repositorio}/{resto}`, sin
/// decodificar: las reglas del protocolo son las unicas que pueden
/// interpretar un `%2f`.
fn the_split(uri: &OriginalUri) -> Result<(String, String), Failure> {
    let rest = uri
        .path()
        .strip_prefix("/terraform/")
        .ok_or(RawApiError::NotFound)?;
    let (repository, rest) = rest.split_once('/').ok_or(RawApiError::NotFound)?;
    if !crate::raw::path::valid_repository_name(repository) || rest.is_empty() {
        return Err(RawApiError::NotFound.into());
    }
    Ok((repository.to_string(), rest.to_string()))
}

/// Cualquier peticion de `/terraform/{repositorio}/...`.
///
/// El mirror solo lee: un `PUT` o un `POST` no es una publicacion, es un
/// metodo que el protocolo no tiene.
pub async fn handle(
    State(state): State<AppState>,
    method: Method,
    uri: OriginalUri,
    headers: HeaderMap,
) -> Result<Response, Failure> {
    let context = the_context(&state, &headers).await?;
    let correlation = correlation_id(&headers, &state.identity);
    let (repository_key, rest) = the_split(&uri)?;
    let route = Route::parse(&rest)?;
    let repository = the_repository_in(
        &state,
        &context,
        &repository_key,
        &[RepositoryFormat::Terraform],
    )
    .await?;

    let reading = method == Method::GET || method == Method::HEAD;
    let outcome = if reading {
        read(
            &state,
            &context,
            &repository,
            &route,
            method == Method::HEAD,
            &correlation,
        )
        .await
    } else {
        Err(RawApiError::MethodNotAllowed(
            "a mirror does not publish: the provider network mirror protocol is read-only",
        )
        .into())
    };
    let provider_label = route
        .provider_coordinates()
        .map(|(host, namespace, provider)| format!("{host}/{namespace}/{provider}"))
        .unwrap_or_else(|| "index".to_string());
    record_outcome(
        &state,
        &context,
        &repository_key,
        &provider_label,
        &outcome,
        &correlation,
    );
    outcome
}

fn record_outcome(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &str,
    provider: &str,
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
        &format!("{repository}/{provider}"),
        result,
        correlation,
    );
}

// ---------------------------------------------------------------------------
// Lectura (mirror)
// ---------------------------------------------------------------------------

/// Una lectura del mirror: la ruta del protocolo ES el camino del asset,
/// y el upstream sirve por el mismo camino. El documento llega del
/// upstream y se sirve tal cual: reescribir un indice o un zip romperia
/// los checksums que el cliente compara contra su lockfile.
async fn read(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    repository: &RepositoryDescriptor,
    route: &Route,
    head: bool,
    correlation: &str,
) -> Result<Response, Failure> {
    the_decision(state, context, crate::actions::download_asset(), repository)?;
    let asset_path = route.asset_path();
    let content_type = match route {
        Route::RootIndex | Route::ProviderIndex { .. } | Route::VersionDoc { .. } => {
            "application/json"
        }
        Route::ProviderFile { .. } => "application/zip",
    };
    let served = the_proxy_asset_at(
        state,
        context,
        repository,
        &asset_path,
        None,
        Some(content_type),
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
    let etag = crate::raw::conditional::the_etag(&served.asset.digest);
    let length = if head {
        served.asset.size_bytes as usize
    } else {
        bytes.len()
    };
    let mut response = body_response(StatusCode::OK, bytes, content_type, &etag, correlation);
    apply_cache_headers(&mut response, &Some((served.status, served.provenance)));
    if head && let Ok(value) = HeaderValue::from_str(&length.to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    Ok(response)
}

// ---------------------------------------------------------------------------
// Respuestas
// ---------------------------------------------------------------------------

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
