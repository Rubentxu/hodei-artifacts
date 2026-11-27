//! Verified migration of blobs to the configured destination (HSP-163, slice):
//! `POST /api/v1/storage/migrate`.
//!
//! It copies the named blobs of a repository from the content store to the
//! destination store the **deployment** configured; the request never names a
//! destination. The rules:
//!
//! - Cedar decides **who** with the dedicated action `ManageStorageMigration`
//!   on the repository; no other action implies it, and a denial is audited;
//! - the repository belongs to the caller's account and **every** digest named
//!   must be referenced by a row of that repository: otherwise the whole
//!   request is a 404, indistinguishable from absence, and nothing is moved, so
//!   the route cannot probe other accounts;
//! - bytes are hashed before they are written and verified after (see
//!   [`crate::recovery::migrate`]); the origin is never written or deleted;
//! - repeating the request is harmless and resumes what was left (`deferred`,
//!   a destination that failed): verified digests are reported
//!   `already_present`;
//! - the cost is bounded (digests per request, bytes per blob and per request,
//!   concurrent migrations -> 429) and every failure is closed: nothing is ever
//!   reported verified that was not read back.
//!
//! Answers: `200` everything named is verified in the destination; `207` some
//! is and some is not (the report says which); `503` nothing is verified and a
//! store failed; `409` nothing is verified for another reason (absent or
//! corrupt origin, blob too large). The body is always the report.

use axum::{
    Json,
    body::Body,
    extract::State,
    http::{HeaderMap, Request, StatusCode},
    response::{IntoResponse, Response},
};
use hodei_evidence::ContentDigest;
use serde::Deserialize;

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::correlation_id;
use crate::handlers::freezes::{FreezeFailure, audit, gate};
use crate::handlers::repair::ANY_FORMAT;
use crate::recovery::{MigrateError, Moved};
use crate::resolution::{ResolutionError, the_context, the_repository_in};

/// The body of `POST /api/v1/storage/migrate`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrateBody {
    /// The repository whose blobs are migrated.
    pub repository: String,
    /// The contents, each `sha256:<hex>`.
    pub digests: Vec<String>,
}

fn parse_digest(text: &str) -> Option<ContentDigest> {
    let hex = text.strip_prefix("sha256:")?;
    (hex.len() == 64
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    .then(|| ContentDigest::sha256(hex))
}

/// `POST /api/v1/storage/migrate`
pub async fn migrate(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request<Body>,
) -> Result<Response, FreezeFailure> {
    let context = the_context(&state, &headers).await?;
    let correlation = correlation_id(&headers, &state.identity);
    let bytes = axum::body::to_bytes(request.into_body(), 32 * 1024)
        .await
        .map_err(|_| ResolutionError::BadRequest("the body is too large"))?;
    let body: MigrateBody = serde_json::from_slice(&bytes)
        .map_err(|_| ResolutionError::BadRequest("the body is not a valid migration request"))?;
    if body.digests.is_empty()
        || body.digests.len() > crate::recovery::migrate::MAX_BLOBS_PER_REQUEST
    {
        return Err(
            ResolutionError::BadRequest("a migration names between 1 and 64 digests").into(),
        );
    }
    let mut digests = Vec::with_capacity(body.digests.len());
    for text in &body.digests {
        digests.push(parse_digest(text).ok_or(ResolutionError::BadRequest(
            "a digest is not sha256:<64 hex>",
        ))?);
    }
    // The repository is the caller's own or it does not exist; Cedar decides
    // afterwards and a denial never degrades to a 404.
    let origin = the_repository_in(&state, &context, &body.repository, &ANY_FORMAT).await?;
    let action = crate::actions::manage_storage_migration();
    gate(&state, &context, &origin, action.clone(), &correlation)?;

    // Past the gate every outcome is audited.
    let audited = action.to_string();
    let account = context.account().as_str().to_string();
    let resource = format!("{}[{}]", body.repository, digests.len());
    let record = |result: AuditResult| {
        audit(&state, &context, &audited, &resource, result, &correlation);
    };
    let (Some(migrator), Some(gc)) = (state.migrator.clone(), state.gc.clone()) else {
        record(AuditResult::Failed);
        return Err(ResolutionError::NotImplemented(
            "storage migration needs a configured destination and the durable catalog: a migration that cannot verify its bytes is not offered",
        )
        .into());
    };

    // Only what this repository references can be migrated through it, and
    // one stranger digest refuses the whole request before anything moves.
    for digest in &digests {
        match gc
            .references_digest(&account, &body.repository, digest)
            .await
        {
            Ok(true) => {}
            Ok(false) => {
                record(AuditResult::Denied);
                return Err(ResolutionError::NotFound.into());
            }
            Err(error) => {
                tracing::error!(%error, "the migration could not read the references");
                record(AuditResult::Failed);
                return Err(ResolutionError::Unavailable.into());
            }
        }
    }

    match migrator.migrate(&digests).await {
        Ok(report) => {
            let any_failed = report
                .entries
                .iter()
                .any(|entry| matches!(entry.moved, Moved::SourceFailed | Moved::DestinationFailed));
            let verified = report.migrated + report.already_present;
            let status = if report.pending == 0 {
                StatusCode::OK
            } else if verified > 0 {
                StatusCode::MULTI_STATUS
            } else if any_failed {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::CONFLICT
            };
            record(if report.pending == 0 {
                AuditResult::Allowed
            } else {
                AuditResult::Failed
            });
            Ok((status, Json(report)).into_response())
        }
        Err(error) => {
            record(AuditResult::Failed);
            let (status, code) = match error {
                MigrateError::TooMany { .. } => (StatusCode::BAD_REQUEST, "E_MIGRATE_TOO_MANY"),
                MigrateError::Busy => (StatusCode::TOO_MANY_REQUESTS, "E_MIGRATE_BUSY"),
                MigrateError::Failed => (StatusCode::SERVICE_UNAVAILABLE, "E_MIGRATE_FAILED"),
            };
            let payload = serde_json::json!({"error": code, "message": error.to_string()});
            Ok((status, Json(payload)).into_response())
        }
    }
}
