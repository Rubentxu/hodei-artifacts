//! HSP-220: draining the outbox of the shared lab server.
//!
//! The shared Dgraph server (h186) accumulates the pending audit facts
//! of ALL runs: no suite marks them delivered and
//! there is no deployed worker to consume them. The port's global `pending()`
//! census returns all of them with their full `AuditEvent`, and when the
//! set crosses the gRPC message limit (4 MiB) the response dies with
//! `OutOfRange`: the failure proven by HSP-194 that blocked suites
//! 187/190/191/193 (shared server state, not code).
//!
//! This IT is the lab's drain utility: it claims each pending fact
//! and marks it delivered with THE SAME mechanics that the port promises
//! (atomic `claim` + `mark_delivered`, hsp187), in batches, so
//! that the server does not hold a global pending set that overflows.
//! It deletes and rewrites nothing: delivery is the drain semantics for
//! facts that no consumer is going to reread; the history stays in the
//! graph as `delivered`.
//!
//! The root of the key query is bounded (`first:`), because the
//! drain starts precisely when the unbounded census no longer fits. The
//! counts go to stdout and to `target/uat/hsp220/`; never the payloads.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT`, like all the stack's ITs. A
//! single test: draining is a server operation, not per account.
//!
//! The per-tenant census (`eq(hodei.catalog.outbox.tenant, account)`, exact
//! index) is the prevention for the suites; this drain is the cure for the
//! already overflowed server. The port's real consumer
//! (`AuditDispatcher::dispatch_pending`) still calls `pending()` without
//! bounds: if deployed against a shared server, it needs
//! pagination -- the follow-up recommendation lives in the evidence.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Duration as ChronoDuration, Utc};
use hodei_audit::AuditOutbox;
use hodei_catalog_storage::DgraphAuditOutbox;
use hodei_graph_client::{GraphClient, GraphClientConfig};
use tokio::task::JoinSet;

/// The batch per round: the key response must be small even if the
/// server is overflowed (hence `first:`).
const KEYS_PER_ROUND: usize = 500;

/// The drain's concurrency: each fact costs several gRPC calls
/// (claim + mark + reads) and 8 pairs in flight do not choke the alpha.
const DRAIN_CONCURRENCY: usize = 8;

/// Round cap: a drain that does not finish is not a drain.
const MAX_ROUNDS: usize = 200;

/// The run: a distinct worker per execution.
fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

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

/// The evidence directory: `target/uat/hsp220/`.
fn the_evidence_dir() -> PathBuf {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let target = tmp.parent().unwrap_or(&tmp).to_path_buf();
    let dir = target.join("uat").join("hsp220");
    std::fs::create_dir_all(&dir).expect("the evidence directory");
    dir
}

/// How many facts are awaiting delivery NOW: the `pending` ones and the `claimed`
/// ones whose lease expired, the SAME definition as the port's global census.
/// The count travels bounded (a number), never the payload.
async fn the_waiting_count(client: &GraphClient) -> usize {
    let now = Utc::now();
    let response = client
        .execute_dql(&format!(
            "{{ p(func: eq(hodei.catalog.outbox.state, \"pending\")) {{ count(uid) }} \
             c(func: eq(hodei.catalog.outbox.state, \"claimed\")) \
             @filter(le(hodei.catalog.outbox.lease_until, \"{now}\")) {{ count(uid) }} }}"
        ))
        .await
        .expect("the waiting count answers");
    let pending = response
        .json
        .pointer("/p/0/count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let expired = response
        .json
        .pointer("/c/0/count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    (pending + expired) as usize
}

/// The keys of the next batch awaiting delivery, with the same
/// definition as `pending()`. The root is bounded (`first:`) so that the
/// response is small even if the server is overflowed.
async fn the_next_waiting_keys(client: &GraphClient) -> Vec<String> {
    let now = Utc::now();
    let response = client
        .execute_dql(&format!(
            "{{ p(func: eq(hodei.catalog.outbox.state, \"pending\"), first: {KEYS_PER_ROUND}) \
             {{ hodei.catalog.outbox.key }} \
             c(func: eq(hodei.catalog.outbox.state, \"claimed\")) \
             @filter(le(hodei.catalog.outbox.lease_until, \"{now}\")) \
             {{ hodei.catalog.outbox.key }} }}"
        ))
        .await
        .expect("the waiting keys answer");
    let mut keys = Vec::new();
    for root in ["p", "c"] {
        if let Some(nodes) = response
            .json
            .get(root)
            .and_then(serde_json::Value::as_array)
        {
            for node in nodes {
                if let Some(key) = node
                    .get("hodei.catalog.outbox.key")
                    .and_then(serde_json::Value::as_str)
                {
                    keys.push(key.to_string());
                }
            }
        }
    }
    keys
}

/// A batch is drained in bounded parallel: claiming and marking is the worker's
/// pair; what another one took away is not a failure, it is the claim doing its
/// job.
async fn drain_batch(client: &Arc<GraphClient>, worker: &str, keys: Vec<String>) -> (usize, usize) {
    let mut rounds = JoinSet::new();
    let in_flight = Arc::new(tokio::sync::Semaphore::new(DRAIN_CONCURRENCY));
    for key in keys {
        let permit = in_flight.clone();
        let outbox = DgraphAuditOutbox::new(client.clone());
        let worker = worker.to_string();
        rounds.spawn(async move {
            let _permit = permit.acquire().await.expect("the semaphore lives");
            let now = Utc::now();
            let lease_until = now + ChronoDuration::seconds(30);
            let claimed = outbox
                .claim(&key, &worker, lease_until, now)
                .await
                .expect("the claim answers");
            if claimed.is_none() {
                return false;
            }
            outbox
                .mark_delivered(&key, &worker, Utc::now())
                .await
                .expect("the mark answers");
            true
        });
    }
    let mut delivered = 0;
    let mut skipped = 0;
    while let Some(outcome) = rounds.join_next().await {
        match outcome.expect("the drain task does not panic") {
            true => delivered += 1,
            false => skipped += 1,
        }
    }
    (delivered, skipped)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn drains_the_shared_outbox_waiting_facts_with_claim_and_mark() {
    let Some(config) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    let mut client = GraphClient::connect(config).await.expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    let client = Arc::new(client);
    let worker = format!("hsp220-drain-{}", nonce());

    let before = the_waiting_count(&client).await;
    println!("DRAIN before: {before} waiting facts");
    let mut rounds = 0;
    let mut total_delivered = 0;
    let mut total_skipped = 0;
    while rounds < MAX_ROUNDS {
        rounds += 1;
        let keys = the_next_waiting_keys(&client).await;
        if keys.is_empty() {
            break;
        }
        let (delivered, skipped) = drain_batch(&client, &worker, keys).await;
        total_delivered += delivered;
        total_skipped += skipped;
        println!("DRAIN round {rounds}: {delivered} delivered, {skipped} skipped");
    }
    let after = the_waiting_count(&client).await;
    println!(
        "DRAIN after: {after} waiting facts ({total_delivered} delivered by {worker} \
         in {rounds} rounds, {total_skipped} lost to other workers)"
    );

    // The drain succeeds when the server no longer holds a global
    // pending set that overflows the gRPC message.
    assert_eq!(
        after, 0,
        "the shared outbox must drain to zero waiting facts, not {after}"
    );

    let evidence = the_evidence_dir().join("drenaje.log");
    let line = format!(
        "drenaje {worker}: antes={before} despues={after} entregados={total_delivered} \
         rondas={rounds} saltados={total_skipped}\n"
    );
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&evidence)
        .and_then(|mut file| std::io::Write::write_all(&mut file, line.as_bytes()))
        .expect("the drain evidence line is written");
}
