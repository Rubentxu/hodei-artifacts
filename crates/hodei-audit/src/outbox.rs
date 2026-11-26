//! The outbox: where the event waits between being stated and being delivered.
//!
//! The outbox is the piece that makes the event and the mutation that causes it
//! share **a single commit boundary**. The publisher writes
//! the outbox row inside the same transaction that publishes; the one that
//! delivers takes it afterwards.
//!
//! # States are explicit, because a guessed state is a state
//! that lies
//!
//! - `pending`: stated, waiting for someone to carry it.
//! - `claimed`: a worker holds it, with a **lease**. When the lease
//!   expires, the event becomes claimable again: that is what ensures that a
//!   worker's death does not lose the event.
//! - `delivered`: the sink said that the delivery happened.
//!
//! There is no "delivered" state for what was not delivered. An event whose
//! sink failed goes back to `pending` with its attempt counter and its last
//! error: **observable**, which is what makes it possible to reconcile it.
//!
//! # The claim is a compare-and-set
//!
//! Two workers ask for the same event and **one** wins. The one that arrives
//! second sees that there is nothing left to claim, and does not deliver: a
//! concurrent redelivery is not two deliveries.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::event::AuditEvent;
use crate::sink::AuditError;

/// The state an event is in within the outbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryState {
    /// Stated, waiting.
    Pending,
    /// Claimed by a worker, with a lease.
    Claimed,
    /// Delivered: the sink said so.
    Delivered,
}

impl EntryState {
    /// The name of the state, for the text and for the database.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Claimed => "claimed",
            Self::Delivered => "delivered",
        }
    }
}

/// An event in the outbox, with its state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    /// The event.
    pub event: AuditEvent,
    /// The state it is in.
    pub state: EntryState,
    /// How many delivery attempts it has had.
    pub attempts: u32,
    /// The worker that holds it, while the lease lasts.
    pub lease_owner: Option<String>,
    /// When the lease expires.
    pub lease_until: Option<DateTime<Utc>>,
    /// Why it was not delivered, the last time.
    pub last_error: Option<String>,
    /// When it was delivered.
    pub delivered_at: Option<DateTime<Utc>>,
}

impl AuditEntry {
    /// A pending entry, just stated.
    pub fn pending(event: AuditEvent) -> Self {
        Self {
            event,
            state: EntryState::Pending,
            attempts: 0,
            lease_owner: None,
            lease_until: None,
            last_error: None,
            delivered_at: None,
        }
    }
}

/// The audit outbox.
///
/// The port does not say "store the event": it says **what happens to it**. Whoever
/// implements it decides where it is stored; what cannot change is what
/// the port promises:
///
/// - `put_pending` states an event, and stating it twice is stating it once.
/// - `claim` is a compare-and-set: two workers, one wins.
/// - `mark_delivered` marks what the sink said it delivered.
/// - `mark_undelivered` returns the event to the market with its error and its
///   attempt counted.
/// - `pending` lets you **see** what is waiting.
///
/// # What the port promises, and what it does not
///
/// Exclusivity is given by `claim`: two workers ask for the same event and
/// one wins. But the outbox delivers **at least once**, not exactly
/// once. Between the sink receiving the event and the mark being written
/// there is a window in which the event **was already delivered** and still
/// looks pending: if the process dies there, it is delivered again.
///
/// For that reason `event_id` is the idempotency key and the `AuditSink` has
/// to treat it as such. **A logical effect is the sink's business**: what the
/// outbox guarantees is that redelivery carries **the same** `event_id`, so
/// that a conforming sink recognizes it and does not count it twice.
///
/// # Marks belong to the worker that holds the claim
///
/// The condition of the marks is `lease_owner`, not `lease_until`. That is:
/// whoever marks is **the owner of the claim**, not the owner of a live
/// lease. A worker whose lease expired and whom nobody has reclaimed yet
/// can still mark, and must be able to: the sink told it the delivery
/// happened, and lying about a fact that happened is worse than a
/// redundancy. As soon as **another** worker claims the event, the first one
/// can no longer write: the event is no longer its own.
#[async_trait]
pub trait AuditOutbox: Send + Sync {
    /// States the event: it enters as `pending`.
    ///
    /// Stating it twice is stating it once: an entry that is already delivered
    /// does not go back to pending, because the fact that the delivery happened
    /// is not undone by stating the event again.
    ///
    /// But if the identifier is already used by a **different** event,
    /// that is not the same fact stated twice: it is a contradiction, and
    /// a contradiction that is silenced is a fact that is lost. The port
    /// returns it as an error, so that it is visible.
    async fn put_pending(&self, event: &AuditEvent) -> Result<(), AuditError>;

    /// Claims the event: from `pending`, or from a lease that expired.
    ///
    /// Returns `None` when another worker took it first, or when the
    /// event is not in the outbox.
    async fn claim(
        &self,
        event_id: &str,
        worker: &str,
        lease_until: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<Option<AuditEntry>, AuditError>;

    /// Marks the event as delivered.
    ///
    /// Only from `claimed` **and only by the worker that holds the claim**:
    /// a mark on an entry that nobody claimed is a mark that nobody
    /// can explain, and a mark from a worker whose event was claimed by another
    /// is a mark on an event that is no longer its own.
    async fn mark_delivered(
        &self,
        event_id: &str,
        worker: &str,
        at: DateTime<Utc>,
    ) -> Result<(), AuditError>;

    /// Returns the event to the market: the delivery did not happen.
    ///
    /// The attempt is counted and the error is written: an event that is not
    /// delivered has to be **inspectable**. Only the worker that holds it
    /// can return it: if its lease expired and another worker claimed it, the
    /// event is no longer its own.
    async fn mark_undelivered(
        &self,
        event_id: &str,
        worker: &str,
        detail: &str,
    ) -> Result<(), AuditError>;

    /// The events that are waiting: the `pending` ones and the `claimed` ones whose
    /// lease expired.
    ///
    /// **The census is unbounded**: it brings every entry with its full `AuditEvent`.
    /// On a shared server that does not drain, the response
    /// grows until it overflows the transport's message limit (4 MiB
    /// in Dgraph; HSP-194 proved it). A production consumer
    /// (`AuditDispatcher::dispatch_pending`) that calls it against that
    /// server needs pagination or periodic draining (HSP-220 left the
    /// laboratory drain and the per-tenant census for the ITs; the
    /// pagination of the global census remains as a follow-up decision of the
    /// supervisor).
    async fn pending(&self, now: DateTime<Utc>) -> Result<Vec<AuditEntry>, AuditError>;

    /// The entry of an event, for whoever asks what became of it.
    async fn entry(&self, event_id: &str) -> Result<Option<AuditEntry>, AuditError>;
}

/// The in-memory outbox: the reference material of the port.
///
/// What the durable store keeps in SurrealDB, this keeps in the
/// process, with the **same** rules: the same compare-and-set, the same
/// lease, the same "stating it twice is stating it once".
#[derive(Debug, Clone, Default)]
pub struct InMemoryAuditOutbox {
    inner: Arc<Mutex<HashMap<String, AuditEntry>>>,
}

impl InMemoryAuditOutbox {
    /// An outbox with nothing in it.
    pub fn empty() -> Self {
        Self::default()
    }

    /// The states of all entries, ordered by identifier.
    pub fn states(&self) -> Vec<(String, String)> {
        let mut states: Vec<(String, String)> = self
            .inner
            .lock()
            .expect("the audit outbox is not poisoned")
            .iter()
            .map(|(event_id, entry)| (event_id.clone(), entry.state.as_str().to_string()))
            .collect();
        states.sort();
        states
    }
}

#[async_trait]
impl AuditOutbox for InMemoryAuditOutbox {
    async fn put_pending(&self, event: &AuditEvent) -> Result<(), AuditError> {
        let mut inner = self.inner.lock().expect("the audit outbox is not poisoned");
        // Stating it twice is stating it once: if it is already there and says the same, it is
        // left untouched. If it is already there and says something else, it is not the same fact:
        // it is a contradiction, and silencing it would lose a fact.
        match inner.get(&event.event_id) {
            Some(entry) if entry.event.same_fact(event) => Ok(()),
            Some(_) => Err(AuditError::StoreUnavailable {
                detail: format!(
                    "the event {} is already said with another content",
                    event.event_id
                ),
            }),
            None => {
                inner.insert(event.event_id.clone(), AuditEntry::pending(event.clone()));
                Ok(())
            }
        }
    }

    async fn claim(
        &self,
        event_id: &str,
        worker: &str,
        lease_until: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<Option<AuditEntry>, AuditError> {
        let mut inner = self.inner.lock().expect("the audit outbox is not poisoned");
        let Some(entry) = inner.get_mut(event_id) else {
            return Ok(None);
        };
        // The compare-and-set: pending, or a lease that died.
        let claimable = entry.state == EntryState::Pending
            || (entry.state == EntryState::Claimed
                && entry.lease_until.is_some_and(|until| until <= now));
        if !claimable {
            return Ok(None);
        }
        entry.state = EntryState::Claimed;
        entry.lease_owner = Some(worker.to_string());
        entry.lease_until = Some(lease_until);
        Ok(Some(entry.clone()))
    }

    async fn mark_delivered(
        &self,
        event_id: &str,
        worker: &str,
        at: DateTime<Utc>,
    ) -> Result<(), AuditError> {
        let mut inner = self.inner.lock().expect("the audit outbox is not poisoned");
        let Some(entry) = inner.get_mut(event_id) else {
            return Err(AuditError::StoreUnavailable {
                detail: format!("the event {event_id} is not in the outbox"),
            });
        };
        // What is already true is not changed: a delivery that happened is not
        // undone by marking it again.
        if entry.state == EntryState::Delivered {
            return Ok(());
        }
        if entry.state != EntryState::Claimed {
            return Err(AuditError::StoreUnavailable {
                detail: format!("the event {event_id} is not claimed, so nobody delivered it"),
            });
        }
        // And only the owner of the claim: as soon as another worker claimed the
        // event, this one is no longer its own.
        if entry.lease_owner.as_deref() != Some(worker) {
            return Err(AuditError::StoreUnavailable {
                detail: format!(
                    "the event {event_id} is claimed by another worker, so {worker} did not deliver it"
                ),
            });
        }
        entry.state = EntryState::Delivered;
        entry.lease_owner = None;
        entry.lease_until = None;
        entry.delivered_at = Some(at);
        Ok(())
    }

    async fn mark_undelivered(
        &self,
        event_id: &str,
        worker: &str,
        detail: &str,
    ) -> Result<(), AuditError> {
        let mut inner = self.inner.lock().expect("the audit outbox is not poisoned");
        let Some(entry) = inner.get_mut(event_id) else {
            return Err(AuditError::StoreUnavailable {
                detail: format!("the event {event_id} is not in the outbox"),
            });
        };
        // A delivery that already happened cannot be contradicted: marking it as
        // not delivered would be lying about a fact.
        if entry.state == EntryState::Delivered {
            return Ok(());
        }
        if entry.state != EntryState::Claimed {
            return Err(AuditError::StoreUnavailable {
                detail: format!(
                    "the event {event_id} was not claimed, so nobody can say it failed"
                ),
            });
        }
        // And only the owner of the claim: if its lease expired and another worker
        // claimed the event, returning it to the market would be pulling it out from under
        // the worker that holds it.
        if entry.lease_owner.as_deref() != Some(worker) {
            return Err(AuditError::StoreUnavailable {
                detail: format!(
                    "the event {event_id} is claimed by another worker, so {worker} cannot return it"
                ),
            });
        }
        entry.state = EntryState::Pending;
        entry.lease_owner = None;
        entry.lease_until = None;
        entry.attempts = entry.attempts.saturating_add(1);
        entry.last_error = Some(detail.to_string());
        Ok(())
    }

    async fn pending(&self, now: DateTime<Utc>) -> Result<Vec<AuditEntry>, AuditError> {
        let inner = self.inner.lock().expect("the audit outbox is not poisoned");
        let mut entries: Vec<AuditEntry> = inner
            .values()
            .filter(|entry| {
                entry.state == EntryState::Pending
                    || (entry.state == EntryState::Claimed
                        && entry.lease_until.is_some_and(|until| until <= now))
            })
            .cloned()
            .collect();
        entries.sort_by(|left, right| left.event.event_id.cmp(&right.event.event_id));
        Ok(entries)
    }

    async fn entry(&self, event_id: &str) -> Result<Option<AuditEntry>, AuditError> {
        let inner = self.inner.lock().expect("the audit outbox is not poisoned");
        Ok(inner.get(event_id).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{AuditContext, AuditResult};

    fn the_event(event_id: &str) -> AuditEvent {
        AuditEvent::new(
            event_id.to_string(),
            AuditContext::new("hrn:hodei:iam:acme:User/alice", "acme", "corr-1"),
            DateTime::from_timestamp(1_700_000_000, 0).expect("the moment"),
            "publish",
            "raw/app/one.bin",
            Some("sha256:00".to_string()),
            AuditResult::Allowed,
        )
    }

    fn the_moment() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).expect("the moment")
    }

    #[tokio::test]
    async fn saying_an_event_twice_is_saying_it_once() {
        let outbox = InMemoryAuditOutbox::empty();
        outbox.put_pending(&the_event("e-1")).await.expect("put");
        outbox.put_pending(&the_event("e-1")).await.expect("put");
        assert_eq!(
            outbox.states(),
            vec![("e-1".to_string(), "pending".to_string())]
        );
    }

    #[tokio::test]
    async fn the_claim_is_a_compare_and_set() {
        let outbox = InMemoryAuditOutbox::empty();
        outbox.put_pending(&the_event("e-1")).await.expect("put");
        let until = the_moment() + chrono::Duration::minutes(1);

        let first = outbox
            .claim("e-1", "worker-1", until, the_moment())
            .await
            .expect("claim");
        let second = outbox
            .claim("e-1", "worker-2", until, the_moment())
            .await
            .expect("claim");

        assert!(first.is_some(), "the first worker takes the event");
        assert!(second.is_none(), "the second worker does not");
    }

    #[tokio::test]
    async fn an_expired_lease_can_be_claimed_again() {
        let outbox = InMemoryAuditOutbox::empty();
        outbox.put_pending(&the_event("e-1")).await.expect("put");
        let until = the_moment() + chrono::Duration::minutes(1);
        outbox
            .claim("e-1", "worker-1", until, the_moment())
            .await
            .expect("claim");

        let later = the_moment() + chrono::Duration::minutes(2);
        let reclaimed = outbox
            .claim(
                "e-1",
                "worker-2",
                later + chrono::Duration::minutes(1),
                later,
            )
            .await
            .expect("claim");

        assert!(
            reclaimed.is_some(),
            "a dead worker's event goes back to the market"
        );
    }

    #[tokio::test]
    async fn an_event_that_nobody_claimed_cannot_be_marked_as_delivered() {
        let outbox = InMemoryAuditOutbox::empty();
        outbox.put_pending(&the_event("e-1")).await.expect("put");
        assert!(
            outbox
                .mark_delivered("e-1", "worker-1", the_moment())
                .await
                .is_err(),
            "a delivery that nobody claimed cannot be marked"
        );
    }

    #[tokio::test]
    async fn a_worker_whose_lease_expired_cannot_mark_the_event() {
        let outbox = InMemoryAuditOutbox::empty();
        outbox.put_pending(&the_event("e-1")).await.expect("put");
        let until = the_moment() + chrono::Duration::minutes(1);
        outbox
            .claim("e-1", "worker-1", until, the_moment())
            .await
            .expect("claim");

        // The lease dies and another worker takes it: the first one can no longer
        // say anything about it.
        let later = the_moment() + chrono::Duration::minutes(2);
        outbox
            .claim(
                "e-1",
                "worker-2",
                later + chrono::Duration::minutes(1),
                later,
            )
            .await
            .expect("claim");

        assert!(
            outbox
                .mark_delivered("e-1", "worker-1", later)
                .await
                .is_err(),
            "the worker that lost the lease cannot mark a delivery"
        );
        assert!(
            outbox
                .mark_undelivered("e-1", "worker-1", "the sink is down")
                .await
                .is_err(),
            "the worker that lost the lease cannot return the event"
        );
    }

    #[tokio::test]
    async fn an_event_that_was_not_delivered_goes_back_to_pending_with_its_error() {
        let outbox = InMemoryAuditOutbox::empty();
        outbox.put_pending(&the_event("e-1")).await.expect("put");
        let until = the_moment() + chrono::Duration::minutes(1);
        outbox
            .claim("e-1", "worker-1", until, the_moment())
            .await
            .expect("claim");
        outbox
            .mark_undelivered("e-1", "worker-1", "the sink is down")
            .await
            .expect("mark");

        let entry = outbox.entry("e-1").await.expect("entry").expect("entry");
        assert_eq!(entry.state, EntryState::Pending);
        assert_eq!(entry.attempts, 1);
        assert_eq!(entry.last_error.as_deref(), Some("the sink is down"));
        assert!(outbox.pending(the_moment()).await.expect("pending").len() == 1);
    }

    #[tokio::test]
    async fn a_delivered_event_is_not_pending_anymore() {
        let outbox = InMemoryAuditOutbox::empty();
        outbox.put_pending(&the_event("e-1")).await.expect("put");
        let until = the_moment() + chrono::Duration::minutes(1);
        outbox
            .claim("e-1", "worker-1", until, the_moment())
            .await
            .expect("claim");
        outbox
            .mark_delivered("e-1", "worker-1", the_moment())
            .await
            .expect("mark");

        assert!(
            outbox
                .pending(the_moment())
                .await
                .expect("pending")
                .is_empty()
        );
        let entry = outbox.entry("e-1").await.expect("entry").expect("entry");
        assert_eq!(entry.state, EntryState::Delivered);
        assert!(entry.delivered_at.is_some());
    }
}
