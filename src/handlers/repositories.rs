//! Repository read/write endpoints.
//!
//! This is the first productive use of the shared authorizer: the handler
//! verifies the bearer credential, derives the account from the verified
//! principal, refuses a cross-account HRN with a uniform `404`, authorizes the
//! read, and then loads the repository through the read-only port.

use axum::{
    Json,
    body::{Body, to_bytes},
    extract::{RawQuery, State},
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    response::{IntoResponse, Response},
};
use kernel::Hrn;

use crate::audit::AuditResult;
use serde::Deserialize;

use crate::app_state::AppState;
use crate::handlers::{correlation_id, record_audit, the_context, the_one_value};
use crate::repository::RepositoryView;

#[derive(Debug)]
pub enum RepositoryApiError {
    Unauthorized,
    Forbidden,
    NotFound,
    BadRequest,
    Conflict,
    Internal,
    Unavailable,
}

impl IntoResponse for RepositoryApiError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not found"),
            Self::BadRequest => (StatusCode::BAD_REQUEST, "bad request"),
            Self::Conflict => (StatusCode::CONFLICT, "conflict"),
            Self::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "internal error"),
            Self::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
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

/// `GET /api/v1/repositories?hrn=...`
pub async fn get_repository(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Result<Json<RepositoryView>, RepositoryApiError> {
    // Authentication is the first gate. `RawQuery` cannot fail extraction, so
    // a missing/duplicated hrn cannot turn into a 400 before the 401.
    let context = the_context(&state, &headers)
        .await
        .map_err(|failure| match failure {
            crate::handlers::Authentication::Unauthorized => RepositoryApiError::Unauthorized,
            crate::handlers::Authentication::Unavailable => RepositoryApiError::Unavailable,
        })?;

    let query_hrn =
        the_one_value(raw_query.as_deref(), "hrn").ok_or(RepositoryApiError::BadRequest)?;
    let hrn = Hrn::from_string(&query_hrn).ok_or(RepositoryApiError::BadRequest)?;

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
        .map_err(|_| RepositoryApiError::BadRequest)?;
    if !outcome.allows_access() {
        return Err(RepositoryApiError::Forbidden);
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

    Ok(Json(view))
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
        .map_err(|_| RepositoryApiError::BadRequest)?;
    let request: CreateRepositoryRequest =
        serde_json::from_slice(&body).map_err(|_| RepositoryApiError::BadRequest)?;

    let name = request.name.trim();
    if !valid_repository_name(name) {
        return Err(RepositoryApiError::BadRequest);
    }
    if request
        .description
        .as_deref()
        .is_some_and(|description| description.len() > 1024)
    {
        return Err(RepositoryApiError::BadRequest);
    }
    let region = request.region.as_deref().unwrap_or("global");
    if !valid_region(region) {
        return Err(RepositoryApiError::BadRequest);
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
    if !outcome.allows_access() {
        let result = match outcome {
            hodei_authz::AuthorizationOutcome::Deny(_) => AuditResult::Denied,
            hodei_authz::AuthorizationOutcome::Indeterminate(_) => AuditResult::Failed,
            hodei_authz::AuthorizationOutcome::Permit => AuditResult::Allowed,
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
        return Err(RepositoryApiError::Forbidden);
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
