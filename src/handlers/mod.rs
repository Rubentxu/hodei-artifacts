//! HTTP Handlers for Hodei Artifacts API
//!
//! This module contains all HTTP request handlers organized by domain.
//! Each handler is responsible for:
//! - Extracting request data and validating it
//! - Calling the appropriate use case
//! - Mapping results to HTTP responses
//! - Error handling and logging

pub mod capsules;
pub mod gc;
pub mod health;
pub mod iam;
mod maven;
pub mod npm;
pub mod oci;
pub mod packages;
pub mod playground;
pub mod policies;
pub mod promotions;
pub mod pypi;
pub mod raw;
mod raw_group;
pub mod repositories;
pub mod schemas;
pub mod scrub;
pub mod search;

use axum::http::{HeaderMap, header};
use hodei_authz::{Credentials, VerifiedContext, verify_principal};

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

/// Why a request could not be authenticated.
///
/// The two cases are different and the caller has to answer them
/// differently: a credential that is not valid is the caller's problem
/// (401), and a verifier that could not answer is the service's (503).
/// Answering 401 to an outage tells the caller to change a credential
/// that was fine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Authentication {
    /// The credential is missing or is not valid.
    Unauthorized,
    /// The verifier could not answer.
    Unavailable,
}

/// The verified principal of a request.
///
/// Authentication is the first gate: a request without a credential does
/// not get to look at what it asks for, and so it cannot turn into a 400
/// that says something about a path the caller may not know about.
///
/// It lives here because the surfaces that read the catalog need it, and
/// copies of the same rule are rules that can stop agreeing. The raw
/// and the OCI surfaces still have their own: they answer the challenge
/// with a header of their own, and that is their business.
pub(crate) async fn the_context(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<VerifiedContext, Authentication> {
    let token = bearer_token(headers).ok_or(Authentication::Unauthorized)?;
    let principal =
        match verify_principal(state.authenticator.as_ref(), &Credentials::bearer(token)).await {
            Ok(principal) => principal,
            Err(hodei_authz::AuthenticationError::ProviderUnavailable(_)) => {
                return Err(Authentication::Unavailable);
            }
            Err(_) => return Err(Authentication::Unauthorized),
        };
    Ok(VerifiedContext::from_principal(principal))
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

/// The pairs of a query string, with the values decoded.
///
/// The values arrive percent-encoded, and one that cannot be decoded is a
/// query that does not say what it says: the caller decides what to do
/// with that, and this only tells it.
///
/// It lives here because three surfaces read a query string, and three
/// copies of the same rule are three rules that can stop agreeing.
pub(crate) fn the_query_values(raw: Option<&str>) -> Option<Vec<(String, String)>> {
    let raw = raw?;
    if raw.is_empty() {
        return Some(Vec::new());
    }
    let mut values = Vec::new();
    for pair in raw.split('&') {
        let (key, value) = pair.split_once('=')?;
        values.push((percent_decode(key)?, percent_decode(value)?));
    }
    Some(values)
}

/// The one value of a key in a query string.
///
/// A query that names the key twice does not say which of the two it
/// means, and a query that does not name it says nothing about it: both
/// are a request that is not coherent, and both are the same answer.
///
/// It lives here with the pairs because the two rules are the same rule.
pub(crate) fn the_one_value(raw: Option<&str>, key: &str) -> Option<String> {
    let values = the_query_values(raw)?;
    let mut found: Option<String> = None;
    for (name, value) in values {
        if name != key {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(value);
    }
    found
}

/// The text of a percent-encoded value.
///
/// A value that is not percent-encoded is returned as it is, and one that
/// is malformed is not returned: a query that cannot be read is a query
/// that does not say what it says.
fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let high = hex(*bytes.get(index + 1)?)?;
                let low = hex(*bytes.get(index + 2)?)?;
                out.push((high << 4) | low);
                index += 3;
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// The value of a hexadecimal digit.
fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
