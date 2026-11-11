//! Minimal structured audit events for security-relevant operations.
//!
//! The event contains identifiers and the decision, never credentials. The
//! default sink writes structured `tracing` fields; it is best-effort, not a
//! durable audit store. `AuditSink` is the seam a future Audit Trail adapter
//! implements without changing authorization or create rules.

use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Result carried by an audit event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditResult {
    Allowed,
    Denied,
    Conflict,
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

/// Security audit event. It never carries a bearer token or raw credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub event_id: String,
    pub correlation_id: String,
    pub timestamp: DateTime<Utc>,
    pub actor: String,
    pub action: String,
    pub resource: String,
    pub result: AuditResult,
}

impl AuditEvent {
    /// The event, with the identity and the moment taken from the given
    /// seams.
    ///
    /// The moment and the identifier are facts about the machine, not about
    /// the operation. Taking them from a seam is what lets a test pin them
    /// and compare two executions of the same operation.
    pub fn new(
        event_id: impl Into<String>,
        timestamp: DateTime<Utc>,
        actor: impl Into<String>,
        action: impl Into<String>,
        resource: impl Into<String>,
        result: AuditResult,
        correlation_id: impl Into<String>,
    ) -> Self {
        Self {
            event_id: event_id.into(),
            correlation_id: correlation_id.into(),
            timestamp,
            actor: actor.into(),
            action: action.into(),
            resource: resource.into(),
            result,
        }
    }
}

/// Sink for audit events. Implementations must not receive credentials.
pub trait AuditSink: Send + Sync {
    fn record(&self, event: &AuditEvent) -> Result<(), AuditError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AuditError {
    #[error("audit sink unavailable")]
    Unavailable,
}

/// Best-effort structured tracing sink.
pub struct TracingAuditSink;

impl AuditSink for TracingAuditSink {
    fn record(&self, event: &AuditEvent) -> Result<(), AuditError> {
        tracing::info!(
            audit.event_id = %event.event_id,
            audit.correlation_id = %event.correlation_id,
            audit.timestamp = %event.timestamp,
            audit.actor = %event.actor,
            audit.action = %event.action,
            audit.resource = %event.resource,
            audit.result = %event.result,
            "security audit event"
        );
        Ok(())
    }
}

/// In-memory sink for tests and local verification.
#[derive(Debug, Clone, Default)]
pub struct InMemoryAuditSink {
    events: Arc<Mutex<Vec<AuditEvent>>>,
}

impl InMemoryAuditSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> Vec<AuditEvent> {
        self.events.lock().expect("audit lock").clone()
    }

    pub fn len(&self) -> usize {
        self.events.lock().expect("audit lock").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl AuditSink for InMemoryAuditSink {
    fn record(&self, event: &AuditEvent) -> Result<(), AuditError> {
        self.events.lock().expect("audit lock").push(event.clone());
        Ok(())
    }
}

/// Sink that fails; used to prove audit failures do not corrupt the decision.
#[derive(Debug, Clone, Default)]
pub struct FailingAuditSink {
    attempts: Arc<std::sync::atomic::AtomicUsize>,
}

impl FailingAuditSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn attempts(&self) -> usize {
        self.attempts.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl AuditSink for FailingAuditSink {
    fn record(&self, _event: &AuditEvent) -> Result<(), AuditError> {
        self.attempts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(AuditError::Unavailable)
    }
}
