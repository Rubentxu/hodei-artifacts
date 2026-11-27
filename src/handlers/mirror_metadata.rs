//! The rollback/freeze state of a proxy's signed index (HSP-167):
//! `GET /api/v1/mirror-metadata` and `POST /api/v1/mirror-metadata/reset`.
//!
//! The protection itself runs inside the apt proxy (see
//! [`crate::proxy::metadata_guard`]); this is the operator's surface over it.
//!
//! - Cedar decides **who** with the dedicated action `ManageMirrorMetadata` on
//!   the proxy repository; no other action implies it and a denial is audited;
//! - the repository must be an apt *proxy* of the caller's account: anything
//!   else is a 404 indistinguishable from absence, so another account cannot
//!   probe the lineages of this one;
//! - `reset` is the only way back after an upstream legitimately went to an
//!   older index (a restored mirror, a key rotation with a re-issued
//!   `Release`): it appends an audited re-baseline, and the next `Release`
//!   that passes the pinned signature becomes the new baseline. It never
//!   accepts bytes itself;
//! - a ledger that cannot answer is a 503, never an empty or default state.

use axum::{
    Json,
    body::Body,
    extract::{Query, State},
    http::{HeaderMap, Request, StatusCode},
    response::{IntoResponse, Response},
};
use hodei_catalog::{RepositoryFormat, RepositoryMode};
use serde::Deserialize;

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::correlation_id;
use crate::handlers::freezes::{FreezeFailure, audit, gate};
use crate::proxy::metadata_guard::{self, MetadataKey};
use crate::resolution::{ResolutionError, the_context, the_repository_in};

/// The query of `GET /api/v1/mirror-metadata`.
#[derive(Debug, Deserialize)]
pub struct StateQuery {
    /// The apt proxy repository.
    pub repository: String,
    /// The distribution.
    pub distribution: String,
}

/// The body of `POST /api/v1/mirror-metadata/reset`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResetBody {
    /// The apt proxy repository.
    pub repository: String,
    /// The distribution.
    pub distribution: String,
}

fn valid_distribution(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

async fn resolve(
    state: &AppState,
    headers: &HeaderMap,
    repository: &str,
    distribution: &str,
) -> Result<
    (
        hodei_authz::VerifiedContext,
        String,
        MetadataKey,
        std::sync::Arc<dyn metadata_guard::MirrorMetadataLedger>,
    ),
    FreezeFailure,
> {
    let context = the_context(state, headers).await?;
    let correlation = correlation_id(headers, &state.identity);
    if !valid_distribution(distribution) {
        return Err(ResolutionError::BadRequest("the distribution is not a valid name").into());
    }
    let ledger = state
        .mirror_metadata
        .clone()
        .ok_or(ResolutionError::NotImplemented(
            "mirror metadata protection needs the durable ledger of the graph profile",
        ))?;
    let origin = the_repository_in(state, &context, repository, &[RepositoryFormat::Apt]).await?;
    // Cedar decides first: whether a repository is a proxy, or serves a
    // distribution, is only revealed to whoever may manage it.
    gate(
        state,
        &context,
        &origin,
        crate::actions::manage_mirror_metadata(),
        &correlation,
    )?;
    if origin.mode != RepositoryMode::Proxy {
        return Err(ResolutionError::NotFound.into());
    }
    let served = hodei_catalog::apt::AptProxyConfig::of(&origin.config)
        .map_err(|_| ResolutionError::BadRequest("the apt proxy configuration is invalid"))?;
    if !served.allows_dist(distribution) {
        return Err(ResolutionError::BadRequest(
            "the distribution is not served by this repository",
        )
        .into());
    }
    let key = MetadataKey {
        account: context.account().as_str().to_string(),
        repository: repository.to_string(),
        distribution: distribution.to_string(),
    };
    Ok((context, correlation, key, ledger))
}

/// `GET /api/v1/mirror-metadata`
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<StateQuery>,
) -> Result<Response, FreezeFailure> {
    let (_context, _correlation, key, ledger) =
        resolve(&state, &headers, &query.repository, &query.distribution).await?;
    let head = ledger.head(&key).await.map_err(|error| {
        tracing::error!(%error, "the mirror metadata state could not be read");
        ResolutionError::Unavailable
    })?;
    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "repository": query.repository,
            "distribution": query.distribution,
            "sequence": head.seq,
            "baseline": head.baseline.as_ref().map(|baseline| serde_json::json!({
                "date": baseline.date.to_rfc3339(),
                "digest": baseline.digest,
            })),
            "recorded_at": head.accepted_at.map(|at| at.to_rfc3339()),
        })),
    )
        .into_response())
}

/// `POST /api/v1/mirror-metadata/reset`
pub async fn reset(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request<Body>,
) -> Result<Response, FreezeFailure> {
    let bytes = axum::body::to_bytes(request.into_body(), 4 * 1024)
        .await
        .map_err(|_| ResolutionError::BadRequest("the body is too large"))?;
    let body: ResetBody = serde_json::from_slice(&bytes)
        .map_err(|_| ResolutionError::BadRequest("the body is not a valid reset request"))?;
    let (context, correlation, key, ledger) =
        resolve(&state, &headers, &body.repository, &body.distribution).await?;
    let action = crate::actions::manage_mirror_metadata().to_string();
    let resource = format!("{}/dists/{}", body.repository, body.distribution);
    match metadata_guard::reset(
        ledger.as_ref(),
        &key,
        context.principal().subject().as_str(),
        chrono::Utc::now(),
    )
    .await
    {
        Ok(sequence) => {
            audit(
                &state,
                &context,
                &action,
                &resource,
                AuditResult::Allowed,
                &correlation,
            );
            Ok((
                StatusCode::OK,
                Json(serde_json::json!({
                    "repository": body.repository,
                    "distribution": body.distribution,
                    "sequence": sequence,
                    "baseline": null,
                })),
            )
                .into_response())
        }
        Err(error) => {
            tracing::error!(%error, "the mirror metadata reset failed");
            audit(
                &state,
                &context,
                &action,
                &resource,
                AuditResult::Failed,
                &correlation,
            );
            Err(ResolutionError::Unavailable.into())
        }
    }
}
