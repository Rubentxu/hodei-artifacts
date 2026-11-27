//! Read-only tiering advice (HSP-326): `GET /api/v1/storage/tiering/advice`.
//!
//! `?repository=<key>[&after=sha256:<hex>][&limit=<n>]`. For each digest the
//! repository references, the answer recommends a tier from the ladder the
//! deployment configured and cites every fact it used (size, age, references,
//! cost); the facts the platform does not have (access frequency, last access)
//! are declared missing. Nothing is placed, moved or written. The rules:
//!
//! - Cedar decides **who** with the dedicated action `ExploreStorageTiering` on
//!   the repository, **before** the ledger or the store are read; no other
//!   action implies it;
//! - the repository belongs to the caller's account, otherwise 404, and only
//!   digests that repository references are listed, so another account's
//!   blobs cannot be probed;
//! - the work runs in a task of its own and is audited there (`Allowed`,
//!   `Denied`, `Failed`), so a client that hangs up leaves a trace;
//! - everything fails closed: no ladder configured or no paging ledger is a
//!   501, a ledger or store failure a 503, a deadline a 504, no free place a
//!   429; never a partial answer that reads as complete;
//! - the cost is bounded: digests per answer, ledger rows per request, bytes
//!   of the answer, a deadline and a semaphore, all from validated config.

use std::time::Duration;

use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use hodei_evidence::ContentDigest;
use hodei_hub::blob::BlobError;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::correlation_id;
use crate::handlers::freezes::{FreezeFailure, audit, gate};
use crate::handlers::repair::ANY_FORMAT;
use crate::resolution::{ResolutionError, the_context, the_repository_in};
use crate::tiering::{Facts, MISSING_FACTS, advise};

/// The query of `GET /api/v1/storage/tiering/advice`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdviceQuery {
    /// The repository to advise on.
    pub repository: String,
    /// Resume after this digest (the `next_after` of a truncated answer).
    pub after: Option<String>,
    /// At most this many digests (never above the configured bound).
    pub limit: Option<usize>,
}

fn parse_digest(text: &str) -> Option<ContentDigest> {
    let hex = text.strip_prefix("sha256:")?;
    (hex.len() == 64
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    .then(|| ContentDigest::sha256(hex))
}

fn refusal(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({"error": code, "message": message}))).into_response()
}

/// `GET /api/v1/storage/tiering/advice`
pub async fn advice(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AdviceQuery>,
) -> Result<Response, FreezeFailure> {
    let context = the_context(&state, &headers).await?;
    let correlation = correlation_id(&headers, &state.identity);
    let after = match query.after.as_deref() {
        Some(text) => Some(
            parse_digest(text)
                .ok_or(ResolutionError::BadRequest("after is not sha256:<64 hex>"))?,
        ),
        None => None,
    };
    if query.limit == Some(0) {
        return Err(ResolutionError::BadRequest("limit must be at least 1").into());
    }
    // The repository is the caller's own or it does not exist (404); Cedar
    // decides next and a denial never degrades to a 404.
    let origin = the_repository_in(&state, &context, &query.repository, &ANY_FORMAT).await?;
    let repository = query.repository.clone();
    let limit = query.limit;

    let task = tokio::spawn(async move {
        let action = crate::actions::explore_storage_tiering();
        // Cedar first: nothing below runs for a caller without the action, and
        // the gate audits the denial.
        gate(&state, &context, &origin, action.clone(), &correlation)?;
        let audited = action.to_string();
        let account = context.account().as_str().to_string();
        let record = |result: AuditResult, detail: &str| {
            let resource = format!("{repository} {detail}");
            audit(&state, &context, &audited, &resource, result, &correlation);
        };
        let failed = |status: StatusCode, code: &str, message: &str| {
            record(AuditResult::Failed, code);
            Ok(refusal(status, code, message))
        };
        let (Some(advisor), Some(gc), Some(store)) = (
            state.tiering.clone(),
            state.gc.clone(),
            state.content_store.clone(),
        ) else {
            return failed(
                StatusCode::NOT_IMPLEMENTED,
                "E_TIERING_UNAVAILABLE",
                "tiering advice needs a configured ladder of tiers, the durable catalog and the content store",
            );
        };
        let Some(_permit) = advisor.try_reserve() else {
            return failed(
                StatusCode::TOO_MANY_REQUESTS,
                "E_TIERING_BUSY",
                "too many advice requests are running; retry later",
            );
        };
        let config = advisor.config();
        let max_digests = limit.unwrap_or(config.max_digests).min(config.max_digests);

        let work = async {
            let scan = match gc
                .repository_references(
                    &account,
                    &repository,
                    after.as_ref(),
                    max_digests,
                    config.max_rows,
                )
                .await
            {
                Ok(scan) => scan,
                Err(crate::gc::GcError::Unsupported) => {
                    return Err((
                        StatusCode::NOT_IMPLEMENTED,
                        "E_TIERING_UNAVAILABLE",
                        "the ledger cannot be read page by page, and advice never reads the whole census",
                    ));
                }
                Err(error) => {
                    tracing::error!(%error, "the tiering advice could not read the references");
                    return Err((
                        StatusCode::SERVICE_UNAVAILABLE,
                        "E_TIERING_FAILED",
                        "the references could not be read",
                    ));
                }
            };
            let mut entries: Vec<Value> = Vec::new();
            let mut used = 0usize;
            let mut next_after = scan.next_after.clone();
            let mut cut_by = next_after.as_ref().map(|_| {
                if scan.digests.len() >= max_digests {
                    "digests"
                } else {
                    "rows"
                }
            });
            // The envelope (tiers, missing facts, counters) is reserved up front.
            let budget = config.max_response_bytes.saturating_sub(512);
            for (digest, references) in &scan.digests {
                let facts = async {
                    let size = store.length(digest).await?;
                    let age = store.age(digest).await?;
                    Ok::<_, BlobError>((size, age))
                }
                .await;
                let entry = match facts {
                    Ok((size_bytes, age)) => {
                        let facts = Facts {
                            size_bytes,
                            age_secs: age.map(|age| age.as_secs()),
                            references: *references,
                        };
                        let Some(advice) = advise(&config.tiers, facts) else {
                            return Err((
                                StatusCode::SERVICE_UNAVAILABLE,
                                "E_TIERING_FAILED",
                                "the configured ladder gave no advice",
                            ));
                        };
                        let mut entry = serde_json::to_value(&advice).unwrap_or_default();
                        entry["digest"] = Value::String(digest.to_string());
                        entry["status"] = Value::String("advised".to_string());
                        entry
                    }
                    // The ledger names bytes the store does not have: that is a
                    // finding for the scrub, not a tier.
                    Err(BlobError::NotFound) => json!({
                        "digest": digest.to_string(),
                        "status": "blob_missing",
                        "recommended_tier": null,
                        "references": references,
                        "missing_facts": ["size_bytes", "age_secs"],
                    }),
                    Err(error) => {
                        tracing::error!(%error, "the tiering advice could not read the store");
                        return Err((
                            StatusCode::SERVICE_UNAVAILABLE,
                            "E_TIERING_FAILED",
                            "the content store could not be read",
                        ));
                    }
                };
                let size = serde_json::to_vec(&entry)
                    .map(|bytes| bytes.len() + 1)
                    .unwrap_or(usize::MAX);
                if used.saturating_add(size) > budget {
                    if entries.is_empty() {
                        return Err((
                            StatusCode::PAYLOAD_TOO_LARGE,
                            "E_TIERING_RESPONSE_TOO_LARGE",
                            "one entry does not fit in max_response_bytes",
                        ));
                    }
                    next_after = entries
                        .last()
                        .and_then(|last| last["digest"].as_str().map(str::to_string));
                    cut_by = Some("bytes");
                    break;
                }
                used += size;
                entries.push(entry);
            }
            Ok((entries, scan.rows_read, next_after, cut_by))
        };

        match tokio::time::timeout(Duration::from_secs(config.timeout_secs), work).await {
            Err(_) => failed(
                StatusCode::GATEWAY_TIMEOUT,
                "E_TIERING_TIMEOUT",
                "the advice did not finish before its deadline",
            ),
            Ok(Err((status, code, message))) => failed(status, code, message),
            Ok(Ok((entries, rows_read, next_after, cut_by))) => {
                record(
                    AuditResult::Allowed,
                    &format!("entries={} rows={rows_read}", entries.len()),
                );
                let body = json!({
                    "repository": repository,
                    "read_only": true,
                    "cost_unit": config.cost_unit,
                    "tiers": config.tiers.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
                    "entries": entries,
                    "truncated": cut_by.is_some(),
                    "truncated_by": cut_by,
                    "next_after": next_after,
                    "rows_read": rows_read,
                    "missing_facts": MISSING_FACTS,
                });
                Ok((StatusCode::OK, Json(body)).into_response())
            }
        }
    });
    task.await.unwrap_or_else(|error| {
        tracing::error!(%error, "the tiering advice task failed");
        Ok(refusal(
            StatusCode::SERVICE_UNAVAILABLE,
            "E_TIERING_FAILED",
            "the advice could not be completed",
        ))
    })
}
