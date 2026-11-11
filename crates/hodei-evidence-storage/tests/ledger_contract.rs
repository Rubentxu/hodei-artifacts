//! The ledger of events, over the real store.
//!
//! These are the guarantees of the ledger, proven against the engine the
//! deployment runs -- not against a map in memory.
//!
//! # Guarantees
//!
//! - L1 An event that is written survives the closing and reopening of the
//!   database. The history is durable.
//! - L2 The optimistic version check is real: a writer that expected a
//!   position that is not the current one is refused, and nothing is
//!   written.
//! - L3 The same delivery arriving twice writes once. The second one returns
//!   the identity of the event that is already there.
//! - L4 The cursor neither skips nor repeats: reading a stream page by page
//!   yields every event exactly once, in order.
//! - L5 The conflict and the repetition are told apart: two different events
//!   at the same position are a conflict, not a repetition.
//! - L6 The stream is not read from a table that the service cannot reopen:
//!   the adapter declares no index, because defining one on the local engine
//!   keeps the database directory locked by the process that defines it.

use chrono::{DateTime, TimeZone, Utc};
use hodei_evidence::{
    EventEnvelope, EventId, EventLedger, EventType, ExpectedVersion, IdempotencyKey, LedgerCursor,
    LedgerError, Producer,
};
use hodei_evidence_storage::SurrealEventLedger;
use kernel::Hrn;
use std::sync::Arc;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

fn account() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/web-1.0.0").expect("an HRN")
}

fn moment(second: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(1_767_322_800 + second, 0)
        .single()
        .expect("a moment")
}

fn event(id: &str, stream: &str, position: u64) -> EventEnvelope {
    EventEnvelope::builder(EventId::new(id), account(), EventType::EvidenceAccepted)
        .stream(stream, position)
        .occurred_at(moment(10))
        .recorded_at(moment(20))
        .producer(Producer::new("connector", "pipelinek"))
        .idempotency_key(IdempotencyKey::derive(
            &Producer::new("connector", "pipelinek"),
            &hodei_evidence::SourceRef::by_external_id(format!("run-{id}")),
            &hodei_evidence::ContentDigest::sha256("a".repeat(64)),
            "1",
        ))
        .payload(serde_json::json!({"evidence_id": id}))
        .build()
}

/// Opens the database on a throwaway directory.
async fn open(path: &std::path::Path) -> Surreal<Db> {
    let db = Surreal::new::<RocksDb>(path.to_string_lossy().to_string())
        .await
        .expect("the database opens");
    db.use_ns("hodei")
        .use_db("evidence")
        .await
        .expect("the namespace");
    db
}

fn ledger(db: &Arc<Surreal<Db>>) -> SurrealEventLedger<Db> {
    SurrealEventLedger::new(db.clone())
}

/// What the store says about a table.
#[derive(Debug, serde::Deserialize)]
struct TableInfo {
    /// The indexes of the table.
    #[serde(default)]
    indexes: std::collections::BTreeMap<String, serde_json::Value>,
}

/// The name of the environment variable that turns this test into the child
/// that writes the event.
const CHILD_MARKER: &str = "HODEI_LEDGER_CHILD_WRITES";

/// L1 An event that is written survives the closing and reopening of the
/// database.
///
/// The write happens in another process, because that is what a restart is:
/// the process that wrote the event is gone, and the event is still there.
/// The engine keeps the directory locked by the process that opened it, so a
/// write and a read in the same process would prove nothing about
/// durability.
#[tokio::test]
async fn l1_an_event_survives_the_reopening_of_the_database() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("db");

    // This process is the one that writes, when it was started as the child.
    if std::env::var(CHILD_MARKER).is_ok() {
        let path = std::path::PathBuf::from(
            std::env::var("HODEI_LEDGER_CHILD_PATH").expect("the child knows the path"),
        );
        let db = Arc::new(open(&path).await);
        let ledger = ledger(&db);
        ledger
            .append(ExpectedVersion::Absent, event("evt-1", "stream-1", 0))
            .await
            .expect("the event is written");
        return;
    }

    // The child writes the event and exits, which releases the lock.
    let status = std::process::Command::new(std::env::current_exe().expect("the test binary"))
        .args([
            "--exact",
            "l1_an_event_survives_the_reopening_of_the_database",
        ])
        .env(CHILD_MARKER, "1")
        .env("HODEI_LEDGER_CHILD_PATH", &path)
        .status()
        .expect("the child runs");
    assert!(status.success(), "the child wrote the event");

    // The database is opened again: a different process, the same files.
    let db = Arc::new(open(&path).await);
    let ledger = ledger(&db);
    let page = ledger
        .read(LedgerCursor::start_of("stream-1"), 10)
        .await
        .expect("the stream is read");

    assert_eq!(page.events.len(), 1, "the event survived: {page:?}");
    assert_eq!(page.events[0].id().as_str(), "evt-1");
    assert_eq!(page.events[0].stream_position(), 0);
}

/// L2 The optimistic version check is real.
#[tokio::test]
async fn l2_the_optimistic_version_check_is_real() {
    let tmp = tempfile::tempdir().unwrap();
    let db = Arc::new(open(&tmp.path().join("db")).await);
    let ledger = ledger(&db);

    ledger
        .append(ExpectedVersion::Absent, event("evt-1", "stream-1", 0))
        .await
        .expect("the first event is written");

    // The writer expected the stream to be empty. It is not: the position
    // it names is taken.
    let outcome = ledger
        .append(ExpectedVersion::Absent, event("evt-2", "stream-1", 0))
        .await;
    assert!(
        matches!(outcome, Err(LedgerError::VersionConflict { .. })),
        "the writer has to be told that the stream moved: {outcome:?}"
    );

    // And the stream still holds only the first event: the refused write
    // wrote nothing.
    let page = ledger
        .read(LedgerCursor::start_of("stream-1"), 10)
        .await
        .expect("the stream is read");
    assert_eq!(page.events.len(), 1, "the refused write wrote nothing");
}

/// L3 The same delivery arriving twice writes once.
#[tokio::test]
async fn l3_the_same_delivery_arriving_twice_writes_once() {
    let tmp = tempfile::tempdir().unwrap();
    let db = Arc::new(open(&tmp.path().join("db")).await);
    let ledger = ledger(&db);

    let first = ledger
        .append(ExpectedVersion::Absent, event("evt-1", "stream-1", 0))
        .await
        .expect("the first delivery is written");

    let second = ledger
        .append(ExpectedVersion::Absent, event("evt-1", "stream-1", 0))
        .await
        .expect("the repeated delivery is the same delivery");

    assert_eq!(first, second, "the repeated delivery is the same event");

    let page = ledger
        .read(LedgerCursor::start_of("stream-1"), 10)
        .await
        .expect("the stream is read");
    assert_eq!(page.events.len(), 1, "the event was written once");
}

/// L5 Two writers that want the same position: one wins, the other is told.
///
/// This is the race the optimistic check exists for. Both writers believe the
/// stream is where they saw it, both write at the same position, and the
/// store only lets one of them have it. The other is told that the stream
/// moved, which is not a failure to retry blindly: it has to look again.
#[tokio::test]
async fn l5_two_writers_that_want_the_same_position_are_told() {
    let tmp = tempfile::tempdir().unwrap();
    let db = Arc::new(open(&tmp.path().join("db")).await);
    let ledger = ledger(&db);

    let first = ledger.append(ExpectedVersion::Absent, event("evt-1", "stream-1", 0));
    let second = ledger.append(ExpectedVersion::Absent, event("evt-2", "stream-1", 0));
    let (first, second) = tokio::join!(first, second);

    let outcomes = [first, second];
    let written = outcomes.iter().filter(|outcome| outcome.is_ok()).count();
    let refused = outcomes
        .iter()
        .filter(|outcome| matches!(outcome, Err(LedgerError::VersionConflict { .. })))
        .count();

    assert_eq!(written, 1, "one writer wins: {outcomes:?}");
    assert_eq!(
        refused, 1,
        "the other is told that the stream moved: {outcomes:?}"
    );

    // And the stream holds exactly one event.
    let page = ledger
        .read(LedgerCursor::start_of("stream-1"), 10)
        .await
        .expect("the stream is read");
    assert_eq!(page.events.len(), 1, "only one event was written");
}

/// L4 The cursor neither skips nor repeats.
#[tokio::test]
async fn l4_the_cursor_neither_skips_nor_repeats() {
    let tmp = tempfile::tempdir().unwrap();
    let db = Arc::new(open(&tmp.path().join("db")).await);
    let ledger = ledger(&db);

    for position in 0..7 {
        ledger
            .append(
                if position == 0 {
                    ExpectedVersion::Absent
                } else {
                    ExpectedVersion::At(position - 1)
                },
                event(&format!("evt-{position}"), "stream-1", position),
            )
            .await
            .unwrap_or_else(|error| panic!("the event {position} is written: {error:?}"));
    }

    let mut seen = Vec::new();
    let mut cursor = LedgerCursor::start_of("stream-1");
    loop {
        let page = ledger
            .read(cursor.clone(), 3)
            .await
            .expect("the stream is read");
        for event in &page.events {
            seen.push(event.id().as_str().to_string());
        }
        match page.next_cursor {
            Some(next) => cursor = next,
            None => break,
        }
    }

    assert_eq!(
        seen,
        (0..7)
            .map(|position| format!("evt-{position}"))
            .collect::<Vec<_>>(),
        "every event is read once, in order"
    );
}

/// L6 The adapter declares no index.
///
/// Defining an index on the local engine keeps the database directory locked
/// by the process that defines it, and the same process can never reopen the
/// database afterwards. A service that declared an index on its own path
/// could not restart. The identifier of a record is the position, so the
/// uniqueness does not need an index.
///
/// The proof is direct: the table of the ledger has no index at all.
#[tokio::test]
async fn l6_the_table_of_the_ledger_has_no_index() {
    let tmp = tempfile::tempdir().unwrap();
    let db = Arc::new(open(&tmp.path().join("db")).await);
    let ledger = ledger(&db);
    ledger
        .append(ExpectedVersion::Absent, event("evt-1", "stream-1", 0))
        .await
        .expect("the event is written");

    let mut response = db
        .query("INFO FOR TABLE event_ledger")
        .await
        .expect("the table is described");
    let info: Option<TableInfo> = response.take(0).expect("the description is read");
    let info = info.unwrap_or_else(|| TableInfo {
        indexes: Default::default(),
    });

    assert!(
        info.indexes.is_empty(),
        "the table of the ledger must have no index: {:?}",
        info.indexes.keys().collect::<Vec<_>>()
    );
}
