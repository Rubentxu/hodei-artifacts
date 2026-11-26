//! Observable counters of the durable event ledger: writes and replay.
//!
//! The shape follows `ProxyMetrics` of the root crate: atomic, unlabeled
//! counters and a [`LedgerMetricsSnapshot`] reading. Unlabeled on purpose:
//! no stream id, tenant, producer or payload ever reaches a metric, so there
//! is no high-cardinality label and no secret value to leak. The trace
//! (`tracing`) carries the detail.
//!
//! # What is NOT here yet
//!
//! There is no exposition outside the process: the project has no `/metrics`
//! endpoint, no recorder and no exporter. Today the counters are observable
//! in-process through [`LedgerMetrics::snapshot`] only. Exposing them is
//! follow-up work that belongs to whoever introduces the exposition path.
//!
//! # How to use
//!
//! Wrap any [`EventLedger`] in a [`MeteredLedger`]; every append and every
//! read by cursor that goes through the wrapper is counted. Keep the
//! `Arc<LedgerMetrics>` to take snapshots.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use async_trait::async_trait;

use crate::domain::event::{EventEnvelope, EventId};
use crate::ports::{EventLedger, EventPage, ExpectedVersion, LedgerCursor, LedgerError};

/// Counters of the ledger. Atomic and unlabeled.
#[derive(Debug, Default)]
pub struct LedgerMetrics {
    appends_accepted: AtomicU64,
    appends_rejected_admission: AtomicU64,
    version_conflicts: AtomicU64,
    appends_failed: AtomicU64,
    replay_reads: AtomicU64,
    replay_read_failures: AtomicU64,
    events_replayed: AtomicU64,
    replay_nanos: AtomicU64,
}

/// A reading of the ledger counters at an instant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LedgerMetricsSnapshot {
    /// Appends the ledger accepted (a redelivery that returns its committed
    /// identity counts as accepted too).
    pub appends_accepted: u64,
    /// Appends refused at admission: the event cannot be recorded as it is.
    pub appends_rejected_admission: u64,
    /// Appends refused because the stream moved (optimistic concurrency).
    pub version_conflicts: u64,
    /// Appends that failed for any other reason (store unavailable, duplicate).
    pub appends_failed: u64,
    /// Reads by cursor that returned a page.
    pub replay_reads: u64,
    /// Reads by cursor that failed.
    pub replay_read_failures: u64,
    /// Events returned by the reads by cursor.
    pub events_replayed: u64,
    /// Accumulated wall time of the reads by cursor, in nanoseconds.
    pub replay_nanos: u64,
}

impl LedgerMetrics {
    /// A reading of all the counters.
    pub fn snapshot(&self) -> LedgerMetricsSnapshot {
        let read = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        LedgerMetricsSnapshot {
            appends_accepted: read(&self.appends_accepted),
            appends_rejected_admission: read(&self.appends_rejected_admission),
            version_conflicts: read(&self.version_conflicts),
            appends_failed: read(&self.appends_failed),
            replay_reads: read(&self.replay_reads),
            replay_read_failures: read(&self.replay_read_failures),
            events_replayed: read(&self.events_replayed),
            replay_nanos: read(&self.replay_nanos),
        }
    }

    fn record_append(&self, outcome: &Result<EventId, LedgerError>) {
        let counter = match outcome {
            Ok(_) => &self.appends_accepted,
            Err(LedgerError::InvalidEvent(_)) => &self.appends_rejected_admission,
            Err(LedgerError::VersionConflict { .. }) => &self.version_conflicts,
            Err(LedgerError::Duplicate | LedgerError::Unavailable(_)) => &self.appends_failed,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    fn record_read(&self, outcome: &Result<EventPage, LedgerError>, started: Instant) {
        let nanos = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        self.replay_nanos.fetch_add(nanos, Ordering::Relaxed);
        match outcome {
            Ok(page) => {
                self.replay_reads.fetch_add(1, Ordering::Relaxed);
                self.events_replayed
                    .fetch_add(page.events.len() as u64, Ordering::Relaxed);
            }
            Err(_) => {
                self.replay_read_failures.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// An [`EventLedger`] that counts what goes through another one.
///
/// It changes no behavior: every call is delegated and its result returned
/// untouched.
#[derive(Debug)]
pub struct MeteredLedger<L> {
    inner: L,
    metrics: Arc<LedgerMetrics>,
}

impl<L> MeteredLedger<L> {
    /// Wraps a ledger with fresh counters.
    pub fn new(inner: L) -> Self {
        Self::with_metrics(inner, Arc::new(LedgerMetrics::default()))
    }

    /// Wraps a ledger with counters the caller already holds.
    pub fn with_metrics(inner: L, metrics: Arc<LedgerMetrics>) -> Self {
        Self { inner, metrics }
    }

    /// The counters, shared.
    pub fn metrics(&self) -> Arc<LedgerMetrics> {
        Arc::clone(&self.metrics)
    }
}

#[async_trait]
impl<L: EventLedger> EventLedger for MeteredLedger<L> {
    async fn append(
        &self,
        expected: ExpectedVersion,
        event: EventEnvelope,
    ) -> Result<EventId, LedgerError> {
        let outcome = self.inner.append(expected, event).await;
        self.metrics.record_append(&outcome);
        outcome
    }

    async fn read(&self, cursor: LedgerCursor, limit: usize) -> Result<EventPage, LedgerError> {
        let started = Instant::now();
        let outcome = self.inner.read(cursor, limit).await;
        self.metrics.record_read(&outcome, started);
        outcome
    }

    async fn position(&self, stream_id: &str) -> Result<Option<u64>, LedgerError> {
        self.inner.position(stream_id).await
    }
}
