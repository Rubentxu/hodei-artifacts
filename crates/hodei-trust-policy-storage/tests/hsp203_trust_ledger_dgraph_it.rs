//! HSP-203: the versioned trust ledger over persistent Dgraph — strict
//! read-on-access, no stale reads, the event rides the change, and the
//! activation guard decides the race.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT like every IT of the stack. What it
//! proves, against the real engine:
//!
//! - **Round trip**: ALLOW/DENY/INDETERMINATE decisions round-trip
//!   through the Dgraph store, and the grant answers — active, expired,
//!   legacy — are the expected ones for the same facts (AC1).
//! - **Revocation confirmed before a new read prevents the pull even
//!   through another surface**: a brand-new connection reads no grant
//!   after the revoke commit is visible (AC1).
//! - **The stale race and a broken authority never concede**: two reads
//!   with a revoke between them answer granted then nothing, and a
//!   ledger that cannot open on a dead authority is a typed closed
//!   failure, never a permission (AC2). The grant and revoke events ride
//!   the same transaction as the change they record (AC2).
//! - **Concurrent activation of different revisions**: one wins, the
//!   other takes note — the HSP-149 CAS pattern ported to the graph
//!   guards (AC3).

use std::sync::Arc;
use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use hodei_catalog::{TrustGrant, TrustLedger};
use hodei_evidence::ContentDigest;
use hodei_graph_client::{GraphClient, GraphClientConfig, Tenant};
use hodei_trust_policy::ports::DecisionStore;
use hodei_trust_policy::promotion::{DecisionId, PromotionDecision};
use hodei_trust_policy_storage::dgraph::{DgraphDecisionStore, DgraphTrustError, DgraphTrustStore};
use serde_json::{json, Value};

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

/// The ledger over the live authority, with its client so the test can
/// read the events through the same graph.
async fn open_store() -> Option<(Arc<GraphClient>, DgraphTrustStore)> {
    let client = fresh_client().await?;
    let tenant = Tenant::new("hodei").expect("the platform tenant");
    let store = DgraphTrustStore::open(client.clone(), tenant)
        .await
        .expect("the trust ledger opens on the validated schema");
    eprintln!(
        "trust ledger boot: decision={:?} binary_version={} applied={}",
        store.boot_report().decision,
        store.boot_report().binary_version,
        store.boot_report().applied,
    );
    Some((client, store))
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

fn a_grant(
    account: &str,
    repository: &str,
    content: &[u8],
    decision: &str,
    at: chrono::DateTime<Utc>,
) -> TrustGrant {
    TrustGrant {
        account: account.to_owned(),
        repository: repository.to_owned(),
        digest: ContentDigest::of_bytes(content),
        decision_id: decision.to_owned(),
        actor: "hrn:hodei:iam:acme:User/alice".to_owned(),
        granted_at: at,
        revoked_at: None,
        revoked_by: None,
        revoke_reason: None,
        subject: Some("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app".to_owned()),
        policy_version: Some("1.0.0".to_owned()),
        capsule_hash: Some("sha256:cap".to_owned()),
        risk_version: Some(7),
        valid_until: Some(at + ChronoDuration::hours(1)),
    }
}

/// A promotion decision of one outcome, rebuilt from the same shape the
/// engine writes: the equivalence is of the PERSISTED decision, not of a
/// fixture. The account and the subject are the caller's, so the runs of
/// the shared authority never read each other's history.
fn a_decision(id: &str, outcome: &str, account: &str, subject: &str) -> PromotionDecision {
    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let outcome = json!(outcome);
    serde_json::from_value(json!({
        "id": id,
        "request": {
            "correlation_id": format!("corr-{id}"),
            "principal": "hrn:hodei:iam:acme:User/alice",
            "account": account,
            "subject": subject,
            "distribution": "trusted",
            "requested_at": now,
        },
        "outcome": outcome,
        "decided_by": "neither",
        "authorization": "Permit",
        "trust": {
            "outcome": outcome,
            "reasons": [],
            "requirements": [],
            "context_version": "1.0.0",
            "policy_id": "release-policy",
            "policy_version": "1.0.0",
            "schema_version": "1.0.0",
            "capsule_hash": null,
            "evidence_ids": [],
            "risk_version": 0,
            "decided_at": now,
        },
            "context": {
            "version": "1.0.0",
            "subject": {
                "hrn": subject,
                "digest": null,
            },
            "target": "production",
            "dimensions": [],
            "capsule_hash": null,
            "risk": {
                "critical_known": 0,
                "critical_reachable": 0,
                "exposure": "INTERNAL",
                "vex_accepted": 0,
                "established": true,
                "risk_version": 0,
            },
            "exceptions": [],
            "evidence_ids": [],
            "assembled_at": now,
        },
        "reasons": [],
        "applied": true,
        "decided_at": now,
    }))
    .expect("the decision of the fixture parses")
}

/// The decision as it reads on the wire: the comparison two stores must
/// pass.
fn decision_wire(decision: &PromotionDecision) -> Value {
    serde_json::to_value(decision).expect("a decision serializes")
}

/// AC1 (decisions): ALLOW, DENY and INDETERMINATE round-trip through the
/// Dgraph port.
#[tokio::test]
async fn allow_deny_indeterminate_round_trip_through_the_graph_store() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let graph = DgraphDecisionStore::open(client.clone())
        .await
        .expect("the decision store opens");
    let scope = run_scope();
    let account = &scope;
    let subject = format!("hrn:hodei:artifacts:eu-west-1:{scope}:Artifact/web-1.0.0");
    // The ALLOW decision is built once: the rewrite below reuses it, so
    // what the store keeps after a repeated write is what was written.
    let allow_decision = a_decision(&format!("{scope}-ALLOW"), "ALLOW", account, &subject);
    for outcome in ["ALLOW", "DENY", "INDETERMINATE"] {
        let id = DecisionId::new(format!("{scope}-{outcome}"));
        let decision = if outcome == "ALLOW" {
            allow_decision.clone()
        } else {
            a_decision(&format!("{scope}-{outcome}"), outcome, account, &subject)
        };
        graph
            .put(&decision)
            .await
            .expect("the graph writes the decision");

        let from_graph = graph
            .get(&id)
            .await
            .expect("the graph reads")
            .expect("the graph kept the decision");
        assert_eq!(
            decision_wire(&decision),
            decision_wire(&from_graph),
            "what was written is what is read ({outcome})"
        );
    }

    // A decision is written once: writing it twice leaves one decision,
    // the one that was written first.
    let id = DecisionId::new(format!("{scope}-ALLOW"));
    graph
        .put(&allow_decision)
        .await
        .expect("the rewrite answers");
    let again = graph.get(&id).await.expect("the read answers");
    assert_eq!(
        decision_wire(&again.expect("the decision stays")),
        decision_wire(&allow_decision),
        "the rewrite did not change the decision"
    );

    // The decisions of a subject and of a correlation are findable.
    let listed = graph
        .list_for_subject(account, &subject)
        .await
        .expect("the subject reads");
    assert_eq!(listed.len(), 3, "the three decisions of the subject");
    let by_correlation = graph
        .find_by_correlation(&format!("corr-{scope}-DENY"))
        .await
        .expect("the correlation reads")
        .expect("the correlation names its decision");
    assert_eq!(by_correlation.outcome.as_str(), "DENY");
}

/// AC1 (grants): the answers of the Dgraph ledger — active, expired,
/// legacy — are the expected ones for the facts, at the given hours.
#[tokio::test]
async fn grant_validity_answers_follow_the_facts() {
    let Some(_client) = fresh_client().await else {
        return;
    };
    let Some((_client, graph)) = open_store().await else {
        return;
    };
    let scope = run_scope();
    let account = &scope;
    let now = Utc::now();

    // The matrix: an active grant, an expired one, a legacy one without
    // declared validity, and one of another digest that inherits nothing.
    let grants = vec![
        a_grant(account, "raw", b"active", "d-active", now),
        a_grant(
            account,
            "raw",
            b"expired",
            "d-expired",
            now - ChronoDuration::hours(2),
        ),
    ];
    let mut legacy = a_grant(account, "raw", b"legacy", "d-legacy", now);
    legacy.valid_until = None;
    legacy.policy_version = None;
    let all: Vec<TrustGrant> = grants.into_iter().chain([legacy.clone()]).collect();
    graph.grant(&all).await.expect("the graph grants");

    for content in ["active", "expired", "legacy", "never-granted"] {
        let digest = ContentDigest::of_bytes(content.as_bytes());
        let from_graph = graph
            .active_grant(account, "raw", &digest, now)
            .await
            .expect("the graph answers");
        // Only a grant with a declared, still-open validity serves: the
        // expired one, the legacy one (no declared validity) and the
        // never-granted digest are all closed.
        let expected = content == "active";
        assert_eq!(
            from_graph.is_some(),
            expected,
            "the presence for {content} follows its validity"
        );
    }
    // The exact hours: an active grant inside its horizon, expired at the
    // deadline (valid_until is exclusive), gone after.
    let digest = ContentDigest::of_bytes(b"active");
    let inside = graph
        .active_grant(account, "raw", &digest, now + ChronoDuration::minutes(59))
        .await
        .expect("read")
        .expect("inside the horizon the grant serves");
    assert_eq!(inside.decision_id, "d-active");
    let at_deadline = graph
        .active_grant(account, "raw", &digest, now + ChronoDuration::hours(1))
        .await
        .expect("read");
    assert!(at_deadline.is_none(), "at the deadline the grant is over");

    // Repeating a grant neither duplicates nor revives.
    graph
        .grant(&[a_grant(account, "raw", b"active", "d-active", now)])
        .await
        .expect("the repeat answers");
    let listed = graph
        .grants_of(account, "raw")
        .await
        .expect("the ledger lists");
    let active_rows = listed
        .iter()
        .filter(|grant| grant.decision_id == "d-active")
        .count();
    assert_eq!(active_rows, 1, "the repeat wrote nothing");
}

/// AC1: a revocation whose commit is visible prevents the pull even
/// through another surface — a brand-new connection reads nothing.
#[tokio::test]
async fn a_confirmed_revoke_stops_the_next_read_through_a_new_surface() {
    let Some((client, graph)) = open_store().await else {
        return;
    };
    let scope = run_scope();
    let account = &scope;
    let now = Utc::now();
    let grant = a_grant(account, "raw", b"the bytes", "d-1", now);
    let digest = grant.digest.clone();
    graph.grant(&[grant]).await.expect("the grant commits");

    // The first surface reads the grant.
    let first = graph
        .active_grant(account, "raw", &digest, now)
        .await
        .expect("read")
        .expect("the grant serves");
    assert_eq!(first.decision_id, "d-1");

    // The revoke commits, and the ledger says what it moved.
    let revoked = graph
        .revoke_decision(
            account,
            "raw",
            "d-1",
            "hrn:hodei:iam:acme:User/root",
            "the CVE",
            now,
        )
        .await
        .expect("the revoke commits");
    assert_eq!(revoked, 1, "exactly the grant that was live");

    // The second surface: a brand-new connection to the same authority.
    let other_client = fresh_client().await.expect("the other surface opens");
    let other = DgraphTrustStore::open(other_client, Tenant::new("hodei").expect("tenant"))
        .await
        .expect("the other ledger opens");
    let second = other
        .active_grant(account, "raw", &digest, now)
        .await
        .expect("read");
    assert!(
        second.is_none(),
        "a revoked concession authorizes nothing after the revoke commit"
    );

    // The history keeps the revocation: the ledger does not forget.
    let history = other
        .grants_of(account, "raw")
        .await
        .expect("the history reads");
    assert_eq!(history.len(), 1, "the row stays as history");
    let row = &history[0];
    assert!(row.revoked_at.is_some(), "the row carries the revocation");
    assert_eq!(
        row.revoked_by.as_deref(),
        Some("hrn:hodei:iam:acme:User/root")
    );
    assert_eq!(row.revoke_reason.as_deref(), Some("the CVE"));

    // A second revocation of the same grant moves nothing.
    let again = graph
        .revoke_decision(account, "raw", "d-1", "root", "again", now)
        .await
        .expect("the second revoke answers");
    assert_eq!(again, 0, "a grant is not revoked twice");

    // The same holds for the digest-wide revocation.
    let second_grant = a_grant(account, "raw", b"the bytes", "d-2", now);
    graph
        .grant(&[second_grant])
        .await
        .expect("the re-grant commits");
    let by_digest = graph
        .revoke_digest(account, "raw", &digest, "root", "the CVE again", now)
        .await
        .expect("the digest revoke commits");
    assert_eq!(
        by_digest, 1,
        "the digest revocation took the new grant back"
    );
    let after = other
        .active_grant(account, "raw", &digest, now)
        .await
        .expect("read");
    assert!(after.is_none(), "the digest revocation ends every version");
    let _ = client;
}

/// AC2: the stale race and a broken authority never concede, and the
/// grant/revoke events ride the same transaction as the change.
#[tokio::test]
async fn the_stale_race_and_a_broken_authority_never_concede_and_the_events_travel_along() {
    let Some((client, graph)) = open_store().await else {
        return;
    };
    let scope = run_scope();
    let account = &scope;
    let now = Utc::now();
    let grant = a_grant(account, "raw", b"stale", "d-1", now);
    let digest = grant.digest.clone();
    graph.grant(&[grant]).await.expect("the grant commits");

    // The first read sees the grant.
    let first = graph
        .active_grant(account, "raw", &digest, now)
        .await
        .expect("the first read answers")
        .expect("the grant is active");
    assert_eq!(first.decision_id, "d-1");

    // The revoke commits between the two reads.
    let revoked = graph
        .revoke_decision(account, "raw", "d-1", "root", "race", now)
        .await
        .expect("the revoke commits");
    assert_eq!(revoked, 1, "the revoke is confirmed, not assumed");

    // The second read cannot answer with the state before the revoke:
    // the read-on-access profile has no cache to be stale.
    let second = graph
        .active_grant(account, "raw", &digest, now)
        .await
        .expect("the second read answers");
    assert!(
        second.is_none(),
        "a stale snapshot never becomes a concession"
    );

    // A ledger that cannot open on a dead authority is a typed closed
    // failure. It never answers with a permission it does not have.
    let dead = "endpoint = \"127.0.0.1:1\"\n\n[tls]\nenabled = false\n".to_owned();
    let dead_config =
        GraphClientConfig::from_toml_str(&dead).expect("the dead configuration parses");
    let tenant = Tenant::new("hodei").expect("tenant");
    match GraphClient::connect(dead_config).await {
        Err(_) => {
            // The connection itself is refused: fail-closed at the
            // boundary, before any ledger exists.
        }
        Ok(dead_client) => {
            let dead_store = DgraphTrustStore::open(Arc::new(dead_client), tenant).await;
            let error = match dead_store {
                Err(error) => error,
                Ok(_) => panic!("a dead authority opens no ledger"),
            };
            assert!(
                matches!(error, DgraphTrustError::Unavailable(_)),
                "the failure of the authority is Unavailable: {error}"
            );
        }
    }

    // The events rode the SAME transactions: the granted event of the
    // grant and the revoked event of the revoke exist as soon as the
    // change is visible, linked to the row they speak about.
    let grant_key = format!(
        "{account}|raw|sha256|{}|d-1",
        ContentDigest::of_bytes(b"stale").value
    );
    let events = client
        .execute_dql(&format!(
            "{{ q(func: eq(hodei.trust.event.account, {account})) {{
                hodei.trust.event.key
                hodei.trust.event.kind
                hodei.trust.event.decision_id
                hodei.trust.event.grant {{
                    hodei.trust.grant.key
                    hodei.trust.grant.revoked_at
                }}
            }} }}",
            account = hodei_graph_client::schema::dql_string(account),
        ))
        .await
        .expect("the events read")
        .json
        .get("q")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let kinds: Vec<(&str, &str)> = events
        .iter()
        .map(|event| {
            (
                event
                    .get("hodei.trust.event.kind")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                event
                    .get("hodei.trust.event.grant")
                    .and_then(Value::as_array)
                    .and_then(|grants| grants.first())
                    .and_then(|grant| grant.get("hodei.trust.grant.key"))
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            )
        })
        .collect();
    assert!(
        kinds.contains(&("granted", grant_key.as_str())),
        "the granted event is linked to its row: {kinds:?}"
    );
    assert!(
        kinds.contains(&("revoked", grant_key.as_str())),
        "the revoked event is linked to its row: {kinds:?}"
    );
}

/// AC3: two writers activating different revisions of one digest
/// aggregate — one wins, the other takes note.
#[tokio::test]
async fn two_writers_activating_different_revisions_one_wins_the_other_constata() {
    let Some((_client, store)) = open_store().await else {
        return;
    };
    let store = Arc::new(store);
    let scope = run_scope();
    let account = &scope;
    let base = Utc::now() - ChronoDuration::seconds(10);
    let first = a_grant(account, "raw", b"the digest", "d-1", base);
    let contender_one = a_grant(
        account,
        "raw",
        b"the digest",
        "d-2",
        base + ChronoDuration::seconds(5),
    );
    let contender_two = a_grant(
        account,
        "raw",
        b"the digest",
        "d-3",
        base + ChronoDuration::seconds(6),
    );
    let digest = first.digest.clone();

    // The first activation claims the fresh aggregate.
    let first_activation = store
        .activate_grant_expecting_revision(&first, 0)
        .await
        .expect("the first activation wins the fresh aggregate");
    assert_eq!(
        first_activation.revision, 1,
        "the aggregate is at version 1"
    );
    assert!(
        !first_activation.replayed,
        "the first activation is not a replay"
    );

    // Two writers activate different revisions at the same claimed
    // version. Exactly one lands; the other is told what the aggregate
    // actually reached.
    let one = store.clone();
    let two = store.clone();
    let grant_one = contender_one.clone();
    let grant_two = contender_two.clone();
    let (first, second) = tokio::join!(
        async move { one.activate_grant_expecting_revision(&grant_one, 1).await },
        async move { two.activate_grant_expecting_revision(&grant_two, 1).await },
    );
    // Exactly one lands; the other is told the CAS it lost and reads the
    // revision the winner left behind.
    let (winner, winner_decision, loser) = match (first, second) {
        (Ok(winner), Err(loser)) => (winner, "d-2", loser.to_string()),
        (Err(loser), Ok(winner)) => (winner, "d-3", loser.to_string()),
        (Ok(_), Ok(_)) => panic!("two writers cannot both confirm the same revision"),
        (Err(one), Err(two)) => panic!("neither writer landed: {one} / {two}"),
    };
    assert_eq!(winner.revision, 2, "the winner advanced the aggregate to 2");
    assert!(!winner.replayed, "the winner committed a new state");
    assert_eq!(winner.recorded_result, winner_decision);
    eprintln!("the loser constata: {loser}");
    assert!(
        loser.contains("lost the CAS check"),
        "the loser is told the CAS it lost: {loser}"
    );
    assert!(
        loser.contains("the scope is at revision 2"),
        "the loser constata the revision the aggregate reached: {loser}"
    );

    // The confirmation: the aggregate reads at the version the winner
    // left, and the active grant is the winner's decision — the newest
    // version of the digest is the one that serves.
    let revision = store
        .aggregate_revision(account, "raw", &digest)
        .await
        .expect("the aggregate revision reads");
    assert_eq!(revision, 2, "the loser reads the revision the winner left");
    let active = store
        .active_grant(account, "raw", &digest, Utc::now())
        .await
        .expect("the active grant reads")
        .expect("the aggregate serves its newest version");
    assert_eq!(
        active.decision_id, winner_decision,
        "the active grant is the decision that won the activation"
    );

    // Replaying the winner's activation is the same operation: it says
    // so instead of writing again.
    let winner_grant = match winner_decision {
        "d-2" => contender_one,
        _ => contender_two,
    };
    let replay = store
        .activate_grant_expecting_revision(&winner_grant, 2)
        .await
        .expect("the replay answers");
    assert!(
        replay.replayed,
        "the same activation answers as what it already was"
    );
    assert_eq!(replay.recorded_result, winner_decision);
}
