//! Repository read endpoints.
//!
//! This is the first productive use of the shared authorizer: the handler
//! verifies the bearer credential, derives the account from the verified
//! principal, refuses a cross-account HRN with a uniform `404`, authorizes the
//! read, and then loads the repository through the read-only port.

use axum::{
    Json,
    extract::{RawQuery, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use hodei_authz::{ActionId, Credentials, VerifiedContext, verify_principal};
use kernel::Hrn;

use crate::app_state::AppState;
use crate::repository::RepositoryView;

#[derive(Debug)]
pub enum RepositoryApiError {
    Unauthorized,
    Forbidden,
    NotFound,
    BadRequest,
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
    let token = bearer_token(&headers).ok_or(RepositoryApiError::Unauthorized)?;
    let principal =
        match verify_principal(state.authenticator.as_ref(), &Credentials::bearer(token)).await {
            Ok(principal) => principal,
            Err(hodei_authz::AuthenticationError::ProviderUnavailable(_)) => {
                return Err(RepositoryApiError::Unavailable);
            }
            Err(_) => return Err(RepositoryApiError::Unauthorized),
        };
    let context = VerifiedContext::from_principal(principal);

    let query_hrn = parse_hrn_query(raw_query.as_deref()).ok_or(RepositoryApiError::BadRequest)?;
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

    let action = ActionId::new("artifacts", "ReadRepository")
        .map_err(|_| RepositoryApiError::Unavailable)?;
    let outcome = state
        .authorize_resource
        .execute_canonical(context.clone(), action, &hrn)
        .map_err(|_| RepositoryApiError::Internal)?;
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

/// Extract exactly one `hrn` from a raw query string and percent-decode it.
fn parse_hrn_query(raw: Option<&str>) -> Option<String> {
    let raw = raw?;
    let mut values = Vec::new();
    for pair in raw.split('&') {
        let (key, value) = pair.split_once('=')?;
        if key == "hrn" {
            values.push(percent_decode(value)?);
        }
    }
    if values.len() != 1 {
        return None;
    }
    values.pop()
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let high = hex(*bytes.get(index + 1)?)?;
                let low = hex(*bytes.get(index + 2)?)?;
                out.push((high << 4) | low);
                index += 3;
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    let token = token.trim();
    if !scheme.eq_ignore_ascii_case("Bearer") || token.is_empty() {
        return None;
    }
    Some(token.to_string())
}
