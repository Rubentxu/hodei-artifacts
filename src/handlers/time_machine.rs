//! Repository time machine (HSP-165, slice 1): `GET /api/v1/freezes/diff`.
//!
//! Explains what changed between two freezes of the same repository, citing the
//! historical facts each manifest recorded. The whole operation is a read:
//!
//! - it writes no decision, grant, hold or freeze (only the access audit event,
//!   as every authorised read does);
//! - Cedar decides **who** with the dedicated action `ExploreRepositoryHistory`
//!   on the origin repository, evaluated now, not as of the freeze;
//! - both freezes must belong to the caller's account (a freeze of another
//!   account is a 404, indistinguishable from absence);
//! - the answer is bounded: at most [`MAX_CHANGES`] differences are listed
//!   (totals stay exact) and only their digests are probed in the store;
//! - any failure to establish the facts (store, trust ledger) is a 503, never a
//!   partial answer that reads as "nothing changed" or "the bytes exist".
//!
//! The bytes a freeze names may have been reclaimed since: that is reported as
//! `availability: gone` and nothing is reconstructed. Whether the trusted
//! channel admits a digest today is reported separately (`admitted_now`), so
//! historical identity and present permission are never merged.

use std::collections::{BTreeMap, BTreeSet};

use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use hodei_evidence::ContentDigest;
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::correlation_id;
use crate::handlers::freezes::{FreezeFailure, audit, gate, load, registered, the_origin};
use crate::resolution::{ResolutionError, the_content_store, the_context};
use crate::time_machine::{Availability, Change, explain};

/// The query of `GET /api/v1/freezes/diff`.
#[derive(Debug, Deserialize)]
pub struct DiffQuery {
    /// The earlier freeze (its id).
    pub from: String,
    /// The later freeze (its id).
    pub to: String,
}

/// What a manifest says about itself.
#[derive(Debug, Serialize)]
pub struct Side {
    /// The freeze id.
    pub id: String,
    /// When it was frozen.
    pub created_at: DateTime<Utc>,
    /// Who froze it.
    pub created_by: String,
    /// How many paths it names.
    pub entries: usize,
}

/// What is true of a digest today, apart from what the manifests recorded.
#[derive(Debug, Serialize)]
pub struct DigestState {
    /// Whether the store still holds the bytes.
    pub availability: Availability,
    /// Whether the trusted channel of the repository admits the digest now.
    /// `null` when the repository has no trusted channel to ask.
    pub admitted_now: Option<bool>,
}

/// The answer.
#[derive(Debug, Serialize)]
pub struct DiffResponse {
    /// The repository both freezes were taken from.
    pub repository: String,
    /// The earlier freeze.
    pub from: Side,
    /// The later freeze.
    pub to: Side,
    /// The differences, ordered by path, bounded.
    pub changes: Vec<Change>,
    /// The exact number of differences.
    pub total_changes: usize,
    /// Whether `changes` is a prefix of the total.
    pub truncated: bool,
    /// Paths named identically by both.
    pub unchanged: usize,
    /// Today's state of every digest cited in `changes`.
    pub digests: BTreeMap<String, DigestState>,
}

fn side(id: &str, manifest: &crate::freeze::FreezeManifest) -> Side {
    Side {
        id: id.to_string(),
        created_at: manifest.created_at,
        created_by: manifest.created_by.clone(),
        entries: manifest.entries.len(),
    }
}

/// `GET /api/v1/freezes/diff?from=<id>&to=<id>`
pub async fn diff(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<DiffQuery>,
) -> Result<Response, FreezeFailure> {
    let context = the_context(&state, &headers).await?;
    let correlation = correlation_id(&headers, &state.identity);
    // The account is checked first, from the registry alone, for both ids.
    registered(&state, &context, &query.from).await?;
    registered(&state, &context, &query.to).await?;
    let from = load(&state, &context, &query.from).await?.manifest;
    let to = load(&state, &context, &query.to).await?.manifest;
    if from.repository != to.repository {
        return Err(FreezeFailure::Plain(
            StatusCode::UNPROCESSABLE_ENTITY,
            "the two freezes belong to different repositories and are not comparable",
        ));
    }
    let origin = the_origin(&state, &context, &from.repository).await?;
    let action = crate::actions::explore_repository_history();
    gate(&state, &context, &origin, action.clone(), &correlation)?;

    let result = facts(&state, &from, &to, &origin).await;
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(failure) => {
            audit(
                &state,
                &context,
                &action.to_string(),
                &from.repository,
                AuditResult::Failed,
                &correlation,
            );
            return Err(failure);
        }
    };
    audit(
        &state,
        &context,
        &action.to_string(),
        &from.repository,
        AuditResult::Allowed,
        &correlation,
    );
    let (diff, digests) = outcome;
    let response = DiffResponse {
        repository: from.repository.clone(),
        from: side(&query.from, &from),
        to: side(&query.to, &to),
        changes: diff.changes,
        total_changes: diff.total_changes,
        truncated: diff.truncated,
        unchanged: diff.unchanged,
        digests,
    };
    Ok((StatusCode::OK, Json(response)).into_response())
}

/// The comparison and today's state of the digests it cites. Any error stops
/// the whole answer.
async fn facts(
    state: &AppState,
    from: &crate::freeze::FreezeManifest,
    to: &crate::freeze::FreezeManifest,
    origin: &hodei_catalog::RepositoryDescriptor,
) -> Result<(crate::time_machine::Diff, BTreeMap<String, DigestState>), FreezeFailure> {
    let diff = explain(from, to);
    let cited: BTreeSet<&str> = diff
        .changes
        .iter()
        .flat_map(|change| [change.from.as_ref(), change.to.as_ref()])
        .flatten()
        .map(|entry| entry.digest.as_str())
        .collect();
    let granted = crate::trust::servable(state, &from.account, origin)
        .await
        .map_err(crate::resolution::the_admission_error)?;
    let store = the_content_store(state)?;
    let mut digests = BTreeMap::new();
    for digest in cited {
        let hex = digest
            .strip_prefix("sha256:")
            .ok_or(ResolutionError::Internal)?;
        let present = store
            .exists(&ContentDigest::sha256(hex))
            .await
            .map_err(|_| ResolutionError::Unavailable)?;
        digests.insert(
            digest.to_string(),
            DigestState {
                availability: if present {
                    Availability::Present
                } else {
                    Availability::Gone
                },
                admitted_now: granted.as_ref().map(|set| set.contains(hex)),
            },
        );
    }
    Ok((diff, digests))
}
