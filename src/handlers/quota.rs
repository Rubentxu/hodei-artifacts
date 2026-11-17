//! What one account has, said to its owner and to nobody else.
//!
//! The quota is a gate, and a gate that refuses without saying why is a
//! mystery, not a contract. This endpoint is the saying: the stored
//! bytes, the bytes reserved by uploads in flight, the number of active
//! uploads and the limits the deployment declared — for the **caller's
//! own account**, read through the verified context. There is no route
//! to another account and no parameter to name one: what the ledger of
//! someone else holds is not this caller's to know.

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};

use crate::app_state::AppState;
use crate::handlers::{error_body, the_context};

/// What can fail when the usage is asked for.
#[derive(Debug)]
pub enum UsageApiError {
    /// The request could not be authenticated.
    Unauthorized,
    /// The service cannot answer.
    Unavailable,
}

impl IntoResponse for UsageApiError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                error_body("unauthenticated", "unauthorized"),
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                error_body("unavailable", "the usage is not available"),
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

impl From<crate::handlers::Authentication> for UsageApiError {
    fn from(error: crate::handlers::Authentication) -> Self {
        match error {
            crate::handlers::Authentication::Unauthorized => Self::Unauthorized,
            crate::handlers::Authentication::Unavailable => Self::Unavailable,
        }
    }
}

/// `GET /api/v1/account/usage`
///
/// The ledger of the caller's own account, as it is now: a deployment
/// without quotas says so with nulls instead of inventing limits, and
/// an account that has never uploaded reads zeros instead of a 404 —
/// having nothing is an answer, not an absence.
pub async fn get_usage(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, UsageApiError> {
    let context = the_context(&state, &headers).await?;
    let account = context.account().as_str();
    let usage = state.quota.usage(account);
    Ok(Json(serde_json::json!({
        "account": account,
        "stored_bytes": usage.stored_bytes,
        "in_flight_bytes": usage.in_flight_bytes,
        "active_uploads": usage.active_uploads,
        "limits": {
            "max_stored_bytes_per_account": usage.limits.max_stored_bytes_per_account,
            "max_concurrent_uploads_per_account": usage.limits.max_concurrent_uploads_per_account,
            "retry_after_secs": usage.limits.retry_after_secs,
        },
        "note": "the ledger is in memory and rate-shaping: a restart forgets it, and it is not a billing book",
    })))
}
