//! The audit event: what is said about an operation.
//!
//! The event is a statement about an operation, and for that reason it carries what
//! makes it verifiable: **who** acted, **in which account**, **what** they did,
//! **on which** resource, **with what content**, **how it ended** and **with
//! what correlation**. What it never carries is a credential: an event that
//! says who acted does not need to say what they authenticated with, and if it
//! did, the trail would become one more secret that has to be
//! protected.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The bound of a text field of the event.
///
/// The text fields of the event are set by the caller, and they go both to a
/// **durable** row and to a log line. Without a bound, a principal who is denied
/// a publication can write arbitrary and unbounded rows into the outbox, and a
/// line break in `resource` is a log
/// injection.
pub const MAX_FIELD_LENGTH: usize = 256;

/// Cleans a text that goes into the event or the outbox.
///
/// Removes control characters (a line break in a log is an
/// injection) and truncates whatever exceeds the bound. The character count
/// is in characters, not bytes, so the text is not split in the
/// middle of a UTF-8 sequence.
///
/// **This is not secret sanitization.** The event carries no
/// credentials because the code does not put them in, not because they are cleaned here.
pub fn sanitize_text(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_FIELD_LENGTH)
        .collect()
}

/// How the operation that the event describes ended.
///
/// The four outcomes are four distinct facts and are not conflated: a
/// rejection is not a failure, and a conflict is not a rejection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditResult {
    /// It was allowed and the operation completed.
    Allowed,
    /// Authorization denied it and nothing was written.
    Denied,
    /// There was already something with different content and it was not overwritten.
    Conflict,
    /// Something failed and the operation did not complete.
    Failed,
}

impl std::fmt::Display for AuditResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Allowed => "allowed",
            Self::Denied => "denied",
            Self::Conflict => "conflict",
            Self::Failed => "failed",
        })
    }
}

/// Who acted, in which account and with what correlation.
///
/// The three things travel together because they always travel together: the account is
/// the **verified tenant**, and it is not deduced from the request but from the
/// principal that the machine already verified. An account that is deduced is an
/// account that can be handed out.
///
/// The **actor** is the HRN of whoever acted. In the HTTP layer it comes from the
/// verified principal, so it is verified. In the catalog it is declared by
/// the publisher, because the catalog verifies the **account** against the
/// authorizer and not the subject: for that reason the word "verified" is not used
/// here, and whoever publishes has to supply the actor that actually acted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditContext {
    /// The actor: the HRN of the subject.
    pub actor: String,
    /// The verified account of the actor: the tenant.
    pub tenant: String,
    /// The correlation of the request.
    pub correlation_id: String,
}

impl AuditContext {
    /// The context of an operation.
    pub fn new(
        actor: impl Into<String>,
        tenant: impl Into<String>,
        correlation_id: impl Into<String>,
    ) -> Self {
        Self {
            actor: sanitize_text(&actor.into()),
            tenant: sanitize_text(&tenant.into()),
            correlation_id: sanitize_text(&correlation_id.into()),
        }
    }
}

/// An audit event.
///
/// It never carries a bearer token or a credential: only identities, facts
/// and the outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    /// The identifier of the event.
    ///
    /// It is the **idempotency key**: redelivering this event is delivering
    /// the same event, and a sink that treats it that way does not duplicate the effect.
    pub event_id: String,
    /// Who acted, in which account and with what correlation.
    pub context: AuditContext,
    /// When it happened.
    pub timestamp: DateTime<Utc>,
    /// What was done: `publish`, `create_repository`, `move_tag`...
    pub action: String,
    /// On what it was done: the canonical resource.
    pub resource: String,
    /// The digest of the content, when the operation deals with content.
    ///
    /// It is a string (`algorithm:value`) so that the event does not depend on the
    /// model of any domain: the event says what content was moved, not
    /// how the catalog represents it.
    pub digest: Option<String>,
    /// How it ended.
    pub result: AuditResult,
}

impl AuditEvent {
    /// The event, with the identifier and the moment taken from the seams
    /// that produce them.
    ///
    /// The moment and the identifier are **facts of the machine**, not of the
    /// operation: taking them from a seam is what makes it possible to pin them in
    /// a test and compare two executions of the same operation.
    ///
    /// The texts are **bounded**: they are set by the caller, and they go to a durable
    /// row and to a log line. See [`sanitize_text`].
    pub fn new(
        event_id: impl Into<String>,
        context: AuditContext,
        timestamp: DateTime<Utc>,
        action: impl Into<String>,
        resource: impl Into<String>,
        digest: Option<String>,
        result: AuditResult,
    ) -> Self {
        Self {
            event_id: event_id.into(),
            context,
            timestamp,
            action: sanitize_text(&action.into()),
            resource: sanitize_text(&resource.into()),
            digest: digest.map(|value| sanitize_text(&value)),
            result,
        }
    }

    /// The actor that acted.
    pub fn actor(&self) -> &str {
        &self.context.actor
    }

    /// The verified account in which it acted.
    pub fn tenant(&self) -> &str {
        &self.context.tenant
    }

    /// The correlation of the operation.
    pub fn correlation_id(&self) -> &str {
        &self.context.correlation_id
    }

    /// Whether two events state the same fact.
    ///
    /// The instant is **not** compared: a clock does not necessarily keep the
    /// same precision when passing through a store, and two expressions of the same
    /// fact would stop being equal just because of that. What decides is who, in which
    /// account, what they did, on what, with what content and how it ended.
    pub fn same_fact(&self, other: &AuditEvent) -> bool {
        self.context == other.context
            && self.action == other.action
            && self.resource == other.resource
            && self.digest == other.digest
            && self.result == other.result
    }
}
