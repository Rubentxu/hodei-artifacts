//! Integrity scrub of the registry.
//!
//! `GET /scrub` reads every blob of the store and every reference of the
//! catalog and answers with the findings: a corrupt blob, a reference with no
//! blob, a blob nobody references. It never writes and never repairs: a
//! repair takes bytes from a verified backup, and that is an operator
//! decision made with `hodei-recovery`, not something a request does.
//!
//! The route is its own resource for the administrative gate, so a policy can
//! grant it apart from the sweep. Only one scrub runs at a time; a second one
//! is answered with 409.

use crate::app_state::AppState;
use crate::recovery::{RecoveryError, ScrubReport};
use axum::{
    Extension, Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use hodei_authz::VerifiedContext;

/// The scrub of the whole registry.
pub async fn report(
    State(state): State<AppState>,
    Extension(_context): Extension<VerifiedContext>,
) -> Result<Json<ScrubReport>, ScrubApiError> {
    let scrub = state.scrub.clone().ok_or_else(|| {
        ScrubApiError::new(
            StatusCode::NOT_IMPLEMENTED,
            "the scrub needs the durable catalog and a store that can list its blobs",
        )
    })?;
    match scrub.try_run().await {
        Ok(Some(report)) => Ok(Json(report)),
        Ok(None) => Err(ScrubApiError::new(
            StatusCode::CONFLICT,
            "a scrub is already running",
        )),
        Err(error) => {
            // The detail is for the log: a path is not for the client.
            tracing::error!(%error, "the scrub failed");
            let status = match error {
                RecoveryError::Invalid(_) => StatusCode::BAD_REQUEST,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            Err(ScrubApiError::new(status, "the scrub failed"))
        }
    }
}

/// How a failure of the scrub reaches the client.
#[derive(Debug)]
pub struct ScrubApiError {
    status: StatusCode,
    message: String,
}

impl ScrubApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl IntoResponse for ScrubApiError {
    fn into_response(self) -> Response {
        let body = Json(serde_json::json!({
            "error": self.message,
            "status": self.status.as_u16(),
        }));
        (self.status, body).into_response()
    }
}
