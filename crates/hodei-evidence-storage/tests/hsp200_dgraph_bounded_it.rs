//! HSP-200: the bounded native lineage and the receipt of every read.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT like every IT of the stack. What it
//! proves, against the real engine:
//!
//! - SHAPE: the bounded lineage and the bounded dependency traversal
//!   say the expected things for a known changeset;
//! - CYCLES: a graph with a cycle neither hangs the walk nor
//!   duplicates a node in the answer;
//! - RETIRED SUPPORTS: an edge whose last support was withdrawn is not
//!   walked;
//! - TENANT ON EVERY HOP: two accounts with the same shapes see only
//!   their own graph;
//! - EXPLICIT INCOMPLETENESS: a depth, node, edge or round-trip limit
//!   that binds makes the result say `incomplete` with the reason and
//!   what was left behind — never best-effort in silence;
//! - TRACEABILITY: every answer carries how it was resolved;
//! - p95: bounded reads against unbounded ones on a small-to-medium
//!   synthetic graph, numbers recorded in the evidence.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use hodei_evidence::domain::graph::NodeKind;
use hodei_evidence::ports::graph::{BoundedGraphQuery, GraphQuery, GraphReadLimits, GraphWriter};
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
        "hrn:hodei:artifacts:eu-west-1:{suffix}:Artifact/hsp200"
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
        chrono::Utc::now(),
    )
}

/// The lineage changeset of the UAT: a repository, a revision, a
/// build, an artefact and a deployment, with the relations the
/// projector writes between them.
fn lineage_changeset(tenant: &Hrn) -> GraphChangeSet {
    let repository = hrn_of(tenant, "Repository", "r-1");
    let revision = hrn_of(tenant, "SourceRevision", "rev-1");
    let build = hrn_of(tenant, "Build", "b-1");
    let artifact = hrn_of(tenant, "Artifact", "a-1");
    let deployment = hrn_of(tenant, "Deployment", "d-1");
    GraphChangeSet::empty()
        .with_node(node(tenant, NodeKind::Repository, "r-1"))
        .with_node(node(tenant, NodeKind::SourceRevision, "rev-1"))
        .with_node(node(tenant, NodeKind::Build, "b-1"))
        .with_node(node(tenant, NodeKind::Artifact, "a-1"))
        .with_node(node(tenant, NodeKind::Deployment, "d-1"))
        .with_node(node(tenant, NodeKind::Evidence, "ev-1"))
        .with_relation(relation(
            tenant,
            RelationType::HasRevision,
            &repository,
            &revision,
            "ev-1",
        ))
        .with_relation(relation(
            tenant,
            RelationType::Triggered,
            &revision,
            &build,
            "ev-1",
        ))
        .with_relation(relation(
            tenant,
            RelationType::UsedSource,
            &build,
            &revision,
            "ev-1",
        ))
        .with_relation(relation(
            tenant,
            RelationType::Produced,
            &build,
            &artifact,
            "ev-1",
        ))
        .with_relation(relation(
            tenant,
            RelationType::DeployedAs,
            &artifact,
            &deployment,
            "ev-1",
        ))
}

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// AC1 — the bounded lineage and the bounded traversal of the Dgraph
/// port say the expected things for a known changeset.
#[tokio::test]
async fn bounded_reads_say_the_expected_things() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let store = DgraphGraphStore::new(client.clone());
    let tenant = tenant(&format!("pr{}", &nonce()[..6]));
    let changes = lineage_changeset(&tenant);
    let artifact = hrn_of(&tenant, "Artifact", "a-1");
    let repository = hrn_of(&tenant, "Repository", "r-1");

    store
        .apply(changes)
        .await
        .expect("the changeset commits in Dgraph");

    // SHAPE: the bounded lineage of the changeset.
    let probe = GraphReadLimits::probe();
    let dgraph_lineage = store
        .bounded_lineage(&tenant, &artifact, &probe)
        .await
        .expect("the bounded lineage reads");
    assert!(
        !dgraph_lineage.trace.incomplete,
        "the probe bound does not truncate this graph: {:?}",
        dgraph_lineage.trace
    );
    assert_eq!(
        dgraph_lineage.lineage.builds.len(),
        1,
        "the build is in the lineage"
    );
    assert_eq!(
        dgraph_lineage
            .lineage
            .repository
            .as_ref()
            .map(|n| n.id.clone()),
        Some(repository.clone()),
        "the repository is found through the revision"
    );
    assert_eq!(
        dgraph_lineage.lineage.deployments.len(),
        1,
        "the deployment is in the lineage"
    );

    // PARITY: the corrected walk — a build produced its artefact, so
    // the artefacts of the build read the relations that START at it.
    let dgraph_artifacts = store
        .artifacts_of_build(&tenant, &hrn_of(&tenant, "Build", "b-1"))
        .await
        .expect("the corrected walk reads");
    assert_eq!(
        dgraph_artifacts.len(),
        1,
        "the build produced the artefact: {:?}",
        dgraph_artifacts
    );
    let repository_artifacts = store
        .artifacts_of_repository(&tenant, &repository)
        .await
        .expect("the three-hop walk reads");
    assert_eq!(
        repository_artifacts.len(),
        1,
        "the repository walk reaches the artefact now: {:?}",
        repository_artifacts
    );

    // SHAPE: the bounded dependency traversal reaches every edge.
    let dgraph_deps = store
        .bounded_dependencies(&tenant, &repository, &probe)
        .await
        .expect("the bounded traversal reads");
    let dgraph_edge_pairs: Vec<String> = dgraph_deps
        .edges
        .iter()
        .map(|r| format!("{}|{}", r.from, r.to))
        .collect();
    assert_eq!(dgraph_edge_pairs.len(), 5, "every relation is crossed");

    // TRACEABILITY: the receipt explains the walk.
    assert_eq!(dgraph_lineage.trace.limits, Some(probe));
    assert!(dgraph_lineage.trace.hops_taken >= 4, "four lineage hops");
    assert!(dgraph_lineage.trace.round_trips > 0);
    assert!(dgraph_lineage.trace.nodes_seen > 0);
    store.clear().await.expect("the graph empties");
}

/// AC1 — a graph with a cycle neither hangs the walk nor duplicates a
/// node: A produces B, B produces C, C produces A again.
#[tokio::test]
async fn a_cycle_does_not_hang_or_duplicate() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let store = DgraphGraphStore::new(client.clone());
    let tenant = tenant(&format!("cy{}", &nonce()[..6]));
    let a = hrn_of(&tenant, "Component", "cyc-a");
    let b = hrn_of(&tenant, "Component", "cyc-b");
    let c = hrn_of(&tenant, "Component", "cyc-c");
    let changes = GraphChangeSet::empty()
        .with_node(node(&tenant, NodeKind::Component, "cyc-a"))
        .with_node(node(&tenant, NodeKind::Component, "cyc-b"))
        .with_node(node(&tenant, NodeKind::Component, "cyc-c"))
        .with_node(node(&tenant, NodeKind::Evidence, "ev-c"))
        .with_relation(relation(&tenant, RelationType::Produced, &a, &b, "ev-c"))
        .with_relation(relation(&tenant, RelationType::Produced, &b, &c, "ev-c"))
        .with_relation(relation(&tenant, RelationType::Produced, &c, &a, "ev-c"));
    store.apply(changes).await.expect("the cycle commits");

    let started = Instant::now();
    let walk = store
        .bounded_dependencies(&tenant, &a, &GraphReadLimits::probe())
        .await
        .expect("the cyclic walk answers");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the cycle did not hang the walk"
    );
    let ids: Vec<String> = walk.nodes.iter().map(|n| n.id.to_string()).collect();
    assert_eq!(ids.len(), 2, "B and C, never the root again: {ids:?}");
    assert_eq!(walk.edges.len(), 3, "every edge of the cycle is crossed");
    assert!(!walk.trace.incomplete, "the cycle is not a truncation");
    store.clear().await.expect("the graph empties");
}

/// AC1 — an edge whose last support was withdrawn does not appear as
/// active in any bounded walk.
#[tokio::test]
async fn a_retired_support_never_walks() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let store = DgraphGraphStore::new(client.clone());
    let tenant = tenant(&format!("rt{}", &nonce()[..6]));
    let build = hrn_of(&tenant, "Build", "b-r");
    let artifact = hrn_of(&tenant, "Artifact", "a-r");
    let changes = GraphChangeSet::empty()
        .with_node(node(&tenant, NodeKind::Build, "b-r"))
        .with_node(node(&tenant, NodeKind::Artifact, "a-r"))
        .with_node(node(&tenant, NodeKind::Evidence, "ev-r1"))
        .with_node(node(&tenant, NodeKind::Evidence, "ev-r2"))
        .with_relation(relation(
            &tenant,
            RelationType::Produced,
            &build,
            &artifact,
            "ev-r1",
        ))
        .with_relation(relation(
            &tenant,
            RelationType::Produced,
            &build,
            &artifact,
            "ev-r2",
        ));
    store.apply(changes).await.expect("the edge commits");

    let walk = store
        .bounded_dependencies(&tenant, &build, &GraphReadLimits::probe())
        .await
        .expect("the walk reads");
    assert_eq!(walk.edges.len(), 1, "the asserted edge walks");

    // The last support is withdrawn: the edge retires and no bounded
    // walk shows it again.
    let withdrawal = hodei_evidence::GraphRelationWithdrawal::new(
        RelationType::Produced,
        build.clone(),
        artifact.clone(),
        tenant.clone(),
        EvidenceId::new("ev-r1".to_owned()),
        chrono::Utc::now(),
    );
    let last = hodei_evidence::GraphRelationWithdrawal::new(
        RelationType::Produced,
        build.clone(),
        artifact.clone(),
        tenant.clone(),
        EvidenceId::new("ev-r2".to_owned()),
        chrono::Utc::now(),
    );
    store
        .apply(GraphChangeSet::empty().with_withdrawal(withdrawal))
        .await
        .expect("the first withdrawal commits");
    store
        .apply(GraphChangeSet::empty().with_withdrawal(last))
        .await
        .expect("the last withdrawal commits");

    let walk = store
        .bounded_dependencies(&tenant, &build, &GraphReadLimits::probe())
        .await
        .expect("the walk reads");
    assert!(
        walk.edges.is_empty(),
        "a retired edge is not walked: {:?}",
        walk.edges
    );
    assert!(walk.nodes.is_empty(), "nothing is reached through it");
    let lineage = store
        .bounded_lineage(&tenant, &artifact, &GraphReadLimits::probe())
        .await
        .expect("the lineage reads");
    assert!(
        lineage.lineage.evidence.is_empty(),
        "a retired edge is not lineage evidence either"
    );
    store.clear().await.expect("the graph empties");
}

/// AC1 — two accounts with the same shapes see only their own graph:
/// the account rides on every hop of the walk.
#[tokio::test]
async fn tenants_are_isolated_on_every_hop() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let store = DgraphGraphStore::new(client.clone());
    let nonce = nonce();
    let first = tenant(&format!("i1{nonce}"));
    let second = tenant(&format!("i2{nonce}"));
    for tenant in [&first, &second] {
        store
            .apply(lineage_changeset(tenant))
            .await
            .expect("each account's changeset commits");
    }

    let probe = GraphReadLimits::probe();
    let artifact_of_first = hrn_of(&first, "Artifact", "a-1");
    let repository_of_first = hrn_of(&first, "Repository", "r-1");
    let repository_of_second = hrn_of(&second, "Repository", "r-1");

    let lineage = store
        .bounded_lineage(&first, &artifact_of_first, &probe)
        .await
        .expect("the lineage reads");
    for node in lineage
        .lineage
        .builds
        .iter()
        .chain(lineage.lineage.revisions.iter())
    {
        assert_eq!(
            node.tenant_hrn, first,
            "every node of the walk belongs to the account"
        );
    }
    for relation in &lineage.lineage.evidence {
        assert_eq!(relation.tenant_hrn, first, "every edge belongs to it too");
    }

    let deps = store
        .bounded_dependencies(&first, &repository_of_first, &probe)
        .await
        .expect("the traversal reads");
    assert!(
        deps.nodes.iter().all(|n| n.tenant_hrn == first),
        "the traversal reaches only its own nodes: {:?}",
        deps.nodes
    );
    assert!(
        deps.edges.iter().all(|r| r.tenant_hrn == first),
        "the traversal crosses only its own edges"
    );

    // The other account's repository answers for nothing here.
    let cross = store
        .bounded_dependencies(&first, &repository_of_second, &probe)
        .await
        .expect("the cross traversal reads");
    assert!(
        cross.root.is_none() && cross.nodes.is_empty() && cross.edges.is_empty(),
        "another account's node does not answer for this account"
    );
    store.clear().await.expect("the graph empties");
}

/// AC2 — a limit that binds makes the result say `incomplete` with the
/// reason and what was left behind, in every direction: depth, nodes,
/// edges and the round-trip budget. The probe bound completes.
#[tokio::test]
async fn limits_make_incompleteness_explicit() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let store = DgraphGraphStore::new(client.clone());
    let tenant = tenant(&format!("lm{}", &nonce()[..6]));
    let artifact = hrn_of(&tenant, "Artifact", "a-1");
    let repository = hrn_of(&tenant, "Repository", "r-1");
    store
        .apply(lineage_changeset(&tenant))
        .await
        .expect("the changeset commits");

    // DEPTH: one hop reaches the builds, and says what it could not.
    let shallow = GraphReadLimits {
        max_depth: 1,
        ..GraphReadLimits::default()
    };
    let lineage = store
        .bounded_lineage(&tenant, &artifact, &shallow)
        .await
        .expect("the shallow lineage reads");
    assert!(lineage.trace.incomplete, "one hop is not the whole lineage");
    assert!(
        lineage.trace.reasons.contains(&"depth-limit".to_owned()),
        "the depth is the reason: {:?}",
        lineage.trace.reasons
    );
    assert!(
        lineage.lineage.builds.len() == 1 && lineage.lineage.revisions.is_empty(),
        "the first hop answered, the rest is left behind"
    );
    assert!(
        lineage.trace.frontier_left > 0,
        "the trace says how much was left behind"
    );

    // EDGE BUDGET: the traversal that may cross three edges stops
    // saying so.
    let tight_edges = GraphReadLimits {
        max_edges: 3,
        ..GraphReadLimits::probe()
    };
    let deps = store
        .bounded_dependencies(&tenant, &repository, &tight_edges)
        .await
        .expect("the tight traversal reads");
    assert!(deps.trace.incomplete);
    assert!(
        deps.trace.reasons.contains(&"edge-limit".to_owned()),
        "the edge budget is the reason: {:?}",
        deps.trace.reasons
    );
    assert!(deps.edges.len() <= 3, "the answer carries at most the cap");

    // NODE CAP: two nodes, and the walk says it stopped there.
    let tight_nodes = GraphReadLimits {
        max_nodes: 2,
        ..GraphReadLimits::probe()
    };
    let deps = store
        .bounded_dependencies(&tenant, &repository, &tight_nodes)
        .await
        .expect("the tight traversal reads");
    assert!(deps.trace.incomplete);
    assert!(
        deps.trace.reasons.contains(&"node-limit".to_owned()),
        "the node cap is the reason: {:?}",
        deps.trace.reasons
    );
    assert!(deps.nodes.len() <= 2);

    // ROUND-TRIP BUDGET: the walk stops as soon as the budget runs out.
    let tight_budget = GraphReadLimits {
        max_round_trips: 2,
        ..GraphReadLimits::probe()
    };
    let deps = store
        .bounded_dependencies(&tenant, &repository, &tight_budget)
        .await
        .expect("the budgeted traversal reads");
    assert!(deps.trace.incomplete);
    assert!(
        deps.trace.reasons.contains(&"budget-limit".to_owned()),
        "the budget is the reason: {:?}",
        deps.trace.reasons
    );
    assert!(
        deps.trace.round_trips <= 2,
        "the walk spent no more than its budget"
    );

    // The probe bound completes the same walk.
    let whole = store
        .bounded_dependencies(&tenant, &repository, &GraphReadLimits::probe())
        .await
        .expect("the probe traversal reads");
    assert!(!whole.trace.incomplete);
    assert_eq!(whole.trace.reasons, Vec::<String>::new());
    assert_eq!(whole.trace.frontier_left, 0);
    assert_eq!(whole.edges.len(), 5);
    store.clear().await.expect("the graph empties");
}

/// The p95 of a bounded read against an unbounded one, over a
/// small-to-medium synthetic graph: 1 repository, 25 revisions, 25
/// builds, 25 artefacts, 25 deployments, 125 relations. The numbers go
/// to the evidence; the assertion keeps the comparison honest.
#[tokio::test]
async fn bounded_reads_compare_p95_against_unbounded() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let store = DgraphGraphStore::new(client.clone());
    let tenant = tenant(&format!("p95{}", &nonce()[..6]));
    let width = 25;

    let mut changes = GraphChangeSet::empty().with_node(node(&tenant, NodeKind::Evidence, "ev-b"));
    let repository = hrn_of(&tenant, "Repository", "r-main");
    changes = changes.with_node(node(&tenant, NodeKind::Repository, "r-main"));
    for index in 0..width {
        let revision = hrn_of(&tenant, "SourceRevision", &format!("rev-{index}"));
        let build = hrn_of(&tenant, "Build", &format!("b-{index}"));
        let artifact = hrn_of(&tenant, "Artifact", &format!("a-{index}"));
        let deployment = hrn_of(&tenant, "Deployment", &format!("d-{index}"));
        changes = changes
            .with_node(node(
                &tenant,
                NodeKind::SourceRevision,
                &format!("rev-{index}"),
            ))
            .with_node(node(&tenant, NodeKind::Build, &format!("b-{index}")))
            .with_node(node(&tenant, NodeKind::Artifact, &format!("a-{index}")))
            .with_node(node(&tenant, NodeKind::Deployment, &format!("d-{index}")))
            .with_relation(relation(
                &tenant,
                RelationType::HasRevision,
                &repository,
                &revision,
                "ev-b",
            ))
            .with_relation(relation(
                &tenant,
                RelationType::Triggered,
                &revision,
                &build,
                "ev-b",
            ))
            .with_relation(relation(
                &tenant,
                RelationType::UsedSource,
                &build,
                &revision,
                "ev-b",
            ))
            .with_relation(relation(
                &tenant,
                RelationType::Produced,
                &build,
                &artifact,
                "ev-b",
            ))
            .with_relation(relation(
                &tenant,
                RelationType::DeployedAs,
                &artifact,
                &deployment,
                "ev-b",
            ));
    }
    store
        .apply(changes)
        .await
        .expect("the synthetic graph commits");

    let artifact = hrn_of(&tenant, "Artifact", "a-12");
    let repository_hrn = repository.clone();
    let rounds = 25u32;

    let p95_of = |mut samples: Vec<Duration>| {
        samples.sort();
        samples
            .get(((samples.len() as f64) * 0.95).floor() as usize)
            .copied()
            .unwrap_or(*samples.last().unwrap_or(&Duration::ZERO))
    };

    // The unbounded read of the reference port: the lineage walk that
    // asks the store once per node.
    let mut unbounded = Vec::new();
    for _ in 0..rounds {
        let started = Instant::now();
        let lineage = hodei_evidence::ports::graph::GraphQuery::lineage(&store, &tenant, &artifact)
            .await
            .expect("the unbounded lineage reads");
        unbounded.push(started.elapsed());
        assert_eq!(lineage.builds.len(), 1);
    }
    // The bounded read with the probe bound: the walk at the server,
    // one batched expansion per hop.
    let mut probe_samples = Vec::new();
    for _ in 0..rounds {
        let started = Instant::now();
        let lineage = store
            .bounded_lineage(&tenant, &artifact, &GraphReadLimits::probe())
            .await
            .expect("the probe lineage reads");
        probe_samples.push(started.elapsed());
        assert!(!lineage.trace.incomplete);
    }
    // The bounded read with the default bounds.
    let mut bounded_samples = Vec::new();
    for _ in 0..rounds {
        let started = Instant::now();
        let lineage = store
            .bounded_lineage(&tenant, &artifact, &GraphReadLimits::default())
            .await
            .expect("the bounded lineage reads");
        bounded_samples.push(started.elapsed());
        assert!(!lineage.trace.incomplete);
    }
    // The bounded dependency traversal from the repository: five hops,
    // one batched expansion each.
    let mut deps_samples = Vec::new();
    for _ in 0..rounds {
        let started = Instant::now();
        let deps = store
            .bounded_dependencies(&tenant, &repository_hrn, &GraphReadLimits::probe())
            .await
            .expect("the traversal reads");
        deps_samples.push(started.elapsed());
        assert_eq!(deps.edges.len(), width * 5);
    }

    let p95_unbounded = p95_of(unbounded);
    let p95_probe = p95_of(probe_samples);
    let p95_bounded = p95_of(bounded_samples);
    let p95_deps = p95_of(deps_samples);
    eprintln!(
        "HSP200 p95 lineage unbounded: {} us | bounded probe: {} us | bounded default: {} us | deps probe: {} us",
        p95_unbounded.as_micros(),
        p95_probe.as_micros(),
        p95_bounded.as_micros(),
        p95_deps.as_micros()
    );
    println!(
        "HSP200 p95 lineage unbounded: {} us | bounded probe: {} us | bounded default: {} us | deps probe: {} us",
        p95_unbounded.as_micros(),
        p95_probe.as_micros(),
        p95_bounded.as_micros(),
        p95_deps.as_micros()
    );
    assert!(
        p95_probe <= p95_unbounded,
        "the server-side bounded walk is not slower than the per-node walk: {p95_probe:?} vs {p95_unbounded:?}"
    );
    store.clear().await.expect("the graph empties");
}
