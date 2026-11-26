//! The status of the reactive effects (HSP-78 AC3, slice of HSP-323).
//!
//! `GET /api/v1/reactions/intents` answers what became of the effects the
//! reactive cycle said: which effect, for which tenant, caused by what, how
//! many times a worker took it, in which state it is and why it failed.
//!
//! It is a **read** and it sits on the administrative surface: the gate of
//! `/api/v1` authenticates the caller (401) and asks Cedar for the concrete
//! route (403), exactly as it does for the other operation views (`/gc/plan`,
//! `/scrub`). The tenant comes from the verified principal, never from the
//! request: an effect of another account is neither listed nor counted.
//!
//! # What the view does NOT expose
//!
//! - The payload of the effect: it is the one place where a value that is
//!   sensitive could travel, so no field of it is copied to the response.
//! - Secrets in the failure reason: the reason is free text written by an
//!   executor, so it is redacted and bounded before it leaves.
//!
//! # Known gaps
//!
//! - The execution records (the "runs") carry no tenant, so they cannot be
//!   attributed to a caller and are not exposed. The effect row carries the
//!   causation chain, which is the cause the run would have shown.
//! - `attempts` is the number of claims of the effect (the lease
//!   generation); the execution record's own attempt counter is not joined.
//! - The tenant of an intent is the owner derived from the HRN its payload
//!   names (see [`crate::trust::reaction::intent_owner_account`]); the scan
//!   is bounded, and a truncated answer says so.

use axum::{
    Extension, Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use hodei_authz::VerifiedContext;
use hodei_reactive::intent::{IntentStatus, Receipt};
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::trust::reaction::intent_owner_account;

/// How many effects of the outbox one request reads, owners included.
const MAX_SCAN: usize = 5000;
/// The default and the maximum size of the answer.
const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 500;
/// The longest failure reason the view returns.
const MAX_REASON: usize = 240;

/// What the client may narrow.
#[derive(Debug, Deserialize)]
pub struct StatusQuery {
    /// Only the effects in this state.
    #[serde(default)]
    pub state: Option<String>,
    /// How many effects at most. It can only narrow the maximum.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Why a failed effect failed.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct FailureView {
    /// The reason, redacted and bounded.
    pub reason: String,
    /// Whether trying again can work (`null` when it is not known).
    pub retryable: Option<bool>,
}

/// One effect of the reactive cycle.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct EffectView {
    /// The stable identity of the effect.
    pub effect_id: String,
    /// The tenant: the verified account of the caller.
    pub tenant: String,
    /// What kind of effect it is.
    pub kind: String,
    /// The reaction that planned it.
    pub behavior: String,
    /// The event that set it off.
    pub cause_event: String,
    /// The chain of events that led to it, oldest first.
    pub causation: Vec<String>,
    /// How many times a worker took it.
    pub attempts: u64,
    /// `pending`, `claimed`, `applied`, `failed` or `unknown`.
    pub state: String,
    /// Why it failed, when a receipt says so.
    pub failure: Option<FailureView>,
}

/// The answer.
#[derive(Debug, Serialize)]
pub struct StatusView {
    /// The effects of the caller's tenant, ordered by identity.
    pub items: Vec<EffectView>,
    /// Whether the outbox held more than the bounded read covered, or more
    /// than `limit` matched.
    pub truncated: bool,
}

/// How the view can fail.
pub struct ReactionsApiError {
    status: StatusCode,
    message: &'static str,
}

impl IntoResponse for ReactionsApiError {
    fn into_response(self) -> Response {
        let body = Json(serde_json::json!({
            "error": self.message,
            "status": self.status.as_u16(),
        }));
        (self.status, body).into_response()
    }
}

/// `GET /api/v1/reactions/intents`
pub async fn list_intents(
    State(state): State<AppState>,
    Extension(context): Extension<VerifiedContext>,
    Query(query): Query<StatusQuery>,
) -> Result<Json<StatusView>, ReactionsApiError> {
    let account = context.account().as_str().to_string();
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    let reactor = state.reactor.as_ref().ok_or(ReactionsApiError {
        status: StatusCode::NOT_IMPLEMENTED,
        message: "the reactive cycle is not wired in this deployment",
    })?;
    let scan = match reactor.scan_intents(MAX_SCAN).await {
        Some(Ok(scan)) => scan,
        Some(Err(error)) => {
            // The detail belongs to the log, not to the client.
            tracing::warn!(%error, "the reaction status could not be read");
            return Err(ReactionsApiError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                message: "the reaction status is not available",
            });
        }
        None => {
            return Err(ReactionsApiError {
                status: StatusCode::NOT_IMPLEMENTED,
                message: "the outbox of this deployment cannot be read",
            });
        }
    };

    let mut items: Vec<EffectView> = scan
        .rows
        .iter()
        // The tenant filter: only what the caller's account owns.
        .filter(|row| intent_owner_account(&row.intent).as_deref() == Some(account.as_str()))
        .filter(|row| {
            query
                .state
                .as_deref()
                .is_none_or(|wanted| row.state == wanted)
        })
        .map(|row| view_of(row, &account))
        .collect();
    let mut truncated = scan.truncated;
    if items.len() > limit {
        items.truncate(limit);
        truncated = true;
    }
    Ok(Json(StatusView { items, truncated }))
}

/// The view of one effect. The payload is not read.
fn view_of(row: &IntentStatus, account: &str) -> EffectView {
    let failure = match &row.receipt {
        Some(Receipt::Failed { reason, retryable }) => Some(FailureView {
            reason: redact(reason),
            retryable: Some(*retryable),
        }),
        Some(Receipt::Unknown { reason }) => Some(FailureView {
            reason: redact(reason),
            retryable: None,
        }),
        Some(Receipt::Applied) | None => None,
    };
    EffectView {
        effect_id: row.intent.identity.clone(),
        tenant: account.to_string(),
        kind: row.intent.kind.clone(),
        behavior: row.intent.origin.behavior.clone(),
        cause_event: row.intent.origin.event.event_id.clone(),
        causation: row.intent.origin.event.causation.clone(),
        attempts: row.attempts,
        state: row.state.clone(),
        failure,
    }
}

/// Words that say the next value is a secret.
const SENSITIVE: [&str; 8] = [
    "secret",
    "token",
    "password",
    "passwd",
    "authorization",
    "bearer",
    "apikey",
    "credential",
];

fn is_sensitive_key(word: &str) -> bool {
    let key: String = word
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    SENSITIVE.iter().any(|needle| key.contains(needle)) || key.contains("apikey")
}

/// Removes what looks like a secret from a free-text reason and bounds it.
///
/// A word that names a sensitive key and carries a value (`key=value`,
/// `key:value`) loses the value; a sensitive key that ends a word (`key:` or
/// `key =`) or the word `Bearer` loses the NEXT word. It is conservative on
/// purpose: a reason with a redaction too many is a nuisance, a reason with
/// a secret is an incident.
pub(crate) fn redact(reason: &str) -> String {
    const MASK: &str = "[redacted]";
    let mut out: Vec<String> = Vec::new();
    let mut mask_next = false;
    for word in reason.split_whitespace() {
        if mask_next {
            // `=` or `:` alone between key and value.
            if word == "=" || word == ":" {
                out.push(word.to_string());
            } else {
                out.push(MASK.to_string());
                // `Authorization: Bearer <token>`: the scheme is masked and
                // the credential after it is the real secret.
                mask_next = matches!(word.to_ascii_lowercase().as_str(), "bearer" | "basic");
            }
            continue;
        }
        if let Some(position) = word.find(['=', ':']) {
            let (key, value) = word.split_at(position);
            if is_sensitive_key(key) {
                if value.len() > 1 {
                    out.push(format!("{key}{}{MASK}", &value[..1]));
                } else {
                    out.push(word.to_string());
                    mask_next = true;
                }
                continue;
            }
        }
        if is_sensitive_key(word) {
            out.push(word.to_string());
            mask_next = true;
            continue;
        }
        out.push(word.to_string());
    }
    let joined = out.join(" ");
    if joined.chars().count() > MAX_REASON {
        let cut: String = joined.chars().take(MAX_REASON).collect();
        format!("{cut}...")
    } else {
        joined
    }
}

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn a_value_after_a_sensitive_key_is_masked() {
        assert_eq!(
            redact("call failed: password=hunter2 on host"),
            "call failed: password=[redacted] on host"
        );
        assert_eq!(
            redact("sent Authorization: Bearer abc.def.ghi"),
            "sent Authorization: [redacted] [redacted]"
        );
        assert_eq!(redact("api_key : s3cr3t rest"), "api_key : [redacted] rest");
    }

    #[test]
    fn a_plain_reason_is_kept_and_a_long_one_is_bounded() {
        assert_eq!(redact("the graph timed out"), "the graph timed out");
        assert!(redact(&"x ".repeat(400)).chars().count() <= 243);
    }
}
