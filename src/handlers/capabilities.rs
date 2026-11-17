//! What a client can create.
//!
//! The console has to offer the modalities that the API serves, and it
//! learns them here: the formats and the modalities that can be declared,
//! asked to the API that will have to serve them.
//!
//! It is the API that answers, and not a table of the frontend: a table
//! of the frontend is a second truth about the same thing, and a second
//! truth drifts without anybody noticing. The rule of what a format
//! admits lives in the catalog, and this endpoint is what it says out
//! loud.
//!
//! # The credential is asked for
//!
//! It is asked for, and it is not asked for a permission: what it says
//! is what the service can do, and not what the caller may do. What the
//! caller may do is decided by Cedar, on the resource, when the caller
//! asks for it. But the surface is the console, and the console is of
//! the people who are signed in, and the answer says what this
//! deployment serves.

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use hodei_catalog::RepositoryFormat;

use crate::app_state::AppState;
use crate::handlers::{error_body, the_context};

/// What can fail when the capabilities are asked for.
#[derive(Debug)]
pub enum CapabilitiesApiError {
    /// The request could not be authenticated.
    Unauthorized,
    /// The service cannot answer.
    Unavailable,
}

impl IntoResponse for CapabilitiesApiError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                error_body("unauthenticated", "unauthorized"),
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                error_body("unavailable", "the capabilities are not available"),
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

/// What can be created.
#[derive(Debug, serde::Serialize)]
pub struct Capabilities {
    /// The formats that this release serves, and the modalities of each
    /// one.
    pub formats: Vec<FormatCapability>,
}

/// What a format admits.
#[derive(Debug, serde::Serialize)]
pub struct FormatCapability {
    /// The name of the format, the one that goes on the wire.
    pub name: &'static str,
    /// The modalities it can be declared in.
    pub modes: Vec<&'static str>,
}

/// `GET /api/v1/capabilities`
///
/// The formats that can be declared and the modalities of each one.
///
/// It asks for the credential and not for a permission: it says what the
/// service can do, not what the caller may do. What the caller may do is
/// decided by Cedar, on the resource, when the caller asks for it.
pub async fn get_capabilities(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Capabilities>, CapabilitiesApiError> {
    the_context(&state, &headers)
        .await
        .map_err(|failure| match failure {
            crate::handlers::Authentication::Unauthorized => CapabilitiesApiError::Unauthorized,
            crate::handlers::Authentication::Unavailable => CapabilitiesApiError::Unavailable,
        })?;

    let formats = RepositoryFormat::served()
        .iter()
        .map(|format| FormatCapability {
            name: format.as_str(),
            modes: format
                .serves_modes()
                .iter()
                .map(|mode| mode.as_str())
                .collect(),
        })
        .collect();
    Ok(Json(Capabilities { formats }))
}
