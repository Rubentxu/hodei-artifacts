//! HSP-204: the reachability question over the native graph.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT like every IT of the stack. What it
//! proves, against the real engine:
//!
//! - PROVEN PATH: a REACHABLE answer carries the chain of asserted
//!   edges it arrived by, and every hop names the evidence that
//!   supports its edge — the route is traceable, never a bare yes;
//! - PROVEN ABSENCE: an UNREACHABLE answer comes from a frontier
//!   explored WHOLE within the budget (the trace is complete and leaves
//!   nothing behind), so the absence is a proof and not a guess;
//! - RETIRED SUPPORTS: an edge whose last support was withdrawn does
//!   not count as a path;
//! - FOREIGN EVIDENCE: another account's edge validates nothing — the
//!   account rides on every hop;
//! - EXPLICIT INCOMPLETENESS: a budget that runs out before the
//!   frontier is exhausted answers UNKNOWN with the reason and what was
//!   left behind — never best-effort in silence, never a weaker proof
//!   of absence;
//! - CYCLES: a graph with a cycle neither hangs the walk nor turns the
//!   cycle into a path that is not there;
//! - PARITY: the answer matches the reference contract — the plain
//!   breadth-first derivation over the edges the changeset asserted,
//!   the same oracle the investigator's reachability facts hold — for
//!   the same changeset.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use hodei_evidence::domain::graph::NodeKind;
use hodei_evidence::ports::graph::{
    GraphReadLimits, GraphWriter, PathReachability, ReachabilityQuery,
};
use hodei_evidence::{EvidenceId, GraphChangeSet, GraphNode, GraphRelation, RelationType};
use hodei_evidence_storage::DgraphGraphStore;
use hodei_graph_client::{GraphClient, GraphClientConfig, schema::ensure_ready};
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

async fn connect() -> Option<Arc<GraphClient>> {
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
        "schema boot: decision={:?} binary_version={} stored_version={:?} applied={}",
        report.decision, report.binary_version, report.stored_version, report.applied
    );
    Some(Arc::new(client))
}

fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

fn tenant(suffix: &str) -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{suffix}:Artifact/hsp204"
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

fn node(tenant: &Hrn, kind: NodeKind, name: &str) -> GraphNode {
    GraphNode::new(
        hrn_of(tenant, kind_name(&kind), name),
        tenant.clone(),
        kind,
        chrono::Utc::now(),
    )
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

fn component(tenant: &Hrn, name: &str) -> GraphNode {
    node(tenant, NodeKind::Component, name)
}

fn relation(tenant: &Hrn, from: &Hrn, to: &Hrn, evidence: &str) -> GraphRelation {
    GraphRelation::new(
        RelationType::Produced,
        from.clone(),
        to.clone(),
        tenant.clone(),
        EvidenceId::new(evidence.to_owned()),
        chrono::Utc::now(),
    )
}

fn withdraw(
    tenant: &Hrn,
    from: &Hrn,
    to: &Hrn,
    evidence: &str,
) -> hodei_evidence::GraphRelationWithdrawal {
    hodei_evidence::GraphRelationWithdrawal::new(
        RelationType::Produced,
        from.clone(),
        to.clone(),
        tenant.clone(),
        EvidenceId::new(evidence.to_owned()),
        chrono::Utc::now(),
    )
}

/// The reachability changeset of the UAT: a chain of components the
/// analysis asserts, one evidence per edge, plus a distractor branch
/// that reaches a dead end, so the shortest path to the destination is
/// unique and both walks have to find the same one.
///
/// o -> m1 -> m2 -> dest, and o -> dead (which goes nowhere).
fn chain_changeset(tenant: &Hrn) -> (GraphChangeSet, Hrn, Hrn) {
    let origin = hrn_of(tenant, "Component", "rc-origin");
    let m1 = hrn_of(tenant, "Component", "rc-mid-1");
    let m2 = hrn_of(tenant, "Component", "rc-mid-2");
    let dest = hrn_of(tenant, "Component", "rc-dest");
    let dead = hrn_of(tenant, "Component", "rc-dead");
    let changes = GraphChangeSet::empty()
        .with_node(component(tenant, "rc-origin"))
        .with_node(component(tenant, "rc-mid-1"))
        .with_node(component(tenant, "rc-mid-2"))
        .with_node(component(tenant, "rc-dest"))
        .with_node(component(tenant, "rc-dead"))
        .with_node(node(tenant, NodeKind::Evidence, "ev-c1"))
        .with_node(node(tenant, NodeKind::Evidence, "ev-c2"))
        .with_node(node(tenant, NodeKind::Evidence, "ev-c3"))
        .with_node(node(tenant, NodeKind::Evidence, "ev-c4"))
        .with_relation(relation(tenant, &origin, &m1, "ev-c1"))
        .with_relation(relation(tenant, &m1, &m2, "ev-c2"))
        .with_relation(relation(tenant, &m2, &dest, "ev-c3"))
        .with_relation(relation(tenant, &origin, &dead, "ev-c4"));
    (changes, origin, dest)
}

/// The reference oracle of the contract: the plain breadth-first
/// derivation over the edges the changeset asserted — the same shape
/// the investigator's reachability facts answer with, with no store
/// under it and no budget. It returns the chain of nodes and the chain
/// of edges when there is a path, and nothing when the walk exhausted
/// every node the origin could reach without meeting the destination.
fn reference_reachability(
    changes: &GraphChangeSet,
    origin: &Hrn,
    destination: &Hrn,
) -> Option<(Vec<Hrn>, Vec<GraphRelation>)> {
    let mut queue: VecDeque<Hrn> = VecDeque::new();
    queue.push_back(origin.clone());
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut parent_of: BTreeMap<String, GraphRelation> = BTreeMap::new();

    while let Some(current) = queue.pop_front() {
        if !seen.insert(current.to_string()) {
            continue;
        }
        if current == *destination {
            break;
        }
        for edge in changes.relations.iter().filter(|r| r.from == current) {
            if !seen.contains(&edge.to.to_string()) {
                parent_of
                    .entry(edge.to.to_string())
                    .or_insert_with(|| edge.clone());
                queue.push_back(edge.to.clone());
            }
        }
    }

    if !seen.contains(&destination.to_string()) {
        return None;
    }
    let mut edges = Vec::new();
    let mut cursor = destination.to_string();
    while cursor != origin.to_string() {
        let edge = parent_of
            .get(&cursor)
            .expect("every reached node has a parent");
        cursor = edge.from.to_string();
        edges.push(edge.clone());
    }
    edges.reverse();
    let mut nodes = vec![origin.clone()];
    nodes.extend(edges.iter().map(|edge| edge.to.clone()));
    Some((nodes, edges))
}

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// AC1/AC2 — the path is real: the answer carries the chain of asserted
/// edges and the evidence of every one of them, it matches the
/// reference contract for the same changeset, and the trace says how
/// the walk that proved it was spent.
#[tokio::test]
async fn a_proven_path_carries_its_edges_and_evidence() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let store = DgraphGraphStore::new(client.clone());
    let tenant = tenant(&format!("pa{}", &nonce()[..6]));
    let (changes, origin, dest) = chain_changeset(&tenant);
    store
        .apply(changes.clone())
        .await
        .expect("the chain commits");

    // PARITY: the reference contract derives the same path for the same
    // graph, and the Dgraph walk answers with exactly that chain.
    let (reference_nodes, reference_edges) =
        reference_reachability(&changes, &origin, &dest).expect("the reference finds the path");
    let answer = store
        .reachability(&tenant, &origin, &dest, &GraphReadLimits::probe())
        .await
        .expect("the reachability reads");
    assert_eq!(
        answer.state,
        PathReachability::Reachable,
        "the chain reaches the destination: {:?}",
        answer
    );
    assert!(!answer.state.is_proof_of_absence());
    assert_eq!(answer.path.len(), 3, "origin, mid, mid, dest: three hops");
    let hop_nodes: Vec<String> = answer.path.iter().map(|h| h.node.id.to_string()).collect();
    let reference_node_ids: Vec<String> = reference_nodes
        .iter()
        .skip(1)
        .map(|n| n.to_string())
        .collect();
    assert_eq!(
        hop_nodes, reference_node_ids,
        "the walk found the same shortest path the reference did"
    );
    let hop_edges: Vec<String> = answer.path.iter().map(|h| h.via.identity()).collect();
    let reference_edge_ids: Vec<String> = reference_edges.iter().map(|e| e.identity()).collect();
    assert_eq!(
        hop_edges, reference_edge_ids,
        "every edge of the path is the asserted one, in order"
    );

    // TRACEABILITY: every hop carries the evidence that supports its
    // edge, and the trace explains the walk that proved it.
    for hop in &answer.path {
        assert!(
            !hop.via.evidence_id.to_string().is_empty(),
            "the hop carries its support: {:?}",
            hop.via
        );
        assert_eq!(hop.via.tenant_hrn, tenant, "the hop belongs to the account");
        assert_eq!(
            hop.node.tenant_hrn, tenant,
            "the node belongs to the account"
        );
    }
    assert_eq!(
        answer.path[0].via.evidence_id.to_string(),
        EvidenceId::new("ev-c1".to_owned()).to_string(),
        "the first hop is the edge the first evidence asserted"
    );
    assert!(
        !answer.trace.incomplete,
        "the probe bound does not truncate this walk: {:?}",
        answer.trace
    );
    assert_eq!(answer.trace.limits, Some(GraphReadLimits::probe()));
    assert!(
        answer.trace.round_trips > 0,
        "the walk spent its budget honestly"
    );
    assert!(answer.trace.hops_taken >= 3, "it walked to the destination");
    assert_eq!(answer.trace.frontier_left, 0, "nothing was left behind");
    assert_eq!(answer.state.as_str(), "REACHABLE");
    store.clear().await.expect("the graph empties");
}

/// AC1 — the absence is DEMONSTRATED: the destination is not met, the
/// walk explored every node the origin could reach within the budget,
/// and the trace is whole — no reason, nothing left behind. Another
/// account's destination answers for nothing here either.
#[tokio::test]
async fn absence_is_proven_when_the_frontier_is_exhausted() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let store = DgraphGraphStore::new(client.clone());
    let other = tenant(&format!("ab-other{}", &nonce()[..6]));
    let tenant = tenant(&format!("ab{}", &nonce()[..6]));
    let (changes, origin, _dest) = chain_changeset(&tenant);
    store
        .apply(changes.clone())
        .await
        .expect("the chain commits");

    // PARITY: the reference contract also finds no path.
    let absent = hrn_of(&tenant, "Component", "rc-never-there");
    assert!(
        reference_reachability(&changes, &origin, &absent).is_none(),
        "the reference finds no path either"
    );
    let answer = store
        .reachability(&tenant, &origin, &absent, &GraphReadLimits::probe())
        .await
        .expect("the reachability reads");
    assert_eq!(answer.state, PathReachability::Unreachable);
    assert!(
        answer.state.is_proof_of_absence(),
        "a whole frontier IS a proof of absence"
    );
    assert!(answer.path.is_empty(), "an absence carries no path");
    assert!(
        !answer.trace.incomplete,
        "the frontier was explored whole: {:?}",
        answer.trace
    );
    assert!(answer.trace.reasons.is_empty());
    assert_eq!(answer.trace.frontier_left, 0);

    // The destination of ANOTHER account does not answer for this one:
    // the same shapes, the same names, a different account — and the
    // absence is still proven, not guessed.
    let foreign_dest = hrn_of(&other, "Component", "rc-dest");
    let cross = store
        .reachability(&tenant, &origin, &foreign_dest, &GraphReadLimits::probe())
        .await
        .expect("the cross reachability reads");
    assert_eq!(cross.state, PathReachability::Unreachable);
    assert!(!cross.trace.incomplete);
    store.clear().await.expect("the graph empties");
}

/// AC1 — an edge whose last support was withdrawn does NOT count as a
/// path: the route that was proven before the withdrawal is proven
/// absent after it, and the absence is proven whole, not guessed.
#[tokio::test]
async fn a_retired_support_breaks_the_path() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let store = DgraphGraphStore::new(client.clone());
    let tenant = tenant(&format!("rt{}", &nonce()[..6]));
    let origin = hrn_of(&tenant, "Component", "rr-origin");
    let mid = hrn_of(&tenant, "Component", "rr-mid");
    let dest = hrn_of(&tenant, "Component", "rr-dest");
    let changes = GraphChangeSet::empty()
        .with_node(component(&tenant, "rr-origin"))
        .with_node(component(&tenant, "rr-mid"))
        .with_node(component(&tenant, "rr-dest"))
        .with_node(node(&tenant, NodeKind::Evidence, "ev-r1"))
        .with_node(node(&tenant, NodeKind::Evidence, "ev-r2"))
        .with_relation(relation(&tenant, &origin, &mid, "ev-r1"))
        .with_relation(relation(&tenant, &mid, &dest, "ev-r2"));
    store.apply(changes).await.expect("the chain commits");

    let before = store
        .reachability(&tenant, &origin, &dest, &GraphReadLimits::probe())
        .await
        .expect("the reachability reads");
    assert_eq!(before.state, PathReachability::Reachable, "two hops");
    assert_eq!(before.path.len(), 2);

    // The last support of the middle edge is withdrawn: the edge
    // retires and the path with it.
    store
        .apply(GraphChangeSet::empty().with_withdrawal(withdraw(&tenant, &mid, &dest, "ev-r2")))
        .await
        .expect("the withdrawal commits");

    let after = store
        .reachability(&tenant, &origin, &dest, &GraphReadLimits::probe())
        .await
        .expect("the reachability reads");
    assert_eq!(
        after.state,
        PathReachability::Unreachable,
        "a retired edge is not a path: {:?}",
        after
    );
    assert!(after.path.is_empty());
    assert!(
        !after.trace.incomplete,
        "the frontier without the retired edge is still whole: {:?}",
        after.trace
    );
    assert!(after.trace.reasons.is_empty());
    store.clear().await.expect("the graph empties");
}

/// AC1 — the evidence of ANOTHER account never validates a path: the
/// same relative shape asserted in a second account does not speak for
/// the first, and the first account's answer stays a proven absence.
#[tokio::test]
async fn foreign_evidence_never_validates_a_path() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let store = DgraphGraphStore::new(client.clone());
    let nonce = nonce();
    let first = tenant(&format!("fe1{nonce}"));
    let second = tenant(&format!("fe2{nonce}"));
    let origin = hrn_of(&first, "Component", "rf-origin");
    let mid = hrn_of(&first, "Component", "rf-mid");
    let dest = hrn_of(&first, "Component", "rf-dest");
    // The first account: origin -> mid (its own evidence), and the
    // support of mid -> dest is about to go away.
    let first_changes = GraphChangeSet::empty()
        .with_node(component(&first, "rf-origin"))
        .with_node(component(&first, "rf-mid"))
        .with_node(component(&first, "rf-dest"))
        .with_node(node(&first, NodeKind::Evidence, "ev-f1"))
        .with_node(node(&first, NodeKind::Evidence, "ev-f2"))
        .with_relation(relation(&first, &origin, &mid, "ev-f1"))
        .with_relation(relation(&first, &mid, &dest, "ev-f2"));
    store
        .apply(first_changes)
        .await
        .expect("the first account's chain commits");

    // The second account asserts the same RELATIVE shape with its own
    // evidence: its own mid and dest, joined by its own edge.
    let second_mid = hrn_of(&second, "Component", "rf-mid");
    let second_dest = hrn_of(&second, "Component", "rf-dest");
    let second_changes = GraphChangeSet::empty()
        .with_node(component(&second, "rf-mid"))
        .with_node(component(&second, "rf-dest"))
        .with_node(node(&second, NodeKind::Evidence, "ev-g1"))
        .with_relation(relation(&second, &second_mid, &second_dest, "ev-g1"));
    store
        .apply(second_changes)
        .await
        .expect("the second account's chain commits");

    // The first account's middle support is withdrawn: the only edge
    // that would join mid to dest is now the OTHER account's, and that
    // one validates nothing here.
    store
        .apply(GraphChangeSet::empty().with_withdrawal(withdraw(&first, &mid, &dest, "ev-f2")))
        .await
        .expect("the withdrawal commits");

    let answer = store
        .reachability(&first, &origin, &dest, &GraphReadLimits::probe())
        .await
        .expect("the reachability reads");
    assert_eq!(
        answer.state,
        PathReachability::Unreachable,
        "another account's edge is not a path for this account: {:?}",
        answer
    );
    assert!(answer.path.is_empty());
    assert!(
        !answer.trace.incomplete,
        "the account's own frontier is whole: {:?}",
        answer.trace
    );

    // Sanity: the second account's own question answers with its own
    // edge and its own evidence, and nothing of the first account's.
    let second_origin_check = store
        .reachability(
            &second,
            &second_mid,
            &second_dest,
            &GraphReadLimits::probe(),
        )
        .await
        .expect("the second account reads");
    assert_eq!(second_origin_check.state, PathReachability::Reachable);
    for hop in &second_origin_check.path {
        assert_eq!(hop.via.tenant_hrn, second);
        assert_eq!(hop.node.tenant_hrn, second);
    }
    store.clear().await.expect("the graph empties");
}

/// AC1/AC2 — a budget that runs out before the frontier is exhausted
/// answers UNKNOWN with the reason and what was left behind: a walk
/// that could not look everywhere is not a proof of absence, and it
/// never passes for one. The same graph under the probe bound answers
/// REACHABLE with the same path the reference contract derives.
#[tokio::test]
async fn a_budget_that_runs_out_says_unknown_and_why() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let store = DgraphGraphStore::new(client.clone());
    let tenant = tenant(&format!("bu{}", &nonce()[..6]));
    let origin = hrn_of(&tenant, "Component", "rb-origin");
    let dest = hrn_of(&tenant, "Component", "rb-dest");
    let mut changes = GraphChangeSet::empty()
        .with_node(component(&tenant, "rb-origin"))
        .with_node(node(&tenant, NodeKind::Evidence, "ev-b"));
    // A chain of six edges: too deep for a budget of two hops.
    let mut previous = origin.clone();
    for index in 0..5 {
        let next = hrn_of(&tenant, "Component", &format!("rb-hop-{index}"));
        changes = changes
            .with_node(component(&tenant, &format!("rb-hop-{index}")))
            .with_relation(relation(&tenant, &previous, &next, "ev-b"));
        previous = next;
    }
    changes = changes
        .with_node(component(&tenant, "rb-dest"))
        .with_relation(relation(&tenant, &previous, &dest, "ev-b"));
    store
        .apply(changes.clone())
        .await
        .expect("the chain commits");

    // The depth budget stops the walk after two hops: the answer is
    // UNKNOWN, it says which limit bound it and what was left behind,
    // and it never claims the destination is unreachable.
    let shallow = GraphReadLimits {
        max_depth: 2,
        ..GraphReadLimits::probe()
    };
    let answer = store
        .reachability(&tenant, &origin, &dest, &shallow)
        .await
        .expect("the bounded reachability reads");
    assert_eq!(
        answer.state,
        PathReachability::Unknown,
        "two hops cannot say anything about a six-edge chain: {:?}",
        answer
    );
    assert!(
        !answer.state.is_proof_of_absence(),
        "an exhausted budget is not a proof of absence"
    );
    assert!(answer.path.is_empty(), "an unknown carries no path");
    assert!(
        answer.trace.incomplete,
        "the walk says it could not look everywhere"
    );
    assert!(
        answer.trace.reasons.contains(&"depth-limit".to_owned()),
        "the depth is the reason: {:?}",
        answer.trace.reasons
    );
    assert!(
        answer.trace.frontier_left > 0,
        "the trace says how much was left behind"
    );
    assert!(
        answer.reason.contains("not a proof of absence"),
        "the reason declares the limit of the answer: {}",
        answer.reason
    );

    // The round-trip budget binds too: one round trip is not even the
    // first hop, and the answer is UNKNOWN for the same honesty.
    let one_trip = GraphReadLimits {
        max_round_trips: 1,
        ..GraphReadLimits::probe()
    };
    let answer = store
        .reachability(&tenant, &origin, &dest, &one_trip)
        .await
        .expect("the budgeted reachability reads");
    assert_eq!(answer.state, PathReachability::Unknown);
    assert!(
        answer.trace.reasons.contains(&"budget-limit".to_owned()),
        "the round-trip budget is the reason: {:?}",
        answer.trace.reasons
    );

    // PARITY under a whole budget: the same graph answers REACHABLE
    // with exactly the path the reference contract derives.
    let (reference_nodes, reference_edges) =
        reference_reachability(&changes, &origin, &dest).expect("the reference finds the path");
    let whole = store
        .reachability(&tenant, &origin, &dest, &GraphReadLimits::probe())
        .await
        .expect("the probe reachability reads");
    assert_eq!(whole.state, PathReachability::Reachable);
    assert!(!whole.trace.incomplete);
    let whole_edges: Vec<String> = whole.path.iter().map(|h| h.via.identity()).collect();
    let reference_edge_ids: Vec<String> = reference_edges.iter().map(|e| e.identity()).collect();
    assert_eq!(whole_edges, reference_edge_ids, "the same shortest path");
    assert_eq!(whole.path.len(), reference_nodes.len() - 1);
    store.clear().await.expect("the graph empties");
}

/// AC1 — a graph with a cycle neither hangs the walk nor invents a
/// path: the direct edge is the shortest path, and the cycle is walked
/// only as far as the question needs.
#[tokio::test]
async fn a_cycle_does_not_hang_or_deceive_the_walk() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let store = DgraphGraphStore::new(client.clone());
    let tenant = tenant(&format!("cy{}", &nonce()[..6]));
    let a = hrn_of(&tenant, "Component", "rcy-a");
    let b = hrn_of(&tenant, "Component", "rcy-b");
    let c = hrn_of(&tenant, "Component", "rcy-c");
    let d = hrn_of(&tenant, "Component", "rcy-d");
    let changes = GraphChangeSet::empty()
        .with_node(component(&tenant, "rcy-a"))
        .with_node(component(&tenant, "rcy-b"))
        .with_node(component(&tenant, "rcy-c"))
        .with_node(component(&tenant, "rcy-d"))
        .with_node(node(&tenant, NodeKind::Evidence, "ev-y"))
        .with_relation(relation(&tenant, &a, &b, "ev-y"))
        .with_relation(relation(&tenant, &b, &c, "ev-y"))
        .with_relation(relation(&tenant, &c, &a, "ev-y"))
        .with_relation(relation(&tenant, &a, &d, "ev-y"));
    store.apply(changes).await.expect("the cycle commits");

    // The direct edge answers in one hop, and the cycle does not make
    // the walk wander forever.
    let started = Instant::now();
    let direct = store
        .reachability(&tenant, &a, &d, &GraphReadLimits::probe())
        .await
        .expect("the cyclic walk answers");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the cycle did not hang the walk"
    );
    assert_eq!(direct.state, PathReachability::Reachable);
    assert_eq!(direct.path.len(), 1, "the direct edge is the shortest path");
    assert!(!direct.trace.incomplete, "the cycle is not a truncation");

    // Reaching A from B goes THROUGH the cycle, and the path the answer
    // carries is the two-edge route B -> C -> A.
    let through = store
        .reachability(&tenant, &b, &a, &GraphReadLimits::probe())
        .await
        .expect("the walk through the cycle answers");
    assert_eq!(through.state, PathReachability::Reachable);
    let through_edges: Vec<String> = through.path.iter().map(|h| h.via.identity()).collect();
    assert_eq!(
        through_edges,
        vec![
            format!("{}|{}|{}", RelationType::Produced.as_str(), b, c),
            format!("{}|{}|{}", RelationType::Produced.as_str(), c, a),
        ],
        "the path through the cycle is walked once and never repeats a node: {through_edges:?}"
    );
    store.clear().await.expect("the graph empties");
}
