//! HSP-201 over the real graph authority: the resumable-session journal
//! is durable, CAS-arbitrated by the server itself, and its sweep is
//! bounded to the run's tenant.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT like every IT of the stack, and
//! SERIALIZED: the authority is shared (HSP-192, HSP-196), so the tests
//! assert only the membership of their OWN keys —never global censuses—
//! and every scenario runs under a unique `hsp201_*` tenant cleaned at
//! the end.
//!
//! What it proves:
//!
//! - **The journal's CAS is the server's** (AC1): two writers holding
//!   the same revision race behind a barrier and exactly one admits; the
//!   loser gets the authoritative offset, and the stored record moved
//!   once, never twice.
//! - **The offset advances only on admitted durable bytes** (AC1): the
//!   record the other replica would read carries the winner's offset
//!   and revision —which is what `resume` on another replica replays
//!   against.
//! - **The TTL sweep closes only its tenant** (AC3): an abandoned
//!   session of this run is marked closed by the sweep and returned with
//!   its record, and a foreign tenant's stale session is NOT touched —
//!   membership of own keys, the shared-alpha contract.
//! - **Quota across two uploads never overruns the budget** (AC2): the
//!   durable quota store —the same mechanism `reserve`/`pin`/`consume`
//!   uses in production— admits the pair that fits and refuses the pair
//!   that does not, with receipts making the consumption idempotent.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{Duration as ChronoDuration, Utc};
use hodei_artifacts_api::oci::session_journal::{
    DgraphJournal, JournalError, SessionJournal, SessionRecord,
};
use hodei_artifacts_api::quota::QuotaLimits;
use hodei_artifacts_api::quota_dgraph::{DgraphQuotaStore, QuotaFailure, QuotaObserved};
use hodei_graph_client::schema::dql_string;
use hodei_graph_client::{
    GraphClient, GraphClientConfig, GraphMutationBlock, GraphMutationPort, GraphUpsertTransaction,
    Tenant,
};
use serde_json::Value;

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

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The run scope: a tenant unique to this invocation, so the shared
/// authority never mixes two runs or two tests.
fn run_tenant(tag: &str) -> String {
    static RUN: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock moves")
        .as_nanos() as u64;
    format!(
        "hsp201-{tag}-{}-{}",
        nanos,
        RUN.fetch_add(1, Ordering::SeqCst)
    )
}

/// The journal node count for one key: the answer is never two.
async fn session_copies(client: &GraphClient, tenant: &str, id: &str) -> usize {
    let query = format!(
        "{{ q(func: eq(hodei.upload.session.key, {key})) {{ uid }} }}",
        key = dql_string(&format!("upload-session/{tenant}/{id}")),
    );
    let response = client
        .execute_dql(&query)
        .await
        .expect("the session count answers");
    response
        .json
        .get("q")
        .and_then(Value::as_array)
        .map(|nodes| nodes.len())
        .unwrap_or(0)
}

/// Reads one session record straight from the graph: the IT asserts on
/// the server's own truth, not on what the journal returned.
async fn the_graph_record(
    client: &GraphClient,
    tenant: &str,
    id: &str,
) -> (u64, u64, bool, String) {
    let query = format!(
        "{{ q(func: eq(hodei.upload.session.key, {key})) {{ \
         hodei.upload.session.offset hodei.upload.session.version \
         hodei.upload.session.closed hodei.upload.session.handle }} }}",
        key = dql_string(&format!("upload-session/{tenant}/{id}")),
    );
    let response = client.execute_dql(&query).await.expect("the session reads");
    let node = response
        .json
        .get("q")
        .and_then(Value::as_array)
        .and_then(|nodes| nodes.first())
        .expect("the session stands")
        .clone();
    (
        node.get("hodei.upload.session.offset")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        node.get("hodei.upload.session.version")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        node.get("hodei.upload.session.closed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        node.get("hodei.upload.session.handle")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    )
}

/// The deletion of a run's own journal rows: the session node and the
/// revision markers it could have written, by their EXACT keys —
/// membership-scoped cleanup on the shared alpha, never a range census.
async fn clean_session(client: &GraphClient, tenant: &str, id: &str) {
    let base = format!("upload-session/{tenant}/{id}");
    let keys = [base.clone(), format!("{base}#v1"), format!("{base}#v2")];
    let query = keys
        .iter()
        .enumerate()
        .map(|(n, key)| {
            format!(
                "k{n} as var(func: eq(hodei.upload.session.key, {key}))",
                key = dql_string(key),
            )
        })
        .collect::<Vec<_>>()
        .join("\n  ");
    let delete = (0..keys.len())
        .map(|n| format!("uid(k{n}) * * .\n"))
        .collect::<String>();
    client
        .upsert(GraphUpsertTransaction {
            query: format!("{{ {query} }}"),
            blocks: vec![GraphMutationBlock {
                cond: None,
                set_nquads: String::new(),
                delete_nquads: delete,
            }],
            commit_now: true,
        })
        .await
        .expect("the session rows are deleted");
}

/// The limits the quota scenarios share: a small budget, so two uploads
/// that do not fit cannot both confirm.
fn the_limits() -> QuotaLimits {
    QuotaLimits {
        max_stored_bytes_per_account: Some(1000),
        max_concurrent_uploads_per_account: None,
        retry_after_secs: 7,
    }
}

/// AC1 — the journal's CAS is the server's: a stale revision never
/// moves the authority, and the winner's revision stands once.
#[tokio::test]
async fn the_durable_cas_admits_one_writer_per_revision() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let tenant = run_tenant("cas");
    let journal = DgraphJournal::open(client.clone(), &tenant)
        .await
        .expect("the journal opens");
    let id = "hsp201-cas-session";
    journal
        .open(&SessionRecord {
            id: id.to_owned(),
            account: "acme".to_owned(),
            repository: "releases".to_owned(),
            offset: 0,
            handle: "staging/hsp201-cas.part".to_owned(),
            version: 0,
            closed: false,
            touched_at_millis: now_millis(),
        })
        .await
        .expect("the session stands in the journal");

    // Exactly one node for the key: the `@upsert` index and the
    // absent-guard made the open a single fact.
    assert_eq!(
        session_copies(&client, &tenant, id).await,
        1,
        "the open stands once, never twice"
    );

    // The race behind a barrier: both writers hold revision 0. The
    // journal is `Clone` —the same client, the same tenant—, so the
    // two writers are two holders of the same authority.
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let first = {
        let barrier = Arc::clone(&barrier);
        let journal = journal.clone();
        tokio::spawn(async move {
            barrier.wait().await;
            journal.advance(id, 0, 400, now_millis()).await
        })
    };
    let second = {
        let barrier = Arc::clone(&barrier);
        let journal = journal.clone();
        tokio::spawn(async move {
            barrier.wait().await;
            journal.advance(id, 0, 700, now_millis()).await
        })
    };
    let (first, second) = (
        first.await.expect("the writer"),
        second.await.expect("the writer"),
    );
    let winner = match (first, second) {
        (Ok(revision), Err(JournalError::Conflict { offset, .. }))
        | (Err(JournalError::Conflict { offset, .. }), Ok(revision)) => {
            assert_eq!(revision, 1, "the admitted advance lands on revision one");
            assert!(
                offset == 400 || offset == 700,
                "the loser sees the real offset"
            );
            offset
        }
        (Ok(_), Ok(_)) => panic!("two writers cannot both admit on one revision"),
        (Err(one), Err(other)) => panic!("the race failed instead of arbitrating: {one} / {other}"),
        (one, other) => panic!("the race mixed an admit with a non-conflict: {one:?} / {other:?}"),
    };

    // The graph moved once: the winner's offset, revision 1, one node.
    let (offset, version, closed, _) = the_graph_record(&client, &tenant, id).await;
    assert_eq!(offset, winner);
    assert_eq!(version, 1);
    assert!(!closed);
    assert_eq!(session_copies(&client, &tenant, id).await, 1);

    // A stale writer after the race is refused with the authority's
    // truth in hand: this is the `416` that keeps a replay from
    // duplicating a part.
    match journal.advance(id, 0, 999_999, now_millis()).await {
        Err(JournalError::Conflict {
            offset, revision, ..
        }) => {
            assert_eq!(offset, winner);
            assert_eq!(revision, 1);
        }
        other => panic!("the stale writer was not refused: {other:?}"),
    }

    clean_session(&client, &tenant, id).await;
}

/// AC1/AC3 — a session resumed through the journal carries the
/// authority's offset and handle, and the sweep closes ONLY its tenant.
#[tokio::test]
async fn the_durable_journal_carries_the_resume_and_sweeps_only_its_tenant() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let tenant = run_tenant("resume");
    let foreign = run_tenant("resume-foreign");
    let journal = DgraphJournal::open(client.clone(), &tenant)
        .await
        .expect("the journal opens");
    let foreign_journal = DgraphJournal::open(client.clone(), &foreign)
        .await
        .expect("the foreign journal opens");

    let mine = "hsp201-resume-session";
    let theirs = "hsp201-resume-alien-session";
    journal
        .open(&SessionRecord {
            id: mine.to_owned(),
            account: "acme".to_owned(),
            repository: "releases".to_owned(),
            offset: 0,
            handle: "staging/hsp201-resume.part".to_owned(),
            version: 0,
            closed: false,
            touched_at_millis: now_millis(),
        })
        .await
        .expect("my session opens");
    foreign_journal
        .open(&SessionRecord {
            id: theirs.to_owned(),
            account: "acme".to_owned(),
            repository: "releases".to_owned(),
            offset: 0,
            handle: "staging/hsp201-alien.part".to_owned(),
            version: 0,
            closed: false,
            touched_at_millis: now_millis(),
        })
        .await
        .expect("the foreign session opens");

    // The advance that the replica that owns the writer makes: the
    // record the next replica resumes from.
    journal
        .advance(mine, 0, 512, now_millis())
        .await
        .expect("the offset advances");
    let record = journal
        .get(mine)
        .await
        .expect("the journal answers")
        .expect("the session stands");
    assert_eq!(record.offset, 512, "the resume reads the authority's bytes");
    assert_eq!(record.version, 1);
    assert_eq!(
        record.handle, "staging/hsp201-resume.part",
        "the handle the other replica reopens with"
    );

    // The sweep with the grace already past: my session is returned
    // closed, the foreign tenant's stale session is NOT touched.
    let expired = journal
        .sweep_expired(now_millis() + 10_000, 1_000)
        .await
        .expect("the sweep answers");
    assert_eq!(
        expired.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        vec![mine],
        "the sweep returns this tenant's orphan, with its record"
    );
    assert!(expired[0].closed);
    let (_, _, closed, _) = the_graph_record(&client, &foreign, theirs).await;
    assert!(
        !closed,
        "the sweep of one tenant closed a session of another: census"
    );

    clean_session(&client, &tenant, mine).await;
    clean_session(&client, &foreign, theirs).await;
}

/// AC2 — two uploads that together overrun the budget never both
/// confirm against the durable quota, the same authority production
/// uses; the consumption carries its receipt and replays idempotently.
#[tokio::test]
async fn two_resumed_uploads_never_overrun_the_durable_budget() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let name = run_tenant("quota");
    // The quota ledger keys on the guard scope: the durable mechanism
    // the brief names —reserve/pin/consume/release—, the same one
    // production uses.
    let tenant = Tenant::new(&name).expect("the quota tenant is valid");
    let store = Arc::new(
        DgraphQuotaStore::open(client.clone())
            .await
            .expect("the quota store opens"),
    );
    let limits = the_limits();

    // Upload A opens on replica one and reserves 600 of the thousand.
    let a = store
        .reserve(
            &tenant,
            "acct-201",
            "upload-a",
            &QuotaObserved::fresh(),
            Some(600),
            &limits,
            "replica-a",
            Utc::now() + ChronoDuration::seconds(120),
        )
        .await
        .expect("the first reservation fits");
    assert_eq!(a.reserved, 600);

    // Upload B —the resumed session of the other replica— declares 600
    // too: the pair does not fit, so the second reservation is refused,
    // not queued.
    let observed = store
        .observe(&tenant, "acct-201")
        .await
        .expect("the ledger reads");
    let refused = store
        .reserve(
            &tenant,
            "acct-201",
            "upload-b",
            &observed,
            Some(600),
            &limits,
            "replica-b",
            Utc::now() + ChronoDuration::seconds(120),
        )
        .await;
    match refused {
        Err(QuotaFailure::Refusal(_)) => {}
        other => panic!("the second upload must be refused, not queued: {other:?}"),
    }
    // Without a double reservation: the budget still says 600 live.
    let after = store
        .observe(&tenant, "acct-201")
        .await
        .expect("the ledger reads");
    assert_eq!(after.in_flight, 600, "one reservation stands");
    assert_eq!(after.active, 1);

    // B fits when it declares what the budget still has: the two
    // together touch the thousand and never cross it.
    let b = store
        .reserve(
            &tenant,
            "acct-201",
            "upload-b",
            &after,
            Some(400),
            &limits,
            "replica-b",
            Utc::now() + ChronoDuration::seconds(120),
        )
        .await
        .expect("the pair fits the budget exactly");
    assert_eq!(b.reserved, 400);
    let peak = store
        .observe(&tenant, "acct-201")
        .await
        .expect("the ledger reads");
    assert_eq!(peak.stored + peak.in_flight, 1000, "the budget is touched");
    assert!(
        peak.stored + peak.in_flight <= 1000,
        "the pair never exceeded the budget"
    );

    // Upload A completes: the consumption carries its receipt, and the
    // re-delivery of the same consumption counts nothing again.
    store
        .consume(
            &tenant,
            "acct-201",
            "upload-a",
            600,
            "publish-a",
            Utc::now(),
        )
        .await
        .expect("the consumption commits");
    let replay = store
        .consume(
            &tenant,
            "acct-201",
            "upload-a",
            600,
            "publish-a",
            Utc::now(),
        )
        .await
        .expect("the same receipt replays");
    assert!(replay.replayed, "the re-delivery counted nothing again");
    let settled = store
        .observe(&tenant, "acct-201")
        .await
        .expect("the ledger reads");
    assert_eq!(settled.stored, 600);
    assert_eq!(settled.in_flight, 400, "B still holds its reservation");

    // B's resumed session is ABORTED —the crash path of AC3—: the
    // explicit release brings the room back. Never the global expired
    // sweep: on the shared alpha, release_expired is HSP-171's own
    // reconciler and this test has no claim on other tenants' leases.
    let released = store
        .release(&tenant, "acct-201", "upload-b")
        .await
        .expect("the release commits");
    assert!(released, "the live reservation came back");
    let freed = store
        .observe(&tenant, "acct-201")
        .await
        .expect("the ledger reads");
    assert_eq!(freed.in_flight, 0);

    // B reopens on the other replica under a fresh reservation: the
    // room is there again, and the budget still holds.
    let again = store
        .reserve(
            &tenant,
            "acct-201",
            "upload-b2",
            &freed,
            Some(400),
            &limits,
            "replica-b",
            Utc::now() + ChronoDuration::seconds(120),
        )
        .await
        .expect("the reopened upload fits");
    assert_eq!(again.reserved, 400);
    store
        .consume(
            &tenant,
            "acct-201",
            "upload-b2",
            400,
            "publish-b",
            Utc::now(),
        )
        .await
        .expect("the second upload publishes");

    // The invariant, after everything: stored exactly the budget, no
    // live reservations, and a third upload cannot start.
    let final_state = store
        .observe(&tenant, "acct-201")
        .await
        .expect("the ledger reads");
    assert_eq!(final_state.stored, 1000);
    assert_eq!(final_state.in_flight, 0);
    let closed = store
        .reserve(
            &tenant,
            "acct-201",
            "upload-c",
            &final_state,
            Some(1),
            &limits,
            "replica-a",
            Utc::now() + ChronoDuration::seconds(120),
        )
        .await;
    assert!(
        matches!(closed, Err(QuotaFailure::Refusal(_))),
        "a byte over the budget is refused, never queued: {closed:?}"
    );

    // Best-effort cleanup of this tenant's ledger rows by key: the
    // shared alpha holds only what THIS run wrote under this name.
    clean_quota(&client, &tenant, "acct-201").await;
}

/// Deletes this run's quota rows by their exact keys: the ledger, its
/// tickets and its receipts, in one transaction. The conflict anchors
/// move with the ledger's advances —the previous anchor is deleted by
/// the next one, the ledger's own rule—, so they need no sweep here.
/// Membership-scoped, never a census.
async fn clean_quota(client: &GraphClient, tenant: &Tenant, account: &str) {
    let scope = DgraphQuotaStore::scope_of(tenant, account).expect("the scope is valid");
    let ledger_key = hodei_graph_client::guard_key(&scope);
    let keys = [
        ("hodei.quota.key", vec![ledger_key.clone()]),
        (
            "hodei.quota.ticket.key",
            vec![
                format!("{ledger_key}/tickets/upload-a"),
                format!("{ledger_key}/tickets/upload-b"),
                format!("{ledger_key}/tickets/upload-b2"),
            ],
        ),
        (
            "hodei.quota.receipt.key",
            vec![
                format!("{ledger_key}/receipts/publish-a"),
                format!("{ledger_key}/receipts/publish-b"),
            ],
        ),
    ];
    let mut vars = Vec::new();
    let mut deletes = String::new();
    for (index, (predicate, value)) in keys
        .iter()
        .flat_map(|(predicate, values)| values.iter().map(move |value| (predicate, value)))
        .enumerate()
    {
        vars.push(format!(
            "k{index} as var(func: eq({predicate}, {key}))",
            predicate = predicate,
            key = dql_string(value),
        ));
        deletes.push_str(&format!("uid(k{index}) * * .\n"));
    }
    client
        .upsert(GraphUpsertTransaction {
            query: format!("{{\n  {}\n}}", vars.join("\n  ")),
            blocks: vec![GraphMutationBlock {
                cond: None,
                set_nquads: String::new(),
                delete_nquads: deletes,
            }],
            commit_now: true,
        })
        .await
        .expect("the run's quota rows are deleted");
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}
