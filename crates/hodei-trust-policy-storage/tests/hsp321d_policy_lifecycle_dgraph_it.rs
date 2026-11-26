//! HSP-151 / HSP-321 (persistence leg): the policy lifecycle over persistent
//! Dgraph.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT like every IT of the stack (without it
//! each test SKIPs, which is NOT a green). What it proves against the real
//! engine, ported from the retired HSP-149 suite:
//!
//! - **Persistence**: revisions, head and events survive reopening the store
//!   on a brand-new connection.
//! - **CAS**: concurrent activations expecting the same head have exactly one
//!   winner; the losers are told the head that beat them and nothing is
//!   overwritten. A stale expected head is a typed `StaleHead`.
//! - **Rollback** is a new revision under the same CAS, with its
//!   invalidation event; the history keeps every revision.
//! - **Tenant isolation**: a set of one account is invisible to another.
//! - **Simulation and shadow do not mutate** the store.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use hodei_evidence::domain::capsule::{DimensionName, DimensionState};
use hodei_graph_client::{GraphClient, GraphClientConfig};
use hodei_trust_policy::lifecycle::{Actors, Coverage, LifecycleError, SeparationPolicy};
use hodei_trust_policy::policy::{Requirement, RequirementKind, TrustPolicy};
use hodei_trust_policy::shadow::shadow_evaluate;
use hodei_trust_policy::simulation::simulate;
use hodei_trust_policy::LifecycleStore;
use hodei_trust_policy_storage::DgraphLifecycleStore;

/// The env configuration of the stack ITs, exactly as hsp199 reads it.
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

/// The run scope: an account name unique to this invocation, so the
/// shared authority never mixes two runs or two tests.
fn run_scope() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static RUN: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock moves")
        .as_nanos() as u64;
    format!("hsp203-{}-{}", nanos, RUN.fetch_add(1, Ordering::SeqCst))
}

fn policy(id: &str, version: &str) -> TrustPolicy {
    TrustPolicy::new(
        id,
        version,
        vec![Requirement::denying(RequirementKind::Dimension {
            dimension: DimensionName::Integrity,
            accepts: vec![DimensionState::Verified],
        })],
    )
}

async fn open(separation: SeparationPolicy) -> Option<DgraphLifecycleStore> {
    let client = fresh_client().await?;
    Some(
        DgraphLifecycleStore::open(client, separation)
            .await
            .expect("the lifecycle store opens on the validated schema"),
    )
}

macro_rules! store {
    () => {
        match open(SeparationPolicy::default()).await {
            Some(store) => store,
            None => return,
        }
    };
}

/// Revisions, head and events survive a brand-new connection and store.
#[tokio::test]
async fn the_lifecycle_survives_reopening_the_store() {
    let store = store!();
    let account = run_scope();
    store
        .save(
            &account,
            policy("prod", "1.0.0"),
            "alice",
            "first",
            Utc::now(),
        )
        .await
        .expect("save 1");
    store
        .save(
            &account,
            policy("prod", "2.0.0"),
            "alice",
            "second",
            Utc::now(),
        )
        .await
        .expect("save 2");
    store
        .activate(&account, "prod", 2, None, "bob", "2 goes live", Utc::now())
        .await
        .expect("activate 2");
    drop(store);

    let reopened = store!();
    let history = reopened.history(&account, "prod").await.expect("history");
    assert_eq!(
        history.iter().map(|r| r.sequence).collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(history.iter().all(|revision| revision.digest_holds()));
    assert_eq!(history[1].policy.version, "2.0.0");
    let head = reopened
        .head(&account, "prod")
        .await
        .expect("head")
        .expect("there is a head");
    assert_eq!((head.sequence, head.activated_by.as_str()), (2, "bob"));
    assert_eq!(head.editor, "alice");
    let kinds: Vec<_> = reopened
        .events(&account, "prod")
        .await
        .expect("events")
        .into_iter()
        .map(|event| event.kind)
        .collect();
    assert_eq!(kinds, vec!["revision_saved", "revision_saved", "activated"]);
    assert!(reopened
        .head(&account, "never")
        .await
        .expect("head")
        .is_none());
}

/// Content that does not validate never reaches a revision.
#[tokio::test]
async fn invalid_content_is_refused_before_writing() {
    let store = store!();
    let account = run_scope();
    let mut broken = policy("prod", "1.0.0");
    broken.schema_version = "9.9.9".to_string();
    let error = store
        .save(&account, broken, "alice", "future", Utc::now())
        .await
        .expect_err("unknown schema");
    assert!(matches!(error, LifecycleError::SchemaMismatch(..)));
    let error = store
        .save(
            &account,
            policy("prod", "1.0.0"),
            "  ",
            "anonymous",
            Utc::now(),
        )
        .await
        .expect_err("anonymous editor");
    assert!(matches!(error, LifecycleError::InvalidPolicy(..)));
    assert!(store
        .history(&account, "prod")
        .await
        .expect("history")
        .is_empty());
    let error = store
        .activate(&account, "prod", 7, None, "bob", "ghost", Utc::now())
        .await
        .expect_err("no such revision");
    assert!(matches!(error, LifecycleError::RevisionNotFound(..)));
}

/// Ten activations that all believe nothing is active: one wins, nine are
/// told the head that beat them, and the history is intact.
#[tokio::test]
async fn ten_concurrent_activations_have_exactly_one_winner() {
    let store = Arc::new(store!());
    let account = run_scope();
    for version in ["1.0.0", "2.0.0"] {
        store
            .save(
                &account,
                policy("prod", version),
                "alice",
                "draft",
                Utc::now(),
            )
            .await
            .expect("save");
    }
    let mut tasks = Vec::new();
    for index in 0..10u64 {
        let store = Arc::clone(&store);
        let account = account.clone();
        // Half of the contenders want revision 1, the other half revision 2:
        // all of them expect the same (absent) head.
        let sequence = 1 + index % 2;
        tasks.push(tokio::spawn(async move {
            store
                .activate(
                    &account,
                    "prod",
                    sequence,
                    None,
                    &format!("approver-{index}"),
                    "the race",
                    Utc::now(),
                )
                .await
                .map(|head| (head.sequence, head.activated_by))
        }));
    }
    let mut winners = Vec::new();
    let mut stale = 0;
    for task in tasks {
        match task.await.expect("no panic") {
            Ok(won) => winners.push(won),
            Err(LifecycleError::StaleHead(_, 0, current)) => {
                assert!(current == 1 || current == 2, "the loser names the winner");
                stale += 1;
            }
            Err(other) => panic!("the race answered {other}"),
        }
    }
    assert_eq!(winners.len(), 1, "one winner");
    assert_eq!(stale, 9, "nine typed losers");
    let head = store
        .head(&account, "prod")
        .await
        .expect("head")
        .expect("a head");
    assert_eq!((head.sequence, head.activated_by.clone()), winners[0]);
    assert_eq!(
        store
            .history(&account, "prod")
            .await
            .expect("history")
            .len(),
        2
    );
    let activations = store
        .events(&account, "prod")
        .await
        .expect("events")
        .into_iter()
        .filter(|event| event.kind == "activated")
        .count();
    assert_eq!(activations, 1, "only the winner left an event");
}

/// An activation that expects a head other than the real one is refused and
/// the head is not touched.
#[tokio::test]
async fn a_stale_expected_head_never_overwrites() {
    let store = store!();
    let account = run_scope();
    for version in ["1.0.0", "2.0.0", "3.0.0"] {
        store
            .save(
                &account,
                policy("prod", version),
                "alice",
                "draft",
                Utc::now(),
            )
            .await
            .expect("save");
    }
    store
        .activate(&account, "prod", 1, None, "bob", "go", Utc::now())
        .await
        .expect("first activation");
    store
        .activate(&account, "prod", 2, Some(1), "bob", "next", Utc::now())
        .await
        .expect("second activation");

    let error = store
        .activate(&account, "prod", 3, Some(1), "carol", "stale", Utc::now())
        .await
        .expect_err("head is 2, not 1");
    assert!(
        matches!(error, LifecycleError::StaleHead(_, 1, 2)),
        "{error}"
    );
    let error = store
        .activate(&account, "prod", 3, None, "carol", "stale", Utc::now())
        .await
        .expect_err("head is 2, not absent");
    assert!(
        matches!(error, LifecycleError::StaleHead(_, 0, 2)),
        "{error}"
    );
    let head = store
        .head(&account, "prod")
        .await
        .expect("head")
        .expect("a head");
    assert_eq!((head.sequence, head.activated_by.as_str()), (2, "bob"));
}

/// Ten concurrent saves of one set take ten distinct revisions.
#[tokio::test]
async fn ten_concurrent_saves_take_ten_distinct_revisions() {
    let store = Arc::new(store!());
    let account = run_scope();
    let mut tasks = Vec::new();
    for index in 0..10 {
        let store = Arc::clone(&store);
        let account = account.clone();
        tasks.push(tokio::spawn(async move {
            store
                .save(
                    &account,
                    policy("prod", "1.0.0"),
                    &format!("author-{index}"),
                    "a draft",
                    Utc::now(),
                )
                .await
                .map(|revision| revision.sequence)
        }));
    }
    let mut sequences = Vec::new();
    for task in tasks {
        sequences.push(task.await.expect("no panic").expect("every save lands"));
    }
    sequences.sort_unstable();
    assert_eq!(sequences, (1..=10).collect::<Vec<u64>>());
    assert_eq!(
        store
            .history(&account, "prod")
            .await
            .expect("history")
            .len(),
        10
    );
}

/// A rollback is a new revision under the same CAS: the old content is
/// copied, the head points to the copy, the history keeps every revision and
/// an invalidation event names what stopped deciding. A stale rollback loses.
#[tokio::test]
async fn a_rollback_is_a_new_revision_under_the_cas() {
    let store = store!();
    let account = run_scope();
    store
        .save(
            &account,
            policy("prod", "1.0.0"),
            "alice",
            "first",
            Utc::now(),
        )
        .await
        .expect("save 1");
    store
        .save(
            &account,
            policy("prod", "2.0.0"),
            "alice",
            "second",
            Utc::now(),
        )
        .await
        .expect("save 2");

    let never = store
        .rollback(
            &account,
            "prod",
            1,
            None,
            &Actors::new("bob", "carol"),
            "x",
            Utc::now(),
        )
        .await
        .expect_err("nothing was ever activated");
    assert!(matches!(never, LifecycleError::NeverActivated(..)));

    store
        .activate(&account, "prod", 2, None, "bob", "2 goes live", Utc::now())
        .await
        .expect("activate 2");

    let stale = store
        .rollback(
            &account,
            "prod",
            1,
            Some(1),
            &Actors::new("bob", "carol"),
            "x",
            Utc::now(),
        )
        .await
        .expect_err("head is 2");
    assert!(
        matches!(stale, LifecycleError::StaleHead(_, 1, 2)),
        "{stale}"
    );
    assert_eq!(
        store
            .history(&account, "prod")
            .await
            .expect("history")
            .len(),
        2
    );

    let head = store
        .rollback(
            &account,
            "prod",
            1,
            Some(2),
            &Actors::new("bob", "carol"),
            "2 broke staging",
            Utc::now(),
        )
        .await
        .expect("rollback");
    assert_eq!(head.sequence, 3);
    let revision = store
        .revision(&account, "prod", 3)
        .await
        .expect("revision")
        .expect("3 exists");
    assert!(revision.is_rollback());
    assert_eq!(revision.policy.version, "1.0.0-rb3");
    let history = store.history(&account, "prod").await.expect("history");
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].policy.version, "1.0.0", "the old one is intact");

    let events = store.events(&account, "prod").await.expect("events");
    let kinds: Vec<_> = events.iter().map(|event| event.kind.as_str()).collect();
    assert_eq!(
        kinds,
        vec![
            "revision_saved",
            "revision_saved",
            "activated",
            "revision_saved",
            "rolled_back",
            "invalidated"
        ]
    );
    let invalidated = events.last().expect("events");
    assert_eq!((invalidated.sequence, invalidated.new_head), (2, 3));
}

/// The separation of editor and approver is enforced and audited.
#[tokio::test]
async fn the_separation_of_editor_and_approver_is_enforced_and_audited() {
    let Some(store) = open(SeparationPolicy::required()).await else {
        return;
    };
    let account = run_scope();
    store
        .save(
            &account,
            policy("prod", "1.0.0"),
            "alice",
            "first",
            Utc::now(),
        )
        .await
        .expect("save");
    let error = store
        .activate(&account, "prod", 1, None, "alice", "self", Utc::now())
        .await
        .expect_err("alice cannot approve her own revision");
    assert!(matches!(error, LifecycleError::InvalidPolicy(..)));
    assert!(store.head(&account, "prod").await.expect("head").is_none());
    let head = store
        .activate(&account, "prod", 1, None, "bob", "ok", Utc::now())
        .await
        .expect("bob may");
    assert_eq!(
        (head.editor.as_str(), head.activated_by.as_str()),
        ("alice", "bob")
    );
}

/// An account never sees, nor can it move, the sets of another account, even
/// when both use the same policy identifier.
#[tokio::test]
async fn accounts_are_isolated_from_each_other() {
    let store = store!();
    let alpha = run_scope();
    let beta = run_scope();
    store
        .save(
            &alpha,
            policy("prod", "1.0.0"),
            "alice",
            "alpha",
            Utc::now(),
        )
        .await
        .expect("alpha save");
    store
        .activate(&alpha, "prod", 1, None, "bob", "alpha live", Utc::now())
        .await
        .expect("alpha activate");

    assert!(store
        .history(&beta, "prod")
        .await
        .expect("history")
        .is_empty());
    assert!(store.head(&beta, "prod").await.expect("head").is_none());
    assert!(store
        .events(&beta, "prod")
        .await
        .expect("events")
        .is_empty());
    assert!(store
        .revision(&beta, "prod", 1)
        .await
        .expect("revision")
        .is_none());
    let error = store
        .activate(&beta, "prod", 1, None, "bob", "intruder", Utc::now())
        .await
        .expect_err("beta has no such revision");
    assert!(matches!(error, LifecycleError::RevisionNotFound(..)));

    // Beta has its own independent sequence and head for the same id.
    let revision = store
        .save(
            &beta,
            policy("prod", "9.0.0"),
            "mallory",
            "beta",
            Utc::now(),
        )
        .await
        .expect("beta save");
    assert_eq!(revision.sequence, 1);
    store
        .activate(&beta, "prod", 1, None, "erin", "beta live", Utc::now())
        .await
        .expect("beta activates independently of alpha's head");
    let alpha_head = store
        .head(&alpha, "prod")
        .await
        .expect("head")
        .expect("alpha head");
    assert_eq!(alpha_head.activated_by, "bob");
    assert_eq!(
        store.history(&alpha, "prod").await.expect("history").len(),
        1
    );
    assert_eq!(
        store.history(&beta, "prod").await.expect("history")[0]
            .policy
            .version,
        "9.0.0"
    );
}

/// The shadow reads the published head and mutates nothing: the head, the
/// history and the events are exactly as before.
#[tokio::test]
async fn shadow_evaluation_does_not_mutate_the_store() {
    let store = store!();
    let account = run_scope();
    store
        .save(
            &account,
            policy("prod", "1.0.0"),
            "alice",
            "first",
            Utc::now(),
        )
        .await
        .expect("save");
    let published = store
        .activate(&account, "prod", 1, None, "bob", "live", Utc::now())
        .await
        .expect("activate");
    let active = store
        .revision(&account, "prod", published.sequence)
        .await
        .expect("revision")
        .expect("the head names a revision")
        .policy;
    let events_before = store.events(&account, "prod").await.expect("events").len();

    let report = shadow_evaluate(
        Some(&active),
        &policy("prod", "2.0.0"),
        &[],
        Coverage::new(12, 0),
        Utc::now(),
    )
    .expect("the candidate validates");
    assert!(!report.effects_executed);
    assert!(!report.snapshot_complete);
    let simulated = simulate(Some(&active), &policy("prod", "3.0.0"), &[]);
    assert_eq!(simulated.changed_count(), 0);

    let head = store
        .head(&account, "prod")
        .await
        .expect("head")
        .expect("a head");
    assert_eq!(head.digest, published.digest);
    assert_eq!(
        store
            .history(&account, "prod")
            .await
            .expect("history")
            .len(),
        1
    );
    assert_eq!(
        store.events(&account, "prod").await.expect("events").len(),
        events_before
    );
}
