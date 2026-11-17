//! The catalog search.
//!
//! The search is the "what is there?" question of the registry, and the
//! catalog itself answers it: there is no separate engine to synchronise
//! with it, and that is why what the search promises is what there is.
//!
//! # The account and the permission come first
//!
//! In this order, and not the other one around:
//!
//! 1. The account comes from the **verified principal**. Never from a
//!    field of the request, and never from the query.
//! 2. The repositories the principal may read are named by **Cedar**, one
//!    by one. The others do not come out, and they are not counted
//!    either: a total that counted what cannot be read would say there is
//!    something where there is nothing to see.
//!
//! That is why the search of another account returns neither names nor
//! totals: the other account never reaches the query.
//!
//! # The filters
//!
//! All of them are optional and they combine with "and". Each one is
//! named once: a query that says the same thing twice does not say what
//! it says, and a filter that does not exist is not a filter: it is a
//! query the client does not understand, and it is answered with a 400
//! instead of looking for something the client did not ask for.
//!
//! Free text is a **walk**: there is no full text index, and the filter
//! compares the package name and the path of the asset. The text search
//! is the only one that does not narrow through an index.

use std::collections::HashSet;

use axum::{
    Json,
    extract::{RawQuery, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use hodei_authz::{AuthorizationOutcome, VerifiedContext};
use hodei_catalog::search::{RepositoryAttribute, SearchPage, SearchQuery};
use hodei_catalog::{
    CatalogError, RepositoryDescriptor, RepositoryFormat, ServeChannel, TrustState,
};
use hodei_evidence::ContentDigest;

use crate::app_state::AppState;
use crate::handlers::{Authentication, error_body, the_context, the_query_values};

/// What can fail in a search.
#[derive(Debug)]
pub enum SearchApiError {
    /// The request could not be authenticated.
    Unauthorized,
    /// The query does not say what it says.
    BadRequest(String),
    /// The catalog could not answer.
    Unavailable,
    /// What is stored cannot be read.
    Corrupt,
    /// Something failed inside.
    Internal,
}

impl IntoResponse for SearchApiError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                error_body("unauthenticated", "unauthorized"),
            ),
            Self::BadRequest(reason) => (StatusCode::BAD_REQUEST, error_body("invalid", &reason)),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                error_body("unavailable", "the search is not available"),
            ),
            Self::Corrupt => (
                StatusCode::INTERNAL_SERVER_ERROR,
                error_body("corruption", "what is stored cannot be read"),
            ),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                error_body("indeterminate", "internal error"),
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

/// The page of results that the API returns.
#[derive(Debug, serde::Serialize)]
pub struct SearchView {
    /// How many results there are in total, when they could be counted.
    ///
    /// It is `null` when there are more matches than are counted: there
    /// is more, and how much is not said because counting it would cost
    /// what walking the whole catalog would cost.
    pub total: Option<usize>,
    /// Whether there are more results after the page.
    pub more: bool,
    /// How many results the page brings.
    pub limit: usize,
    /// How many results are left behind.
    pub offset: usize,
    /// The results of the page.
    pub results: Vec<SearchHitView>,
}

/// An asset found by the search.
#[derive(Debug, serde::Serialize)]
pub struct SearchHitView {
    /// The repository.
    pub repository: String,
    /// The format of the repository.
    pub format: RepositoryFormat,
    /// The path of the asset inside the repository.
    pub asset_path: String,
    /// The package name.
    pub package_name: String,
    /// The package version.
    pub package_version: String,
    /// The digest of the content.
    pub digest: ContentDigest,
    /// The bytes the content has.
    pub size_bytes: u64,
    /// The trust state of the content in this repository.
    pub trust_state: TrustState,
    /// When it moved last.
    pub updated_at: DateTime<Utc>,
}

impl SearchView {
    /// The view of a page of the catalog.
    fn of(page: SearchPage) -> Self {
        Self {
            total: page.total,
            more: page.more,
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
/// The filters are written in the query: `repository`, `package`,
/// `version`, `digest`, `tag`, `attribute`, `text`, `limit` and `offset`.
/// The digest goes with its algorithm (`sha256:<hexadecimal>`) and the
/// attribute with its name and its value (`format=pypi`, `mode=hosted`,
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

    // Without a catalog there is no search, and it says that it is not
    // available: a 404 would say there is nothing, and that is not what
    // happens.
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
    let permitted = the_permitted(&state, &context, repositories)?;

    // A repository served on the trusted channel without a ledger of the
    // grants is a repository whose concessions cannot be known. The
    // search says that it cannot answer: an empty page would say that
    // there is nothing there, and not knowing is not nothing.
    if state.trust_ledger.is_none()
        && permitted
            .iter()
            .any(|repository| ServeChannel::of(&repository.config) == ServeChannel::Trusted)
    {
        return Err(SearchApiError::Unavailable);
    }

    let page = searcher
        .search(account, &permitted, &query)
        .await
        .map_err(the_catalog_error)?;

    Ok(Json(SearchView::of(page)))
}

/// The repositories the principal may read.
///
/// Cedar decides one by one, and what cannot be read neither comes out
/// nor is counted. A repository with a name that cannot be interpreted
/// does not come out either: what cannot be evaluated cannot be read.
///
/// But a decision that **could not be taken** is not a decision: if the
/// authorizer fails, or if it cannot reach a verdict, the search says
/// that it cannot answer. An empty page would say that there is nothing
/// there, and that is not what happened.
///
/// The action is the one of the content, and not the one of the
/// repository: a listing says what is inside, and the other listings of
/// the service ask for the same permission. A deployment that wants a
/// principal to know that a repository exists without knowing what is
/// inside does not give this one.
fn the_permitted(
    state: &AppState,
    context: &VerifiedContext,
    repositories: Vec<RepositoryDescriptor>,
) -> Result<Vec<RepositoryDescriptor>, SearchApiError> {
    let action = crate::actions::download_asset();
    let mut permitted = Vec::with_capacity(repositories.len());
    for repository in repositories {
        let outcome = state
            .authorize_resource
            .execute_canonical(context.clone(), action.clone(), &repository.hrn)
            .map_err(|_| SearchApiError::Internal)?;
        match outcome {
            AuthorizationOutcome::Permit => permitted.push(repository),
            // A denial is a decision: what is not permitted does not come
            // out, and it is not counted either.
            AuthorizationOutcome::Deny(_) => {}
            // An authorizer that could not decide did not say no: it did
            // not say anything, and the search does not fill the silence
            // with an empty page.
            AuthorizationOutcome::Indeterminate(_) => return Err(SearchApiError::Internal),
        }
    }
    Ok(permitted)
}

/// The query, as the client writes it.
///
/// A filter is named once, and a filter that does not exist is a 400: a
/// search that returns more than was asked for is worse than one that
/// fails.
///
/// A request **without** a query is not an error: it is the walk over
/// what there is, which is where the package browser starts. The limit
/// of the page bounds it.
fn the_query(raw: Option<&str>) -> Result<SearchQuery, SearchApiError> {
    let mut query = SearchQuery::new();
    let Some(raw) = raw else {
        return Ok(query);
    };
    let Some(values) = the_query_values(Some(raw)) else {
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

/// The error of the catalog, translated into what the client sees.
fn the_catalog_error(error: CatalogError) -> SearchApiError {
    match error {
        CatalogError::InvalidRequest { detail } => SearchApiError::BadRequest(detail),
        CatalogError::Unavailable { .. } => SearchApiError::Unavailable,
        // A corrupt row is not a failure of the search: it is a failure of
        // what is stored, and it is worth telling apart from a decision
        // that could not be taken.
        CatalogError::Corrupt { .. } => SearchApiError::Corrupt,
        _ => SearchApiError::Internal,
    }
}
