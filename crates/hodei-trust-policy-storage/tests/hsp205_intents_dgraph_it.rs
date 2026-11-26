//! HSP-205: the reactive intents and their receipts over persistent
//! Dgraph — the claim is atomic, the lease fences, and the receipt and
//! its effect are one fact.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT like every IT of the stack, and
//! SERIALIZED: the tests share one authority and one outbox contract.
//! What it proves, against the real engine:
//!
//! - **The claim is a compare-and-set**: two executors race for the same
//!   intent and ONE wins; the loser constata (AC1).
//! - **A crash BEFORE the effect loses nothing**: the lease expires, the
//!   intent goes back to the market, another worker claims it and
//!   applies it (AC1).
//! - **A crash AFTER the effect duplicates nothing**: the effect row
//!   stands, the re-claimer sees it and closes the intent with the
//!   Applied receipt WITHOUT applying again (AC1).
//! - **Fencing is monotonic**: a worker whose lease died cannot write,
//!   and a worker whose token was overtaken is refused with a typed
//!   error; the token of the claim that applied an effect is IN the fact
//!   (AC3).
//! - **The re-drive keeps the scope and the watermark**: re-saying the
//!   intents of one CVE re-drive does not touch another scope's intents,
//!   the watermark advances one receipt at a time with no gaps, and
//!   every refusal is an explicit error (AC2).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use hodei_graph_client::schema::dql_string;
use hodei_graph_client::{GraphClient, GraphClientConfig};
use hodei_reactive::intent::{Intent, IntentOrigin, IntentOutbox, Receipt};
use hodei_reactive::runtime::ExecutionStore;
use hodei_reactive::Event;
use hodei_trust_policy_storage::dgraph::{DgraphIntentError, DgraphIntentStore};
use serde_json::{json, Value};

/// The env configuration of the stack ITs, exactly as hsp199 and hsp203
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
                tokio::time::sleep(Duration::from_millis(250 * attempt as u64)).await;
                continue;
            }
        };
        match client.login().await {
            Ok(()) => return Some(Arc::new(client)),
            Err(error) => {
                last = error.to_string();
                tokio::time::sleep(Duration::from_millis(250 * attempt as u64)).await;
            }
        }
    }
    panic!("Dgraph did not open after 3 attempts: {last}");
}

/// The store over the live authority, with its client.
async fn open_store() -> Option<(Arc<GraphClient>, Arc<DgraphIntentStore>)> {
    let client = fresh_client().await?;
    let store = DgraphIntentStore::open(client.clone())
        .await
        .expect("the intent store opens on the validated schema");
    Some((client, Arc::new(store)))
}

/// The run scope: a name unique to this invocation, so the shared
/// authority never mixes two runs or two tests.
fn run_scope(tag: &str) -> String {
    static RUN: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock moves")
        .as_nanos() as u64;
    format!(
        "hsp205-{tag}-{}-{}",
        nanos,
        RUN.fetch_add(1, Ordering::SeqCst)
    )
}

/// An intent of the scenario: said the way the reactions say it.
fn an_intent(scope: &str, tenant: &str, tag: &str, advisory: &str) -> Intent {
    Intent::new(
        IntentOrigin::of(
            "vulnerability_impact",
            &Event::new(
                format!("evt-{scope}-{tag}"),
                hodei_reactive::EventKind::VulnerabilityAccepted,
                json!({"advisory": advisory}),
                Utc::now(),
            ),
            &[format!("evt-{scope}-{tag}")],
        ),
        tenant,
        "security_case",
        format!("case:{scope}:{advisory}"),
        "evidence:1.0.0",
        json!({"advisory": advisory}),
    )
}

/// Works one CLAIMED intent the way the runtime does: the effect that
/// already landed closes the intent with the Applied receipt WITHOUT
/// touching again; otherwise the effect lands and the receipt follows.
/// The store write IS the external touch of this scenario, and
/// `crash_after_effect` simulates the process dying between the two.
async fn work_claimed(
    store: &DgraphIntentStore,
    intent: &Intent,
    crash_after_effect: bool,
) -> Result<bool, String> {
    let already = store
        .applied_effects()
        .await
        .iter()
        .any(|applied| applied.idempotency_key == intent.idempotency_key);
    if already {
        store
            .record_receipt(&intent.identity, &Receipt::Applied)
            .await?;
        return Ok(false);
    }
    store.record_effect(intent.as_effect()).await;
    if crash_after_effect {
        return Ok(true);
    }
    store
        .record_receipt(&intent.identity, &Receipt::Applied)
        .await?;
    Ok(true)
}

/// The state of one intent, read from the graph.
async fn the_state(client: &GraphClient, identity: &str) -> String {
    let query = format!(
        "{{ q(func: eq(hodei.intent.key, {key})) {{ hodei.intent.state }} }}",
        key = dql_string(identity),
    );
    let response = client
        .execute_dql(&query)
        .await
        .expect("the state read answers");
    response
        .json
        .get("q")
        .and_then(Value::as_array)
        .and_then(|nodes| nodes.first())
        .and_then(|node| node.get("hodei.intent.state"))
        .and_then(Value::as_str)
        .unwrap_or("absent")
        .to_owned()
}

/// The consumer watermark of one scope: how many intents of the tenant
/// have reached a terminal state. The re-drive must never move it back
/// or make it jump.
async fn the_watermark(client: &GraphClient, tenant: &str) -> usize {
    let query = format!(
        "{{ q(func: eq(hodei.intent.tenant, {tenant})) {{ hodei.intent.state }} }}",
        tenant = dql_string(tenant),
    );
    let response = client
        .execute_dql(&query)
        .await
        .expect("the watermark read answers");
    response
        .json
        .get("q")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter(|node| {
            let state = node
                .get("hodei.intent.state")
                .and_then(Value::as_str)
                .unwrap_or("absent");
            state != "pending" && state != "claimed"
        })
        .count()
}

/// The generation the effect row of one key carries inside the fact: the
/// token of the claim that applied it.
async fn the_effect_generation(client: &GraphClient, idempotency_key: &str) -> Option<u64> {
    let query = format!(
        "{{ q(func: eq(hodei.effect.key, {key})) {{ hodei.effect.generation }} }}",
        key = dql_string(idempotency_key),
    );
    let response = client
        .execute_dql(&query)
        .await
        .expect("the effect read answers");
    response
        .json
        .get("q")
        .and_then(Value::as_array)
        .and_then(|nodes| nodes.first())
        .and_then(|node| node.get("hodei.effect.generation"))
        .and_then(Value::as_u64)
}

/// How many effect rows the graph holds for one idempotency key: the
/// decision of the scenario is never two.
async fn the_effect_copies(client: &GraphClient, idempotency_key: &str) -> usize {
    let query = format!(
        "{{ q(func: eq(hodei.effect.key, {key})) {{ uid }} }}",
        key = dql_string(idempotency_key),
    );
    let response = client
        .execute_dql(&query)
        .await
        .expect("the effect count answers");
    response
        .json
        .get("q")
        .and_then(Value::as_array)
        .map(|nodes| nodes.len())
        .unwrap_or(0)
}

/// How many claim markers the graph holds for one intent at the
/// generation the race wrote: the marker is the fact one claim leaves,
/// and two of them would mean the index did not decide.
async fn claim_markers(client: &GraphClient, identity: &str) -> usize {
    let query = format!(
        "{{ q(func: eq(hodei.claim.identity, {identity})) {{ uid }} }}",
        identity = dql_string(identity),
    );
    let response = client
        .execute_dql(&query)
        .await
        .expect("the marker count answers");
    response
        .json
        .get("q")
        .and_then(Value::as_array)
        .map(|nodes| nodes.len())
        .unwrap_or(0)
}

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// AC1 — two executors race for the same intent: the compare-and-set
/// makes ONE winner, the loser constata, and the winner's receipt closes
/// the intent for everybody.
#[tokio::test]
async fn two_executors_race_for_one_intent_and_one_wins() {
    let Some((client, store)) = open_store().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let scope = run_scope("race");
    let tenant = format!("{scope}-a");
    let intent = an_intent(&scope, &tenant, "race", "GHSA-race");
    store
        .put_pending(&intent)
        .await
        .expect("the intent is said");

    // The race, for real: two executors claim the same intent at the
    // same time.
    let one = store.clone();
    let two = store.clone();
    let identity_one = intent.identity.clone();
    let identity_two = intent.identity.clone();
    let owner_a = format!("{scope}-worker-a");
    let owner_b = format!("{scope}-worker-b");
    let until = Utc::now() + ChronoDuration::seconds(60);
    let (first, second) = tokio::join!(
        async move { one.claim(&identity_one, &owner_a, until, Utc::now()).await },
        async move { two.claim(&identity_two, &owner_b, until, Utc::now()).await },
    );
    let (winner_intent, loser) = match (first, second) {
        (Ok(Some(winner)), Ok(None)) => (winner, "b"),
        (Ok(None), Ok(Some(winner))) => (winner, "a"),
        (Ok(None), Ok(None)) => panic!("nobody won a claimable intent"),
        (Ok(Some(_)), Ok(Some(_))) => panic!("two executors cannot win the same claim"),
        (Err(one), _) => panic!("the claim failed instead of racing: {one}"),
        (_, Err(two)) => panic!("the claim failed instead of racing: {two}"),
    };
    assert_eq!(winner_intent.identity, intent.identity);
    eprintln!("the executor {loser} constata: the intent is somebody else's");
    assert_eq!(
        claim_markers(&client, &intent.identity).await,
        1,
        "one claim at one generation leaves ONE claim marker"
    );

    // The winner applies and the receipt closes the intent.
    let applied = work_claimed(&store, &winner_intent, false)
        .await
        .expect("the winner applies");
    assert!(applied, "the winner's effect was the first one");
    assert_eq!(the_state(&client, &intent.identity).await, "applied");
    assert_eq!(
        the_effect_copies(&client, &intent.idempotency_key).await,
        1,
        "the decision is one, never two"
    );

    // And an applied intent is not claimable again, not pending, and the
    // loser's late claim finds nothing.
    let late = store
        .claim(
            &intent.identity,
            &format!("{scope}-worker-b"),
            Utc::now() + ChronoDuration::seconds(60),
            Utc::now(),
        )
        .await
        .expect("the late claim runs");
    assert!(late.is_none(), "an applied intent is not claimed again");
    assert!(
        store
            .recover_pending(Utc::now())
            .await
            .expect("the recovery runs")
            .iter()
            .all(|one| one.identity != intent.identity),
        "an applied intent is not pending"
    );
}

/// AC1 — a crash BEFORE the effect: the worker claimed and died. The
/// lease expires, the intent goes back to the market, another worker
/// claims it and applies it. The effect is not lost.
#[tokio::test]
async fn a_crash_before_the_effect_is_repaired_by_the_lease() {
    let Some((client, store)) = open_store().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let scope = run_scope("before");
    let tenant = format!("{scope}-a");
    let intent = an_intent(&scope, &tenant, "before", "GHSA-before");
    store
        .put_pending(&intent)
        .await
        .expect("the intent is said");

    // The worker that takes the intent and dies before doing anything:
    // its lease is born with a horizon it will never reach alive.
    let base = Utc::now();
    let dead_claim = store
        .claim(
            &intent.identity,
            &format!("{scope}-dead"),
            base + ChronoDuration::seconds(1),
            base,
        )
        .await
        .expect("the claim runs")
        .expect("the dead worker did win the claim");
    assert_eq!(dead_claim.identity, intent.identity);
    assert_eq!(the_state(&client, &intent.identity).await, "claimed");

    // The market re-opens after the lease dies, and another worker
    // claims and applies.
    let re_marketed = store
        .recover_pending(base + ChronoDuration::seconds(2))
        .await
        .expect("the recovery runs");
    assert!(
        re_marketed
            .iter()
            .any(|one| one.identity == intent.identity),
        "the intent of the dead lease is back"
    );
    let claim = store
        .claim(
            &intent.identity,
            &format!("{scope}-worker-b"),
            Utc::now() + ChronoDuration::seconds(60),
            Utc::now(),
        )
        .await
        .expect("the claim runs")
        .expect("the second worker takes it");
    let applied = work_claimed(&store, &claim, false)
        .await
        .expect("the second worker applies");
    assert!(applied, "the effect lands exactly once");
    assert_eq!(the_state(&client, &intent.identity).await, "applied");
    assert_eq!(
        the_effect_copies(&client, &intent.idempotency_key).await,
        1,
        "the effect is not lost and not doubled"
    );
}

/// AC1 — a crash AFTER the effect: the worker touched and died before
/// the receipt. The effect row stands (idempotency by effect ID); the
/// re-claimer sees it and closes the intent with the Applied receipt
/// WITHOUT applying again. The decision is not duplicated.
#[tokio::test]
async fn a_crash_after_the_effect_is_closed_by_the_receipt() {
    let Some((client, store)) = open_store().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let scope = run_scope("after");
    let tenant = format!("{scope}-a");
    let intent = an_intent(&scope, &tenant, "after", "GHSA-after");
    store
        .put_pending(&intent)
        .await
        .expect("the intent is said");

    // The worker applies and dies before the receipt: a short lease
    // marks the crash window, and the effect row stands alone.
    let base = Utc::now();
    let claim = store
        .claim(
            &intent.identity,
            &format!("{scope}-crashed"),
            base + ChronoDuration::seconds(1),
            base,
        )
        .await
        .expect("the claim runs")
        .expect("the worker takes it");
    let crashed = work_claimed(&store, &claim, true)
        .await
        .expect("the touch happens");
    assert!(crashed, "the effect landed before the crash");
    assert_eq!(
        the_state(&client, &intent.identity).await,
        "claimed",
        "the receipt never came, and the intent waits"
    );
    assert_eq!(
        the_effect_copies(&client, &intent.idempotency_key).await,
        1,
        "the effect row stands, alone"
    );

    // The re-claimer: the lease expires, the intent comes back, and the
    // worker sees the effect already applied — it closes the intent
    // WITHOUT applying again.
    let re_marketed = store
        .recover_pending(base + ChronoDuration::seconds(2))
        .await
        .expect("the recovery runs");
    assert!(re_marketed
        .iter()
        .any(|one| one.identity == intent.identity));
    let reclaim = store
        .claim(
            &intent.identity,
            &format!("{scope}-worker-b"),
            Utc::now() + ChronoDuration::seconds(60),
            Utc::now(),
        )
        .await
        .expect("the claim runs")
        .expect("the re-claimer takes it");
    let applied_again = work_claimed(&store, &reclaim, false)
        .await
        .expect("the re-claimer constata");
    assert!(
        !applied_again,
        "the re-claimer did NOT apply: the effect was already there"
    );
    assert_eq!(the_state(&client, &intent.identity).await, "applied");
    assert_eq!(
        the_effect_copies(&client, &intent.idempotency_key).await,
        1,
        "the decision is not duplicated"
    );
}

/// AC3 — fencing is monotonic: a worker whose lease died cannot write,
/// and a worker whose token was overtaken is refused with a typed error
/// naming the generation the intent reached. The token of the claim that
/// applied the effect is IN the fact.
#[tokio::test]
async fn an_expired_lease_and_an_overtaken_token_cannot_write() {
    let Some((client, store)) = open_store().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let scope = run_scope("fence");
    let tenant = format!("{scope}-a");
    let intent = an_intent(&scope, &tenant, "fence", "GHSA-fence");
    store
        .put_pending(&intent)
        .await
        .expect("the intent is said");

    // The first claim, with its fencing token.
    let first = store
        .claim_with_token(
            &intent.identity,
            &format!("{scope}-worker-a"),
            Utc::now() - ChronoDuration::seconds(1),
            Utc::now(),
        )
        .await
        .expect("the claim runs")
        .expect("the first worker takes it");
    assert_eq!(first.fencing_token, 1, "the first claim is generation 1");

    // The lease died while the worker was working: NOBODY may receive a
    // receipt from it, not through the port and not through the fenced
    // path either.
    let port_refused = store
        .record_receipt(&intent.identity, &Receipt::Applied)
        .await;
    assert!(
        port_refused.is_err(),
        "a dead lease cannot write: {port_refused:?}"
    );
    let fenced_refused = store
        .record_receipt_fenced(&intent.identity, first.fencing_token, &Receipt::Applied)
        .await;
    match fenced_refused {
        Err(DgraphIntentError::LeaseDead { identity }) => {
            assert_eq!(identity, intent.identity)
        }
        other => panic!("a dead lease is a typed refusal: {other:?}"),
    }

    // The market re-opens: another worker claims, the generation grows,
    // and the FIRST worker's token is now behind the intent. A fenced
    // write with it is refused BY NAME, with the generation the intent
    // reached.
    let re_marketed = store
        .recover_pending(Utc::now())
        .await
        .expect("the recovery runs");
    assert!(re_marketed
        .iter()
        .any(|one| one.identity == intent.identity));
    let second = store
        .claim_with_token(
            &intent.identity,
            &format!("{scope}-worker-b"),
            Utc::now() + ChronoDuration::seconds(60),
            Utc::now(),
        )
        .await
        .expect("the claim runs")
        .expect("the second worker takes it");
    assert_eq!(
        second.fencing_token, 2,
        "the generation is monotonic: the second claim is generation 2"
    );
    let overtaken = store
        .record_receipt_fenced(&intent.identity, first.fencing_token, &Receipt::Applied)
        .await;
    match overtaken {
        Err(DgraphIntentError::FencingRejected {
            presented, current, ..
        }) => {
            assert_eq!(presented, 1);
            assert_eq!(current, 2, "the intent says the generation it reached");
        }
        other => panic!("a stale token is a typed refusal: {other:?}"),
    }
    assert_eq!(
        the_state(&client, &intent.identity).await,
        "claimed",
        "the refusal wrote nothing"
    );

    // The winner applies, and the fact says WHO applied it: the effect
    // row carries the fencing token of the claim that landed it.
    store
        .record_receipt_fenced(&intent.identity, second.fencing_token, &Receipt::Applied)
        .await
        .expect("the live claim with its token writes");
    assert_eq!(the_state(&client, &intent.identity).await, "applied");
    assert_eq!(
        the_effect_generation(&client, &intent.idempotency_key).await,
        Some(2),
        "the fencing token is in the fact"
    );
    assert_eq!(
        the_effect_copies(&client, &intent.idempotency_key).await,
        1,
        "and the effect is still one"
    );
}

/// AC2 — the re-drive of one CVE keeps the scope and the watermark:
/// re-saying the intents of one scope does not touch another scope's
/// intents, the watermark advances one receipt at a time with no gaps,
/// and every refusal is an explicit error, never a best-effort silence.
#[tokio::test]
async fn the_re_drive_keeps_the_scope_and_the_watermark() {
    let Some((client, store)) = open_store().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let scope = run_scope("redrive");
    let tenant_a = format!("{scope}-a");
    let tenant_b = format!("{scope}-b");

    // Scope A: three intents of one advisory family, worked one by one.
    // The watermark of the consumer advances by exactly one per worked
    // intent — no jumps, no gaps.
    let first = an_intent(&scope, &tenant_a, "redrive-a1", "GHSA-a1");
    let second = an_intent(&scope, &tenant_a, "redrive-a2", "GHSA-a2");
    let third = an_intent(&scope, &tenant_a, "redrive-a3", "GHSA-a3");
    let of_scope_a = [&first, &second, &third];
    for (index, intent) in of_scope_a.iter().enumerate() {
        store.put_pending(intent).await.expect("the intent is said");
        let claim = store
            .claim(
                &intent.identity,
                &format!("{scope}-consumer-a"),
                Utc::now() + ChronoDuration::seconds(60),
                Utc::now(),
            )
            .await
            .expect("the claim runs")
            .expect("the consumer takes it");
        work_claimed(&store, &claim, false)
            .await
            .expect("the consumer applies");
        assert_eq!(
            the_watermark(&client, &tenant_a).await,
            index + 1,
            "the watermark advances by one receipt at a time"
        );
    }
    assert_eq!(the_watermark(&client, &tenant_a).await, 3);

    // Scope B: one intent already applied, one still pending — the state
    // of ANOTHER scope, which the re-drive of scope A must not touch.
    let applied_b = an_intent(&scope, &tenant_b, "redrive-b1", "GHSA-b1");
    let pending_b = an_intent(&scope, &tenant_b, "redrive-b2", "GHSA-b2");
    store.put_pending(&applied_b).await.expect("said");
    store.put_pending(&pending_b).await.expect("said");
    let claim_b = store
        .claim(
            &applied_b.identity,
            &format!("{scope}-consumer-b"),
            Utc::now() + ChronoDuration::seconds(60),
            Utc::now(),
        )
        .await
        .expect("the claim runs")
        .expect("the consumer of b takes it");
    work_claimed(&store, &claim_b, false)
        .await
        .expect("the consumer of b applies");
    assert_eq!(the_watermark(&client, &tenant_b).await, 1);

    // The re-drive: the same CVE re-delivered re-says the intents of
    // scope A. What is already applied STAYS applied — the receipt
    // closes it — and the watermark of scope A does not move.
    for intent in [&first, &second, &third] {
        store.put_pending(intent).await.expect("the re-drive says");
    }
    assert_eq!(
        the_watermark(&client, &tenant_a).await,
        3,
        "the re-drive does not move the watermark back or forward"
    );
    assert_eq!(the_state(&client, &first.identity).await, "applied");
    assert_eq!(the_state(&client, &second.identity).await, "applied");
    assert_eq!(the_state(&client, &third.identity).await, "applied");

    // And the other scope is exactly where it was: its applied intent
    // applied, its pending intent pending, its watermark untouched.
    assert_eq!(the_state(&client, &applied_b.identity).await, "applied");
    assert_eq!(the_state(&client, &pending_b.identity).await, "pending");
    assert_eq!(the_watermark(&client, &tenant_b).await, 1);

    // The recovery of the re-drive finds nothing to work for scope A,
    // and the scope B pending intent is NOT picked by scope A's driver:
    // the driver works the identities it planned, one scope at a time.
    let pending = store
        .recover_pending(Utc::now())
        .await
        .expect("the recovery runs");
    assert!(
        pending.iter().all(|one| one.tenant != tenant_a),
        "scope A has nothing pending after the re-drive"
    );
    for intent in [&first, &second, &third] {
        let claim = store
            .claim(
                &intent.identity,
                &format!("{scope}-consumer-a"),
                Utc::now() + ChronoDuration::seconds(60),
                Utc::now(),
            )
            .await
            .expect("the claim runs");
        assert!(
            claim.is_none(),
            "an applied intent of the re-drive is not claimed again"
        );
    }
    assert_eq!(the_watermark(&client, &tenant_a).await, 3);

    // A NEW intent of the re-driven CVE: said, claimed, applied — the
    // watermark advances by exactly one, without gaps.
    let fourth = an_intent(&scope, &tenant_a, "redrive-a4", "GHSA-a4");
    store
        .put_pending(&fourth)
        .await
        .expect("the new intent is said");
    let claim = store
        .claim(
            &fourth.identity,
            &format!("{scope}-consumer-a"),
            Utc::now() + ChronoDuration::seconds(60),
            Utc::now(),
        )
        .await
        .expect("the claim runs")
        .expect("the consumer takes the new intent");
    work_claimed(&store, &claim, false)
        .await
        .expect("the consumer applies it");
    assert_eq!(
        the_watermark(&client, &tenant_a).await,
        4,
        "3 to 4, one receipt at a time"
    );

    // The explicit errors of the contract: a receipt for an intent
    // nobody claimed, a reconciliation of something that is not unknown
    // — both are typed refusals, never a silent pass.
    let ghost = an_intent(&scope, &tenant_a, "redrive-ghost", "GHSA-ghost");
    store.put_pending(&ghost).await.expect("the ghost is said");
    let receipt = store
        .record_receipt(&ghost.identity, &Receipt::Applied)
        .await;
    assert!(receipt.is_err(), "a receipt without a claim is refused");
    let reconcile = store.reconcile(&first.identity).await;
    assert!(
        reconcile.is_err(),
        "reconciling an applied intent is refused"
    );
    assert_eq!(
        the_state(&client, &first.identity).await,
        "applied",
        "the refusals wrote nothing"
    );
}

/// The receipt of an unknown effect says what the store holds: the
/// effects are keyed by their effect ID, and recording one twice is
/// recording it once.
#[tokio::test]
async fn the_effects_are_keyed_and_once() {
    let Some((_client, store)) = open_store().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let scope = run_scope("effects");
    let tenant = format!("{scope}-a");
    let intent = an_intent(&scope, &tenant, "effects", "GHSA-effects");
    let effect = intent.as_effect();
    store.record_effect(effect.clone()).await;
    store.record_effect(effect.clone()).await;
    let effects = store.applied_effects().await;
    assert_eq!(
        effects
            .iter()
            .filter(|one| one.idempotency_key == effect.idempotency_key)
            .count(),
        1,
        "the same effect is one"
    );
    let read = effects
        .iter()
        .find(|one| one.idempotency_key == effect.idempotency_key)
        .expect("the effect is listed");
    assert_eq!(read.kind, effect.kind, "the kind survives");
    assert_eq!(read.payload, effect.payload, "the payload survives");
}
