//! Self-healing of one blob (HSP-164, slice): `POST /api/v1/repair`.
//!
//! The scrub finds a corrupt or missing blob; this gives it its bytes back from
//! the **repair source the deployment configured**. The request never names a
//! source. The rules:
//!
//! - Cedar decides **who** with the dedicated action `ManageStorageRepair` on
//!   the repository; no other action implies it, and a denial is audited;
//! - the repository belongs to the caller's account, and the digest must be
//!   referenced by a row of **that repository**: anything else is a 404,
//!   indistinguishable from absence, so the route cannot probe other accounts;
//! - the bytes are hashed before they are written and a blob that verifies is
//!   never touched (see [`crate::recovery::repair`]);
//! - repeating the request is harmless: the second answer is `already_sound`;
//! - every failure is closed: no copy in the source is a 409, a source whose
//!   bytes do not hash to the digest a 422 (nothing written), an unreadable
//!   source or store a 503. Each attempt is audited.

use axum::{
    Json,
    body::Body,
    extract::State,
    http::{HeaderMap, Request, StatusCode},
    response::{IntoResponse, Response},
};
use hodei_catalog::RepositoryFormat;
use hodei_evidence::ContentDigest;
use serde::Deserialize;

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::correlation_id;
use crate::handlers::freezes::{FreezeFailure, audit, gate};
use crate::recovery::RepairError;
use crate::resolution::{ResolutionError, the_context, the_repository_in};

/// The body of `POST /api/v1/repair`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairBody {
    /// The repository whose blob is repaired.
    pub repository: String,
    /// The content, `sha256:<hex>`.
    pub digest: String,
}

/// Every format of the catalog: a blob is repaired whatever served it.
pub(crate) const ANY_FORMAT: [RepositoryFormat; 14] = [
    RepositoryFormat::Raw,
    RepositoryFormat::Oci,
    RepositoryFormat::Maven2,
    RepositoryFormat::Npm,
    RepositoryFormat::PyPi,
    RepositoryFormat::Cargo,
    RepositoryFormat::NuGet,
    RepositoryFormat::Go,
    RepositoryFormat::Helm,
    RepositoryFormat::Apt,
    RepositoryFormat::Rpm,
    RepositoryFormat::Terraform,
    RepositoryFormat::Conan,
    RepositoryFormat::Galaxy,
];

fn parse_digest(text: &str) -> Option<ContentDigest> {
    let hex = text.strip_prefix("sha256:")?;
    (hex.len() == 64
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    .then(|| ContentDigest::sha256(hex))
}

/// `POST /api/v1/repair`
pub async fn repair(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request<Body>,
) -> Result<Response, FreezeFailure> {
    let context = the_context(&state, &headers).await?;
    let correlation = correlation_id(&headers, &state.identity);
    let bytes = axum::body::to_bytes(request.into_body(), 4 * 1024)
        .await
        .map_err(|_| ResolutionError::BadRequest("the body is too large"))?;
    let body: RepairBody = serde_json::from_slice(&bytes)
        .map_err(|_| ResolutionError::BadRequest("the body is not a valid repair request"))?;
    let digest = parse_digest(&body.digest).ok_or(ResolutionError::BadRequest(
        "the digest is not sha256:<64 hex>",
    ))?;
    // The repository is the caller's own or it does not exist; Cedar decides
    // afterwards and a denial never degrades to a 404.
    let origin = the_repository_in(&state, &context, &body.repository, &ANY_FORMAT).await?;
    let action = crate::actions::manage_storage_repair();
    gate(&state, &context, &origin, action.clone(), &correlation)?;

    // Past the gate every outcome is audited: what was refused, what failed
    // and what was done.
    let audited = action.to_string();
    let account = context.account().as_str().to_string();
    let resource = format!("{}@{}", body.repository, digest);
    let record = |result: AuditResult| {
        audit(&state, &context, &audited, &resource, result, &correlation);
    };
    let (Some(repairer), Some(gc)) = (state.repairer.clone(), state.gc.clone()) else {
        record(AuditResult::Failed);
        return Err(ResolutionError::NotImplemented(
            "self-healing needs a configured repair source and the durable catalog: a repair that cannot verify its bytes is not offered",
        )
        .into());
    };

    // Only what this repository references can be repaired through it.
    let referenced = match gc
        .references_digest(&account, &body.repository, &digest)
        .await
    {
        Ok(referenced) => referenced,
        Err(error) => {
            tracing::error!(%error, "the repair could not read the references");
            record(AuditResult::Failed);
            return Err(ResolutionError::Unavailable.into());
        }
    };
    if !referenced {
        record(AuditResult::Denied);
        return Err(ResolutionError::NotFound.into());
    }

    match repairer.repair(&digest).await {
        Ok(outcome) => {
            record(AuditResult::Allowed);
            let mut answer = serde_json::to_value(&outcome).unwrap_or_default();
            answer["digest"] = serde_json::Value::String(digest.to_string());
            Ok((StatusCode::OK, Json(answer)).into_response())
        }
        Err(error) => {
            record(AuditResult::Failed);
            let (status, code) = match error {
                RepairError::SourceUnavailable => (StatusCode::CONFLICT, "E_REPAIR_UNAVAILABLE"),
                RepairError::SourceMismatch => {
                    (StatusCode::UNPROCESSABLE_ENTITY, "E_REPAIR_SOURCE_MISMATCH")
                }
                RepairError::TooLarge { .. } => {
                    (StatusCode::PAYLOAD_TOO_LARGE, "E_REPAIR_TOO_LARGE")
                }
                RepairError::Raced => (StatusCode::CONFLICT, "E_REPAIR_RACED"),
                RepairError::Unsupported => (StatusCode::NOT_IMPLEMENTED, "E_REPAIR_UNSUPPORTED"),
                RepairError::TimedOut => (StatusCode::GATEWAY_TIMEOUT, "E_REPAIR_TIMEOUT"),
                RepairError::SourceFailed | RepairError::StoreFailed => {
                    (StatusCode::SERVICE_UNAVAILABLE, "E_REPAIR_FAILED")
                }
            };
            let payload = serde_json::json!({
                "error": code,
                "message": error.to_string(),
                "digest": digest.to_string(),
            });
            Ok((status, Json(payload)).into_response())
        }
    }
}
