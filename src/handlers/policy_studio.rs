//! Policy Studio: the server-side surface that edits, validates, simulates
//! and activates a trust policy over the durable, per-account lifecycle store (HSP-151).
//!
//! The studio never decides in the browser. Every route here runs on the
//! same engine the promotion gate uses: the candidate is measured against
//! the evidence snapshots the caller brings, the activation is the real
//! compare-and-set of the lifecycle store, and the answer separates the
//! three things a reviewer must not confuse (AC2):
//!
//! - `POLICY_DECISION` — the engine measured the subject and answered;
//! - `EVIDENCE_GAP` — a requirement was never measured because the
//!   evidence for its dimension is absent (the reason code is the one the
//!   engine itself emitted, and the gap names the evidence it waited on);
//! - `ENGINE_ERROR` — the candidate or the context is malformed, so the
//!   engine cannot answer at all. A malformed policy never says "no".
//!
//! The fourth refusal — the *access* decision — never reaches this
//! module: the administrative gate in front of the routes answers it with
//! 401/403 before any handler runs, so an editor without the grant cannot
//! write.
//!
//! Nothing here mutates a grant or the content of the registry. The
//! simulation always answers `applied: false`; only `activate` moves the
//! head, and it moves only a revision that passed the same strict
//! validation the save uses.

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{correlation_id, record_audit};
use axum::{
    Extension, Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use hodei_authz::VerifiedContext;
use hodei_trust_policy::{
    AccountPolicyLoader, LifecycleError, LifecycleStore, PolicyLoader, SimulatedSubject,
    SimulationAnswer, StudioCandidate, TrustPolicy,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Which revision of the studio's policy set the caller asks about.
#[derive(Debug, Deserialize)]
pub struct RevisionQuery {
    /// The policy set. Defaults to the one the deployment declares.
    #[serde(default)]
    pub policy_id: Option<String>,
    /// The sequence to load. Without it the head's revision is loaded.
    #[serde(default)]
    pub sequence: Option<u64>,
}

/// A revision as the studio reads it.
#[derive(Debug, Serialize)]
pub struct RevisionResponse {
    /// The policy set the revision belongs to.
    pub policy_id: String,
    /// The position in the history, from 1.
    pub sequence: u64,
    /// The digest of the content, `sha256:<hex>`. The activation compares
    /// against the head's sequence; the digest is what binds a review to
    /// the exact bytes it reviewed (AC2).
    pub digest: String,
    /// The policy the revision carries, in its own JSON shape.
    pub policy: serde_json::Value,
    /// Who saved it.
    pub saved_by: String,
    /// Why this revision exists.
    pub reason: String,
    /// Whether this revision is the one that decides right now.
    pub active: bool,
}

/// What the head of a policy set is, as the studio reads it.
#[derive(Debug, Serialize)]
pub struct HeadResponse {
    /// Whether the set has ever been activated.
    pub exists: bool,
    /// The head itself, when it exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<serde_json::Value>,
}

/// A draft to validate.
#[derive(Debug, Deserialize)]
pub struct ValidateRequest {
    /// The policy candidate.
    #[serde(default)]
    pub policy: Option<serde_json::Value>,
}

/// The answer of a validation.
#[derive(Debug, Serialize)]
pub struct ValidateResponse {
    /// Whether the store would accept the candidate as a revision.
    pub valid: bool,
    /// What is wrong, when it is not valid.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Always `false`: validating saves nothing.
    pub applied: bool,
}

/// A draft to store as a new immutable revision.
#[derive(Debug, Deserialize)]
pub struct SaveRequest {
    /// The policy to save.
    pub policy: serde_json::Value,
    /// Why this revision exists, in the words of the editor.
    #[serde(default)]
    pub reason: String,
}

/// The answer of a save.
#[derive(Debug, Serialize)]
pub struct SaveResponse {
    /// The revision that now exists.
    pub revision: serde_json::Value,
    /// Always `false`: saving does not activate — a draft never changes
    /// what decides (AC4).
    pub applied: bool,
}

/// The candidate of a simulation: either a draft the caller is editing or
/// a revision already saved.
#[derive(Debug, Deserialize)]
pub struct SimulateRequest {
    /// The draft policy. Mutually exclusive with `sequence`.
    #[serde(default)]
    pub policy: Option<serde_json::Value>,
    /// The policy set. Defaults to the deployment's.
    #[serde(default)]
    pub policy_id: Option<String>,
    /// The sequence of a saved candidate.
    #[serde(default)]
    pub sequence: Option<u64>,
    /// The subjects to measure, each with its evidence snapshot.
    pub subjects: Vec<SimulatedSubject>,
}

/// What a simulation answered.
#[derive(Debug, Serialize)]
pub struct SimulateResponse {
    /// The candidate that was measured, as the studio resolved it.
    pub candidate: serde_json::Value,
    /// The version of the policy that decides right now, when the set has
    /// ever been activated. The baseline of the diff.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_version: Option<String>,
    /// One effect per subject, typed: `POLICY_DECISION`, `EVIDENCE_GAP` or
    /// `ENGINE_ERROR` (AC2).
    pub effects: serde_json::Value,
    /// Where the candidate would change the answer, subject by subject.
    pub changes: serde_json::Value,
    /// Always `false`: a simulation never changes what decides (AC4).
    pub applied: bool,
}

/// The activation of a saved revision — the real lifecycle CAS.
#[derive(Debug, Deserialize)]
pub struct ActivateRequest {
    /// The policy set. Defaults to the deployment's.
    #[serde(default)]
    pub policy_id: Option<String>,
    /// The revision to put at the head.
    pub sequence: u64,
    /// The head the caller believes is current. The CAS: if the store has
    /// moved, the activation is refused with a conflict naming the head
    /// that was found, never applied on top of a head the caller never
    /// saw.
    #[serde(default)]
    pub expected_head: Option<u64>,
    /// Why, in the words of the approver.
    #[serde(default)]
    pub reason: String,
}

/// The rollback of the head to an older revision. Like every activation
/// it is a CAS over the head the caller saw.
#[derive(Debug, Deserialize)]
pub struct RollbackRequest {
    /// The policy set. Defaults to the deployment's.
    #[serde(default)]
    pub policy_id: Option<String>,
    /// The revision to go back to.
    pub target: u64,
    /// The head the caller believes is current.
    #[serde(default)]
    pub expected_head: Option<u64>,
    /// Why, in the words of the approver.
    #[serde(default)]
    pub reason: String,
}

/// The answer of an activation or a rollback.
#[derive(Debug, Serialize)]
pub struct ActivationResponse {
    /// The head after the operation.
    pub head: serde_json::Value,
    /// Always `true` when this body is returned: it is the only path in
    /// the studio that changes what decides.
    pub applied: bool,
}

/// The window of the history a caller asks for. The default and the cap
/// keep one request from reading an unbounded set.
#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    /// The policy set. Defaults to the one the deployment declares.
    #[serde(default)]
    pub policy_id: Option<String>,
    /// How many items to skip, oldest first. Default 0.
    #[serde(default)]
    pub offset: Option<usize>,
    /// How many items at most, per list. Default 50, capped at 200.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// The default and the maximum size of one history window.
pub const DEFAULT_HISTORY_LIMIT: usize = 50;
/// The cap of one history window.
pub const MAX_HISTORY_LIMIT: usize = 200;
/// The largest request body any studio route reads (HTTP 413 above it): a
/// policy document, or a simulation with its snapshots, is far smaller.
pub const STUDIO_BODY_LIMIT: usize = 512 * 1024;

/// The history and events of a policy set.
#[derive(Debug, Serialize)]
pub struct HistoryResponse {
    /// The window skipped, in items.
    pub offset: usize,
    /// The window size that applied.
    pub limit: usize,
    /// Whether more revisions exist after this window.
    pub revisions_truncated: bool,
    /// Whether more events exist after this window.
    pub events_truncated: bool,
    /// The policy set.
    pub policy_id: String,
    /// Every revision still held, oldest first.
    pub revisions: serde_json::Value,
    /// Every lifecycle event, oldest first.
    pub events: serde_json::Value,
}

/// What the studio answers when it cannot do what was asked. The kind is
/// machine-readable so the UI can tell a stale head (someone else
/// activated while the editor was open) from a malformed policy from the
/// store being down — the conflict is AC3's conflict test.
pub struct StudioError {
    status: StatusCode,
    kind: &'static str,
    message: String,
    found_head: Option<u64>,
}

impl StudioError {
    fn new(status: StatusCode, kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            kind,
            message: message.into(),
            found_head: None,
        }
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_policy", message)
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    fn not_implemented(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_IMPLEMENTED, "not_implemented", message)
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, "unavailable", message)
    }

    /// The head moved under the caller. The body says where it actually
    /// is, so the editor can reload and re-diff instead of guessing.
    fn conflict(found_head: u64) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            kind: "stale_head",
            message: format!("the head is {found_head}, not the head the caller saw"),
            found_head: Some(found_head),
        }
    }
}

impl From<LifecycleError> for StudioError {
    fn from(error: LifecycleError) -> Self {
        match &error {
            LifecycleError::RevisionNotFound(_, _) | LifecycleError::NeverActivated(_) => {
                Self::not_found(error.to_string())
            }
            LifecycleError::StaleHead(_, _, found) => Self::conflict(*found),
            LifecycleError::InvalidPolicy(_, reason)
                // The separation of duties is a permission-shaped
                // refusal, not a malformed policy: the same principal
                // cannot be on both sides of it.
                if reason.contains("separation") =>
            {
                Self::new(StatusCode::FORBIDDEN, "forbidden", error.to_string())
            }
            LifecycleError::InvalidPolicy(_, _)
            | LifecycleError::InvalidContext(..)
            | LifecycleError::SchemaMismatch(..)
            | LifecycleError::CorruptRevision(..) => Self::invalid(error.to_string()),
            LifecycleError::RollbackToHead(..) => {
                Self::new(StatusCode::CONFLICT, "rollback_to_head", error.to_string())
            }
            LifecycleError::HistoryFull(_) => {
                Self::new(StatusCode::CONFLICT, "history_full", error.to_string())
            }
            LifecycleError::Unavailable(_) => Self::unavailable(error.to_string()),
        }
    }
}

impl IntoResponse for StudioError {
    fn into_response(self) -> Response {
        let mut body = serde_json::json!({
            "error": self.kind,
            "message": self.message,
        });
        if let Some(found) = self.found_head {
            body["found_head"] = serde_json::json!(found);
        }
        (
            self.status,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            body.to_string(),
        )
            .into_response()
    }
}

/// The lifecycle of the deployment, or the answer every studio route
/// gives when the durable store is not there: like the other optional
/// surfaces, a deployment without it answers `501`.
fn lifecycle(state: &AppState) -> Result<&Arc<dyn LifecycleStore>, StudioError> {
    state.policy_lifecycle.as_ref().ok_or_else(|| {
        StudioError::not_implemented("the policy lifecycle is not backed by a durable store")
    })
}

/// The account of the verified principal. It never comes from the request.
fn account_of(context: &VerifiedContext) -> String {
    context.account().as_str().to_string()
}

/// The policy set a request points at: the one it names, or the one the
/// deployment declares.
fn studio_policy_id(state: &AppState, asked: Option<&str>) -> String {
    asked
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| state.studio_policy_id.clone())
}

/// The candidate of a simulate: a draft, or a saved revision. The saved
/// path re-reads through the store, so a simulation of a revision always
/// measures the bytes its digest names — never what the caller claims.
async fn resolve_candidate(
    state: &AppState,
    account: &str,
    request: &SimulateRequest,
) -> Result<StudioCandidate, StudioError> {
    let has_draft = request.policy.is_some();
    let has_saved = request.sequence.is_some();
    if has_draft == has_saved {
        return Err(StudioError::invalid(
            "the candidate is either a draft policy or a saved sequence, not both or neither",
        ));
    }

    if let Some(policy) = &request.policy {
        let policy: TrustPolicy = serde_json::from_value(policy.clone())
            .map_err(|error| StudioError::invalid(format!("the draft cannot be read: {error}")))?;
        return Ok(StudioCandidate::draft(policy));
    }

    let sequence = request
        .sequence
        .ok_or_else(|| StudioError::invalid("a saved candidate needs its sequence"))?;
    let policy_id = studio_policy_id(state, request.policy_id.as_deref());
    let store = lifecycle(state)?;
    let revision = store
        .revision(account, &policy_id, sequence)
        .await?
        .ok_or_else(|| {
            StudioError::not_found(format!("revision {sequence} of {policy_id} does not exist"))
        })?;
    if !revision.digest_holds() {
        return Err(StudioError::from(LifecycleError::CorruptRevision(
            policy_id, sequence,
        )));
    }
    Ok(StudioCandidate::revision(revision.policy, sequence))
}

/// The JSON mirror of an answer, so the studio's wire shape is this
/// module's to own and the domain types can move without breaking it.
fn answer_body(answer: &SimulationAnswer) -> Result<SimulateResponse, StudioError> {
    Ok(SimulateResponse {
        candidate: serde_json::to_value(&answer.candidate)
            .map_err(|error| StudioError::unavailable(error.to_string()))?,
        active_version: answer.active_version.clone(),
        effects: serde_json::to_value(&answer.effects)
            .map_err(|error| StudioError::unavailable(error.to_string()))?,
        changes: serde_json::to_value(&answer.changes)
            .map_err(|error| StudioError::unavailable(error.to_string()))?,
        applied: answer.applied,
    })
}

/// The policy that decides now for the account, when the set has a head:
/// the baseline every simulation diffs against. It reads through the
/// fail-closed per-account loader, so a head whose revision is lost or
/// whose digest does not match is an error, never an empty baseline.
async fn active_policy(
    store: &Arc<dyn LifecycleStore>,
    account: &str,
    policy_id: &str,
) -> Result<Option<TrustPolicy>, StudioError> {
    if store.head(account, policy_id).await?.is_none() {
        return Ok(None);
    }
    AccountPolicyLoader::new(store.clone(), account)
        .active_policy(policy_id)
        .await
        .map(Some)
        .map_err(|error| StudioError::unavailable(error.to_string()))
}

/// `GET /policy-studio/revisions` — load a revision, or the head's
/// revision when none is named.
pub async fn load_revision(
    State(state): State<AppState>,
    Extension(context): Extension<VerifiedContext>,
    Query(query): Query<RevisionQuery>,
) -> Result<Json<RevisionResponse>, StudioError> {
    let store = lifecycle(&state)?;
    let account = account_of(&context);
    let policy_id = studio_policy_id(&state, query.policy_id.as_deref());
    let head = store.head(&account, &policy_id).await?;
    let sequence = match query.sequence {
        Some(sequence) => sequence,
        None => {
            let head = head.as_ref().ok_or_else(|| {
                StudioError::not_found(format!("{policy_id} has never been activated"))
            })?;
            head.sequence
        }
    };
    let revision = store
        .revision(&account, &policy_id, sequence)
        .await?
        .ok_or_else(|| {
            StudioError::not_found(format!("revision {sequence} of {policy_id} does not exist"))
        })?;
    if !revision.digest_holds() {
        return Err(StudioError::from(LifecycleError::CorruptRevision(
            policy_id, sequence,
        )));
    }
    let active = head.is_some_and(|head| head.sequence == revision.sequence);
    Ok(Json(RevisionResponse {
        policy_id: revision.policy_id.clone(),
        sequence: revision.sequence,
        digest: revision.digest.clone(),
        policy: serde_json::to_value(&revision.policy)
            .map_err(|error| StudioError::unavailable(error.to_string()))?,
        saved_by: revision.saved_by.clone(),
        reason: revision.reason.clone(),
        active,
    }))
}

/// `GET /policy-studio/head` — the compare-and-set anchor of the editor:
/// the head a save was made against, so the activation can name what it
/// saw.
pub async fn head(
    State(state): State<AppState>,
    Extension(context): Extension<VerifiedContext>,
    Query(query): Query<RevisionQuery>,
) -> Result<Json<HeadResponse>, StudioError> {
    let store = lifecycle(&state)?;
    let policy_id = studio_policy_id(&state, query.policy_id.as_deref());
    let head = store.head(&account_of(&context), &policy_id).await?;
    Ok(Json(HeadResponse {
        exists: head.is_some(),
        head: head
            .map(|head| {
                serde_json::to_value(&head)
                    .map_err(|error| StudioError::unavailable(error.to_string()))
            })
            .transpose()?,
    }))
}

/// `POST /policy-studio/validate` — the strict lifecycle validation of a
/// draft, without saving it. It is the same gate the save uses, so a
/// draft the studio calls valid is a draft the store will accept.
pub async fn validate(
    _: State<AppState>,
    _: Extension<VerifiedContext>,
    Json(request): Json<ValidateRequest>,
) -> Result<Json<ValidateResponse>, StudioError> {
    let Some(policy) = request.policy else {
        return Err(StudioError::invalid("the validation needs a policy"));
    };
    let policy: TrustPolicy = serde_json::from_value(policy)
        .map_err(|error| StudioError::invalid(format!("the draft cannot be read: {error}")))?;
    let answer = hodei_trust_policy::validate_candidate(&policy);
    Ok(Json(ValidateResponse {
        valid: answer.is_ok(),
        error: answer.err().map(|error| error.to_string()),
        applied: false,
    }))
}

/// `POST /policy-studio/revisions` — save a draft as a new immutable
/// revision. This writes history, never the head: what decides does not
/// change (AC4, `applied: false`).
pub async fn save(
    State(state): State<AppState>,
    Extension(context): Extension<VerifiedContext>,
    headers: HeaderMap,
    Json(request): Json<SaveRequest>,
) -> Result<Response, StudioError> {
    let policy: TrustPolicy = serde_json::from_value(request.policy)
        .map_err(|error| StudioError::invalid(format!("the draft cannot be read: {error}")))?;
    let store = lifecycle(&state)?;
    let actor = context.principal().subject().as_str().to_string();
    let revision = store
        .save(
            context.account().as_str(),
            policy,
            &actor,
            &request.reason,
            state.identity.now(),
        )
        .await?;
    record_audit(
        &state,
        &actor,
        context.account().as_str(),
        "policy-studio.save",
        &format!("{}#{}", revision.policy_id, revision.sequence),
        AuditResult::Allowed,
        &correlation_id(&headers, &state.identity),
    );
    Ok((
        StatusCode::CREATED,
        Json(SaveResponse {
            revision: serde_json::to_value(&revision)
                .map_err(|error| StudioError::unavailable(error.to_string()))?,
            applied: false,
        }),
    )
        .into_response())
}

/// `POST /policy-studio/simulate` — measure a candidate (a draft or a
/// saved revision) against the evidence snapshots the caller brings, and
/// diff its effects against the policy that decides right now. The
/// answer is typed per subject (AC2) and never applied (AC4).
pub async fn simulate(
    State(state): State<AppState>,
    Extension(context): Extension<VerifiedContext>,
    Json(request): Json<SimulateRequest>,
) -> Result<Json<SimulateResponse>, StudioError> {
    let account = account_of(&context);
    let candidate = resolve_candidate(&state, &account, &request).await?;
    let store = lifecycle(&state)?;
    let active = active_policy(store, &account, &candidate.policy.id).await?;
    let answer =
        hodei_trust_policy::simulate_effects(active.as_ref(), &candidate, &request.subjects);
    Ok(Json(answer_body(&answer)?))
}

/// `POST /policy-studio/activate` — the real lifecycle activation: a
/// compare-and-set on the head. Only a revision the
/// store accepted (and whose digest still holds) can become the head;
/// there is no path from a draft or a simulation to this route that skips
/// the save (AC1, AC3).
pub async fn activate(
    State(state): State<AppState>,
    Extension(context): Extension<VerifiedContext>,
    headers: HeaderMap,
    Json(request): Json<ActivateRequest>,
) -> Result<Json<ActivationResponse>, StudioError> {
    let store = lifecycle(&state)?;
    let policy_id = studio_policy_id(&state, request.policy_id.as_deref());
    let approver = context.principal().subject().as_str().to_string();
    let correlation = correlation_id(&headers, &state.identity);
    let outcome = store
        .activate(
            context.account().as_str(),
            &policy_id,
            request.sequence,
            request.expected_head,
            &approver,
            &request.reason,
            state.identity.now(),
        )
        .await;
    let head = match outcome {
        Ok(head) => head,
        Err(error) => {
            if matches!(error, LifecycleError::StaleHead(..)) {
                record_audit(
                    &state,
                    &approver,
                    context.account().as_str(),
                    "policy-studio.activate",
                    &policy_id,
                    AuditResult::Conflict,
                    &correlation,
                );
            }
            return Err(StudioError::from(error));
        }
    };
    // The durable head is per account and is what the promotion plane reads
    // on every decision (`AccountScopedPolicies`), so this activation applies
    // to this account at once and to no other.
    record_audit(
        &state,
        &approver,
        context.account().as_str(),
        "policy-studio.activate",
        &format!("{policy_id}#{}", head.sequence),
        AuditResult::Allowed,
        &correlation,
    );
    Ok(Json(ActivationResponse {
        head: serde_json::to_value(&head)
            .map_err(|error| StudioError::unavailable(error.to_string()))?,
        applied: true,
    }))
}

/// `POST /policy-studio/rollback` — go back to an older revision. It is a
/// new activation of an old byte-for-byte revision (the rollback saves it
/// again under a `-rb` version), never an edit of history, and it is the
/// same CAS.
pub async fn rollback(
    State(state): State<AppState>,
    Extension(context): Extension<VerifiedContext>,
    headers: HeaderMap,
    Json(request): Json<RollbackRequest>,
) -> Result<Json<ActivationResponse>, StudioError> {
    let store = lifecycle(&state)?;
    let policy_id = studio_policy_id(&state, request.policy_id.as_deref());
    let actor = context.principal().subject().as_str().to_string();
    let account = account_of(&context);
    // The rollback re-saves the target's content, so its editor is the one
    // who wrote that content; the caller is the approver. Under the
    // separation of duties an author cannot approve their own content.
    let target = store
        .revision(&account, &policy_id, request.target)
        .await?
        .ok_or_else(|| {
            StudioError::not_found(format!(
                "revision {} of {policy_id} does not exist",
                request.target
            ))
        })?;
    let actors = hodei_trust_policy::Actors::new(target.saved_by.clone(), actor.clone());
    let head = store
        .rollback(
            &account,
            &policy_id,
            request.target,
            request.expected_head,
            &actors,
            &request.reason,
            state.identity.now(),
        )
        .await?;
    record_audit(
        &state,
        &actor,
        context.account().as_str(),
        "policy-studio.rollback",
        &format!("{policy_id}#{}", head.sequence),
        AuditResult::Allowed,
        &correlation_id(&headers, &state.identity),
    );
    Ok(Json(ActivationResponse {
        head: serde_json::to_value(&head)
            .map_err(|error| StudioError::unavailable(error.to_string()))?,
        applied: true,
    }))
}

/// `GET /policy-studio/history` — the revisions and the events of a set:
/// who saved what, who activated it, against which head, and why. The
/// binding of every reason to a policy and a revision that AC2 asks for.
pub async fn history(
    State(state): State<AppState>,
    Extension(context): Extension<VerifiedContext>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<HistoryResponse>, StudioError> {
    let store = lifecycle(&state)?;
    let account = account_of(&context);
    let policy_id = studio_policy_id(&state, query.policy_id.as_deref());
    let limit = query
        .limit
        .unwrap_or(DEFAULT_HISTORY_LIMIT)
        .clamp(1, MAX_HISTORY_LIMIT);
    let offset = query.offset.unwrap_or(0);
    // One extra row says whether the window was cut, without a count.
    let mut revisions = store
        .history_page(&account, &policy_id, offset, limit + 1)
        .await?;
    let mut events = store
        .events_page(&account, &policy_id, offset, limit + 1)
        .await?;
    let revisions_truncated = revisions.len() > limit;
    let events_truncated = events.len() > limit;
    revisions.truncate(limit);
    events.truncate(limit);
    Ok(Json(HistoryResponse {
        offset,
        limit,
        revisions_truncated,
        events_truncated,
        policy_id,
        revisions: serde_json::to_value(&revisions)
            .map_err(|error| StudioError::unavailable(error.to_string()))?,
        events: serde_json::to_value(&events)
            .map_err(|error| StudioError::unavailable(error.to_string()))?,
    }))
}
