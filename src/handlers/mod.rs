//! HTTP Handlers for Hodei Artifacts API
//!
//! This module contains all HTTP request handlers organized by domain.
//! Each handler is responsible for:
//! - Extracting request data and validating it
//! - Calling the appropriate use case
//! - Mapping results to HTTP responses
//! - Error handling and logging

pub mod capsules;
pub mod health;
pub mod iam;
pub mod playground;
pub mod policies;
pub mod raw;
pub mod repositories;
pub mod schemas;

use axum::http::{HeaderMap, header};

use crate::app_state::AppState;
use crate::audit::{AuditContext, AuditEvent, AuditResult};
use kernel::application::ports::clock::Identity;

/// The bearer token of a request.
///
/// Only `Bearer <token>` is accepted, and the token cannot be empty: a
/// header with the scheme but without a token is not a credential.
///
/// It lives here because three surfaces need it, and three copies of the
/// same rule are three rules that can stop agreeing.
pub(crate) fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    let token = token.trim();
    if !scheme.eq_ignore_ascii_case("Bearer") || token.is_empty() {
        return None;
    }
    Some(token.to_string())
}

/// Record a security event.
///
/// The event never carries a credential: it identifies who acted, in which
/// verified account, what they acted on and how it ended. The sink is
/// best-effort -- a sink that is unavailable is reported and never turns
/// into a decision, because losing the trail must not change the answer the
/// caller gets. The durability limit is declared in `docs/security/audit-events.md`.
pub(crate) fn record_audit(
    state: &AppState,
    actor: &str,
    tenant: &str,
    action: &str,
    resource: &str,
    result: AuditResult,
    correlation_id: &str,
) {
    let event = AuditEvent::new(
        state.identity.new_id(),
        AuditContext::new(actor, tenant, correlation_id),
        state.identity.now(),
        action,
        resource,
        None,
        result,
    );
    if state.audit_sink.record(&event).is_err() {
        tracing::warn!(
            correlation_id = %correlation_id,
            "audit sink failed; security event was not persisted"
        );
    }
}

/// The identifier that ties the audit events of one request together.
///
/// It comes from the `x-request-id` header when the caller sends one, and it
/// is generated otherwise. A caller-supplied value is accepted only when it
/// is short and free of control characters and whitespace, because it ends up
/// in a log line.
pub(crate) fn correlation_id(headers: &HeaderMap, identity: &Identity) -> String {
    headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .chars()
                    .all(|character| !character.is_control() && !character.is_whitespace())
        })
        .map(str::to_string)
        .unwrap_or_else(|| identity.new_id())
}
