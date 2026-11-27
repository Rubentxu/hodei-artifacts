//! Relocation of reads and verified retirement of the origin copy (HSP-325):
//! `POST /api/v1/storage/relocate`.
//!
//! `{repository, operation, digests}` where `operation` is `switch` (reads go to
//! the destination the deployment configured), `retire` (the origin copy is
//! moved aside, never deleted) or `rollback` (reads go back to the origin). The
//! rules:
//!
//! - Cedar decides **who** with the dedicated action `ManageStorageRelocation`
//!   on the repository; no other action implies it, and a denial is audited;
//! - the repository belongs to the caller's account and **every** digest named
//!   must be referenced by a row of that repository: otherwise the whole
//!   request is a 404, indistinguishable from absence, and nothing is touched;
//! - the work (see [`crate::recovery::relocate`]) runs in a task of its own and
//!   is audited there, so a client that hangs up leaves a trace and no place
//!   taken; everything fails closed and the origin copy is never deleted.
//!
//! Answers: `200` every digest is where the operation leads; `207` some are;
//! `504` the deadline passed; `503` none is and a store, the journal or the
//! holds failed; `409` none is for another reason (a hold, a destination that
//! does not verify, a lease held by someone else...). The body is the report.

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
use crate::recovery::{RelocateError, RelocateOperation};
use crate::resolution::{ResolutionError, the_context, the_repository_in};

/// The body of `POST /api/v1/storage/relocate`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelocateBody {
    /// The repository whose blobs are relocated.
    pub repository: String,
    /// What to do with them.
    pub operation: RelocateOperation,
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

/// How long the single pass over the references may take.
const REFERENCES_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// `POST /api/v1/storage/relocate`
pub async fn relocate(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request<Body>,
) -> Result<Response, FreezeFailure> {
    let context = the_context(&state, &headers).await?;
    let correlation = correlation_id(&headers, &state.identity);
    let bytes = axum::body::to_bytes(request.into_body(), 32 * 1024)
        .await
        .map_err(|_| ResolutionError::BadRequest("the body is too large"))?;
    let body: RelocateBody = serde_json::from_slice(&bytes)
        .map_err(|_| ResolutionError::BadRequest("the body is not a valid relocation request"))?;
    if body.digests.is_empty()
        || body.digests.len() > crate::recovery::relocate::MAX_BLOBS_PER_REQUEST
    {
        return Err(
            ResolutionError::BadRequest("a relocation names between 1 and 64 digests").into(),
        );
    }
    let mut digests = Vec::with_capacity(body.digests.len());
    for text in &body.digests {
        digests.push(parse_digest(text).ok_or(ResolutionError::BadRequest(
            "a digest is not sha256:<64 hex>",
        ))?);
    }
    let origin = the_repository_in(&state, &context, &body.repository, &ANY_FORMAT).await?;
    let action = crate::actions::manage_storage_relocation();
    gate(&state, &context, &origin, action.clone(), &correlation)?;

    let account = context.account().as_str().to_string();
    let repository = body.repository.clone();
    let operation = body.operation;
    let task = tokio::spawn(async move {
        let audited = action.to_string();
        let record = |resource: &str, result: AuditResult| {
            audit(&state, &context, &audited, resource, result, &correlation);
        };
        let resource = format!("{repository}[{}] op={}", digests.len(), operation.as_str());
        let (Some(relocator), Some(gc)) = (state.relocator.clone(), state.gc.clone()) else {
            record(&resource, AuditResult::Failed);
            return Err(ResolutionError::NotImplemented(
                "storage relocation needs a configured destination, the location journal and the durable catalog",
            )
            .into());
        };
        let place = match relocator.reserve(&account) {
            Ok(place) => place,
            Err(error) => {
                record(&resource, AuditResult::Failed);
                return Ok(refused(error));
            }
        };
        // Only what this repository references can be moved through it, and
        // one stranger digest refuses the whole request before anything moves.
        let referenced = tokio::time::timeout(
            REFERENCES_TIMEOUT,
            gc.references_all(&account, &repository, &digests),
        )
        .await;
        match referenced {
            Ok(Ok(true)) => {}
            Ok(Ok(false)) => {
                record(&resource, AuditResult::Denied);
                return Err(ResolutionError::NotFound.into());
            }
            Ok(Err(error)) => {
                tracing::error!(%error, "the relocation could not read the references");
                record(&resource, AuditResult::Failed);
                return Err(ResolutionError::Unavailable.into());
            }
            Err(_) => {
                tracing::error!("the relocation timed out reading the references");
                record(&resource, AuditResult::Failed);
                return Err(ResolutionError::Unavailable.into());
            }
        }

        match relocator.relocate(&place, operation, &digests).await {
            Ok(report) => {
                let status = if report.pending == 0 {
                    StatusCode::OK
                } else if report.deadline_exceeded {
                    StatusCode::GATEWAY_TIMEOUT
                } else if report.succeeded > 0 {
                    StatusCode::MULTI_STATUS
                } else if report.any_store_failure() {
                    StatusCode::SERVICE_UNAVAILABLE
                } else {
                    StatusCode::CONFLICT
                };
                let result = if report.pending == 0 {
                    AuditResult::Allowed
                } else if report.succeeded > 0 {
                    AuditResult::Conflict
                } else {
                    AuditResult::Failed
                };
                // The audit field is bounded (256 chars): counts by state first,
                // the digests bound by a hash of the set.
                let mut states: std::collections::BTreeMap<String, usize> = Default::default();
                for entry in &report.entries {
                    let name = serde_json::to_value(&entry.result)
                        .ok()
                        .and_then(|v| v["outcome"].as_str().map(str::to_string))
                        .unwrap_or_default();
                    *states.entry(name).or_default() += 1;
                }
                let states = states
                    .iter()
                    .map(|(name, count)| format!("{name}={count}"))
                    .collect::<Vec<_>>()
                    .join(",");
                let set = ContentDigest::of_bytes(
                    digests
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(",")
                        .as_bytes(),
                );
                record(
                    &format!("{resource} {states} set={}", &set.value[..16]),
                    result,
                );
                Ok((status, Json(report)).into_response())
            }
            Err(error) => {
                record(&resource, AuditResult::Failed);
                Ok(refused(error))
            }
        }
    });
    task.await.unwrap_or_else(|error| {
        tracing::error!(%error, "the relocation task failed");
        Ok(refused(RelocateError::Failed))
    })
}

fn refused(error: RelocateError) -> Response {
    let (status, code) = match error {
        RelocateError::TooMany { .. } => (StatusCode::BAD_REQUEST, "E_RELOCATE_TOO_MANY"),
        RelocateError::Busy => (StatusCode::TOO_MANY_REQUESTS, "E_RELOCATE_BUSY"),
        RelocateError::Failed => (StatusCode::SERVICE_UNAVAILABLE, "E_RELOCATE_FAILED"),
    };
    let payload = serde_json::json!({"error": code, "message": error.to_string()});
    (status, Json(payload)).into_response()
}
