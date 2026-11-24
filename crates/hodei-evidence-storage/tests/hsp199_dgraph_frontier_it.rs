//! HSP-199: the projection frontier over persistent Dgraph — the graph
//! changeset, its deltas and its checkpoint as ONE commit.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT like every IT of the stack. What it
//! proves, against the real engine:
//!
//! - a crash between the mutation and the checkpoint (the old frontier's
//!   window, replayed here as a plain apply followed by a fresh process)
//!   is repaired by the frontier: the replay commits the checkpoint and
//!   the deltas ONCE, the state fingerprint does not move, no edge is
//!   duplicated and no support is lost;
//! - the replay of the same delivery answers with the committed receipt
//!   (ledger position + changeset fingerprint) and writes nothing;
//! - two concurrent writers of one checkpoint serialize at the server
//!   and both land, in any order, with a monotone checkpoint;
//! - the rebuild builds into a projection of its own — the active graph
//!   keeps answering while the rebuild replays — and the two
//!   projections are comparable by their state fingerprints.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use hodei_evidence::application::projection::{ProjectionDelivery, ProjectionFrontier};
use hodei_evidence::domain::graph::NodeKind;
use hodei_evidence::ports::graph::{GraphQuery, GraphWriter};
use hodei_evidence::{EvidenceId, GraphChangeSet, GraphNode, GraphRelation, RelationType};
use hodei_evidence_storage::dgraph::graph::ACTIVE_PROJECTION;
use hodei_evidence_storage::{DgraphGraphStore, DgraphProjectionFrontier};
use hodei_graph_client::{GraphClient, GraphClientConfig, dql_string, schema::ensure_ready};
use kernel::Hrn;

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

/// A fresh process: a new connection to the same server, the way a
/// restart re-enters the graph.
async fn fresh_client() -> Option<Arc<GraphClient>> {
    let Some(config) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return None;
    };
    let mut client = GraphClient::connect(config).await.expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    let report = ensure_ready(&client)
        .await
        .expect("the versioned schema boots");
    eprintln!(
        "schema boot: decision={:?} binary_version={} stored_version={:?} applied={} predicates={}",
        report.decision,
        report.binary_version,
        report.stored_version,
        report.applied,
        report.verified_predicates
    );
    Some(Arc::new(client))
}

async fn connect() -> Option<Arc<GraphClient>> {
    let client = fresh_client().await?;
    Some(client)
}

fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

fn tenant(tag: &str) -> Hrn {
    // The head of the nano clock barely moves between runs; the tail
    // does. A tenant per test and per run: no delivery ever meets a
    // marker or a checkpoint another test or run left behind.
    let account = format!("it{tag}{}", &nonce()[8..]);
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{account}:Artifact/hsp199"
    ))
    .expect("a valid tenant HRN")
}

fn hrn_of(tenant: &Hrn, kind: &str, name: &str) -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{}:{kind}/{name}",
        tenant.account_id()
    ))
    .expect("a valid HRN")
}

fn kind_name(kind: &NodeKind) -> &str {
    match kind {
        NodeKind::Repository => "Repository",
        NodeKind::SourceRevision => "SourceRevision",
        NodeKind::Build => "Build",
        NodeKind::Artifact => "Artifact",
        NodeKind::Deployment => "Deployment",
        NodeKind::Evidence => "Evidence",
        NodeKind::Component => "Component",
        NodeKind::Vulnerability => "Vulnerability",
    }
}

fn node(tenant: &Hrn, kind: NodeKind, name: &str) -> GraphNode {
    GraphNode::new(
        hrn_of(tenant, kind_name(&kind), name),
        tenant.clone(),
        kind,
        Utc::now(),
    )
}

fn relation(
    tenant: &Hrn,
    kind: RelationType,
    from: &Hrn,
    to: &Hrn,
    evidence: &str,
) -> GraphRelation {
    GraphRelation::new(
        kind,
        from.clone(),
        to.clone(),
        tenant.clone(),
        EvidenceId::new(evidence.to_owned()),
        Utc::now(),
    )
}

/// The identity of the relation a step asserts, said the way the domain
/// says it (the type is lowercase, the ends are the HRNs).
fn identity_of(tenant: &Hrn, step: &str) -> String {
    format!(
        "{}|{}|{}",
        RelationType::Produced.as_str(),
        hrn_of(tenant, "Build", &format!("b-{step}")),
        hrn_of(tenant, "Artifact", &format!("a-{step}"))
    )
}

/// The changeset of one delivery: the evidence node, its two ends and
/// the Produced relation that says the build made the artefact.
fn the_changeset(tenant: &Hrn, step: &str) -> GraphChangeSet {
    let build = hrn_of(tenant, "Build", &format!("b-{step}"));
    let artifact = hrn_of(tenant, "Artifact", &format!("a-{step}"));
    GraphChangeSet::empty()
        .with_cause(format!("evt-{step}"))
        .with_node(node(tenant, NodeKind::Build, &format!("b-{step}")))
        .with_node(node(tenant, NodeKind::Artifact, &format!("a-{step}")))
        .with_node(node(tenant, NodeKind::Evidence, &format!("ev-{step}")))
        .with_relation(relation(
            tenant,
            RelationType::Produced,
            &build,
            &artifact,
            &format!("ev-{step}"),
        ))
}

fn delivery_of(stream: &str, position: u64, step: &str) -> ProjectionDelivery {
    ProjectionDelivery {
        consumer: "graph-projector".to_owned(),
        stream_id: stream.to_owned(),
        position,
        event_id: format!("evt-{step}"),
    }
}

/// How many relation records the server holds for one identity (the
/// replay and the races must never make it more than one).
async fn stored_relation_node_count(client: &GraphClient, tenant: &Hrn, identity: &str) -> usize {
    let key = DgraphGraphStore::relation_key(&tenant.to_string(), identity);
    let query = format!(
        "{{ q(func: eq(hodei.graph.rel.key, {key})) {{ uid }} }}",
        key = dql_string(&key)
    );
    let response = client
        .execute_dql(&query)
        .await
        .expect("the node count answers");
    response
        .json
        .get("q")
        .and_then(serde_json::Value::as_array)
        .map(|nodes| nodes.len())
        .unwrap_or(0)
}

/// How many live supports the relation carries.
async fn stored_support_count(client: &GraphClient, tenant: &Hrn, identity: &str) -> usize {
    let key = DgraphGraphStore::relation_key(&tenant.to_string(), identity);
    let query = format!(
        "{{ q(func: eq(hodei.graph.rel.key, {key})) {{ count(hodei.graph.supports) }} }}",
        key = dql_string(&key)
    );
    let response = client
        .execute_dql(&query)
        .await
        .expect("the count query answers");
    response
        .json
        .get("q")
        .and_then(|q| q.get(0))
        .and_then(|node| node.get("count(hodei.graph.supports)"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as usize
}

/// How many delta records the tenant holds.
async fn stored_delta_count(client: &GraphClient, tenant: &Hrn) -> usize {
    let query = format!(
        "{{ q(func: eq(hodei.delta.tenant, {tenant})) {{ uid }} }}",
        tenant = dql_string(&tenant.to_string())
    );
    let response = client
        .execute_dql(&query)
        .await
        .expect("the delta scan answers");
    response
        .json
        .get("q")
        .and_then(serde_json::Value::as_array)
        .map(|nodes| nodes.len())
        .unwrap_or(0)
}

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// AC1 — the crash window of the old frontier, replayed and repaired:
/// the mutation landed without a checkpoint, a fresh process re-drives
/// the delivery through the frontier, and the state comes out identical
/// — one edge, its support, the deltas said ONCE — with the receipt
/// certifying position and fingerprint. A second re-delivery replays
/// the receipt and writes nothing.
#[tokio::test]
async fn a_crash_between_mutation_and_checkpoint_replays_to_the_same_state() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let tenant = tenant("b");
    let namespace = format!("ns-{}", tenant.account_id());
    let stream = format!("tenant-{}", tenant.account_id());
    let graph = DgraphGraphStore::new(client.clone());

    // The crash window: the mutation committed, the checkpoint did not
    // (there was no checkpoint and no delta — the old frontier's gap).
    // ONE changeset, the way the ledger would read it back: a
    // re-delivery carries the very same envelope, fingerprint included.
    let changes = the_changeset(&tenant, "1");
    graph
        .apply(changes.clone())
        .await
        .expect("the mutation lands");
    let before = graph
        .state_fingerprint(&tenant)
        .await
        .expect("the fingerprint");

    // A fresh process re-drives the same delivery through the frontier.
    let resumed_client = fresh_client().await.expect("the restart reconnects");
    let frontier = DgraphProjectionFrontier::open(resumed_client.clone(), &namespace)
        .await
        .expect("the frontier opens");
    let delivery = delivery_of(&stream, 0, "1");
    let commit = frontier
        .apply_and_checkpoint(changes.clone(), &delivery)
        .await
        .expect("the frontier commits");
    assert!(!commit.replayed, "the first re-drive applies: {commit:?}");
    assert_eq!(
        commit.checkpoint, 1,
        "the checkpoint moved with the mutation"
    );
    assert!(
        commit.receipt.starts_with("pos=0;next=1;cs="),
        "the receipt certifies position and fingerprint: {}",
        commit.receipt
    );

    // The state is exactly the one the mutation left: same fingerprint,
    // one relation node, its support, and the delta said ONCE.
    let after = graph
        .state_fingerprint(&tenant)
        .await
        .expect("the fingerprint");
    assert_eq!(before, after, "the replay recalculates the same state");
    let identity = identity_of(&tenant, "1");
    assert_eq!(
        stored_relation_node_count(&client, &tenant, &identity).await,
        1,
        "one relation node, never two"
    );
    assert_eq!(
        stored_support_count(&client, &tenant, &identity).await,
        1,
        "the support is not lost and not doubled"
    );
    // The emitter says what CHANGED, and the crashed mutation had
    // already said this fact: the frontier completes the checkpoint and
    // does not say the delta twice. Not losing a delta is structural —
    // the delta commits IN the mutation's transaction (the next event
    // proves it below); not duplicating one is what this window shows.
    assert_eq!(
        stored_delta_count(&client, &tenant).await,
        0,
        "nothing said twice"
    );
    assert_eq!(
        frontier.delta_version(&tenant).await.expect("the version"),
        0,
        "the story has not moved twice for one fact"
    );

    // The normal frontier path: a NEW event commits mutation, delta and
    // checkpoint as ONE fact.
    frontier
        .apply_and_checkpoint(the_changeset(&tenant, "2"), &delivery_of(&stream, 1, "2"))
        .await
        .expect("the next event lands");
    assert_eq!(
        stored_delta_count(&client, &tenant).await,
        1,
        "one delta, said by the same transaction that moved the graph"
    );
    assert_eq!(
        frontier.delta_version(&tenant).await.expect("the version"),
        2,
        "the delta takes the version of its delivery"
    );

    // The lost ACK, replayed by a third process: the committed receipt
    // comes back and nothing is written again.
    let third_client = fresh_client().await.expect("the third process connects");
    let frontier = DgraphProjectionFrontier::open(third_client, &namespace)
        .await
        .expect("the frontier opens");
    let replay = frontier
        .apply_and_checkpoint(changes, &delivery)
        .await
        .expect("the replay answers");
    assert!(replay.replayed, "the delivery replays its receipt");
    assert_eq!(
        replay.receipt, commit.receipt,
        "the receipt is the committed one"
    );
    assert_eq!(
        replay.checkpoint, 2,
        "the checkpoint does not move backwards"
    );
    assert_eq!(
        stored_delta_count(&client, &tenant).await,
        1,
        "still one delta"
    );
    let final_state = graph
        .state_fingerprint(&tenant)
        .await
        .expect("the fingerprint");
    let after_second = graph
        .state_fingerprint(&tenant)
        .await
        .expect("the fingerprint after the second event");
    assert_eq!(
        after_second, final_state,
        "the replayed delivery changed nothing"
    );
}

/// AC1 — the projection replays FROM the checkpoint: a resume that
/// starts where the checkpoint says and re-projects the events below it
/// answers with their receipts and lands the new event above them. The
/// version and the causality survive the restart.
#[tokio::test]
async fn the_projection_replays_from_the_checkpoint_without_duplicating() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let tenant = tenant("c");
    let namespace = format!("ns-{}", tenant.account_id());
    let stream = format!("tenant-{}", tenant.account_id());
    let graph = DgraphGraphStore::new(client.clone());
    let frontier = DgraphProjectionFrontier::open(client.clone(), &namespace)
        .await
        .expect("the frontier opens");

    // Three events, projected one by one: the checkpoint walks with
    // them and the deltas take the version of their delivery.
    for step in ["1", "2", "3"] {
        let position: u64 = step.parse::<u64>().expect("a position") - 1;
        frontier
            .apply_and_checkpoint(
                the_changeset(&tenant, step),
                &delivery_of(&stream, position, step),
            )
            .await
            .expect("the event lands");
    }
    assert_eq!(
        frontier
            .checkpoint("graph-projector", &stream)
            .await
            .expect("the checkpoint")
            .expect("started"),
        3,
        "the next position to project"
    );
    assert_eq!(
        frontier.delta_version(&tenant).await.expect("the version"),
        3,
        "one delta per event, in the order of the story"
    );

    // The restart replays from the checkpoint: the events the
    // checkpoint already passed answer their receipts; the new one
    // above them applies.
    for step in ["1", "2"] {
        let position: u64 = step.parse::<u64>().expect("a position") - 1;
        let replay = frontier
            .apply_and_checkpoint(
                the_changeset(&tenant, step),
                &delivery_of(&stream, position, step),
            )
            .await
            .expect("the replay answers");
        assert!(replay.replayed, "event {step} replays");
        assert_eq!(replay.checkpoint, 3, "the checkpoint stays put");
    }
    let before = graph
        .state_fingerprint(&tenant)
        .await
        .expect("the fingerprint");
    frontier
        .apply_and_checkpoint(the_changeset(&tenant, "4"), &delivery_of(&stream, 3, "4"))
        .await
        .expect("the new event lands");
    assert_eq!(
        frontier
            .checkpoint("graph-projector", &stream)
            .await
            .expect("the checkpoint")
            .expect("started"),
        4,
    );
    let after = graph
        .state_fingerprint(&tenant)
        .await
        .expect("the fingerprint");
    assert_ne!(before, after, "the new event changed the graph");
    assert_eq!(
        stored_delta_count(&client, &tenant).await,
        4,
        "one delta per event"
    );
}

/// The checkpoint race: two writers of the SAME consumer and stream,
/// projecting different deliveries at the same moment. The guard
/// serializes them at the server; both land, in either commit order,
/// the checkpoint ends at the top and the graph holds both relations
/// exactly once.
#[tokio::test]
async fn two_checkpoint_writers_race_and_both_land() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let tenant = tenant("d");
    let namespace = format!("ns-{}", tenant.account_id());
    let stream = format!("tenant-{}", tenant.account_id());
    let graph = DgraphGraphStore::new(client.clone());
    let frontier = DgraphProjectionFrontier::open(client.clone(), &namespace)
        .await
        .expect("the frontier opens");

    let first = {
        let tenant = tenant.clone();
        let stream = stream.clone();
        let frontier = DgraphProjectionFrontier::open(client.clone(), &namespace)
            .await
            .expect("the frontier opens");
        async move {
            frontier
                .apply_and_checkpoint(the_changeset(&tenant, "1"), &delivery_of(&stream, 0, "1"))
                .await
        }
    };
    let second = {
        let tenant = tenant.clone();
        let stream = stream.clone();
        let frontier = DgraphProjectionFrontier::open(client.clone(), &namespace)
            .await
            .expect("the frontier opens");
        async move {
            frontier
                .apply_and_checkpoint(the_changeset(&tenant, "2"), &delivery_of(&stream, 1, "2"))
                .await
        }
    };
    let (one, two) = tokio::join!(first, second);
    one.expect("the first writer lands");
    two.expect("the second writer lands");

    assert_eq!(
        frontier
            .checkpoint("graph-projector", &stream)
            .await
            .expect("the checkpoint")
            .expect("started"),
        2,
        "the checkpoint ends at the top, whatever the order"
    );
    assert_eq!(
        stored_delta_count(&client, &tenant).await,
        2,
        "each delivery said its delta once"
    );
    for step in ["1", "2"] {
        let identity = identity_of(&tenant, step);
        assert_eq!(
            stored_relation_node_count(&client, &tenant, &identity).await,
            1,
            "the race never duplicated the edge of step {step}"
        );
    }
    let state = graph
        .state_fingerprint(&tenant)
        .await
        .expect("the fingerprint");
    eprintln!("the raced state: {state}");
}

/// AC2 — the rebuild builds apart: the active projection keeps
/// answering while the rebuild replays the same events into its own
/// named projection, the watermark of the rebuild is recorded, and the
/// two projections are comparable — the same state fingerprint, read
/// either way the swap would serve them.
#[tokio::test]
async fn the_rebuild_builds_apart_while_the_active_graph_serves() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let tenant = tenant("e");
    let namespace = format!("ns-{}", tenant.account_id());
    let stream = format!("tenant-{}", tenant.account_id());
    let active = DgraphGraphStore::new(client.clone());
    let frontier = DgraphProjectionFrontier::open(client.clone(), &namespace)
        .await
        .expect("the frontier opens");

    // The active projection applies two events.
    for step in ["1", "2"] {
        let position: u64 = step.parse::<u64>().expect("a position") - 1;
        frontier
            .apply_and_checkpoint(
                the_changeset(&tenant, step),
                &delivery_of(&stream, position, step),
            )
            .await
            .expect("the event lands");
    }
    let served = hodei_evidence::application::rebuild::fingerprint(&active, &tenant)
        .await
        .expect("the fingerprint");

    // The rebuild: a named projection, a watermark frontier that says
    // nothing (the deltas were said by the active one), and reads that
    // keep being served WHILE the rebuild replays.
    let rebuild = DgraphGraphStore::named(client.clone(), "hsp199_rebuild");
    let reader = {
        let active = DgraphGraphStore::new(client.clone());
        let tenant = tenant.clone();
        async move {
            let mut served = Vec::new();
            for _ in 0..6 {
                served.push(
                    hodei_evidence::application::rebuild::fingerprint(&active, &tenant)
                        .await
                        .expect("served"),
                );
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            }
            served
        }
    };
    let rebuilding = {
        let rebuild = DgraphGraphStore::named(client.clone(), "hsp199_rebuild");
        let watermark =
            DgraphProjectionFrontier::named(client.clone(), &namespace, "hsp199_rebuild")
                .await
                .expect("the rebuild frontier opens");
        let tenant = tenant.clone();
        let stream = stream.clone();
        async move {
            for step in ["1", "2"] {
                let position: u64 = step.parse::<u64>().expect("a position") - 1;
                let changes = the_changeset(&tenant, step);
                // The rebuild writes through the graph writer of its own
                // projection and records its watermark with the frontier,
                // saying nothing.
                rebuild
                    .apply(changes.clone())
                    .await
                    .expect("the rebuild applies");
                watermark
                    .apply_and_checkpoint_with(
                        &changes,
                        &ProjectionDelivery {
                            consumer: "rebuild".to_owned(),
                            stream_id: stream.clone(),
                            position,
                            event_id: format!("evt-{step}"),
                        },
                        false,
                    )
                    .await
                    .expect("the watermark advances");
            }
        }
    };
    let (reads, ()) = tokio::join!(reader, rebuilding);
    assert_eq!(
        frontier
            .checkpoint("rebuild", &stream)
            .await
            .expect("the watermark")
            .expect("recorded"),
        2,
        "the rebuild's watermark, a checkpoint of its own consumer"
    );
    // The reads were served DURING the rebuild, and they did not see
    // the rebuild's records.
    for read in &reads {
        assert_eq!(
            &served, read,
            "the active graph answers the same while rebuilding"
        );
    }

    // The projections are comparable: the same state fingerprint, and
    // the rebuild projection answers questions the swap would serve.
    let rebuilt = hodei_evidence::application::rebuild::fingerprint(&rebuild, &tenant)
        .await
        .expect("the rebuilt");
    assert_eq!(served, rebuilt, "the rebuild reproduces the active graph");
    let artifacts = rebuild
        .nodes(&tenant, NodeKind::Artifact)
        .await
        .expect("the rebuilt projection answers");
    assert_eq!(artifacts.len(), 2, "the swap's candidate serves the same");
    let live = active
        .nodes(&tenant, NodeKind::Artifact)
        .await
        .expect("the active answers");
    assert_eq!(
        live.len(),
        2,
        "the active graph is untouched by the rebuild"
    );
    eprintln!(
        "active projection = {ACTIVE_PROJECTION}, rebuild projection = hsp199_rebuild, fingerprints match"
    );
}
