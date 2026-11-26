//! The write and replay counters of the event ledger (HSP-48 AC5).
//!
//! Real writes go through a [`MeteredLedger`] over an in-memory ledger that
//! enforces the same contract as the durable one: admission refuses an event
//! without a stream, and the optimistic check refuses a stale expectation.
//! The counters must move by exactly the amount each call implies.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use hodei_evidence::{
    EventEnvelope, EventId, EventLedger, EventPage, EventType, ExpectedVersion, LedgerCursor,
    LedgerError, LedgerMetricsSnapshot, MeteredLedger, Producer,
};
use kernel::Hrn;

#[derive(Default)]
struct MemoryLedger {
    streams: Mutex<HashMap<String, Vec<EventEnvelope>>>,
}

#[async_trait]
impl EventLedger for MemoryLedger {
    async fn append(
        &self,
        expected: ExpectedVersion,
        event: EventEnvelope,
    ) -> Result<EventId, LedgerError> {
        if event.stream_id().is_empty() {
            return Err(LedgerError::InvalidEvent("the stream id is empty".into()));
        }
        let mut streams = self.streams.lock().expect("lock");
        let stream = streams.entry(event.stream_id().to_string()).or_default();
        let current = stream.len().checked_sub(1).map(|last| last as u64);
        if !expected.accepts(current) {
            return Err(LedgerError::VersionConflict {
                expected: format!("{expected:?}"),
                actual: current,
            });
        }
        let id = event.id().clone();
        stream.push(event);
        Ok(id)
    }

    async fn read(&self, cursor: LedgerCursor, limit: usize) -> Result<EventPage, LedgerError> {
        if cursor.stream_id == "broken" {
            return Err(LedgerError::Unavailable("down".into()));
        }
        let streams = self.streams.lock().expect("lock");
        let all = streams.get(&cursor.stream_id).cloned().unwrap_or_default();
        let from = cursor.next_position as usize;
        let events: Vec<_> = all.iter().skip(from).take(limit).cloned().collect();
        let next = from + events.len();
        let next_cursor =
            (next < all.len()).then(|| LedgerCursor::from_position(cursor.stream_id, next as u64));
        Ok(EventPage {
            events,
            next_cursor,
        })
    }

    async fn position(&self, stream_id: &str) -> Result<Option<u64>, LedgerError> {
        let streams = self.streams.lock().expect("lock");
        Ok(streams
            .get(stream_id)
            .and_then(|events| events.len().checked_sub(1))
            .map(|last| last as u64))
    }
}

fn event(id: &str, stream: &str, position: u64) -> EventEnvelope {
    let tenant =
        Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/web-1.0.0").expect("an HRN");
    let moment = Utc.timestamp_opt(1_767_322_800, 0).single().expect("time");
    EventEnvelope::builder(EventId::new(id), tenant, EventType::EvidenceAccepted)
        .stream(stream, position)
        .occurred_at(moment)
        .recorded_at(moment)
        .producer(Producer::new("connector", "pipelinek"))
        .payload(serde_json::json!({ "evidence_id": id }))
        .build()
}

#[tokio::test]
async fn writes_move_each_counter_by_exactly_one() {
    let ledger = MeteredLedger::new(MemoryLedger::default());
    let metrics = ledger.metrics();
    assert_eq!(metrics.snapshot(), LedgerMetricsSnapshot::default());

    // Two accepted writes.
    ledger
        .append(ExpectedVersion::Absent, event("e0", "s1", 0))
        .await
        .expect("accepted");
    ledger
        .append(ExpectedVersion::At(0), event("e1", "s1", 1))
        .await
        .expect("accepted");
    // A stale writer: the stream is at 1 and it still believes 0.
    let conflict = ledger
        .append(ExpectedVersion::At(0), event("e2", "s1", 1))
        .await;
    assert!(matches!(conflict, Err(LedgerError::VersionConflict { .. })));
    // An event the admission refuses.
    let refused = ledger
        .append(ExpectedVersion::Absent, event("e3", "", 0))
        .await;
    assert!(matches!(refused, Err(LedgerError::InvalidEvent(_))));

    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.appends_accepted, 2);
    assert_eq!(snapshot.version_conflicts, 1);
    assert_eq!(snapshot.appends_rejected_admission, 1);
    assert_eq!(snapshot.appends_failed, 0);
    assert_eq!(snapshot.replay_reads, 0);
}

#[tokio::test]
async fn replay_counts_events_pages_failures_and_duration() {
    let ledger = MeteredLedger::new(MemoryLedger::default());
    let metrics = ledger.metrics();
    ledger
        .append(ExpectedVersion::Absent, event("e0", "s1", 0))
        .await
        .expect("accepted");
    ledger
        .append(ExpectedVersion::At(0), event("e1", "s1", 1))
        .await
        .expect("accepted");
    ledger
        .append(ExpectedVersion::At(1), event("e2", "s1", 2))
        .await
        .expect("accepted");

    // Replay the stream by cursor in pages of two: 2 pages, 3 events.
    let mut cursor = LedgerCursor::start_of("s1");
    loop {
        let page = ledger.read(cursor, 2).await.expect("page");
        match page.next_cursor {
            Some(next) => cursor = next,
            None => break,
        }
    }
    assert!(
        ledger
            .read(LedgerCursor::start_of("broken"), 2)
            .await
            .is_err()
    );

    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.replay_reads, 2);
    assert_eq!(snapshot.events_replayed, 3);
    assert_eq!(snapshot.replay_read_failures, 1);
    assert!(snapshot.replay_nanos > 0, "the duration accumulates");
    // The reads did not touch the write counters.
    assert_eq!(snapshot.appends_accepted, 3);
}
