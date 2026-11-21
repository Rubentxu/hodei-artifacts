//! HSP-198: the durable quota ledger over the graph authority — the
//! reservation is persisted and arbitrated by the guard, the consumption
//! carries its receipt in the same transaction, and a failure releases or
//! replays without ever counting twice.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT like every IT of the stack, and
//! SERIALIZED: the tests share one authority and one ledger contract.
//! What it proves, against the real engine and against the in-memory
//! ledger of `hodei_artifacts_api::quota` that holds the same semantics:
//!
//! - **Two writers that together overrun the quota cannot both confirm**
//!   (AC1): the race is forced with a DETERMINISTIC BARRIER — both hold
//!   the same observed revision and both attempt behind a barrier, and the
//!   guard's revision anchor decides. One wins, the other re-decides and
//!   constata WITHOUT a double reservation.
//! - **A crash BEFORE the consumption** (AC2): the reservation's lease
//!   dies, the reconciler releases exactly what the dead worker held, and
//!   the room comes back. The explicit release is the abort path.
//! - **A crash AFTER the commit** (AC2): the receipt stands; the
//!   re-delivery of the same consumption replays the committed answer and
//!   counts nothing again. The consumption and its receipt are ONE fact.
//! - **The re-drive never counts twice** (AC2): the same reservation, the
//!   same pin and the same consumption, re-driven, are each one fact.
//! - **Parity**: the same scenarios leave the SAME verdicts and the SAME
//!   counters in the durable ledger and in the in-memory ledger.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{Duration as ChronoDuration, Utc};
use hodei_artifacts_api::quota::{QuotaLedger, QuotaLimits, QuotaRefusal};
use hodei_artifacts_api::quota_dgraph::{
    DgraphQuotaStore, QuotaFailure, QuotaGraphError, QuotaObserved, decide_reserve,
};
use hodei_graph_client::schema::dql_string;
use hodei_graph_client::{GraphClient, GraphClientConfig, Tenant};
use serde_json::Value;

/// The limits of the scenarios: one thousand bytes, no concurrency gate
/// unless a test says otherwise.
fn the_limits() -> QuotaLimits {
    QuotaLimits {
        max_stored_bytes_per_account: Some(1000),
        max_concurrent_uploads_per_account: None,
        retry_after_secs: 7,
    }
}

/// The limits a test names, said whole.
fn limited(max_bytes: Option<u64>, max_uploads: Option<usize>) -> QuotaLimits {
    QuotaLimits {
        max_stored_bytes_per_account: max_bytes,
        max_concurrent_uploads_per_account: max_uploads,
        retry_after_secs: 7,
    }
}

/// The env configuration of the stack ITs, exactly as hsp199 and hsp205
/// read it.
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
    Some(GraphClientConfig::from_toml_str(&text).expect("the IT configuration validates"))
}

/// A fresh connection to the shared authority, with the bounded open
/// retry the shared server sometimes needs (the HSP-219 lesson).
async fn fresh_client() -> Option<Arc<GraphClient>> {
    let Some(config) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return None;
    };
    let mut last = String::new();
    for attempt in 1..=3 {
        let mut client = match GraphClient::connect(config.clone()).await {
            Ok(client) => client,
            Err(error) => {
                last = error.to_string();
                tokio::time::sleep(std::time::Duration::from_millis(250 * attempt as u64)).await;
                continue;
            }
        };
        match client.login().await {
            Ok(()) => return Some(Arc::new(client)),
            Err(error) => {
                last = error.to_string();
                tokio::time::sleep(std::time::Duration::from_millis(250 * attempt as u64)).await;
            }
        }
    }
    panic!("Dgraph did not open after 3 attempts: {last}");
}

/// The store over the live authority, shared through an `Arc` so the tests
/// can race writers that are genuinely concurrent.
async fn open_store() -> Option<(Arc<GraphClient>, Arc<DgraphQuotaStore>)> {
    let client = fresh_client().await?;
    let store = DgraphQuotaStore::open(client.clone())
        .await
        .expect("the quota store opens on the validated schema");
    Some((client, Arc::new(store)))
}

/// The run scope: a tenant unique to this invocation, so the shared
/// authority never mixes two runs or two tests.
fn run_tenant(tag: &str) -> Tenant {
    static RUN: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock moves")
        .as_nanos() as u64;
    let name = format!(
        "hsp198-{tag}-{}-{}",
        nanos,
        RUN.fetch_add(1, Ordering::SeqCst)
    );
    Tenant::new(&name).expect("the run tenant is valid")
}

/// How many ticket nodes the graph holds under one ticket key: the
/// decision of every scenario is never two.
async fn ticket_copies(client: &GraphClient, ticket_key: &str) -> usize {
    let query = format!(
        "{{ q(func: eq(hodei.quota.ticket.key, {key})) {{ uid }} }}",
        key = dql_string(ticket_key),
    );
    let response = client
        .execute_dql(&query)
        .await
        .expect("the ticket count answers");
    response
        .json
        .get("q")
        .and_then(Value::as_array)
        .map(|nodes| nodes.len())
        .unwrap_or(0)
}

/// The state of one ticket, read from the graph.
async fn the_ticket_state(client: &GraphClient, ticket_key: &str) -> String {
    let query = format!(
        "{{ q(func: eq(hodei.quota.ticket.key, {key})) {{ hodei.quota.ticket.state }} }}",
        key = dql_string(ticket_key),
    );
    let response = client
        .execute_dql(&query)
        .await
        .expect("the ticket state answers");
    response
        .json
        .get("q")
        .and_then(Value::as_array)
        .and_then(|nodes| nodes.first())
        .and_then(|node| node.get("hodei.quota.ticket.state"))
        .and_then(Value::as_str)
        .unwrap_or("absent")
        .to_owned()
}

/// How many receipt nodes the graph holds for one receipt key.
async fn receipt_copies(client: &GraphClient, receipt_key: &str) -> usize {
    let query = format!(
        "{{ q(func: eq(hodei.quota.receipt.key, {key})) {{ uid }} }}",
        key = dql_string(receipt_key),
    );
    let response = client
        .execute_dql(&query)
        .await
        .expect("the receipt count answers");
    response
        .json
        .get("q")
        .and_then(Value::as_array)
        .map(|nodes| nodes.len())
        .unwrap_or(0)
}

/// The ledger key of a test's scope, for the graph-level assertions.
fn ledger_key_of(tenant: &Tenant, account: &str) -> String {
    let scope = DgraphQuotaStore::scope_of(tenant, account).expect("the scope is valid");
    hodei_graph_client::guard_key(&scope)
}

/// The durable observation of an in-memory ledger's usage: the parity
/// compares the SAME state, reached through real admissions on both sides.
fn observed_of(memory: &QuotaLedger, account: &str) -> QuotaObserved {
    let usage = memory.usage(account);
    QuotaObserved {
        stored: usage.stored_bytes,
        in_flight: usage.in_flight_bytes,
        active: usage.active_uploads as u64,
        revision: 0,
    }
}

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// AC1 — two writers whose declarations together overrun the quota cannot
/// both confirm: the reservation is persisted and arbitrated by the guard,
/// one wins, the other constata, and the counters show ONE reservation.
#[tokio::test]
async fn two_writers_that_overrun_the_quota_cannot_both_confirm() {
    let Some((_client, store)) = open_store().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let tenant = run_tenant("race");
    let account = "acct-race";
    let limits = the_limits();

    // The account is born: one small reservation stands so the race runs
    // over an EXISTING ledger revision. Both writers will hold THIS
    // observed revision, which is the deterministic barrier.
    store
        .reserve(
            &tenant,
            account,
            "seed",
            &QuotaObserved::fresh(),
            Some(50),
            &limits,
            "seeder",
            Utc::now() + ChronoDuration::seconds(120),
        )
        .await
        .expect("the seed reservation commits");
    let shared = store
        .observe(&tenant, account)
        .await
        .expect("the ledger reads");
    assert_eq!(shared.in_flight, 50);
    assert_eq!(shared.active, 1);

    // The race, behind the barrier: BOTH writers hold the same revision,
    // both declare eight hundred (50 + 800 + 800 overruns the thousand).
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let barrier_one = Arc::clone(&barrier);
    let barrier_two = Arc::clone(&barrier);
    let one = store.clone();
    let two = store.clone();
    let tenant_a = tenant.clone();
    let tenant_b = tenant.clone();
    let observed_one = shared.clone();
    let observed_two = shared.clone();
    let (first, second) = tokio::join!(
        async move {
            barrier_one.wait().await;
            one.reserve(
                &tenant_a,
                account,
                "writer-a",
                &observed_one,
                Some(800),
                &the_limits(),
                "writer-a",
                Utc::now() + ChronoDuration::seconds(120),
            )
            .await
        },
        async move {
            barrier_two.wait().await;
            two.reserve(
                &tenant_b,
                account,
                "writer-b",
                &observed_two,
                Some(800),
                &the_limits(),
                "writer-b",
                Utc::now() + ChronoDuration::seconds(120),
            )
            .await
        },
    );
    let winner = match (first, second) {
        (Ok(w), Err(lost)) | (Err(lost), Ok(w)) => {
            assert!(
                matches!(
                    lost,
                    QuotaFailure::Refusal(QuotaRefusal::StorageOver { .. })
                ),
                "the loser constata a StorageOver, not a failure: {lost}"
            );
            assert_eq!(w.reserved, 800, "the winner reserved what it declared");
            w
        }
        (Ok(_), Ok(_)) => panic!("two writers that overrun the quota cannot both confirm"),
        (Err(one), Err(other)) => {
            panic!("the race failed instead of arbitrating: {one} / {other}")
        }
    };
    eprintln!("the race decided: one reservation stood, the loser constata the refusal");

    // Without a double reservation: exactly one race ticket stands, the
    // revision advanced exactly once beyond the shared one, and the
    // ledger says the seed plus ONE reservation, never both.
    let ledger_key = ledger_key_of(&tenant, account);
    let stood = (
        ticket_copies(&_client, &format!("{ledger_key}/tickets/writer-a")).await,
        ticket_copies(&_client, &format!("{ledger_key}/tickets/writer-b")).await,
    );
    assert_eq!(
        stood.0 + stood.1,
        1,
        "one race ticket stands, never two: {stood:?}"
    );
    let after = store
        .observe(&tenant, account)
        .await
        .expect("the ledger reads");
    assert_eq!(
        after.in_flight, 850,
        "the seed plus ONE reservation, never both: {after:?}"
    );
    assert_eq!(after.active, 2, "the seed and the winner, not the loser");
    assert_eq!(after.stored, 0);
    assert_eq!(
        after.revision,
        shared.revision + 1,
        "the guard advanced once for the winner; the loser wrote nothing"
    );
    let _ = winner;

    // Parity with the in-memory ledger: the same scenario through real
    // admissions gives the same verdict and the same counters.
    let memory = QuotaLedger::new(the_limits());
    let _seed = memory.begin(account, Some(50)).expect("the seed fits");
    let raced_a = memory.begin(account, Some(800));
    let raced_b = memory.begin(account, Some(800));
    let refused = [raced_a.is_err(), raced_b.is_err()];
    assert_eq!(
        refused.iter().filter(|lost| **lost).count(),
        1,
        "the in-memory ledger refuses exactly one of the two: {refused:?}"
    );
    let usage = memory.usage(account);
    assert_eq!(usage.in_flight_bytes, 850, "the same accounting, in memory");
    assert_eq!(usage.active_uploads, 2);

    // The room the refusal left: the loser reserved NOTHING, so a hundred
    // bytes still fit under the thousand.
    store
        .reserve(
            &tenant,
            account,
            "room",
            &after,
            Some(100),
            &the_limits(),
            "roomer",
            Utc::now() + ChronoDuration::seconds(120),
        )
        .await
        .expect("the room fits after the refusal");
}

/// AC2 — a crash BEFORE the consumption: the worker reserved and died.
/// Its lease dies, the reconciler releases exactly what it held, and the
/// room comes back. The explicit release is the abort path, and a double
/// release constata.
#[tokio::test]
async fn a_crash_before_the_consumption_is_repaired_by_the_release() {
    let Some((_client, store)) = open_store().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let tenant = run_tenant("crash-before");
    let account = "acct-crash";
    let limits = the_limits();
    let base = Utc::now();

    // The worker that reserves with a one-second lease and dies before
    // consuming anything.
    let crashed = store
        .reserve(
            &tenant,
            account,
            "dead-worker",
            &QuotaObserved::fresh(),
            Some(600),
            &limits,
            "dead-worker",
            base + ChronoDuration::seconds(1),
        )
        .await
        .expect("the dead worker reserved");
    assert_eq!(crashed.reserved, 600);
    let held = store
        .observe(&tenant, account)
        .await
        .expect("the ledger reads");
    assert_eq!(
        held.in_flight, 600,
        "the reservation stood while the lease lived"
    );

    // The reconciler, after the lease died: exactly what the dead worker
    // held comes back, and nothing else moves.
    let released = store
        .release_expired(base + ChronoDuration::seconds(2))
        .await
        .expect("the reconciliation runs");
    assert_eq!(
        released.len(),
        1,
        "one expired ticket released: {released:?}"
    );
    let freed = store
        .observe(&tenant, account)
        .await
        .expect("the ledger reads");
    assert_eq!(freed.in_flight, 0, "the dead reservation is back");
    assert_eq!(freed.active, 0);

    // The room is real: the same bytes fit again.
    let again = store
        .reserve(
            &tenant,
            account,
            "again",
            &freed,
            Some(600),
            &limits,
            "again",
            base + ChronoDuration::seconds(120),
        )
        .await
        .expect("the freed room admits the same bytes");
    assert!(!again.replayed);

    // The explicit release is the abort path: it returns the reservation,
    // and a second release constata.
    assert!(
        store
            .release(&tenant, account, "again")
            .await
            .expect("the abort releases"),
        "the explicit release returns the reservation"
    );
    assert!(
        !store
            .release(&tenant, account, "again")
            .await
            .expect("the second release constata"),
        "releasing twice releases once"
    );
    let empty = store
        .observe(&tenant, account)
        .await
        .expect("the ledger reads");
    assert_eq!(empty.in_flight, 0);
    assert_eq!(empty.active, 0, "the abort freed the slot too");
}

/// AC2 — a crash AFTER the commit: the consumption landed but the ACK was
/// lost. The receipt stands; the re-delivery of the same consumption
/// replays the committed answer and counts nothing again. The consumption
/// and its receipt are ONE fact in the graph.
#[tokio::test]
async fn a_crash_after_the_commit_is_closed_by_the_receipt() {
    let Some((client, store)) = open_store().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let tenant = run_tenant("crash-after");
    let account = "acct-ack";
    let limits = the_limits();
    let base = Utc::now();

    store
        .reserve(
            &tenant,
            account,
            "upload-one",
            &QuotaObserved::fresh(),
            Some(400),
            &limits,
            "worker",
            base + ChronoDuration::seconds(120),
        )
        .await
        .expect("the reservation commits");

    // The worker consumes and dies before the ACK reaches it.
    let ledger_key = ledger_key_of(&tenant, account);
    let receipt_key = format!("{ledger_key}/receipts/ack-lost");
    let consumed = store
        .consume(&tenant, account, "upload-one", 400, "ack-lost", base)
        .await
        .expect("the consumption commits");
    assert!(!consumed.replayed);
    assert_eq!(consumed.stored, 400);
    assert_eq!(
        receipt_copies(&client, &receipt_key).await,
        1,
        "the consumption and its receipt are one fact"
    );
    assert_eq!(
        the_ticket_state(&client, &format!("{ledger_key}/tickets/upload-one")).await,
        "settled"
    );

    // The re-delivery of the SAME consumption: the receipt replays, the
    // stored total does not grow again, and the ticket is not resurrected.
    let again = store
        .consume(&tenant, account, "upload-one", 400, "ack-lost", base)
        .await
        .expect("the re-delivery replays");
    assert!(again.replayed, "the re-delivery replays the receipt");
    assert_eq!(again.stored, 400, "the stored total is not counted twice");
    assert_eq!(receipt_copies(&client, &receipt_key).await, 1);
    let after = store
        .observe(&tenant, account)
        .await
        .expect("the ledger reads");
    assert_eq!(after.stored, 400);
    assert_eq!(after.in_flight, 0, "the settled reservation left in-flight");
    assert_eq!(after.active, 0);

    // A DIFFERENT consumption id on the same ticket is not a replay: the
    // ticket is settled, so nothing can consume it again.
    let ghost = store
        .consume(&tenant, account, "upload-one", 400, "other-receipt", base)
        .await;
    match ghost {
        Err(QuotaGraphError::NotReserved { ticket, state }) => {
            assert_eq!(ticket, "upload-one");
            assert_eq!(state, "settled");
        }
        other => panic!("a settled ticket cannot be consumed again: {other:?}"),
    }

    // And the release after the settle is a constata: the stored bytes are
    // real content, releasing them would un-count the account.
    assert!(
        !store
            .release(&tenant, account, "upload-one")
            .await
            .expect("the release constata"),
        "a settled ticket is not released"
    );
    let still = store
        .observe(&tenant, account)
        .await
        .expect("the ledger reads");
    assert_eq!(still.stored, 400, "the constata wrote nothing");
}

/// AC2 — the re-drive never counts twice: the same reservation re-driven
/// replays, the same pin re-driven constata, the same consumption
/// re-driven replays, and every counter says ONE fact, not two.
#[tokio::test]
async fn the_redrive_never_counts_twice() {
    let Some((_client, store)) = open_store().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let tenant = run_tenant("redrive");
    let account = "acct-redrive";
    let limits = the_limits();
    let base = Utc::now();
    let until = base + ChronoDuration::seconds(120);

    // The reservation re-driven: one fact, however many times it is said.
    let first = store
        .reserve(
            &tenant,
            account,
            "the-ticket",
            &QuotaObserved::fresh(),
            Some(300),
            &limits,
            "worker",
            until,
        )
        .await
        .expect("the reservation commits");
    assert!(!first.replayed);
    let repeated = store
        .reserve(
            &tenant,
            account,
            "the-ticket",
            &QuotaObserved::fresh(),
            Some(300),
            &limits,
            "worker",
            until,
        )
        .await
        .expect("the re-drive replays");
    assert!(repeated.replayed, "the same reservation is said once");
    assert_eq!(repeated.reserved, 300);
    let observed = store
        .observe(&tenant, account)
        .await
        .expect("the ledger reads");
    assert_eq!(observed.in_flight, 300, "one reservation, in flight once");

    // The pin re-driven at the same total: a constata, and the ledger
    // holds the running total once. A pin that adds nothing constata too.
    store
        .pin(&tenant, account, "the-ticket", 300, &limits, until, base)
        .await
        .expect("the pin that adds nothing constata");
    store
        .pin(&tenant, account, "the-ticket", 450, &limits, until, base)
        .await
        .expect("the pin commits");
    store
        .pin(&tenant, account, "the-ticket", 450, &limits, until, base)
        .await
        .expect("the re-pin constata");
    let after_pin = store
        .observe(&tenant, account)
        .await
        .expect("the ledger reads");
    assert_eq!(after_pin.in_flight, 450, "the pin is one fact");

    // The consumption re-driven: the same receipt is one fact.
    let consumed = store
        .consume(&tenant, account, "the-ticket", 450, "the-receipt", base)
        .await
        .expect("the consumption commits");
    assert!(!consumed.replayed);
    assert_eq!(consumed.stored, 450);
    let redelivered = store
        .consume(&tenant, account, "the-ticket", 450, "the-receipt", base)
        .await
        .expect("the re-delivery replays");
    assert!(redelivered.replayed);
    assert_eq!(
        redelivered.stored, 450,
        "the stored total is not counted twice"
    );
    let settled = store
        .observe(&tenant, account)
        .await
        .expect("the ledger reads");
    assert_eq!(settled.stored, 450);
    assert_eq!(settled.in_flight, 0);
    assert_eq!(settled.active, 0);

    // The release re-driven after the settle: a constata, and a ticket
    // that never was is nothing to release.
    assert!(
        !store
            .release(&tenant, account, "the-ticket")
            .await
            .expect("the release constata")
    );
    assert!(
        !store
            .release(&tenant, account, "ghost-ticket")
            .await
            .expect("the ghost is nothing to release")
    );
}

/// The saturation parity: the concurrency gate refuses with the SAME typed
/// refusal the in-memory ledger gives, and releasing the slot frees it.
#[tokio::test]
async fn the_saturation_refuses_and_the_release_frees() {
    let Some((_client, store)) = open_store().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let tenant = run_tenant("saturate");
    let account = "acct-sat";
    let limits = limited(Some(10_000), Some(1));
    let until = Utc::now() + ChronoDuration::seconds(120);

    let first = store
        .reserve(
            &tenant,
            account,
            "slot-one",
            &QuotaObserved::fresh(),
            Some(10),
            &limits,
            "worker-a",
            until,
        )
        .await
        .expect("the first slot is taken");
    assert!(!first.replayed);
    let held = store
        .observe(&tenant, account)
        .await
        .expect("the ledger reads");
    let second = store
        .reserve(
            &tenant,
            account,
            "slot-two",
            &held,
            Some(10),
            &limits,
            "worker-b",
            until,
        )
        .await;
    match second {
        Err(QuotaFailure::Refusal(QuotaRefusal::UploadsSaturated { retry_after_secs })) => {
            assert_eq!(retry_after_secs, 7, "the refusal says when to come back");
        }
        other => panic!("the saturated account refuses the second upload: {other:?}"),
    }

    // The parity: the in-memory ledger, at the SAME state reached through
    // its own real admission, gives the SAME verdict.
    let memory = QuotaLedger::new(limited(Some(10_000), Some(1)));
    let _slot = memory.begin(account, Some(10)).expect("the slot is taken");
    let refused = memory.begin(account, Some(10));
    match refused {
        Err(QuotaRefusal::UploadsSaturated { retry_after_secs }) => {
            assert_eq!(retry_after_secs, 7);
        }
        other => panic!("the same typed refusal, in memory: {other:?}"),
    }

    // The release frees the slot, and the freed slot admits again.
    assert!(
        store
            .release(&tenant, account, "slot-one")
            .await
            .expect("the release frees the slot")
    );
    let freed = store
        .observe(&tenant, account)
        .await
        .expect("the ledger reads");
    let third = store
        .reserve(
            &tenant,
            account,
            "slot-three",
            &freed,
            Some(10),
            &limits,
            "worker-c",
            until,
        )
        .await
        .expect("the freed slot admits");
    assert!(!third.replayed);
}

/// The pure parity of the admission rules, without a server: for the
/// states an in-memory ledger reaches through real admissions, the durable
/// decision refuses EXACTLY what the in-memory begin refuses, and admits
/// exactly what it admits.
#[test]
fn the_admission_rules_are_the_same_rules_the_memory_ledger_applies() {
    let limits = the_limits();
    let memory = QuotaLedger::new(limits.clone());

    // The empty account admits what the durable decision admits. The
    // ticket lives until the end of the scope: dropping it would return
    // the reservation and the parity would compare different states.
    let declared = Some(600);
    assert_eq!(
        decide_reserve(&QuotaObserved::fresh(), &limits, declared).expect("it fits"),
        600
    );
    let _reserved = memory.begin("parity", declared).expect("it fits");

    // The SAME state, reached on both sides, refuses the SAME way: six
    // hundred more would pass the thousand, in flight or stored.
    let state = observed_of(&memory, "parity");
    assert_eq!(state.in_flight, 600);
    assert!(matches!(
        decide_reserve(&state, &limits, Some(500)),
        Err(QuotaRefusal::StorageOver {
            retry_after_secs: 7
        })
    ));
    assert!(matches!(
        memory.begin("parity", Some(500)),
        Err(QuotaRefusal::StorageOver { .. })
    ));

    // The state that fits the margin admits on both sides.
    assert_eq!(
        decide_reserve(&state, &limits, Some(400)).expect("four hundred fit"),
        400
    );
    assert!(memory.begin("parity", Some(400)).is_ok());

    // The concurrency gate saturates the same way, at the same state.
    let gated = QuotaLedger::new(limited(None, Some(1)));
    let _slot = gated.begin("parity", Some(10)).expect("the slot is taken");
    let saturated = observed_of(&gated, "parity");
    assert!(matches!(
        decide_reserve(&saturated, &limited(None, Some(1)), Some(10)),
        Err(QuotaRefusal::UploadsSaturated {
            retry_after_secs: 7
        })
    ));
    assert!(matches!(
        gated.begin("parity", Some(10)),
        Err(QuotaRefusal::UploadsSaturated { .. })
    ));
}
