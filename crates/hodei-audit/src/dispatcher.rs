//! The dispatcher: whoever carries the event from the outbox to the sink.
//!
//! The dispatcher is the only one that touches the sink, and it does three
//! things in an order that cannot be changed:
//!
//! 1. **Claims** the event (compare-and-set: if another worker took it,
//!    it is not delivered).
//! 2. **Delivers** to the sink.
//! 3. **Marks** what was delivered, **only if the sink said so**.
//!
//! # What the dispatcher never says
//!
//! It never says "delivered" for a delivery that did not happen. The sink can
//! fail, and when it fails the event goes back to the market with its error and
//! its attempt counted. And there is a third case, the least comfortable: the sink said
//! yes, but the mark could not be written. There the delivery **happened** and
//! the outbox **does not know it**: that is not "delivered", it is "uncertain", and it is
//! said as such. Reconciliation resolves it: the `event_id` makes
//! the redelivery the same event.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::outbox::AuditOutbox;
use crate::sink::{AuditError, AuditSink};

/// What happened to an event in a delivery round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchOutcome {
    /// The sink said so and the mark was written.
    Delivered,
    /// The sink did not say so: the event is pending again.
    NotDelivered {
        /// Why.
        detail: String,
    },
    /// The sink said so, but the mark could not be written: nobody knows whether
    /// the outbox knows it.
    Uncertain {
        /// What failed when marking.
        detail: String,
    },
}

/// What happened in a delivery round.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DispatchReport {
    /// The events that ended up marked as delivered.
    pub delivered: Vec<String>,
    /// The events that could not be delivered, with the reason.
    pub not_delivered: Vec<(String, String)>,
    /// The events whose delivery happened and whose mark could not be written.
    pub uncertain: Vec<(String, String)>,
}

impl DispatchReport {
    /// How many events were delivered.
    pub fn delivered(&self) -> usize {
        self.delivered.len()
    }

    /// Whether nothing happened in the round.
    pub fn is_empty(&self) -> bool {
        self.delivered.is_empty() && self.not_delivered.is_empty() && self.uncertain.is_empty()
    }
}

/// Whoever carries the events from the outbox to the sink.
pub struct AuditDispatcher {
    outbox: Arc<dyn AuditOutbox>,
    sink: Arc<dyn AuditSink>,
    lease: Duration,
}

impl AuditDispatcher {
    /// A dispatcher over an outbox and a sink.
    pub fn new(outbox: Arc<dyn AuditOutbox>, sink: Arc<dyn AuditSink>) -> Self {
        Self {
            outbox,
            sink,
            lease: Duration::from_secs(30),
        }
    }

    /// The dispatcher with another lease.
    ///
    /// The lease is how long a worker may hold an event without
    /// delivering it: when it expires, the event goes back to the market and another
    /// worker can take it. A short lease keeps a dead worker from
    /// holding the event for long; a long one keeps an event from being
    /// delivered twice along the way.
    pub fn with_lease(mut self, lease: Duration) -> Self {
        self.lease = lease;
        self
    }

    /// The lease the dispatcher uses.
    pub fn lease(&self) -> Duration {
        self.lease
    }

    /// Delivers what is waiting.
    ///
    /// Makes a full pass: claims each pending event, delivers it and
    /// marks it. An event that another worker holds at this moment is not
    /// delivered, and that is not a failure: it is the compare-and-set doing its
    /// job.
    pub async fn dispatch_pending(
        &self,
        worker: &str,
        now: DateTime<Utc>,
    ) -> Result<DispatchReport, AuditError> {
        let mut report = DispatchReport::default();
        let waiting = self.outbox.pending(now).await?;
        let lease_until = now
            + chrono::Duration::from_std(self.lease)
                .unwrap_or_else(|_| chrono::Duration::seconds(30));

        for entry in waiting {
            let Some(claimed) = self
                .outbox
                .claim(&entry.event.event_id, worker, lease_until, now)
                .await?
            else {
                // Another worker took it: it is not delivered twice.
                continue;
            };

            match self.sink.record(&claimed.event) {
                Ok(()) => match self
                    .outbox
                    .mark_delivered(&claimed.event.event_id, worker, now)
                    .await
                {
                    Ok(()) => report.delivered.push(claimed.event.event_id.clone()),
                    Err(error) => report
                        .uncertain
                        .push((claimed.event.event_id.clone(), error.to_string())),
                },
                Err(error) => {
                    let mut detail = error.to_string();
                    // The delivery did NOT happen: the event goes back to the market with
                    // its error and its attempt counted. Marking it as delivered
                    // would be lying about a fact.
                    if let Err(mark_error) = self
                        .outbox
                        .mark_undelivered(&claimed.event.event_id, worker, &detail)
                        .await
                    {
                        // It could not even be returned to the market: the event
                        // stays claimed until its lease expires. This is
                        // said, because a stuck event is a fact that
                        // has to be possible to look at.
                        detail = format!(
                            "{detail} (and it could not be returned to the market: {mark_error})"
                        );
                    }
                    report
                        .not_delivered
                        .push((claimed.event.event_id.clone(), detail));
                }
            }
        }

        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{AuditContext, AuditResult};
    use crate::outbox::InMemoryAuditOutbox;
    use crate::sink::{FailingAuditSink, InMemoryAuditSink};

    fn the_event(event_id: &str) -> crate::event::AuditEvent {
        crate::event::AuditEvent::new(
            event_id.to_string(),
            AuditContext::new("hrn:hodei:iam:acme:User/alice", "acme", "corr-1"),
            chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("the moment"),
            "publish",
            "raw/app/one.bin",
            Some("sha256:00".to_string()),
            AuditResult::Allowed,
        )
    }

    fn the_moment() -> chrono::DateTime<Utc> {
        chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("the moment")
    }

    #[tokio::test]
    async fn the_dispatcher_delivers_the_pending_and_marks_it_delivered() {
        let outbox = Arc::new(InMemoryAuditOutbox::empty());
        outbox.put_pending(&the_event("e-1")).await.expect("put");
        let sink = Arc::new(InMemoryAuditSink::new());
        let dispatcher = AuditDispatcher::new(outbox.clone(), sink.clone());

        let report = dispatcher
            .dispatch_pending("worker-1", the_moment())
            .await
            .expect("dispatch");

        assert_eq!(report.delivered(), 1);
        assert!(report.not_delivered.is_empty());
        assert_eq!(sink.len(), 1);
        let entry = outbox.entry("e-1").await.expect("entry").expect("entry");
        assert_eq!(entry.state, crate::outbox::EntryState::Delivered);
    }

    #[tokio::test]
    async fn a_sink_that_fails_leaves_the_event_pending_and_visible() {
        let outbox = Arc::new(InMemoryAuditOutbox::empty());
        outbox.put_pending(&the_event("e-1")).await.expect("put");
        let sink = Arc::new(FailingAuditSink::new());
        let dispatcher = AuditDispatcher::new(outbox.clone(), sink.clone());

        let report = dispatcher
            .dispatch_pending("worker-1", the_moment())
            .await
            .expect("dispatch");

        assert_eq!(report.delivered(), 0);
        assert_eq!(report.not_delivered.len(), 1);
        let entry = outbox.entry("e-1").await.expect("entry").expect("entry");
        assert_eq!(entry.state, crate::outbox::EntryState::Pending);
        assert_eq!(entry.attempts, 1);
        assert!(entry.last_error.is_some());
    }

    #[tokio::test]
    async fn a_redelivery_is_one_logical_effect() {
        let outbox = Arc::new(InMemoryAuditOutbox::empty());
        outbox.put_pending(&the_event("e-1")).await.expect("put");
        let sink = Arc::new(InMemoryAuditSink::new());
        let dispatcher = AuditDispatcher::new(outbox.clone(), sink.clone());

        // The failure: the sink received the event, but the mark was not written.
        let claimed = outbox
            .claim(
                "e-1",
                "worker-1",
                the_moment() + chrono::Duration::minutes(1),
                the_moment(),
            )
            .await
            .expect("claim")
            .expect("the claim");
        sink.record(&claimed.event).expect("the sink received it");

        // The lease expires and the event goes back to the market.
        let later = the_moment() + chrono::Duration::minutes(10);
        let report = dispatcher
            .dispatch_pending("worker-2", later)
            .await
            .expect("dispatch");

        assert_eq!(report.delivered(), 1);
        assert_eq!(
            sink.len(),
            1,
            "the same event delivered twice is one logical effect"
        );
    }
}
