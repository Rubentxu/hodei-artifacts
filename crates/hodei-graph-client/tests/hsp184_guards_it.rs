//! HSP-184 integration tests against a REAL local Dgraph server.
//!
//! Environment-gated like `hsp183_identity_it.rs`: without
//! `HODEI_GRAPH_IT_ENDPOINT` the suite skips itself (exit 0), so ordinary
//! hermetic CI stays green. With the variables set (1 Zero + 1 Alpha with
//! TLS and ACL) the tests prove the two acceptance criteria with REAL
//! concurrent races — two clients, two separate connections, spawned in
//! parallel:
//!
//! - **AC#1**: two clients cannot confirm the same tag revision (the
//!   loser reports the CAS rejection with the current revision); ten
//!   concurrent writers of one scope serialize through the COMMON guard
//!   (one guard node, ten distinct revisions, ten effects exactly once);
//!   and two clients ensuring the same identity converge on one node.
//! - **AC#2**: a lost ACK (the operation retried with the same
//!   operation id) replays the already-committed result and does NOT
//!   duplicate the effect — the node counts after the retry are
//!   unchanged, the guard revision is unchanged, and the replay wins
//!   over a stale CAS demand because the id decides first.
//!
//! The server state persists between runs, so every fixture uses
//! run-unique keys and no test drops data.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use hodei_graph_client::{
    CanonicalHrn, GraphClient, GraphClientConfig, GuardError, GuardKind, GuardScope, GuardStore,
    GuardedEffect, IdentityStore, OperationId, RevisionDemand, Tenant, guard_key, identity_key,
};

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
/// configured. Every call makes a NEW connection: the concurrent tests
/// need genuinely separate clients (channels), not clones of one.
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

/// Every gated test shares the one server, so the suite runs one test at
/// a time (the boot is idempotent, but serializing keeps the fixtures
/// independent).
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A run-unique suffix so fixtures never collide across runs (the server
/// persists; no test drops data).
fn nonce() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock must be set")
        .as_millis();
    format!("h184-{millis}")
}

/// Counts the nodes that carry `predicate = value`, straight from the
/// graph.
async fn count_nodes_by_predicate(client: &GraphClient, predicate: &str, value: &str) -> usize {
    let query = format!(
        "{{ q(func: eq({predicate}, \"{}\")) {{ uid }} }}",
        value.replace('"', "\\\"")
    );
    let response = client.execute_dql(&query).await.expect("the count query");
    response
        .json
        .get("q")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}

/// The typed effect marker node an effect creates (reuses existing
/// predicates; the IT needs no ontology beyond the declared one).
fn marker_effect(marker_name: &str, tenant: &str, result: String) -> GuardedEffect {
    GuardedEffect {
        query_lines: Vec::new(),
        set_nquads: format!(
            "_:m <dgraph.type> \"HodeiOwner\" .\n\
             _:m <hodei.identity.tenant> \"{tenant}\" .\n\
             _:m <hodei.identity.name> \"{marker_name}\" .\n"
        ),
        recorded_result: result,
    }
}

/// AC#1, first half: two clients demanding the SAME revision of a fresh
/// tag scope cannot both confirm. Exactly one commits the scope at
/// revision 1; the other is rejected with the current revision — and a
/// later caller demanding the same revision again is rejected too,
/// because the revision moved and nobody can re-take it.
#[tokio::test]
async fn ac1_two_clients_cannot_confirm_the_same_tag_revision() {
    let _serial = SERIAL.lock().await;
    let Some(client) = client_or_skip().await else {
        return;
    };
    // Two genuinely separate clients (two connections, two channels).
    let client_b = client_or_skip().await.expect("the second connection");
    let store_a = Arc::new(
        GuardStore::open(client.clone())
            .await
            .expect("the first guard store must open"),
    );
    let store_b = Arc::new(
        GuardStore::open(client_b.clone())
            .await
            .expect("the second guard store must open"),
    );
    let nonce = nonce();
    let tenant = Tenant::new("tenant-h184-tag").expect("valid tenant");
    let subject = format!("repo/tagged-{nonce}");
    let scope = GuardScope::new(GuardKind::Tag, &tenant, &subject).expect("valid scope");
    let key = guard_key(&scope);

    // Both writers demand revision 0 of the fresh scope, concurrently,
    // from two different clients.
    let scope_a = scope.clone();
    let scope_b = scope.clone();
    let store_a2 = store_a.clone();
    let op_a = OperationId::new(&format!("tag-take-{nonce}-a")).expect("valid id");
    let op_b = OperationId::new(&format!("tag-take-{nonce}-b")).expect("valid id");
    let result_a = format!("tag-a-committed-{nonce}");
    let result_b = format!("tag-b-committed-{nonce}");
    let effect_a = marker_effect(
        &format!("tag-marker-a-{nonce}"),
        "tenant-h184-tag",
        result_a,
    );
    let effect_b = marker_effect(
        &format!("tag-marker-b-{nonce}"),
        "tenant-h184-tag",
        result_b,
    );
    let task_a = tokio::spawn(async move {
        store_a2
            .execute_guarded(&scope_a, &op_a, RevisionDemand::At(0), effect_a)
            .await
    });
    let task_b = tokio::spawn(async move {
        store_b
            .execute_guarded(&scope_b, &op_b, RevisionDemand::At(0), effect_b)
            .await
    });
    let outcome_a = task_a.await.expect("the first task must not panic");
    let outcome_b = task_b.await.expect("the second task must not panic");

    // Exactly one confirmation at revision 1, one typed CAS rejection.
    let (committed, rejected) = match (outcome_a, outcome_b) {
        (Ok(a), Err(b)) => (a, b),
        (Err(a), Ok(b)) => (b, a),
        (Ok(a), Ok(b)) => panic!("two clients confirmed the same revision: {a:?} / {b:?}"),
        (Err(a), Err(b)) => panic!("no client confirmed the tag: {a} / {b}"),
    };
    assert!(
        matches!(
            &rejected,
            GuardError::CasRejected {
                expected_revision: 0,
                current_revision: 1,
                ..
            }
        ),
        "the loser must report the CAS rejection with the current revision: {rejected}"
    );
    assert!(!committed.replayed);
    assert_eq!(
        committed.guard_revision, 1,
        "the winner confirmed revision 1"
    );
    eprintln!("ac1: winner {committed:?}; loser rejected: {rejected}");

    // Exactly one guard node for the scope, from the graph.
    assert_eq!(
        count_nodes_by_predicate(&client, "hodei.guard.key", &key).await,
        1,
        "the scope must have exactly one guard node"
    );
    // Exactly one of the two effects landed.
    let markers_a = count_nodes_by_predicate(
        &client,
        "hodei.identity.name",
        &format!("tag-marker-a-{nonce}"),
    )
    .await;
    let markers_b = count_nodes_by_predicate(
        &client,
        "hodei.identity.name",
        &format!("tag-marker-b-{nonce}"),
    )
    .await;
    assert_eq!(
        markers_a + markers_b,
        1,
        "exactly one effect was applied: the loser's write must not commit"
    );
    let markers_op_a = count_nodes_by_predicate(
        &client,
        "hodei.operation.id",
        &format!("tag-take-{nonce}-a"),
    )
    .await;
    let markers_op_b = count_nodes_by_predicate(
        &client,
        "hodei.operation.id",
        &format!("tag-take-{nonce}-b"),
    )
    .await;
    assert_eq!(
        markers_op_a + markers_op_b,
        1,
        "exactly one operation marker was committed"
    );

    // And nobody can re-take revision 0 afterwards: the revision moved.
    let late = OperationId::new(&format!("tag-take-{nonce}-late")).expect("valid id");
    let late_error = store_a
        .execute_guarded(
            &scope,
            &late,
            RevisionDemand::At(0),
            marker_effect("unused-late", "tenant-h184-tag", "unused".to_owned()),
        )
        .await
        .expect_err("revision 0 is taken forever");
    assert!(
        matches!(
            late_error,
            GuardError::CasRejected {
                expected_revision: 0,
                ..
            }
        ),
        "unexpected: {late_error}"
    );
    eprintln!("ac1: the taken revision cannot be re-taken by a late client");
}

/// AC#1, second half: ten concurrent writers of one reference scope —
/// across two separate clients — serialize through the COMMON guard.
/// All ten commit, the guard advances exactly ten revisions on ONE
/// guard node, every operation is recorded once, and every effect lands
/// exactly once. No lost updates, no duplicated effects, no invented
/// per-caller guards.
#[tokio::test]
async fn ac1_ten_concurrent_writers_serialize_through_the_common_guard() {
    let _serial = SERIAL.lock().await;
    let Some(client) = client_or_skip().await else {
        return;
    };
    let client_b = client_or_skip().await.expect("the second connection");
    let store_a = Arc::new(
        GuardStore::open(client.clone())
            .await
            .expect("the first guard store must open"),
    );
    let store_b = Arc::new(
        GuardStore::open(client_b.clone())
            .await
            .expect("the second guard store must open"),
    );
    let nonce = nonce();
    let tenant = Tenant::new("tenant-h184-ref").expect("valid tenant");
    let subject = format!("dataset/referenced-{nonce}");
    let scope =
        Arc::new(GuardScope::new(GuardKind::Reference, &tenant, &subject).expect("valid scope"));
    let key = guard_key(&scope);

    // Ten writers, five per client, all spawned at once.
    let mut handles = Vec::new();
    for index in 0..10 {
        let store = if index % 2 == 0 {
            store_a.clone()
        } else {
            store_b.clone()
        };
        let scope = scope.clone();
        let operation = OperationId::new(&format!("ref-writer-{nonce}-{index}")).expect("valid id");
        let effect = marker_effect(
            &format!("ref-marker-{nonce}-{index}"),
            "tenant-h184-ref",
            format!("reference-registered-{nonce}-{index}"),
        );
        handles.push(tokio::spawn(async move {
            store
                .execute_guarded(&scope, &operation, RevisionDemand::Advance, effect)
                .await
                .expect("every writer must commit, serialized by the common guard")
        }));
    }
    let mut outcomes = Vec::new();
    for handle in handles {
        outcomes.push(handle.await.expect("the task must not panic"));
    }

    // All ten committed as fresh writes (the outcome's revision is the
    // read-back AFTER the commit: concurrent writers may read a later
    // revision, so the read-backs race by design — the invariant is the
    // final state below, not the interleaving).
    for outcome in &outcomes {
        assert!(!outcome.replayed, "each writer is its own operation");
        assert_eq!(
            outcome.scope, key,
            "every writer used the SAME common conflict key"
        );
        assert!(
            outcome.guard_revision >= 1,
            "every writer saw the guard already advanced"
        );
    }

    // One guard node, advanced exactly ten times, visible to both
    // clients.
    assert_eq!(
        count_nodes_by_predicate(&client, "hodei.guard.key", &key).await,
        1,
        "the scope must have exactly one guard node"
    );
    assert_eq!(
        store_a
            .guard_revision(&scope)
            .await
            .expect("read from client A"),
        10,
        "the common guard advanced exactly ten revisions"
    );
    assert_eq!(
        store_b
            .guard_revision(&scope)
            .await
            .expect("read from client B"),
        10,
        "both clients see the same guard state"
    );
    // The conflict anchor carries the final revision: ten distinct
    // sequential tokens were written, one per advance.
    let token_query = format!(
        "{{ q(func: eq(hodei.guard.key, \"{}\")) {{ hodei.guard.token }} }}",
        key.replace('"', "\\\"")
    );
    let token_response = client
        .execute_dql(&token_query)
        .await
        .expect("the token read");
    let token = token_response.json["q"][0]["hodei.guard.token"]
        .as_str()
        .expect("the token must be readable");
    assert_eq!(
        token,
        format!("{key}#10"),
        "the token must name the final revision the writers serialized to"
    );
    // Every operation recorded once, every effect applied once.
    for index in 0..10 {
        let operation = OperationId::new(&format!("ref-writer-{nonce}-{index}")).expect("valid id");
        let record = store_a
            .find_operation(&operation)
            .await
            .expect("the registry read")
            .expect("each writer's operation must be recorded");
        assert_eq!(record.scope, key);
        assert_eq!(
            count_nodes_by_predicate(
                &client,
                "hodei.identity.name",
                &format!("ref-marker-{nonce}-{index}")
            )
            .await,
            1,
            "each effect must be applied exactly once"
        );
    }
    eprintln!("ac1: 10 concurrent writers on one scope took revisions 1..10 with 1 guard node");
}

/// AC#2: a lost ACK does not duplicate the effect. The operation
/// commits, the caller "never hears about it" (the ACK is lost) and
/// retries with the SAME operation id: the registry replays the
/// already-committed result, the guard revision does not move, the
/// first effect stays applied exactly once, and a DIFFERENT effect
/// payload under the same id is NOT applied. The replay also wins over
/// a stale CAS demand: the id decides before the demand.
#[tokio::test]
async fn ac2_lost_ack_replays_the_committed_result_without_duplicating_the_effect() {
    let _serial = SERIAL.lock().await;
    let Some(client) = client_or_skip().await else {
        return;
    };
    let store = Arc::new(
        GuardStore::open(client.clone())
            .await
            .expect("the guard store must open"),
    );
    let nonce = nonce();
    let tenant = Tenant::new("tenant-h184-quota").expect("valid tenant");
    let scope = GuardScope::new(GuardKind::Quota, &tenant, &format!("bucket/{nonce}"))
        .expect("valid scope");
    let operation = OperationId::new(&format!("quota-reserve-{nonce}")).expect("valid id");

    // The original commit.
    let first = store
        .execute_guarded(
            &scope,
            &operation,
            RevisionDemand::Advance,
            marker_effect(
                &format!("quota-marker-1-{nonce}"),
                "tenant-h184-quota",
                format!("quota-reserved-{nonce}"),
            ),
        )
        .await
        .expect("the original operation must commit");
    assert!(!first.replayed);
    let committed_revision = first.guard_revision;
    let committed_result = first.recorded_result.clone();
    assert_eq!(committed_result, format!("quota-reserved-{nonce}"));
    assert_eq!(
        committed_revision, 1,
        "a fresh quota scope starts at revision 1"
    );

    // The retry after the lost ACK: SAME operation id, a DIFFERENT
    // effect payload (as if the caller re-derived it), and even a
    // stale CAS demand. The registry must replay the committed answer
    // and apply nothing.
    let retry = store
        .execute_guarded(
            &scope,
            &operation,
            RevisionDemand::At(0),
            marker_effect(
                &format!("quota-marker-2-{nonce}"),
                "tenant-h184-quota",
                format!("quota-reserved-again-{nonce}"),
            ),
        )
        .await
        .expect("the retried operation must replay, not fail");
    assert!(
        retry.replayed,
        "the retried id must be recognized as already committed"
    );
    assert_eq!(
        retry.recorded_result, committed_result,
        "the replay returns the ALREADY-COMMITTED result, not the new payload"
    );
    assert_eq!(
        retry.guard_revision, committed_revision,
        "a replay does not move the guard"
    );

    // The effect counts prove the once-only contract, from the graph:
    // the first marker exists once, the second (the retried payload)
    // does not exist at all.
    assert_eq!(
        count_nodes_by_predicate(
            &client,
            "hodei.identity.name",
            &format!("quota-marker-1-{nonce}")
        )
        .await,
        1,
        "the original effect must be applied exactly once"
    );
    assert_eq!(
        count_nodes_by_predicate(
            &client,
            "hodei.identity.name",
            &format!("quota-marker-2-{nonce}")
        )
        .await,
        0,
        "the retried effect must NOT be applied again"
    );
    assert_eq!(
        count_nodes_by_predicate(
            &client,
            "hodei.operation.id",
            &format!("quota-reserve-{nonce}")
        )
        .await,
        1,
        "exactly one operation marker for the retried id"
    );
    // The guard revision from the graph agrees: unchanged by the retry.
    assert_eq!(
        store.guard_revision(&scope).await.expect("the guard read"),
        committed_revision,
        "the retry must not advance the guard"
    );
    eprintln!(
        "ac2: the retried id replayed '{committed_result}' with the guard at revision \
         {committed_revision}: no duplicated effect"
    );
}

/// AC#1, third half: two clients ensuring the SAME identity (same
/// tenant, same HRN) converge on exactly one node with the same
/// deterministic key and digest. Same contract as HSP-183, now with two
/// genuinely separate connections racing in parallel.
#[tokio::test]
async fn ac1_two_clients_ensuring_the_same_identity_converge() {
    let _serial = SERIAL.lock().await;
    let Some(client) = client_or_skip().await else {
        return;
    };
    let client_b = client_or_skip().await.expect("the second connection");
    let store_a = Arc::new(
        IdentityStore::open(client.clone())
            .await
            .expect("the first identity store must open"),
    );
    let store_b = Arc::new(
        IdentityStore::open(client_b.clone())
            .await
            .expect("the second identity store must open"),
    );
    let nonce = nonce();
    let tenant = Tenant::new("tenant-h184-id").expect("valid tenant");
    let hrn = CanonicalHrn::new(&format!(
        "hrn:hodei:identity:local:tenant-h184-id:account/converge-{nonce}"
    ))
    .expect("valid HRN");
    let expected_key = identity_key(&tenant, &hrn);

    let tenant_a = tenant.clone();
    let hrn_a = hrn.clone();
    let task_a = tokio::spawn(async move {
        store_a
            .ensure_identity(&tenant_a, &hrn_a, "converging")
            .await
            .expect("the first creator must converge, not fail")
    });
    let task_b = tokio::spawn(async move {
        store_b
            .ensure_identity(&tenant, &hrn, "converging")
            .await
            .expect("the second creator must converge, not fail")
    });
    let record_a = task_a.await.expect("the task must not panic");
    let record_b = task_b.await.expect("the task must not panic");

    assert_eq!(record_a.key, expected_key);
    assert_eq!(
        record_a.key, record_b.key,
        "both clients get the same identity"
    );
    assert_eq!(
        record_a.digest, record_b.digest,
        "both clients get the same deterministic digest"
    );
    assert_eq!(
        count_nodes_by_predicate(&client, "hodei.identity.key", &expected_key).await,
        1,
        "two concurrent creators must produce exactly one identity node"
    );
    eprintln!("ac1: two clients converged on identity {expected_key}");
}
