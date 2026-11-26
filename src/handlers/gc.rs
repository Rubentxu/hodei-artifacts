//! Garbage collection of the store of content.
//!
//! The only surface that deletes bytes. It is deliberately small:
//!
//! - `GET /gc/plan` is the dry-run: it explains, blob by blob, what is kept
//!   and why, and never writes.
//! - `POST /gc/sweep` deletes, and only when the body says `execute: true`.
//!   Without it the answer is the same dry-run.
//! - `GET|POST /gc/holds` and `POST /gc/holds/release` manage the retentions.
//!
//! Each route is a distinct resource for the administrative gate, so a
//! policy can grant the plan without granting the sweep. The collection is
//! platform-wide: the store is shared by every account, and the digest is
//! the identity of the bytes.

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::gc::{GcError, GcService, Plan, SweepReport};
use crate::handlers::{correlation_id, record_audit};
use axum::{
    Extension, Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use hodei_authz::VerifiedContext;
use hodei_catalog::{ContentHold, HoldScope};
use hodei_evidence::ContentDigest;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// The body of a sweep.
#[derive(Debug, Default, Deserialize)]
pub struct SweepRequest {
    /// Delete for real. Anything else is a dry-run.
    #[serde(default)]
    pub execute: bool,
}

/// The answer of a sweep that did not delete.
#[derive(Debug, Serialize)]
pub struct DryRunResponse {
    /// Always `true`: nothing was deleted.
    pub dry_run: bool,
    /// What a sweep would do now. Shared with the plan endpoint's cache.
    pub plan: Arc<Plan>,
}

/// The answer of a sweep that did delete.
#[derive(Debug, Serialize)]
pub struct ExecutedResponse {
    /// Always `false`: this was a real pass.
    pub dry_run: bool,
    /// What the pass did.
    pub report: SweepReport,
}

/// What a hold keeps, as the API says it.
///
/// The digest travels as `sha256:<hex>`, the same way the plan and the
/// registry name it, and not as the internal record of the catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScopeView {
    /// One blob.
    Digest {
        /// `sha256:<hex>`.
        digest: String,
    },
    /// Everything a repository references.
    Repository {
        /// The account.
        account: String,
        /// The repository.
        repository: String,
    },
    /// Everything an account references.
    Account {
        /// The account.
        account: String,
    },
}

impl ScopeView {
    fn into_scope(self) -> Result<HoldScope, GcApiError> {
        Ok(match self {
            Self::Digest { digest } => {
                let hex = digest
                    .strip_prefix("sha256:")
                    .filter(|hex| {
                        hex.len() == 64
                            && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
                    })
                    .ok_or_else(|| {
                        GcApiError::new(
                            StatusCode::BAD_REQUEST,
                            "a digest is `sha256:` followed by 64 lowercase hexadecimal digits",
                        )
                    })?;
                HoldScope::Digest {
                    digest: ContentDigest::sha256(hex),
                }
            }
            Self::Repository {
                account,
                repository,
            } => HoldScope::Repository {
                account,
                repository,
            },
            Self::Account { account } => HoldScope::Account { account },
        })
    }

    fn from_scope(scope: &HoldScope) -> Self {
        match scope {
            HoldScope::Digest { digest } => Self::Digest {
                digest: format!("sha256:{}", digest.value),
            },
            HoldScope::Repository {
                account,
                repository,
            } => Self::Repository {
                account: account.clone(),
                repository: repository.clone(),
            },
            HoldScope::Account { account } => Self::Account {
                account: account.clone(),
            },
        }
    }
}

/// A hold, as the API says it.
#[derive(Debug, Serialize)]
pub struct HoldView {
    /// The identifier.
    pub id: String,
    /// What it keeps.
    pub scope: ScopeView,
    /// Why it was placed.
    pub reason: String,
    /// Who placed it.
    pub actor: String,
    /// When it was placed.
    pub created_at: DateTime<Utc>,
    /// When it lapses; without a date it lasts until someone releases it.
    pub expires_at: Option<DateTime<Utc>>,
    /// Whether it holds right now.
    pub active: bool,
}

impl From<&ContentHold> for HoldView {
    fn from(hold: &ContentHold) -> Self {
        Self {
            id: hold.id.clone(),
            scope: ScopeView::from_scope(&hold.scope),
            reason: hold.reason.clone(),
            actor: hold.actor.clone(),
            created_at: hold.created_at,
            expires_at: hold.expires_at,
            active: hold.is_active(Utc::now()),
        }
    }
}

/// The body that places a hold.
#[derive(Debug, Deserialize)]
pub struct PlaceHoldRequest {
    /// What the hold keeps.
    pub scope: ScopeView,
    /// Why. It cannot be empty.
    pub reason: String,
    /// When it lapses. Without a date it lasts until someone releases it.
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

/// The body that releases a hold.
#[derive(Debug, Deserialize)]
pub struct ReleaseHoldRequest {
    /// The hold to release.
    pub id: String,
}

/// The answer of a release.
#[derive(Debug, Serialize)]
pub struct ReleaseHoldResponse {
    /// The hold that was released.
    pub id: String,
}

/// The list of holds.
#[derive(Debug, Serialize)]
pub struct HoldsResponse {
    /// Every registered hold, active or lapsed.
    pub holds: Vec<HoldView>,
}

/// How a failure of the collection reaches the client.
///
/// An unknown state is a 503, not a 500: nothing is wrong with the request
/// and nothing was deleted; the caller can retry when the catalog answers.
#[derive(Debug)]
pub struct GcApiError {
    status: StatusCode,
    message: String,
}

impl GcApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn result(&self) -> AuditResult {
        match self.status {
            StatusCode::CONFLICT => AuditResult::Conflict,
            _ => AuditResult::Failed,
        }
    }
}

impl From<GcError> for GcApiError {
    fn from(error: GcError) -> Self {
        match error {
            GcError::Unsupported => Self::new(StatusCode::NOT_IMPLEMENTED, error.to_string()),
            GcError::Busy => Self::new(StatusCode::CONFLICT, error.to_string()),
            GcError::Unknown(_) => Self::new(StatusCode::SERVICE_UNAVAILABLE, error.to_string()),
            GcError::Invalid(message) => Self::new(StatusCode::BAD_REQUEST, message),
            GcError::Ledger(_) | GcError::Store(_) => {
                // The detail is for the log: a path or a query is not for
                // the client.
                tracing::error!(%error, "the garbage collection failed");
                Self::new(StatusCode::INTERNAL_SERVER_ERROR, "the collection failed")
            }
        }
    }
}

impl IntoResponse for GcApiError {
    fn into_response(self) -> Response {
        let body = Json(serde_json::json!({
            "error": self.message,
            "status": self.status.as_u16(),
        }));
        (self.status, body).into_response()
    }
}

fn service(state: &AppState) -> Result<Arc<GcService>, GcApiError> {
    state.gc.clone().ok_or_else(|| {
        GcApiError::new(
            StatusCode::NOT_IMPLEMENTED,
            "the garbage collection needs the durable catalog and a store that can reclaim",
        )
    })
}

fn audit(
    state: &AppState,
    context: &VerifiedContext,
    action: &str,
    resource: &str,
    result: AuditResult,
    correlation: &str,
) {
    record_audit(
        state,
        context.principal().subject().as_str(),
        context.account().as_str(),
        action,
        resource,
        result,
        correlation,
    );
}

/// The dry-run: what a sweep would do now. Never writes.
pub async fn plan(
    State(state): State<AppState>,
    Extension(_context): Extension<VerifiedContext>,
) -> Result<Json<Arc<Plan>>, GcApiError> {
    let service = service(&state)?;
    Ok(Json(service.dry_run().await?))
}

/// A sweep. Without `execute: true` it is the dry-run.
pub async fn sweep(
    State(state): State<AppState>,
    Extension(context): Extension<VerifiedContext>,
    headers: HeaderMap,
    body: Option<Json<SweepRequest>>,
) -> Result<Response, GcApiError> {
    let service = service(&state)?;
    let execute = body.is_some_and(|Json(request)| request.execute);
    if !execute {
        let plan = service.dry_run().await?;
        return Ok(Json(DryRunResponse {
            dry_run: true,
            plan,
        })
        .into_response());
    }
    let correlation = correlation_id(&headers, &state.identity);
    let actor = context.principal().subject().as_str().to_string();
    let account = context.account().as_str().to_string();
    // The pass runs in its own task. The request can be cut -- the timeout
    // of the router, a client that hangs up -- and a future that is dropped
    // in the middle of a delete would release the barrier of the store
    // while the removal is still queued. Detached, the pass finishes, and
    // its audit is written, whatever happens to the connection.
    let task = {
        let state = state.clone();
        let context = context.clone();
        tokio::spawn(async move {
            match service.sweep_platform(&actor, &account).await {
                Ok(report) => {
                    audit(
                        &state,
                        &context,
                        "SweepContent",
                        &report.run_id,
                        AuditResult::Allowed,
                        &correlation,
                    );
                    Ok(report)
                }
                Err(error) => {
                    let error = GcApiError::from(error);
                    audit(
                        &state,
                        &context,
                        "SweepContent",
                        "gc-sweep",
                        error.result(),
                        &correlation,
                    );
                    Err(error)
                }
            }
        })
    };
    match task.await {
        Ok(Ok(report)) => Ok(Json(ExecutedResponse {
            dry_run: false,
            report,
        })
        .into_response()),
        Ok(Err(error)) => Err(error),
        Err(join) => {
            tracing::error!(%join, "the sweep task did not finish");
            Err(GcApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "the collection failed",
            ))
        }
    }
}

/// The registered holds.
pub async fn list_holds(
    State(state): State<AppState>,
    Extension(_context): Extension<VerifiedContext>,
) -> Result<Json<HoldsResponse>, GcApiError> {
    let service = service(&state)?;
    Ok(Json(HoldsResponse {
        holds: service.holds().await?.iter().map(HoldView::from).collect(),
    }))
}

/// Places a hold.
pub async fn place_hold(
    State(state): State<AppState>,
    Extension(context): Extension<VerifiedContext>,
    headers: HeaderMap,
    Json(request): Json<PlaceHoldRequest>,
) -> Result<(StatusCode, Json<HoldView>), GcApiError> {
    let service = service(&state)?;
    let correlation = correlation_id(&headers, &state.identity);
    let actor = context.principal().subject().as_str().to_string();
    let scope = request.scope.into_scope()?;
    match service
        .place_hold(&actor, scope, &request.reason, request.expires_at)
        .await
    {
        Ok(hold) => {
            audit(
                &state,
                &context,
                "PlaceContentHold",
                &hold.id,
                AuditResult::Allowed,
                &correlation,
            );
            Ok((StatusCode::CREATED, Json(HoldView::from(&hold))))
        }
        Err(error) => {
            let error = GcApiError::from(error);
            audit(
                &state,
                &context,
                "PlaceContentHold",
                "gc-hold",
                error.result(),
                &correlation,
            );
            Err(error)
        }
    }
}

/// Releases a hold.
pub async fn release_hold(
    State(state): State<AppState>,
    Extension(context): Extension<VerifiedContext>,
    headers: HeaderMap,
    Json(request): Json<ReleaseHoldRequest>,
) -> Result<Json<ReleaseHoldResponse>, GcApiError> {
    let service = service(&state)?;
    let correlation = correlation_id(&headers, &state.identity);
    match service.release_hold(&request.id).await {
        Ok(true) => {
            audit(
                &state,
                &context,
                "ReleaseContentHold",
                &request.id,
                AuditResult::Allowed,
                &correlation,
            );
            Ok(Json(ReleaseHoldResponse { id: request.id }))
        }
        Ok(false) => Err(GcApiError::new(
            StatusCode::NOT_FOUND,
            "there is no such hold",
        )),
        Err(error) => {
            let error = GcApiError::from(error);
            audit(
                &state,
                &context,
                "ReleaseContentHold",
                &request.id,
                error.result(),
                &correlation,
            );
            Err(error)
        }
    }
}
