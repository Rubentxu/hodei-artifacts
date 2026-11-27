//! Verification of a trust passport: `POST /api/v1/passports/verify?repository=<key>`
//! (HSP-321 / HSP-160).
//!
//! The route returns a **verdict** about a passport the caller brings. It
//! grants nothing and no authorization path accepts it. The rules (the
//! verification itself is in [`crate::passport_verify`]):
//!
//! - the roots of trust are the operator's, pinned in the deployment
//!   configuration; without them the route answers `501`, never "verified";
//! - Cedar decides **who** with the dedicated action `VerifyTrustPassport` on
//!   a repository of the caller's account, **before** the document is read;
//!   a denial is audited. The repository is a query parameter for that reason;
//! - the passport must be addressed to that account and repository: otherwise
//!   the verdict is `audience-mismatch` and nothing about its validity is
//!   computed or said;
//! - everything past the gate runs in a task of its own, so that a client that
//!   hangs up cannot leave the verification without its audit record, and the
//!   audit is written there, not after the response;
//! - the cost is bounded (document size, blobs, concurrent verifications -> 429,
//!   a deadline) and every failure is closed.
//!
//! Answers: `200` the passport is valid (the only success); `422` the
//! passport is not valid, with a stable `code`; `413` too large; `400` the
//! document is not a passport document; `429` too many verifications; `501`
//! no roots configured; `503` the verification could not reach a verdict (the
//! revocation ledger cannot be read, the deadline passed): no verdict, never a
//! pass. The body never repeats the document, the roots or another account.

use axum::{
    Json,
    body::Body,
    extract::{RawQuery, State},
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::correlation_id;
use crate::handlers::freezes::{FreezeFailure, audit, gate};
use crate::handlers::repair::ANY_FORMAT;
use crate::handlers::the_one_value;
use crate::passport_verify::{PassportDocument, Refusal, check_revocation, refusals};
use crate::resolution::{ResolutionError, the_context, the_repository_in};

fn respond(status: StatusCode, body: serde_json::Value) -> Response {
    let mut response = (status, Json(body)).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn invalid(refusal: Refusal) -> Response {
    let status = if refusal == refusals::TOO_LARGE {
        StatusCode::PAYLOAD_TOO_LARGE
    } else {
        StatusCode::UNPROCESSABLE_ENTITY
    };
    respond(
        status,
        json!({
            "verdict": "invalid",
            "code": refusal.code,
            "explanation": refusal.explanation,
            "grants_conferred": false,
        }),
    )
}

fn no_verdict(reason: &'static str) -> Response {
    respond(
        StatusCode::SERVICE_UNAVAILABLE,
        json!({
            "verdict": "none",
            "explanation": reason,
            "grants_conferred": false,
        }),
    )
}

/// `POST /api/v1/passports/verify?repository=<key>`
pub async fn verify(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    request: Request<Body>,
) -> Result<Response, FreezeFailure> {
    let context = the_context(&state, &headers).await?;
    let correlation = correlation_id(&headers, &state.identity);
    let repository = the_one_value(raw_query.as_deref(), "repository")
        .ok_or(ResolutionError::BadRequest("repository is required"))?;
    // The repository is the caller's own or it does not exist; Cedar decides
    // afterwards and a denial never degrades to a 404.
    let origin = the_repository_in(&state, &context, &repository, &ANY_FORMAT).await?;
    let action = crate::actions::verify_trust_passport();
    gate(&state, &context, &origin, action.clone(), &correlation)?;

    let account = context.account().as_str().to_string();
    let task = tokio::spawn(async move {
        let audited = action.to_string();
        let record = |result: AuditResult| {
            audit(
                &state,
                &context,
                &audited,
                &repository,
                result,
                &correlation,
            );
        };
        let Some(verifier) = state.passport_verifier.clone() else {
            record(AuditResult::Failed);
            return Err(ResolutionError::NotImplemented(
                "passport verification needs roots of trust pinned by the deployment: without them nothing is verified",
            )
            .into());
        };
        // The body is read BEFORE a verification slot is taken and under a
        // deadline: a caller that trickles bytes occupies a task, never a slot.
        // Base64 inflates by a third; the margin covers the envelope.
        let ceiling = verifier.max_document_bytes() / 3 * 4 + 64 * 1024;
        let bytes = match read_body(request.into_body(), ceiling, verifier.body_deadline()).await {
            Ok(bytes) => bytes,
            Err(BodyFailure::TooLarge) => {
                record(AuditResult::Denied);
                return Ok(invalid(refusals::TOO_LARGE));
            }
            Err(BodyFailure::Timeout) => {
                record(AuditResult::Failed);
                return Ok(respond(
                    StatusCode::REQUEST_TIMEOUT,
                    json!({
                        "verdict": "none",
                        "explanation": "the request body did not arrive in time",
                    }),
                ));
            }
            Err(BodyFailure::Read) => {
                record(AuditResult::Failed);
                return Ok(respond(
                    StatusCode::BAD_REQUEST,
                    json!({
                        "verdict": "none",
                        "explanation": "the request body could not be read",
                    }),
                ));
            }
        };
        let document: PassportDocument = match serde_json::from_slice(&bytes) {
            Ok(document) => document,
            Err(_) => {
                record(AuditResult::Denied);
                return Err(
                    ResolutionError::BadRequest("the body is not a passport document").into(),
                );
            }
        };
        let Some(slot) = verifier.try_slot() else {
            record(AuditResult::Failed);
            let mut response = respond(
                StatusCode::TOO_MANY_REQUESTS,
                json!({ "verdict": "none", "explanation": "too many verifications are running" }),
            );
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
            return Ok(response);
        };
        // The slot lives until this task ends (the ledger phase included); the
        // blocking verification holds a second reference, so it is released
        // only when both are done, even if the deadline gave the CPU work up.
        let slot = std::sync::Arc::new(slot);
        let _held = slot.clone();
        let now = state.identity.now();

        // The cryptography is CPU bound and runs off the request task; the
        // slot is held until it ends, even when the deadline gave it up.
        let blocking = {
            let verifier = verifier.clone();
            let account = account.clone();
            let repository = repository.clone();
            tokio::task::spawn_blocking(move || {
                let _slot = slot;
                verifier.verify_document(&document, &account, &repository, now)
            })
        };
        let sound =
            match tokio::time::timeout(crate::passport_verify::VERIFY_TIMEOUT, blocking).await {
                Ok(Ok(Ok(sound))) => sound,
                Ok(Ok(Err(refusal))) => {
                    record(AuditResult::Denied);
                    return Ok(invalid(refusal));
                }
                Ok(Err(_)) | Err(_) => {
                    tracing::error!("the passport verification did not finish");
                    record(AuditResult::Failed);
                    return Ok(no_verdict("the verification could not reach a verdict"));
                }
            };

        // Revocation, asked now. No ledger, or a ledger that cannot answer,
        // is no verdict: "not known to be revoked" is never "valid".
        let Some(ledger) = state.trust_ledger.clone() else {
            record(AuditResult::Failed);
            return Ok(no_verdict("the revocation ledger is not available"));
        };
        match check_revocation(
            ledger.as_ref(),
            &account,
            &sound,
            now,
            verifier.ledger_deadline(),
        )
        .await
        {
            Err(_) => {
                record(AuditResult::Failed);
                return Ok(no_verdict("the revocation ledger could not be read"));
            }
            Ok(Some(refusal)) => {
                record(AuditResult::Denied);
                return Ok(invalid(refusal));
            }
            Ok(None) => {}
        }

        record(AuditResult::Allowed);
        Ok(respond(
            StatusCode::OK,
            json!({
                "verdict": "valid",
                "repository": sound.repository,
                "capsule_image_digest": sound.capsule_image_digest,
                "passport_image_digest": sound.passport_image_digest,
                "verified_at": sound.verified_at.to_rfc3339(),
                "expires_at": sound.expires_at.to_rfc3339(),
                "slsa_result": sound.slsa_result,
                "root": { "lineage": sound.root_lineage, "revision": sound.root_revision },
                "signer_fingerprint": sound.signer_fingerprint,
                "revocation": { "checked": "ledger", "decisions": sound.cited.len() },
                "grants_conferred": false,
            }),
        ))
    });
    task.await.map_err(|_| ResolutionError::Unavailable)?
}

enum BodyFailure {
    TooLarge,
    Timeout,
    Read,
}

/// Reads the body up to `ceiling` bytes within `deadline`, telling apart a
/// body that is too large, one that did not arrive and one that broke.
async fn read_body(
    body: Body,
    ceiling: usize,
    deadline: std::time::Duration,
) -> Result<Vec<u8>, BodyFailure> {
    use futures_util::StreamExt;
    let reading = async {
        let mut stream = body.into_data_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| BodyFailure::Read)?;
            if bytes.len().saturating_add(chunk.len()) > ceiling {
                return Err(BodyFailure::TooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    };
    tokio::time::timeout(deadline, reading)
        .await
        .map_err(|_| BodyFailure::Timeout)?
}
