//! The sink: where the event goes when it is delivered.
//!
//! The sink is the **delivery boundary**. What lies on the other side may
//! be a `tracing` (lost with the process) or a durable store
//! (not lost). The contract promises neither: it promises that
//! the sink says whether the delivery happened, and that is why a failed sink can be
//! told apart from a delivery that did happen.
//!
//! # Idempotency belongs to the contract
//!
//! An event may arrive twice: a crash between delivery and the
//! mark leaves the event pending and it is delivered again. That is why
//! `event_id` is the idempotency key, and the sink **must** treat it
//! as such: delivering the same event twice is one logical effect, not
//! two. A sink that does not deduplicate breaks the contract, and the dispatcher
//! cannot know whether it complies.
//!
//! And it is worth saying exactly where the logical effect lies: **in the
//! audit store**, which is where the event is recorded. A
//! `tracing` is a diagnostic projection, and a log is append-only:
//! a redelivery writing it twice is not a duplicated fact, it is
//! one extra line in a log. The audit store, on the other hand, has
//! to keep the event **by its `event_id`**, and then two deliveries
//! of the same event are one record.
//!
//! What the outbox guarantees is the half that is its own: the redelivery
//! carries **the same** `event_id`, so a conforming sink
//! recognizes it.

use std::sync::{Arc, Mutex};

use thiserror::Error;

use crate::event::AuditEvent;

/// What can fail at the audit boundary.
///
/// The two failures are distinct and are told apart: the failed sink is a
/// delivery that did **not** happen, and the store that does not respond is a fact
/// that **could not be checked**. Neither of them is a delivery.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AuditError {
    /// The sink could not record the event.
    #[error("audit sink unavailable")]
    Unavailable,
    /// The outbox store could not respond.
    #[error("the audit outbox is unavailable: {detail}")]
    StoreUnavailable {
        /// What the store said.
        detail: String,
    },
}

/// Where an audit event goes.
///
/// Implementations **do not receive credentials**: the event no longer
/// carries them, and an implementation that asked for them would ask for them by another
/// path.
///
/// The contract demands two things:
///
/// - **Say whether the delivery happened.** An `Err` means it did not happen, and
///   never "maybe": whoever receives the error must be able to leave the
///   event pending.
/// - **Treat `event_id` as the idempotency key.** Delivering the same
///   event twice is one logical effect.
pub trait AuditSink: Send + Sync {
    /// Records the event.
    fn record(&self, event: &AuditEvent) -> Result<(), AuditError>;
}

/// The default sink: structured fields in `tracing`.
///
/// It is **best effort**: it is lost with the process. Whoever needs durability
/// injects another sink; the boundary is the same.
///
/// It is **not the audit store**: it is a diagnostic projection. It does not
/// deduplicate, because a log is append-only and an extra line is not a
/// duplicated fact. The logical effect — the record of the event — belongs to the
/// audit store, which has to key by `event_id`.
pub struct TracingAuditSink;

impl AuditSink for TracingAuditSink {
    fn record(&self, event: &AuditEvent) -> Result<(), AuditError> {
        tracing::info!(
            audit.event_id = %event.event_id,
            audit.correlation_id = %event.correlation_id(),
            audit.timestamp = %event.timestamp,
            audit.actor = %event.actor(),
            audit.tenant = %event.tenant(),
            audit.action = %event.action,
            audit.resource = %event.resource,
            audit.digest = ?event.digest,
            audit.result = %event.result,
            "security audit event"
        );
        Ok(())
    }
}

/// The in-memory sink, for tests and local verification.
///
/// Deduplicates by `event_id`: delivering the same event twice is one
/// event. This is what makes "a redelivery produces exactly one logical
/// effect" a **measurement** and not a promise.
#[derive(Debug, Clone, Default)]
pub struct InMemoryAuditSink {
    events: Arc<Mutex<Vec<AuditEvent>>>,
}

impl InMemoryAuditSink {
    /// A sink with nothing in it.
    pub fn new() -> Self {
        Self::default()
    }

    /// The recorded events, in the order they were recorded.
    pub fn events(&self) -> Vec<AuditEvent> {
        self.events.lock().expect("audit lock").clone()
    }

    /// How many events there are.
    pub fn len(&self) -> usize {
        self.events.lock().expect("audit lock").len()
    }

    /// Whether there are no events.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl AuditSink for InMemoryAuditSink {
    fn record(&self, event: &AuditEvent) -> Result<(), AuditError> {
        let mut events = self.events.lock().expect("audit lock");
        // The same event twice is the same event.
        if events.iter().any(|seen| seen.event_id == event.event_id) {
            return Ok(());
        }
        events.push(event.clone());
        Ok(())
    }
}

/// The sink that fails; it serves to test that an audit failure does not
/// corrupt the decision and that the event is **not** considered delivered.
#[derive(Debug, Clone, Default)]
pub struct FailingAuditSink {
    attempts: Arc<std::sync::atomic::AtomicUsize>,
}

impl FailingAuditSink {
    /// A sink that always fails.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many attempts it has received.
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
