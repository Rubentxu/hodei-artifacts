//! Repository read/write endpoints.
//!
//! This is the first productive use of the shared authorizer: the handler
//! verifies the bearer credential, derives the account from the verified
//! principal, refuses a cross-account HRN with a uniform `404`, authorizes the
//! read, and then loads the repository through the read-only port.
//!
//! # Two reads, one route
//!
//! The route serves two reads, and the query says which one is asked for.
//! Without an `hrn` it is the **list** of the repositories of the account,
//! with the pagination the contract declares. With an `hrn` it is that
//! repository, and a repository of another account is the same `404` that a
//! repository that does not exist is.
//!
//! The list is the account's, and the account comes from the verified
//! principal: the repositories of another account are not in it, and their
//! number is not in it either. Cedar decides one repository at a time, and
//! what it denies does not come out **nor is counted**: a total that counts
//! what the principal may not read says how much there is to see.

use axum::{
    Json,
    body::{Body, to_bytes},
    extract::{RawQuery, State},
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    response::{IntoResponse, Response},
};
use hodei_authz::{AuthorizationOutcome, VerifiedContext};
use kernel::Hrn;

use crate::audit::AuditResult;
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::handlers::{correlation_id, error_body, record_audit, the_context, the_query_values};
use crate::repository::RepositoryView;

/// The size of a page when the caller does not ask for one.
const DEFAULT_LIMIT: usize = 20;

/// The biggest page the contract promises.
const MAX_LIMIT: usize = 100;

#[derive(Debug)]
pub enum RepositoryApiError {
    Unauthorized,
    Forbidden,
    NotFound,
    BadRequest(String),
    Conflict,
    /// What is stored cannot be read.
    Corrupt,
    /// Something failed inside, and nobody could take a decision.
    Internal,
    Unavailable,
}

impl IntoResponse for RepositoryApiError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                error_body("unauthenticated", "unauthorized"),
            ),
            Self::Forbidden => (StatusCode::FORBIDDEN, error_body("permission", "forbidden")),
            Self::NotFound => (StatusCode::NOT_FOUND, error_body("absence", "not found")),
            Self::BadRequest(reason) => (StatusCode::BAD_REQUEST, error_body("invalid", &reason)),
            Self::Conflict => (StatusCode::CONFLICT, error_body("conflict", "conflict")),
            Self::Corrupt => (
                StatusCode::INTERNAL_SERVER_ERROR,
                error_body("corruption", "what is stored cannot be read"),
            ),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                error_body("indeterminate", "internal error"),
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                error_body("unavailable", "the repositories are not available"),
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

/// The list of the repositories of the account.
///
/// The shape is the one the contract declares: how many there are, the
/// page, and the page itself.
#[derive(Debug, Serialize)]
pub struct RepositoryListView {
    /// How many repositories the principal may read.
    pub total: usize,
    /// The page that was asked for.
    pub items: Vec<RepositoryView>,
    /// The size of the page.
    pub limit: usize,
    /// Where the page starts.
    pub offset: usize,
}

/// `GET /api/v1/repositories`
///
/// The list of the repositories of the account, or the repository that the
/// query names.
pub async fn get_repository(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Result<Response, RepositoryApiError> {
    // Authentication is the first gate. `RawQuery` cannot fail extraction, so
    // a malformed query cannot turn into a 400 before the 401.
    let context = the_context(&state, &headers)
        .await
        .map_err(|failure| match failure {
            crate::handlers::Authentication::Unauthorized => RepositoryApiError::Unauthorized,
            crate::handlers::Authentication::Unavailable => RepositoryApiError::Unavailable,
        })?;

    let query = the_query(raw_query.as_deref())?;
    match query.hrn {
        Some(hrn) => the_repository(&state, &context, &hrn).await,
        None => {
            let limit = the_page_size(query.limit.as_deref())?;
            let offset = the_offset(query.offset.as_deref())?;
            the_repositories(&state, &context, limit, offset).await
        }
    }
}

/// The repository that the query names.
async fn the_repository(
    state: &AppState,
    context: &VerifiedContext,
    query_hrn: &str,
) -> Result<Response, RepositoryApiError> {
    let hrn = Hrn::from_string(query_hrn).ok_or_else(|| {
        RepositoryApiError::BadRequest("the hrn is not a canonical name".to_string())
    })?;

    // Strict canonical repository shape: a well-formed HRN for another
    // service/type in the same account is not a repository and is not
    // evaluated.
    if hrn.partition() != "hodei"
        || hrn.service() != "artifacts"
        || hrn.resource_type() != "repository"
    {
        return Err(RepositoryApiError::NotFound);
    }

    // The account comes from the verified principal. A foreign HRN is hidden
    // behind the same 404 as a missing repository; it is not evaluated.
    if hrn.account_id() != context.account().as_str() {
        return Err(RepositoryApiError::NotFound);
    }

    let action = crate::actions::read_repository();
    let outcome = state
        .authorize_resource
        .execute_canonical(context.clone(), action.clone(), &hrn)
        .map_err(|_| RepositoryApiError::Internal)?;
    match outcome {
        AuthorizationOutcome::Permit => {}
        AuthorizationOutcome::Deny(_) => return Err(RepositoryApiError::Forbidden),
        // A decision that could not be taken is not a decision: the read
        // does not fill that silence with a refusal that says more than
        // what happened.
        AuthorizationOutcome::Indeterminate(_) => return Err(RepositoryApiError::Internal),
    }

    let view = state
        .repository_reader
        .find_by_hrn(context.account().as_str(), &hrn)
        .await
        .map_err(|_| RepositoryApiError::Unavailable)?
        .ok_or(RepositoryApiError::NotFound)?;

    // Do not trust a row's self-declared account/HRN: revalidate it against
    // the verified scope before serializing it.
    if view.account != context.account().as_str() || view.hrn != hrn.to_string() {
        return Err(RepositoryApiError::NotFound);
    }

    Ok(Json(view).into_response())
}

/// The list of the repositories of the account.
async fn the_repositories(
    state: &AppState,
    context: &VerifiedContext,
    limit: usize,
    offset: usize,
) -> Result<Response, RepositoryApiError> {
    let rows = state
        .repository_reader
        .list(context.account().as_str())
        .await
        .map_err(|_| RepositoryApiError::Unavailable)?;
    let permitted = the_permitted(state, context, rows)?;

    // The page is cut **after** the permission is decided: a page cut
    // before would count what the principal may not read, and a total
    // that counts it says how much there is to see.
    let total = permitted.len();
    let items = permitted.into_iter().skip(offset).take(limit).collect();

    Ok(Json(RepositoryListView {
        total,
        items,
        limit,
        offset,
    })
    .into_response())
}

/// The repositories the principal may read.
///
/// Cedar decides one by one. A denial is a decision and the repository does
/// not come out; and it is not counted either. A decision that could not be
/// taken is not a decision, and the list does not fill that silence with an
/// empty answer: an empty list says that there is nothing, and what
/// happened is that nobody could ask.
///
/// A row is also revalidated against the verified scope before it is
/// serialized, the same way the read by `hrn` does it: a row that belongs
/// to another account is not shown, and a row whose canonical name and
/// whose account disagree with each other is corrupt.
fn the_permitted(
    state: &AppState,
    context: &VerifiedContext,
    repositories: Vec<RepositoryView>,
) -> Result<Vec<RepositoryView>, RepositoryApiError> {
    let action = crate::actions::read_repository();
    let mut permitted = Vec::with_capacity(repositories.len());
    for repository in repositories {
        // A row whose canonical name cannot be read is corrupt: it says
        // what it is and it cannot be said. It is not a repository that is
        // hidden, and it is not an empty list either.
        let hrn = Hrn::from_string(&repository.hrn).ok_or(RepositoryApiError::Corrupt)?;
        // The row has to be the account's. A row of another account is not
        // shown: it does not exist for this principal, and it is not
        // counted either.
        if repository.account != context.account().as_str() {
            continue;
        }
        // The row has to agree with itself: the account of its canonical
        // name is the account of the row. A row that disagrees with itself
        // is corrupt, and a corrupt row is said, not hidden.
        if hrn.account_id() != repository.account {
            return Err(RepositoryApiError::Corrupt);
        }
        let outcome = state
            .authorize_resource
            .execute_canonical(context.clone(), action.clone(), &hrn)
            .map_err(|_| RepositoryApiError::Internal)?;
        match outcome {
            AuthorizationOutcome::Permit => permitted.push(repository),
            AuthorizationOutcome::Deny(_) => {}
            AuthorizationOutcome::Indeterminate(_) => return Err(RepositoryApiError::Internal),
        }
    }
    Ok(permitted)
}

/// The query of the route.
#[derive(Debug)]
struct RepositoriesQuery {
    /// The repository that is asked for, when the caller names one.
    hrn: Option<String>,
    /// The size of the page, as it was written.
    limit: Option<String>,
    /// Where the page starts, as it was written.
    offset: Option<String>,
}

/// The query, as the caller writes it.
///
/// The three parameters are the ones the contract declares. A parameter that
/// is named twice does not say which of the two it means: the query is not
/// coherent, and the answer is the same one.
///
/// A parameter that the route does not know is ignored: a client may add
/// one, and the route answers with what it does know.
///
/// The value of `limit` and `offset` is **not** checked here: only the read
/// that uses them looks at them. A read by `hrn` does not become a `400`
/// because the client sent a page that does not apply to it.
fn the_query(raw: Option<&str>) -> Result<RepositoriesQuery, RepositoryApiError> {
    let mut hrn: Option<String> = None;
    let mut limit: Option<String> = None;
    let mut offset: Option<String> = None;
    // A request without a query is a request for the list, and not a
    // malformed one: the query is what says which of the two reads is
    // asked for.
    let values = match raw {
        None => Vec::new(),
        Some(raw) => the_query_values(Some(raw)).ok_or_else(|| {
            RepositoryApiError::BadRequest("the query is not readable".to_string())
        })?,
    };
    for (key, value) in values {
        match key.as_str() {
            "hrn" => {
                if hrn.is_some() {
                    return Err(RepositoryApiError::BadRequest(
                        "the hrn is named twice".to_string(),
                    ));
                }
                hrn = Some(value);
            }
            "limit" => {
                if limit.is_some() {
                    return Err(RepositoryApiError::BadRequest(
                        "the limit is named twice".to_string(),
                    ));
                }
                limit = Some(value);
            }
            "offset" => {
                if offset.is_some() {
                    return Err(RepositoryApiError::BadRequest(
                        "the offset is named twice".to_string(),
                    ));
                }
                offset = Some(value);
            }
            _ => {}
        }
    }
    Ok(RepositoriesQuery { hrn, limit, offset })
}

/// The size of the page, as the caller asks for it.
///
/// The contract promises a page of at least one and of at most a hundred. A
/// page outside of that is a request that is not coherent.
fn the_page_size(value: Option<&str>) -> Result<usize, RepositoryApiError> {
    let Some(value) = value else {
        return Ok(DEFAULT_LIMIT);
    };
    let size: usize = value
        .parse()
        .map_err(|_| RepositoryApiError::BadRequest("the limit is not a number".to_string()))?;
    if size == 0 || size > MAX_LIMIT {
        return Err(RepositoryApiError::BadRequest(
            "the limit is outside of what the contract promises".to_string(),
        ));
    }
    Ok(size)
}

/// Where the page starts, as the caller asks for it.
fn the_offset(value: Option<&str>) -> Result<usize, RepositoryApiError> {
    let Some(value) = value else {
        return Ok(0);
    };
    value
        .parse()
        .map_err(|_| RepositoryApiError::BadRequest("the offset is not a number".to_string()))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateRepositoryRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
}

/// `POST /api/v1/repositories`
pub async fn create_repository(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request<Body>,
) -> Result<Response, RepositoryApiError> {
    let context = the_context(&state, &headers)
        .await
        .map_err(|failure| match failure {
            crate::handlers::Authentication::Unauthorized => RepositoryApiError::Unauthorized,
            crate::handlers::Authentication::Unavailable => RepositoryApiError::Unavailable,
        })?;
    let correlation_id = correlation_id(&headers, &state.identity);

    // Parse the body only after authentication: a missing/invalid token is
    // always 401, even if the JSON is malformed.
    let body = to_bytes(request.into_body(), 1024 * 1024)
        .await
        .map_err(|_| RepositoryApiError::BadRequest("the body is not readable".to_string()))?;
    let request: CreateRepositoryRequest = serde_json::from_slice(&body)
        .map_err(|_| RepositoryApiError::BadRequest("the body is not a repository".to_string()))?;

    let name = request.name.trim();
    if !valid_repository_name(name) {
        return Err(RepositoryApiError::BadRequest(
            "the name of the repository is not valid".to_string(),
        ));
    }
    if request
        .description
        .as_deref()
        .is_some_and(|description| description.len() > 1024)
    {
        return Err(RepositoryApiError::BadRequest(
            "the description of the repository is too long".to_string(),
        ));
    }
    let region = request.region.as_deref().unwrap_or("global");
    if !valid_region(region) {
        return Err(RepositoryApiError::BadRequest(
            "the region of the repository is not valid".to_string(),
        ));
    }

    let hrn = Hrn::with_region(
        "hodei".to_string(),
        "artifacts".to_string(),
        region.to_string(),
        context.account().as_str().to_string(),
        "repository".to_string(),
        name.to_string(),
    );
    let action = crate::actions::create_repository();
    let outcome =
        match state
            .authorize_resource
            .execute_canonical(context.clone(), action.clone(), &hrn)
        {
            Ok(outcome) => outcome,
            Err(_) => {
                record_audit(
                    &state,
                    context.principal().subject().as_str(),
                    context.account().as_str(),
                    &action.to_string(),
                    &hrn.to_string(),
                    AuditResult::Failed,
                    &correlation_id,
                );
                return Err(RepositoryApiError::Internal);
            }
        };
    // A denial is a decision; a decision that could not be taken is not
    // one, and it does not become a refusal that says more than what
    // happened.
    match outcome {
        hodei_authz::AuthorizationOutcome::Permit => {}
        hodei_authz::AuthorizationOutcome::Deny(_) => {
            record_audit(
                &state,
                context.principal().subject().as_str(),
                context.account().as_str(),
                &action.to_string(),
                &hrn.to_string(),
                AuditResult::Denied,
                &correlation_id,
            );
            return Err(RepositoryApiError::Forbidden);
        }
        hodei_authz::AuthorizationOutcome::Indeterminate(_) => {
            record_audit(
                &state,
                context.principal().subject().as_str(),
                context.account().as_str(),
                &action.to_string(),
                &hrn.to_string(),
                AuditResult::Failed,
                &correlation_id,
            );
            return Err(RepositoryApiError::Internal);
        }
    }

    let view = RepositoryView::new(
        hrn.to_string(),
        context.account().as_str().to_string(),
        name.to_string(),
        request.description,
    );
    let created = match state
        .repository_writer
        .create(context.account().as_str(), &hrn, view)
        .await
    {
        Ok(created) => created,
        Err(error) => {
            let (result, api_error) = match error {
                crate::repository::RepositoryWriteError::AlreadyExists => {
                    (AuditResult::Conflict, RepositoryApiError::Conflict)
                }
                crate::repository::RepositoryWriteError::Unavailable => {
                    (AuditResult::Failed, RepositoryApiError::Unavailable)
                }
                crate::repository::RepositoryWriteError::InvalidRequest => {
                    (AuditResult::Failed, RepositoryApiError::Internal)
                }
            };
            record_audit(
                &state,
                context.principal().subject().as_str(),
                context.account().as_str(),
                &action.to_string(),
                &hrn.to_string(),
                result,
                &correlation_id,
            );
            return Err(api_error);
        }
    };

    record_audit(
        &state,
        context.principal().subject().as_str(),
        context.account().as_str(),
        &action.to_string(),
        &hrn.to_string(),
        AuditResult::Allowed,
        &correlation_id,
    );
    let mut response = (StatusCode::CREATED, Json(created)).into_response();
    response.headers_mut().insert(
        header::HeaderName::from_static("x-request-id"),
        HeaderValue::from_str(&correlation_id).map_err(|_| RepositoryApiError::Internal)?,
    );
    Ok(response)
}

fn valid_repository_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name != "."
        && name != ".."
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

fn valid_region(region: &str) -> bool {
    !region.is_empty()
        && region.len() <= 64
        && region.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
}
