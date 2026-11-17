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

use axum::{Json, extract::State};
use hodei_catalog::RepositoryFormat;

use crate::app_state::AppState;

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
/// It needs no credential and no permission: it says what the service
/// can do, not what the caller may do. What the caller may do is decided
/// by Cedar, on the resource, when the caller asks for it.
pub async fn get_capabilities(State(_state): State<AppState>) -> Json<Capabilities> {
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
    Json(Capabilities { formats })
}
