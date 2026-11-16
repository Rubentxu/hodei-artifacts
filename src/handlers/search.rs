//! La busqueda del catalogo.
//!
//! La busqueda es la pregunta "que hay?" del registro, y la responde el
//! propio catalogo: no hay un motor aparte que haya que sincronizar con
//! el, y por eso lo que se prometa de la busqueda es lo que hay.
//!
//! # La cuenta y el permiso van por delante
//!
//! En este orden, y no en el otro:
//!
//! 1. La cuenta sale del **principal verificado**. Nunca de un campo de
//!    la peticion, y nunca de la consulta.
//! 2. Los repositorios que el principal puede leer los dice **Cedar**,
//!    uno por uno. Los demas no salen, y tampoco cuentan: un total que
//!    contara lo que no se puede leer diria que hay algo donde no hay
//!    nada que ver.
//!
//! Por eso la busqueda de otra cuenta no devuelve ni nombres ni
//! totales: la cuenta ajena no llega a la consulta.
//!
//! # Los filtros
//!
//! Todos son opcionales y se combinan con "y". Cada uno se nombra una
//! vez: una consulta que dice dos veces lo mismo no dice lo que dice, y
//! un filtro que no existe no es un filtro: es una consulta que el
//! cliente no entiende, y se responde con un 400 en vez de buscar algo
//! que el cliente no pidio.
//!
//! El texto libre es un **recorrido**: no hay indice de texto completo,
//! y el filtro compara el nombre del paquete y la ruta del asset. La
//! busqueda por texto es la unica que no acota por indice.

use std::collections::HashSet;

use axum::{
    Json,
    extract::{RawQuery, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use hodei_authz::VerifiedContext;
use hodei_catalog::search::{RepositoryAttribute, SearchPage, SearchQuery};
use hodei_catalog::{CatalogError, RepositoryDescriptor, RepositoryFormat, TrustState};
use hodei_evidence::ContentDigest;

use crate::app_state::AppState;
use crate::handlers::{Authentication, the_context, the_query_values};

/// Lo que puede fallar en una busqueda.
#[derive(Debug)]
pub enum SearchApiError {
    /// La peticion no se pudo autenticar.
    Unauthorized,
    /// La consulta no dice lo que dice.
    BadRequest(String),
    /// El catalogo no pudo responder.
    Unavailable,
    /// Algo fallo por dentro.
    Internal,
}

impl IntoResponse for SearchApiError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized".to_string()),
            Self::BadRequest(reason) => (StatusCode::BAD_REQUEST, reason),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "the search is not available".to_string(),
            ),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal error".to_string(),
            ),
        };
        let mut response = (status, body).into_response();
        if status == StatusCode::UNAUTHORIZED {
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                header::HeaderValue::from_static("Bearer"),
            );
        }
        response
    }
}

/// La pagina de resultados que la API devuelve.
#[derive(Debug, serde::Serialize)]
pub struct SearchView {
    /// Cuantos resultados hay en total.
    pub total: usize,
    /// Cuantos resultados trae la pagina.
    pub limit: usize,
    /// Cuantos resultados se dejan atras.
    pub offset: usize,
    /// Los resultados de la pagina.
    pub results: Vec<SearchHitView>,
}

/// Un asset encontrado por la busqueda.
#[derive(Debug, serde::Serialize)]
pub struct SearchHitView {
    /// El repositorio.
    pub repository: String,
    /// El formato del repositorio.
    pub format: RepositoryFormat,
    /// La ruta del asset dentro del repositorio.
    pub asset_path: String,
    /// El nombre del paquete.
    pub package_name: String,
    /// La version del paquete.
    pub package_version: String,
    /// El digest del contenido.
    pub digest: ContentDigest,
    /// Los bytes que tiene el contenido.
    pub size_bytes: u64,
    /// El estado de confianza del contenido en este repositorio.
    pub trust_state: TrustState,
    /// Cuando se movio por ultima vez.
    pub updated_at: DateTime<Utc>,
}

impl SearchView {
    /// La vista de una pagina del catalogo.
    fn of(page: SearchPage) -> Self {
        Self {
            total: page.total,
            limit: page.limit,
            offset: page.offset,
            results: page
                .results
                .into_iter()
                .map(|hit| SearchHitView {
                    repository: hit.repository,
                    format: hit.format,
                    asset_path: hit.asset_path,
                    package_name: hit.package_name,
                    package_version: hit.package_version,
                    digest: hit.digest,
                    size_bytes: hit.size_bytes,
                    trust_state: hit.trust_state,
                    updated_at: hit.updated_at,
                })
                .collect(),
        }
    }
}

/// `GET /api/v1/search`
///
/// Los filtros se escriben en la consulta: `repository`, `package`,
/// `version`, `digest`, `tag`, `attribute`, `text`, `limit` y `offset`.
/// El digest va con su algoritmo (`sha256:<hexadecimal>`) y el atributo
/// con su nombre y su valor (`format=pypi`, `mode=hosted`,
/// `channel=trusted`).
pub async fn get_search(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Result<Json<SearchView>, SearchApiError> {
    let context = the_context(&state, &headers)
        .await
        .map_err(|failure| match failure {
            Authentication::Unauthorized => SearchApiError::Unauthorized,
            Authentication::Unavailable => SearchApiError::Unavailable,
        })?;

    // Sin catalogo no hay busqueda, y se dice que no esta disponible: un
    // 404 diria que no hay nada, y no es lo que pasa.
    let searcher = state
        .catalog_search
        .as_ref()
        .ok_or(SearchApiError::Unavailable)?;

    let query = the_query(raw_query.as_deref())?;
    let account = context.account().as_str();

    let repositories = searcher
        .repositories(account)
        .await
        .map_err(the_catalog_error)?;
    let permitted = the_permitted(&state, &context, repositories);

    let page = searcher
        .search(account, &permitted, &query)
        .await
        .map_err(the_catalog_error)?;

    Ok(Json(SearchView::of(page)))
}

/// Los repositorios que el principal puede leer.
///
/// Cedar decide uno por uno, y lo que no se puede leer no sale ni
/// cuenta. Un repositorio con un nombre que no se puede interpretar
/// tampoco sale: lo que no se puede evaluar no se puede leer.
fn the_permitted(
    state: &AppState,
    context: &VerifiedContext,
    repositories: Vec<RepositoryDescriptor>,
) -> Vec<RepositoryDescriptor> {
    let action = crate::actions::read_repository();
    repositories
        .into_iter()
        .filter(|repository| {
            state
                .authorize_resource
                .execute_canonical(context.clone(), action.clone(), &repository.hrn)
                .map(|outcome| outcome.allows_access())
                .unwrap_or(false)
        })
        .collect()
}

/// La consulta, tal y como la escribe el cliente.
///
/// Un filtro se nombra una vez y un filtro que no existe es un 400: una
/// busqueda que devuelve mas de lo que se pidio es peor que una que
/// falla.
fn the_query(raw: Option<&str>) -> Result<SearchQuery, SearchApiError> {
    let mut query = SearchQuery::new();
    let Some(values) = the_query_values(raw) else {
        return Err(SearchApiError::BadRequest(
            "the query string is not readable".to_string(),
        ));
    };
    let mut seen: HashSet<String> = HashSet::new();
    for (key, value) in values {
        if !seen.insert(key.clone()) {
            return Err(SearchApiError::BadRequest(format!(
                "the filter {key} is named twice"
            )));
        }
        match key.as_str() {
            "repository" => query.repository = Some(value),
            "package" => query.package_name = Some(value),
            "version" => query.package_version = Some(value),
            "digest" => {
                query = query
                    .with_digest_text(&value)
                    .map_err(|error| SearchApiError::BadRequest(error.to_string()))?;
            }
            "tag" => query.tag = Some(value),
            "attribute" => {
                query.attribute = Some(
                    RepositoryAttribute::parse(&value)
                        .map_err(|error| SearchApiError::BadRequest(error.to_string()))?,
                );
            }
            "text" => query.text = Some(value),
            "limit" => {
                query.limit = value.parse::<usize>().map_err(|_| {
                    SearchApiError::BadRequest("the limit is not a number".to_string())
                })?;
            }
            "offset" => {
                query.offset = value.parse::<usize>().map_err(|_| {
                    SearchApiError::BadRequest("the offset is not a number".to_string())
                })?;
            }
            other => {
                return Err(SearchApiError::BadRequest(format!(
                    "the filter {other} is not a filter of the search"
                )));
            }
        }
    }
    query
        .validate()
        .map_err(|error| SearchApiError::BadRequest(error.to_string()))?;
    Ok(query)
}

/// El error del catalogo, traducido a lo que ve el cliente.
fn the_catalog_error(error: CatalogError) -> SearchApiError {
    match error {
        CatalogError::InvalidRequest { detail } => SearchApiError::BadRequest(detail),
        CatalogError::Unavailable { .. } => SearchApiError::Unavailable,
        _ => SearchApiError::Internal,
    }
}
