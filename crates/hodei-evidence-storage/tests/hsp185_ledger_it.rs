//! HSP-185: the ledger contract ported to the graph authority.
//!
//! Environment-gated like `hsp184_guards_it.rs`: without
//! `HODEI_GRAPH_IT_ENDPOINT` the suite skips itself (exit 0), so
//! ordinary hermetic CI stays green. With the variables set (1 Zero +
//! 1 Alpha with TLS and ACL) the tests reproduce, against a REAL
//! Dgraph server, the observable guarantees the existing
//! `ledger_contract.rs` proves against the local engine:
//!
//! - L1 durability: the process that wrote the event is gone and the
//!   event is still there — here, the child writes through its own
//!   connection, exits, and the parent reads over a NEW connection.
//!   (The local engine needed two processes because RocksDb locks its
//!   directory; the point of two processes is the same: the write
//!   survived the close.)
//! - L2 the optimistic version check is real: a writer that expected a
//!   position that is not the current one is refused, and nothing is
//!   written.
//! - L3 the same delivery arriving twice writes once, and the second
//!   one returns the identity that is already there.
//! - L4 the cursor neither skips nor repeats across pages.
//! - L5 two writers that want the same position: one wins, the other
//!   is told — with TWO SEPARATE CLIENTS, two channels, the server
//!   between them deciding.
//! - The race with a LOST ACK: two clients append events that collide
//!   at one position; after all the attempts settle, the stream holds
//!   neither duplicates nor holes. A delivery whose answer was lost
//!   (the append re-sent after a simulated failure of the ACK path)
//!   replays instead of writing twice.
//!
//! The server state persists between runs, so every fixture uses
//! run-unique stream names and no test drops data.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, TimeZone, Utc};
use hodei_evidence::{
    EventEnvelope, EventId, EventLedger, EventType, ExpectedVersion, IdempotencyKey, LedgerCursor,
    LedgerError, Producer,
};
use hodei_evidence_storage::DgraphEventLedger;
use hodei_graph_client::{GraphClient, GraphClientConfig};
use kernel::Hrn;

/// The tenant namespace every fixture writes under.
const TENANT: &str = "acme";

/// The TOML configuration from the environment, or `None` when the
/// real-server run is not requested (the suite skips itself).
fn config_from_env() -> Option<GraphClientConfig> {
    let endpoint = std::env::var("HODEI_GRAPH_IT_ENDPOINT").ok()?;
    let ca = std::env::var("HODEI_GRAPH_IT_CA").unwrap_or_default();
    let server_name = std::env::var("HODEI_GRAPH_IT_SERVER_NAME").unwrap_or_default();
    let user = std::env::var("HODEI_GRAPH_IT_USER").unwrap_or_default();
    let password = std::env::var("HODEI_GRAPH_IT_PASSWORD").unwrap_or_default();

    let mut text = format!(
        "endpoint = \"{endpoint}\"\n\n[tls]\nenabled = {enabled}\nca_cert_path = \"{ca}\"\n",
        enabled = !ca.is_empty()
    );
    if !server_name.is_empty() {
        text.push_str(&format!("server_name = \"{server_name}\"\n"));
    }
    if !user.is_empty() {
        text.push_str(&format!(
            "\n[credentials]\nuserid = \"{user}\"\npassword = \"{password}\"\n"
        ));
    }
    Some(
        GraphClientConfig::from_toml_str(&text)
            .expect("the environment configuration must validate"),
    )
}

/// Connects and logs in, or skips the calling test when no server is
/// configured. Every call makes a NEW connection: the concurrent
/// tests need genuinely separate clients, not clones of one.
async fn client_or_skip() -> Option<Arc<GraphClient>> {
    let Some(config) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set; the real-server run is gated");
        return None;
    };
    let mut client = GraphClient::connect(config)
        .await
        .expect("connect to the real server");
    client.login().await.expect("the ACL login must succeed");
    Some(Arc::new(client))
}

/// The gated tests share the one server; serializing keeps the
/// fixtures independent (the schema boot is idempotent).
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A run-unique suffix so fixtures never collide across runs.
fn nonce() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock must be set")
        .as_millis();
    format!("h185-{millis}")
}

fn account() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/web-1.0.0").expect("an HRN")
}

fn moment(second: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(1_767_322_800 + second, 0)
        .single()
        .expect("a moment")
}

/// The fixture event: the same envelope shape the local contract
/// builds — idempotency key from a content digest, producer, payload.
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

async fn ledger_or_skip(client: Arc<GraphClient>) -> Option<DgraphEventLedger> {
    DgraphEventLedger::open(client, TENANT)
        .await
        .expect("the ledger opens on a booted schema")
        .into()
}

/// L1 on the remote engine: the event survives the closing of the
/// writer. The child process writes through its own connection and
/// exits; the parent opens a NEW connection to the same server and
/// reads the event. The durability is the server's, the close is the
/// process': exactly what the local proof measured with two processes.
#[tokio::test]
async fn l1_an_event_survives_the_closing_of_the_writer() {
    let _guard = SERIAL.lock().await;
    let marker = "HODEI_LEDGER_DGRAPH_CHILD_WRITES";
    if std::env::var(marker).is_ok() {
        let stream =
            std::env::var("HODEI_LEDGER_DGRAPH_CHILD_STREAM").expect("the child knows the stream");
        let Some(client) = client_or_skip().await else {
            return;
        };
        let ledger = ledger_or_skip(client).await.expect("the ledger opens");
        ledger
            .append(ExpectedVersion::Absent, event("evt-1", &stream, 0))
            .await
            .expect("the event is written");
        return;
    }

    let stream = format!("l1-{}", nonce());
    let status = std::process::Command::new(std::env::current_exe().expect("the test binary"))
        .args([
            "--exact",
            "l1_an_event_survives_the_closing_of_the_writer",
            "--nocapture",
        ])
        .env(marker, "1")
        .env("HODEI_LEDGER_DGRAPH_CHILD_STREAM", &stream)
        .status()
        .expect("the child runs");
    assert!(status.success(), "the child wrote the event");

    // The parent: a brand-new connection, after the writer exited.
    let Some(client) = client_or_skip().await else {
        eprintln!("SKIP: the parent has no server");
        return;
    };
    let ledger = ledger_or_skip(client).await.expect("the ledger opens");
    let page = ledger
        .read(LedgerCursor::start_of(&stream), 10)
        .await
        .expect("the stream is read");

    assert_eq!(page.events.len(), 1, "the event survived: {page:?}");
    assert_eq!(page.events[0].id().as_str(), "evt-1");
    assert_eq!(page.events[0].stream_position(), 0);
}

/// L2 The optimistic version check is real over the remote engine.
#[tokio::test]
async fn l2_the_optimistic_version_check_is_real() {
    let _guard = SERIAL.lock().await;
    let Some(client) = client_or_skip().await else {
        return;
    };
    let ledger = ledger_or_skip(client).await.expect("the ledger opens");
    let stream = format!("l2-{}", nonce());

    ledger
        .append(ExpectedVersion::Absent, event("evt-1", &stream, 0))
        .await
        .expect("the first event is written");

    // The writer expected the stream empty. It is not: the position it
    // names is taken.
    let outcome = ledger
        .append(ExpectedVersion::Absent, event("evt-2", &stream, 0))
        .await;
    assert!(
        matches!(
            outcome,
            Err(LedgerError::VersionConflict {
                actual: Some(0),
                ..
            })
        ),
        "the writer has to be told that the stream moved to 0: {outcome:?}"
    );

    // And the stream still holds only the first event: the refused
    // write wrote nothing.
    let page = ledger
        .read(LedgerCursor::start_of(&stream), 10)
        .await
        .expect("the stream is read");
    assert_eq!(page.events.len(), 1, "the refused write wrote nothing");
    assert_eq!(page.events[0].id().as_str(), "evt-1");
}

/// L3 The same delivery arriving twice writes once.
#[tokio::test]
async fn l3_the_same_delivery_arriving_twice_writes_once() {
    let _guard = SERIAL.lock().await;
    let Some(client) = client_or_skip().await else {
        return;
    };
    let ledger = ledger_or_skip(client).await.expect("the ledger opens");
    let stream = format!("l3-{}", nonce());

    let first = ledger
        .append(ExpectedVersion::Absent, event("evt-1", &stream, 0))
        .await
        .expect("the first delivery is written");

    // The second delivery arrives AFTER the stream moved (a caller that
    // lost its ACK re-sends with the envelope it already sent): the
    // operation id decides before the CAS, so this is a replay, not a
    // conflict.
    ledger
        .append(ExpectedVersion::At(0), event("evt-2", &stream, 1))
        .await
        .expect("the stream moved on");

    let second = ledger
        .append(ExpectedVersion::Absent, event("evt-1", &stream, 0))
        .await
        .expect("the repeated delivery is the same delivery");

    assert_eq!(first, second, "the repeated delivery is the same event");

    let page = ledger
        .read(LedgerCursor::start_of(&stream), 10)
        .await
        .expect("the stream is read");
    assert_eq!(page.events.len(), 2, "the event was written once");
    assert_eq!(page.events[0].id().as_str(), "evt-1");
    assert_eq!(page.events[1].id().as_str(), "evt-2");
}

/// L3b The replay keeps everything the contract says it keeps: the
/// sequence (the page still reads evt-1 at position 0), the payload
/// byte-semantic, the digest of the delivery and the scope (AC#2).
#[tokio::test]
async fn l3b_the_replay_keeps_the_sequence_payload_digest_and_scope() {
    let _guard = SERIAL.lock().await;
    let Some(client) = client_or_skip().await else {
        return;
    };
    let ledger = ledger_or_skip(client).await.expect("the ledger opens");
    let stream = format!("l3b-{}", nonce());
    let original = event("evt-1", &stream, 0);

    ledger
        .append(ExpectedVersion::Absent, original.clone())
        .await
        .expect("the event is written");
    ledger
        .append(ExpectedVersion::Absent, original.clone())
        .await
        .expect("the repeat replays");

    let page = ledger
        .read(LedgerCursor::start_of(&stream), 10)
        .await
        .expect("the stream is read");
    assert_eq!(page.events.len(), 1, "one event, not two");
    let stored = &page.events[0];
    assert_eq!(stored.id(), original.id(), "the identity is the event's");
    assert_eq!(stored.stream_position(), 0, "the sequence is kept");
    assert_eq!(
        stored.payload(),
        original.payload(),
        "the payload is byte-semantic across the store"
    );
    assert_eq!(
        stored.idempotency_key(),
        original.idempotency_key(),
        "the digest of the delivery survives"
    );
    assert_eq!(
        stored.tenant_hrn(),
        original.tenant_hrn(),
        "the scope survives"
    );
    assert_eq!(stored.producer(), original.producer());
    assert_eq!(stored.occurred_at(), original.occurred_at());
    assert_eq!(stored.recorded_at(), original.recorded_at());

    // An error is distinguishable from an empty stream: a stream that
    // was never written reads empty (a response), while a broken
    // sequence errors. The empty page:
    let empty = ledger
        .read(
            LedgerCursor::start_of(format!("never-written-{}", nonce())),
            10,
        )
        .await
        .expect("an unknown stream is empty, not an error");
    assert!(empty.events.is_empty(), "the absence is a response");
    assert_eq!(empty.next_cursor, None, "and there is no next page");
}

/// L4 The cursor neither skips nor repeats, over the remote engine.
#[tokio::test]
async fn l4_the_cursor_neither_skips_nor_repeats() {
    let _guard = SERIAL.lock().await;
    let Some(client) = client_or_skip().await else {
        return;
    };
    let ledger = ledger_or_skip(client).await.expect("the ledger opens");
    let stream = format!("l4-{}", nonce());

    for position in 0..7 {
        ledger
            .append(
                if position == 0 {
                    ExpectedVersion::Absent
                } else {
                    ExpectedVersion::At(position - 1)
                },
                event(&format!("evt-{position}"), &stream, position),
            )
            .await
            .unwrap_or_else(|error| panic!("the event {position} is written: {error:?}"));
    }

    let mut seen = Vec::new();
    let mut cursor = LedgerCursor::start_of(&stream);
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

/// L5 The race with TWO CLIENTS and two channels: one wins the
/// position, the other is told. This is the local L5 with the server
/// deciding the race instead of one process.
#[tokio::test]
async fn l5_two_clients_that_want_the_same_position_are_told() {
    let _guard = SERIAL.lock().await;
    let Some(client_a) = client_or_skip().await else {
        return;
    };
    let client_b = client_or_skip().await.expect("the second client connects");
    let ledger_a = ledger_or_skip(client_a).await.expect("ledger A opens");
    let ledger_b = ledger_or_skip(client_b).await.expect("ledger B opens");
    let stream = format!("l5-{}", nonce());

    let first = ledger_a.append(ExpectedVersion::Absent, event("evt-1", &stream, 0));
    let second = ledger_b.append(ExpectedVersion::Absent, event("evt-2", &stream, 0));
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

    let page = ledger_a
        .read(LedgerCursor::start_of(&stream), 10)
        .await
        .expect("the stream is read");
    assert_eq!(page.events.len(), 1, "only one event was written");
}

/// AC#1, the race the ticket names: two clients, contention, and — on
/// the loser's side — a LOST ACK: the write commits and the answer
/// never reaches the caller, which re-sends the same envelope. The
/// stream must hold neither duplicates nor holes after everything
/// settles, and the re-send must not write a second copy.
///
/// The shape: N events contending for the same stream from two clients
/// through the retry-or-conflict protocol the port prescribes (the
/// writer re-reads the head after a conflict). One of the writers
/// simulates the lost ACK by appending the SAME envelope a second time
/// with the expectation it had before the commit succeeded — the exact
/// re-send pattern of a caller whose answer vanished.
#[tokio::test]
async fn race_two_clients_with_lost_ack_never_duplicates_or_holes() {
    let _guard = SERIAL.lock().await;
    let Some(client_a) = client_or_skip().await else {
        return;
    };
    let client_b = client_or_skip().await.expect("the second client connects");
    let ledger_a = ledger_or_skip(client_a).await.expect("ledger A opens");
    let ledger_b = ledger_or_skip(client_b).await.expect("ledger B opens");
    let stream = format!("race-{}", nonce());

    // Each writer drives eight events through the prescribed protocol:
    // try at the head, on conflict re-read and try the next position.
    // The record keeps, for every event, the position it committed at:
    // the lost-ACK re-send below re-sends the exact envelope at the
    // exact expectation the writer held when it first sent it.
    async fn drive(
        ledger: &DgraphEventLedger,
        stream: &str,
        tag: &str,
        count: usize,
    ) -> Vec<(String, u64)> {
        let mut committed = Vec::new();
        let mut written = 0;
        while written < count {
            let head = ledger.position(stream).await.expect("the head is read");
            let position = head.map_or(0, |last| last + 1);
            let id = format!("evt-{tag}-{written}");
            let expected = match head {
                None => ExpectedVersion::Absent,
                Some(last) => ExpectedVersion::At(last),
            };
            match ledger.append(expected, event(&id, stream, position)).await {
                Ok(returned) => {
                    assert_eq!(returned.as_str(), id, "the ledger returns what was written");
                    committed.push((id, position));
                    written += 1;
                }
                Err(LedgerError::VersionConflict { .. }) => continue,
                Err(other) => panic!("the append failed: {other:?}"),
            }
        }
        committed
    }

    let (a_events, b_events) = tokio::join!(
        drive(&ledger_a, &stream, "a", 8),
        drive(&ledger_b, &stream, "b", 8)
    );

    // The lost ACK, writer A's side: its first delivery committed and
    // the answer was swallowed. The caller knows neither the commit
    // nor the stream moving; it re-sends the same envelope with the
    // expectation it held at send time (Absent for the event that
    // landed at position 0, At(position - 1) otherwise). The registry
    // answers with the committed identity and writes nothing again.
    let (a_id, a_position) = a_events[0].clone();
    let a_expected = if a_position == 0 {
        ExpectedVersion::Absent
    } else {
        ExpectedVersion::At(a_position - 1)
    };
    let replay_a = ledger_a
        .append(a_expected, event(&a_id, &stream, a_position))
        .await
        .expect("a re-send of a committed event replays, never conflicts");
    assert_eq!(
        replay_a.as_str(),
        a_id,
        "the replay returns the committed identity"
    );

    // The other side of the same coin, writer B: a delivery whose ACK
    // was lost, but the caller re-sends with an expectation the stream
    // has since moved past AND an event the position does not already
    // hold. That is a conflict, not a repetition, and it writes nothing.
    let stale_head = a_events.len() as u64 + b_events.len() as u64 - 2;
    let conflict_b = ledger_b
        .append(
            ExpectedVersion::At(stale_head),
            event("evt-intruder", &stream, stale_head + 1),
        )
        .await
        .expect_err("a DIFFERENT event at a stale expectation is a conflict");
    assert!(
        matches!(conflict_b, LedgerError::VersionConflict { .. }),
        "a stale expectation is a conflict, not a replay: {conflict_b:?}"
    );

    // The reading: the stream holds exactly the sixteen events, once
    // each, with no hole in the sequence.
    let mut all: Vec<(u64, String)> = Vec::new();
    let mut cursor = LedgerCursor::start_of(&stream);
    loop {
        let page = ledger_a
            .read(cursor.clone(), 4)
            .await
            .expect("the stream is read");
        for event in &page.events {
            all.push((event.stream_position(), event.id().as_str().to_string()));
        }
        match page.next_cursor {
            Some(next) => cursor = next,
            None => break,
        }
    }

    let expected: Vec<String> = a_events
        .iter()
        .chain(b_events.iter())
        .map(|(id, _)| id.clone())
        .collect();
    let seen: Vec<String> = all.iter().map(|(_, id)| id.clone()).collect();
    let mut sorted_seen = seen.clone();
    sorted_seen.sort();
    let mut sorted_expected = expected.clone();
    sorted_expected.sort();
    assert_eq!(
        sorted_seen, sorted_expected,
        "every event exactly once — no duplicates, no loss"
    );

    // The sequence itself: positions 0..16 contiguous, in order.
    let positions: Vec<u64> = all.iter().map(|(position, _)| *position).collect();
    assert_eq!(
        positions,
        (0..16).collect::<Vec<_>>(),
        "the sequence is contiguous and ordered"
    );

    eprintln!(
        "race settled: {} events, positions {positions:?}, A wrote {}, B wrote {}",
        all.len(),
        a_events.len(),
        b_events.len()
    );
}
