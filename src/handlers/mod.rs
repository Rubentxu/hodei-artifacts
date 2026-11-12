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
pub mod repositories;
pub mod schemas;

use axum::http::HeaderMap;

use crate::app_state::AppState;
use crate::audit::{AuditEvent, AuditResult};
use kernel::application::ports::clock::Identity;

/// Record a security event.
///
/// The event never carries a credential: it identifies who acted, what they
/// acted on and how it ended. The sink is best-effort -- a sink that is
/// unavailable is reported and never turns into a decision, because losing
/// the trail must not change the answer the caller gets. The durability limit
/// is declared in `docs/security/audit.md`.
pub(crate) fn record_audit(
    state: &AppState,
    actor: &str,
    action: &str,
    resource: &str,
    result: AuditResult,
    correlation_id: &str,
) {
    let event = AuditEvent::new(
        state.identity.new_id(),
        state.identity.now(),
        actor,
        action,
        resource,
        result,
        correlation_id,
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
