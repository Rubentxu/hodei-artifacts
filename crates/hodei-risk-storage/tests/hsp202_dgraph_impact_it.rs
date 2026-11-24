//! HSP-202: the impact view of the risk on the NATIVE Dgraph engine —
//! the same graph the walk reads, no second database.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT like every IT of the stack. What it
//! proves, against the real engine:
//!
//! - PARITY (AC1): the materialized view says what the authoritative
//!   walk says — the same subjects, the same depths — including the
//!   522-subject benchmark fixture, and the Dgraph walk says what the
//!   Surreal reference walk says for the same changesets. THREE
//!   answers, one truth.
//! - SELECTIVE INVALIDATION (AC1): a relation delta, a VEX statement
//!   and a withdrawn support invalidate ONLY the advisories whose route
//!   they touch — the write is per delta, through the frontier of
//!   HSP-199, never a global recompute. A re-delivery repeats nothing.
//! - THE LAG IS SAID (AC2): the watermark of each route is the delta
//!   version it is complete up to; a view left behind answers from the
//!   bounded walk of NOW and says which watermark it substituted, and
//!   `is_current` is false while there is lag. The consumer's own
//!   watermark is explicit state in the engine — a restart resumes
//!   where it said it was.
//! - THE BUDGET IS SAID (AC2): every walk runs under limits and
//!   carries its TraversalTrace; the cost of maintaining the view per
//!   delta is measured and printed for the evidence; a walk that hits
//!   a bound REFUSES to materialize a partial answer.
//!
//! The fixture is the oracle's: a vulnerable component held by N
//! artifacts, so the benchmark case (N=521, 522 subjects with the
//! component) runs against the real engine.

use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use chrono::Utc;
use hodei_evidence::application::projection::ProjectionDelivery;
use hodei_evidence::application::projection::ProjectionFrontier;
use hodei_evidence::domain::delta::{RelationDelta, RelationDeltaKind, RelationDeltaSubscription};
use hodei_evidence::domain::graph::{NodeKind, RelationType};
use hodei_evidence::ports::graph::{GraphReadLimits, GraphWriter};
use hodei_evidence::{
    EvidenceId, GraphChangeSet, GraphNode, GraphRelation, GraphRelationWithdrawal,
};
use hodei_evidence_storage::surreal::SurrealGraphStore;
use hodei_evidence_storage::{DgraphGraphStore, DgraphProjectionFrontier};
use hodei_graph_client::{GraphClient, GraphClientConfig, schema::ensure_ready};
use hodei_risk_storage::dgraph_view::{DgraphImpactView, walk_impact_dgraph};
use hodei_risk_storage::impact_view::Freshness;
use kernel::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::local::Mem;

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
    let started = Instant::now();
    let mut client = GraphClient::connect(config).await.expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    let report = ensure_ready(&client)
        .await
        .expect("the versioned schema boots");
    eprintln!(
        "schema boot: decision={:?} binary_version={} stored_version={:?} applied={} elapsed_ms={}",
        report.decision,
        report.binary_version,
        report.stored_version,
        report.applied,
        started.elapsed().as_millis(),
    );
    Some(Arc::new(client))
}

/// The reference store, on an in-memory Surreal: the parity oracle of
/// the walk, with no dependency on any server.
async fn reference_store() -> SurrealGraphStore<surrealdb::engine::local::Db> {
    let db = Arc::new(Surreal::new::<Mem>(()).await.expect("memory db"));
    db.use_ns("hodei")
        .use_db("hsp202")
        .await
        .expect("the namespace");
    SurrealGraphStore::new(db)
}

fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

fn tenant(tag: &str) -> Hrn {
    let account = format!("it{tag}{}", &nonce()[8..]);
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{account}:Artifact/hsp202"
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
        Utc::now(),
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

/// The component the advisory is about.
fn the_component(tenant: &Hrn) -> Hrn {
    hrn_of(tenant, "Component", "lib-shared")
}

/// The advisory.
fn the_vulnerability(tenant: &Hrn) -> Hrn {
    hrn_of(tenant, "Vulnerability", "GHSA-view-0001")
}

/// An artifact that holds the component.
fn the_artifact(tenant: &Hrn, index: usize) -> Hrn {
    hrn_of(tenant, "Artifact", &format!("app-{index}"))
}

/// The changeset of an artifact that contains the vulnerable component,
/// with its cause — the same shape the Surreal oracle fixture uses, so
/// the same fixture is the same fixture.
fn the_contains(tenant: &Hrn, cause: &str, index: usize) -> GraphChangeSet {
    let evidence = EvidenceId::new(format!("ev-{index}"));
    let shared = EvidenceId::new("ev-shared");
    GraphChangeSet::empty()
        .with_cause(cause)
        .with_node(node(tenant, NodeKind::Component, "lib-shared"))
        .with_node(node(tenant, NodeKind::Vulnerability, "GHSA-view-0001"))
        .with_node(node(tenant, NodeKind::Artifact, &format!("app-{index}")))
        .with_node(node(tenant, NodeKind::Evidence, &format!("ev-{index}")))
        .with_node(node(tenant, NodeKind::Evidence, "ev-shared"))
        .with_relation(GraphRelation::new(
            RelationType::Contains,
            the_artifact(tenant, index),
            the_component(tenant),
            tenant.clone(),
            evidence,
            Utc::now(),
        ))
        .with_relation(GraphRelation::new(
            RelationType::AffectedBy,
            the_component(tenant),
            the_vulnerability(tenant),
            tenant.clone(),
            shared,
            Utc::now(),
        ))
}

/// The withdrawal of the support of ONE Contains relation, with its
/// cause — the way the evidence store retires it.
fn the_withdrawal(tenant: &Hrn, cause: &str, index: usize) -> GraphChangeSet {
    GraphChangeSet::empty()
        .with_cause(cause)
        .with_withdrawal(GraphRelationWithdrawal::new(
            RelationType::Contains,
            the_artifact(tenant, index),
            the_component(tenant),
            tenant.clone(),
            EvidenceId::new(format!("ev-{index}")),
            Utc::now(),
        ))
}

/// The changeset of a VEX statement: a NEW component the advisory is
/// about, held by a NEW artifact, asserted with its own evidence.
fn the_vex_statement(tenant: &Hrn, cause: &str, artifact_index: usize) -> GraphChangeSet {
    let component = hrn_of(tenant, "Component", &format!("lib-vex-{artifact_index}"));
    let artifact = the_artifact(tenant, artifact_index);
    GraphChangeSet::empty()
        .with_cause(cause)
        .with_node(node(
            tenant,
            NodeKind::Component,
            &format!("lib-vex-{artifact_index}"),
        ))
        .with_node(node(
            tenant,
            NodeKind::Artifact,
            &format!("app-{artifact_index}"),
        ))
        .with_node(node(
            tenant,
            NodeKind::Evidence,
            &format!("ev-vex-{artifact_index}"),
        ))
        .with_node(node(
            tenant,
            NodeKind::Evidence,
            &format!("ev-vex-b-{artifact_index}"),
        ))
        .with_relation(GraphRelation::new(
            RelationType::Contains,
            artifact,
            component.clone(),
            tenant.clone(),
            EvidenceId::new(format!("ev-vex-{artifact_index}")),
            Utc::now(),
        ))
        .with_relation(GraphRelation::new(
            RelationType::AffectedBy,
            component,
            the_vulnerability(tenant),
            tenant.clone(),
            EvidenceId::new(format!("ev-vex-{artifact_index}")),
            Utc::now(),
        ))
}

/// A VEX re-assertion: the SAME AffectedBy relation, a NEW evidence —
/// the support changes, the identity does not.
fn the_vex_reassert(tenant: &Hrn, cause: &str, artifact_index: usize) -> GraphChangeSet {
    let component = hrn_of(tenant, "Component", &format!("lib-vex-{artifact_index}"));
    GraphChangeSet::empty()
        .with_cause(cause)
        .with_node(node(
            tenant,
            NodeKind::Evidence,
            &format!("ev-vex-b-{artifact_index}"),
        ))
        .with_relation(GraphRelation::new(
            RelationType::AffectedBy,
            component,
            the_vulnerability(tenant),
            tenant.clone(),
            EvidenceId::new(format!("ev-vex-b-{artifact_index}")),
            Utc::now(),
        ))
}

/// One delivery through the frontier: the projector consumer of the
/// tenant stream, at the given position.
fn delivery_of(stream: &str, position: u64, event: &str) -> ProjectionDelivery {
    ProjectionDelivery {
        consumer: "graph-projector".to_owned(),
        stream_id: stream.to_owned(),
        position,
        event_id: event.to_owned(),
    }
}

/// The subscription of the risk consumer: its tenant, the closed set of
/// relation types it invalidates for.
fn the_subscription(tenant: &Hrn) -> RelationDeltaSubscription {
    RelationDeltaSubscription::new(
        tenant.clone(),
        [
            RelationType::Contains,
            RelationType::DependsOn,
            RelationType::AffectedBy,
            RelationType::DeployedAs,
        ],
    )
}

/// The subject set the reference oracle compares with: the artifacts at
/// their depth, the components at depth zero, sorted.
fn subjects_of_radius(radius: &hodei_risk::case::BlastRadius) -> Vec<(String, usize)> {
    let mut subjects: Vec<(String, usize)> = radius
        .affected_subjects
        .iter()
        .map(|subject| (subject.clone(), 1usize))
        .collect();
    for component in &radius.affected_components {
        subjects.push((component.clone(), 0usize));
    }
    subjects.sort();
    subjects
}

/// One serial lane per binary: the tests share the engine, and the
/// timings the evidence records are cleaner one after another.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// AC1 — the view materializes what the walk says, including the
/// 522-subject benchmark case: the Dgraph walk, the Surreal reference
/// walk and the materialized view agree on the SAME fixture, and the
/// read through the view carries the watermark the walk ran at.
#[tokio::test]
async fn the_view_says_what_the_walk_says_including_522_subjects() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let tenant = tenant("522");
    let namespace = format!("ns-{}", tenant.account_id());
    let stream = format!("tenant-{}", tenant.account_id());
    let graph = Arc::new(DgraphGraphStore::new(client.clone()));
    let frontier = Arc::new(
        DgraphProjectionFrontier::open(client.clone(), &namespace)
            .await
            .expect("the frontier opens"),
    );
    let view = DgraphImpactView::new(client.clone(), graph.clone(), frontier.clone());
    let subscription = the_subscription(&tenant);

    // The benchmark load, through the frontier: every artifact that
    // holds the vulnerable component is ONE delivery, and the frontier
    // says its deltas as it goes.
    let started = Instant::now();
    for index in 0..521 {
        frontier
            .apply_and_checkpoint(
                the_contains(&tenant, "evt-load", index),
                &delivery_of(&stream, index as u64, "evt-load"),
            )
            .await
            .expect("the load goes through the frontier");
    }
    let load_ms = started.elapsed().as_millis();
    let said = view.said_version(&tenant).await.expect("the version");
    eprintln!(
        "HSP202 AC1: 521 deliveries through the frontier in {load_ms}ms, deltas said up to version {said}"
    );

    // The authoritative walk on the NATIVE engine, under limits.
    let limits = GraphReadLimits::probe();
    let started = Instant::now();
    let walk = walk_impact_dgraph(
        graph.as_ref(),
        &tenant,
        &the_vulnerability(&tenant),
        &limits,
    )
    .await
    .expect("the native walk runs");
    let walk_ms = started.elapsed().as_millis();
    eprintln!(
        "HSP202 AC1: native walk {} subjects in {walk_ms}ms -- trace: hops={} round_trips={} nodes_seen={} edges_seen={} incomplete={}",
        walk.snapshot_subjects().len(),
        walk.trace.hops_taken,
        walk.trace.round_trips,
        walk.trace.nodes_seen,
        walk.trace.edges_seen,
        walk.trace.incomplete,
    );
    assert!(
        !walk.trace.incomplete,
        "the walk answers whole: {:?}",
        walk.trace.reasons
    );
    assert_eq!(walk.snapshot_subjects().len(), 522, "the benchmark case");

    // The reference walk on the Surreal store, with the SAME changesets.
    let reference = reference_store().await;
    for index in 0..521 {
        reference
            .apply(the_contains(&tenant, "evt-load", index))
            .await
            .expect("the reference load");
    }
    let radius = hodei_risk_storage::blast_radius(&reference, &tenant, &the_vulnerability(&tenant))
        .await
        .expect("the reference walk runs");
    let reference_subjects = subjects_of_radius(&radius);
    assert_eq!(reference_subjects.len(), 522, "the reference case too");
    assert_eq!(
        walk.snapshot_subjects(),
        reference_subjects,
        "the native walk says what the reference walk says"
    );

    // The view, materialized from the same walk.
    let started = Instant::now();
    let snapshot = view
        .rebuild_advisory(&tenant, &the_vulnerability(&tenant))
        .await
        .expect("the view materializes");
    let rebuild_ms = started.elapsed().as_millis();
    eprintln!(
        "HSP202 AC1: the view of 522 subjects materialized in {rebuild_ms}ms at watermark {}",
        snapshot.watermark
    );
    assert_eq!(
        snapshot.subjects,
        walk.snapshot_subjects(),
        "the view says what the walk says"
    );
    assert_eq!(
        snapshot.watermark, said,
        "the watermark is the version the view is complete up to"
    );

    // The read through the view: the same, and current — nothing moved
    // since the rebuild.
    let read = view
        .read(&tenant, &the_vulnerability(&tenant))
        .await
        .expect("the read runs")
        .expect("the view is there");
    assert_eq!(read.subjects, snapshot.subjects);
    assert_eq!(read.watermark, snapshot.watermark);
    assert!(
        view.is_current(&tenant).await.expect("the check runs"),
        "rebuilt at the head, the view is current"
    );
    let answer = view
        .serve_impact(&tenant, &the_vulnerability(&tenant))
        .await
        .expect("the answer runs");
    assert_eq!(
        answer.freshness,
        Freshness::Current,
        "a view at the head presents as current, and only then"
    );
    assert_eq!(answer.subjects.len(), 522);
    assert_eq!(answer.version, said);

    // The delta the load said is CONSUMABLE from the frontier, and the
    // consumer's watermark is explicit state: not started, then set.
    let before = view
        .consumer_watermark("risk-impact", &stream)
        .await
        .expect("the watermark");
    assert!(before.is_none(), "the consumer has not started: {before:?}");
    let (batch, stopped) = frontier
        .pending_deltas(&subscription, 0, 1_000)
        .await
        .expect("the deltas are readable");
    eprintln!(
        "HSP202 AC1: the frontier offers {} deltas after version 0, reader resumes at {stopped}",
        batch.len(),
    );
    assert!(!batch.is_empty(), "the load said deltas");
    let last = batch.last().expect("the batch is not empty").clone();
    view.advance_consumer_watermark("risk-impact", &stream, last.0, &last.1.cause)
        .await
        .expect("the watermark advances");
    let after = view
        .consumer_watermark("risk-impact", &stream)
        .await
        .expect("the watermark");
    assert_eq!(
        after,
        Some(last.0 + 1),
        "the consumer said where it is: {after:?}"
    );
}

/// AC1 — a relation delta invalidates ONLY the advisory whose route it
/// touches, a foreign account's delta invalidates nothing, and the
/// re-delivery of the same delta repeats nothing. The cost of the
/// maintenance per delta is measured for the evidence.
#[tokio::test]
async fn a_delta_invalidates_only_its_scope_and_the_replay_repeats_nothing() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let tenant = tenant("delta");
    let namespace = format!("ns-{}", tenant.account_id());
    let graph = Arc::new(DgraphGraphStore::new(client.clone()));
    let frontier = Arc::new(
        DgraphProjectionFrontier::open(client.clone(), &namespace)
            .await
            .expect("the frontier opens"),
    );
    let view = DgraphImpactView::new(client.clone(), graph.clone(), frontier.clone());

    for index in 0..4 {
        frontier
            .apply_and_checkpoint(
                the_contains(&tenant, "evt-load", index),
                &delivery_of(
                    &format!("tenant-{}", tenant.account_id()),
                    index as u64,
                    "evt-load",
                ),
            )
            .await
            .expect("the load goes through the frontier");
    }
    let snapshot = view
        .rebuild_advisory(&tenant, &the_vulnerability(&tenant))
        .await
        .expect("the view materializes");
    assert_eq!(
        snapshot.subjects.len(),
        5,
        "four artifacts and the component"
    );

    // The delta of a relation the route used: the advisory IS
    // invalidated — by the relation identity, the evidence and the
    // endpoints it carries.
    let route_delta = RelationDelta {
        kind: RelationDeltaKind::Removed,
        tenant: tenant.clone(),
        relation: RelationType::Contains,
        from: the_artifact(&tenant, 2),
        to: the_component(&tenant),
        evidence: EvidenceId::new("ev-2"),
        cause: "evt-withdraw".to_owned(),
        observed_at: Utc::now(),
    };
    let started = Instant::now();
    let touched = view
        .invalidated_by(&route_delta)
        .await
        .expect("the lookup runs");
    let lookup_ms = started.elapsed().as_millis();
    assert_eq!(
        touched,
        vec![the_vulnerability(&tenant)],
        "the advisory whose route used the relation is invalidated: {touched:?}"
    );
    let started = Instant::now();
    let touched = view
        .apply_delta(&route_delta)
        .await
        .expect("the invalidation runs");
    let delta_ms = started.elapsed().as_millis();
    eprintln!(
        "HSP202 AC2: one delta invalidated in {delta_ms}ms (route lookup {lookup_ms}ms, then one bounded walk of {} round trips and one route write)",
        view.limits().max_round_trips,
    );
    assert_eq!(touched, vec![the_vulnerability(&tenant)]);

    // The delta of another account: NOTHING of this view moves.
    let foreign_delta = RelationDelta {
        kind: RelationDeltaKind::Added,
        tenant: Hrn::from_string("hrn:hodei:artifacts:eu-west-1:mallory:Artifact/mallory")
            .expect("an HRN"),
        relation: RelationType::Contains,
        from: the_artifact(&tenant, 0),
        to: the_component(&tenant),
        evidence: EvidenceId::new("ev-foreign"),
        cause: "evt-foreign".to_owned(),
        observed_at: Utc::now(),
    };
    let untouched = view
        .apply_delta(&foreign_delta)
        .await
        .expect("the invalidation runs");
    assert!(
        untouched.is_empty(),
        "another account's delta invalidates nothing here: {untouched:?}"
    );

    // The re-delivery: the same delta again is the same work once, and
    // the view is left the same, not doubled.
    let before = view
        .read(&tenant, &the_vulnerability(&tenant))
        .await
        .expect("the read runs")
        .expect("the view is there");
    let again = view
        .apply_delta(&route_delta)
        .await
        .expect("the re-delivery runs");
    assert_eq!(
        again,
        vec![the_vulnerability(&tenant)],
        "the re-delivery invalidates the same"
    );
    let after = view
        .read(&tenant, &the_vulnerability(&tenant))
        .await
        .expect("the read runs")
        .expect("the view is there");
    assert_eq!(
        before.subjects, after.subjects,
        "recomputing the same leaves the same"
    );
    assert_eq!(before.watermark, after.watermark);
}

/// AC1 — the retired support takes its subject out of the view, the VEX
/// statement brings its subject in, and the VEX re-assertion changes
/// nothing but the evidence: every one of them through the frontier's
/// deltas, and only their scope moves.
#[tokio::test]
async fn a_withdrawal_and_a_vex_only_reach_their_scope() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let tenant = tenant("vex");
    let namespace = format!("ns-{}", tenant.account_id());
    let stream = format!("tenant-{}", tenant.account_id());
    let graph = Arc::new(DgraphGraphStore::new(client.clone()));
    let frontier = Arc::new(
        DgraphProjectionFrontier::open(client.clone(), &namespace)
            .await
            .expect("the frontier opens"),
    );
    let view = DgraphImpactView::new(client.clone(), graph.clone(), frontier.clone());
    let subscription = the_subscription(&tenant);

    for index in 0..3 {
        frontier
            .apply_and_checkpoint(
                the_contains(&tenant, "evt-load", index),
                &delivery_of(&stream, index as u64, "evt-load"),
            )
            .await
            .expect("the load goes through the frontier");
    }
    let before = view
        .rebuild_advisory(&tenant, &the_vulnerability(&tenant))
        .await
        .expect("the view materializes");
    assert_eq!(
        before.subjects.len(),
        4,
        "three artifacts and the component"
    );

    // The retirement goes through the graph AND the frontier: the
    // support of app-2 is withdrawn, the relation turns UNKNOWN, and
    // the frontier says its Removed delta.
    let mut position = 3u64;
    frontier
        .apply_and_checkpoint(
            the_withdrawal(&tenant, "evt-withdraw", 2),
            &delivery_of(&stream, position, "evt-withdraw"),
        )
        .await
        .expect("the retirement goes through the frontier");
    position += 1;

    // The risk consumer consumes what the frontier said, from where it
    // said it was, and the view invalidates ONLY the touched advisory.
    let (batch, _stopped) = frontier
        .pending_deltas(&subscription, 0, 1_000)
        .await
        .expect("the deltas are readable");
    let removed = batch
        .iter()
        .find(|(_, delta)| delta.kind == RelationDeltaKind::Removed)
        .expect("the retirement said its delta");
    assert_eq!(
        removed.1.from,
        the_artifact(&tenant, 2),
        "the retired support is the delta's"
    );
    let touched = view
        .apply_delta(&removed.1)
        .await
        .expect("the invalidation runs");
    assert_eq!(touched, vec![the_vulnerability(&tenant)]);

    let after = view
        .read(&tenant, &the_vulnerability(&tenant))
        .await
        .expect("the read runs")
        .expect("the view is there");
    assert_eq!(after.subjects.len(), 3, "the retired subject left the view");
    assert!(
        after
            .subjects
            .iter()
            .all(|(subject, _)| subject.as_str() != the_artifact(&tenant, 2).to_string()),
        "the retired subject is not presented as affected: {:?}",
        after.subjects
    );

    // The VEX statement: a NEW component the advisory is about. The
    // view invalidates by the endpoints of the NEW edge and the
    // subject enters — an edge the routes never knew invalidates too.
    frontier
        .apply_and_checkpoint(
            the_vex_statement(&tenant, "vex-statement-1", 9),
            &delivery_of(&stream, position, "vex-statement-1"),
        )
        .await
        .expect("the VEX goes through the frontier");
    position += 1;
    let (batch, _stopped) = frontier
        .pending_deltas(&subscription, 0, 1_000)
        .await
        .expect("the deltas are readable");
    let mut vex_touched: Vec<Hrn> = Vec::new();
    for (seq, delta) in &batch {
        let hit = view
            .apply_delta(delta)
            .await
            .expect("the invalidation runs");
        assert_eq!(
            hit,
            vec![the_vulnerability(&tenant)],
            "the VEX delta touches the advisory"
        );
        vex_touched = hit;
        view.advance_consumer_watermark("risk-impact", &stream, *seq, &delta.cause)
            .await
            .expect("the watermark advances");
    }
    assert_eq!(vex_touched, vec![the_vulnerability(&tenant)]);

    let with_vex = view
        .read(&tenant, &the_vulnerability(&tenant))
        .await
        .expect("the read runs")
        .expect("the view is there");
    assert_eq!(
        with_vex.subjects.len(),
        5,
        "the VEX subject and its artifact entered: {:?}",
        with_vex.subjects
    );
    assert!(
        with_vex
            .subjects
            .iter()
            .any(|(subject, depth)| subject == &the_component(&tenant).to_string() && *depth == 0),
        "the VEX component is a depth-zero subject"
    );
    assert!(
        with_vex
            .subjects
            .iter()
            .any(
                |(subject, depth)| subject.as_str() == the_artifact(&tenant, 9).to_string()
                    && *depth == 1
            ),
        "the artifact that holds the VEX component is at depth one"
    );

    // The VEX re-assertion: the SAME relation, a NEW evidence. The
    // advisory invalidates by its route, and the subjects stay.
    frontier
        .apply_and_checkpoint(
            the_vex_reassert(&tenant, "vex-reassert-1", 9),
            &delivery_of(&stream, position, "vex-reassert-1"),
        )
        .await
        .expect("the re-assertion goes through the frontier");
    let said = view.said_version(&tenant).await.expect("the version");
    let (batch, _stopped) = frontier
        .pending_deltas(&subscription, 0, 1_000)
        .await
        .expect("the deltas are readable");
    let changed = batch
        .iter()
        .find(|(_, delta)| delta.kind == RelationDeltaKind::SupportChanged)
        .expect("the re-assertion said its SupportChanged delta");
    let touched = view
        .apply_delta(&changed.1)
        .await
        .expect("the invalidation runs");
    assert_eq!(touched, vec![the_vulnerability(&tenant)]);
    let rebuilt = view
        .read(&tenant, &the_vulnerability(&tenant))
        .await
        .expect("the read runs")
        .expect("the view is there");
    assert_eq!(
        rebuilt.subjects, with_vex.subjects,
        "a re-assertion changes no subject"
    );
    assert_eq!(
        rebuilt.watermark, said,
        "the view is complete up to the version said"
    );
}

/// AC2 — the lag is said, never hidden: a view left behind answers
/// from the bounded walk of NOW with the watermark it substituted, and
/// the consumer resumes from the watermark the engine stores for it.
#[tokio::test]
async fn the_lag_is_said_and_the_consumer_resumes_where_it_said_it_was() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let tenant = tenant("lag");
    let namespace = format!("ns-{}", tenant.account_id());
    let stream = format!("tenant-{}", tenant.account_id());
    let graph = Arc::new(DgraphGraphStore::new(client.clone()));
    let frontier = Arc::new(
        DgraphProjectionFrontier::open(client.clone(), &namespace)
            .await
            .expect("the frontier opens"),
    );
    let view = DgraphImpactView::new(client.clone(), graph.clone(), frontier.clone());
    let subscription = the_subscription(&tenant);

    frontier
        .apply_and_checkpoint(
            the_contains(&tenant, "evt-load", 0),
            &delivery_of(&stream, 0, "evt-load"),
        )
        .await
        .expect("the load goes through the frontier");
    view.rebuild_advisory(&tenant, &the_vulnerability(&tenant))
        .await
        .expect("the view materializes");
    assert!(
        view.is_current(&tenant).await.expect("the check runs"),
        "a view built at the head is current"
    );

    // The graph moves on and the view does NOT consume it: there is
    // lag, and the view says so.
    frontier
        .apply_and_checkpoint(
            the_contains(&tenant, "evt-load", 1),
            &delivery_of(&stream, 1, "evt-load"),
        )
        .await
        .expect("the new support goes through the frontier");
    let said = view.said_version(&tenant).await.expect("the version");
    assert!(
        !view.is_current(&tenant).await.expect("the check runs"),
        "there is lag, and the view does not pretend otherwise"
    );
    let answer = view
        .serve_impact(&tenant, &the_vulnerability(&tenant))
        .await
        .expect("the answer runs");
    match &answer.freshness {
        Freshness::Authoritative { view_watermark } => {
            assert!(
                *view_watermark < said,
                "the lag is said: {view_watermark} < {said}"
            );
        }
        other => panic!("a view left behind is not presented as current: {other:?}"),
    }
    assert_eq!(
        answer.version, said,
        "the answer is complete up to the version said"
    );
    assert!(
        answer
            .subjects
            .iter()
            .any(|subject| subject.subject.as_str() == the_artifact(&tenant, 1).to_string()),
        "the fallback says what the walk says NOW, with the new subject: {:?}",
        answer.subjects
    );

    // The consumer resumes from the watermark the engine stores for
    // it, closes the lag, and the view is current again.
    let (batch, stopped) = frontier
        .pending_deltas(&subscription, 0, 1_000)
        .await
        .expect("the deltas are readable");
    assert!(!batch.is_empty());
    for (seq, delta) in &batch {
        view.apply_delta(delta)
            .await
            .expect("the invalidation runs");
        view.advance_consumer_watermark("risk-impact", &stream, *seq, &delta.cause)
            .await
            .expect("the watermark advances");
    }
    let where_it_is = view
        .consumer_watermark("risk-impact", &stream)
        .await
        .expect("the watermark")
        .expect("the consumer started");
    assert!(
        where_it_is >= stopped,
        "the consumer is at or past the version it read: {where_it_is} >= {stopped}"
    );
    assert!(
        view.is_current(&tenant).await.expect("the check runs"),
        "the lag closed, the view is current"
    );
    let answer = view
        .serve_impact(&tenant, &the_vulnerability(&tenant))
        .await
        .expect("the answer runs");
    assert_eq!(answer.freshness, Freshness::Current);

    // The restart: a fresh process reads the SAME watermark from the
    // engine and resumes there — no re-processing from zero, no
    // guessing.
    let resumed = DgraphProjectionFrontier::open(client.clone(), &namespace)
        .await
        .expect("the frontier opens again");
    let resumed_at = resumed
        .checkpoint_position("risk-impact", &stream)
        .await
        .expect("the checkpoint")
        .expect("the consumer left its watermark");
    assert_eq!(
        resumed_at, where_it_is,
        "a restart resumes where it said it was"
    );

    // The re-delivery of the LAST watermark advance: the frontier
    // answers with the committed receipt and writes nothing — the
    // once-only contract of the watermark too.
    let last = batch.last().expect("the batch is not empty").clone();
    let receipt = view
        .advance_consumer_watermark("risk-impact", &stream, last.0, &last.1.cause)
        .await
        .expect("the re-delivery answers");
    assert!(
        receipt.replayed,
        "the same watermark advance is replayed, not written again"
    );
    assert_eq!(
        receipt.checkpoint, where_it_is,
        "the checkpoint does not regress on a re-delivery"
    );

    // And a tenant that never materialized anything is not current:
    // there is no view to present as anything.
    let other = Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:itnobody{}:Artifact/hsp202",
        &nonce()[8..]
    ))
    .expect("an HRN");
    assert!(
        !view.is_current(&other).await.expect("the check runs"),
        "without a view there is no currency"
    );
    assert!(
        view.read(&other, &the_vulnerability(&tenant))
            .await
            .expect("the read runs")
            .is_none(),
        "without a view there is no stale answer to disguise"
    );
}

/// AC2 — a walk that hit a bound is REFUSED: a partial answer is never
/// materialized as complete, never served as the authoritative one,
/// and the view the limits could not rebuild stays exactly as it was.
#[tokio::test]
async fn a_walk_that_hits_its_bounds_is_refused_not_best_effort() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    let tenant = tenant("bounds");
    let namespace = format!("ns-{}", tenant.account_id());
    let graph = Arc::new(DgraphGraphStore::new(client.clone()));
    let frontier = Arc::new(
        DgraphProjectionFrontier::open(client.clone(), &namespace)
            .await
            .expect("the frontier opens"),
    );
    let view = DgraphImpactView::new(client.clone(), graph.clone(), frontier.clone());

    for index in 0..2 {
        frontier
            .apply_and_checkpoint(
                the_contains(&tenant, "evt-load", index),
                &delivery_of(
                    &format!("tenant-{}", tenant.account_id()),
                    index as u64,
                    "evt-load",
                ),
            )
            .await
            .expect("the load goes through the frontier");
    }
    let snapshot = view
        .rebuild_advisory(&tenant, &the_vulnerability(&tenant))
        .await
        .expect("the view materializes");
    assert_eq!(
        snapshot.subjects.len(),
        3,
        "two artifacts and the component"
    );

    // The same view, under bounds no walk can satisfy: the rebuild
    // refuses, and the view it could not rebuild is untouched.
    let strict = DgraphImpactView::new(client.clone(), graph.clone(), frontier.clone())
        .with_limits(GraphReadLimits {
            max_depth: 0,
            max_nodes: 1,
            max_edges: 1,
            max_round_trips: 1,
        });
    let refused = strict
        .rebuild_advisory(&tenant, &the_vulnerability(&tenant))
        .await
        .expect_err("a bound walk is refused");
    eprintln!("HSP202 AC2: the bounded rebuild was refused: {refused}");
    let untouched = view
        .read(&tenant, &the_vulnerability(&tenant))
        .await
        .expect("the read runs")
        .expect("the view is there");
    assert_eq!(
        untouched.subjects, snapshot.subjects,
        "the refusal changed nothing"
    );

    // Serving through the strict view: a CURRENT route is served as it
    // is — reading the stored view needs no walk — and the strict
    // bounds never make a valid answer lie about itself.
    let served = strict
        .serve_impact(&tenant, &the_vulnerability(&tenant))
        .await
        .expect("a current route is served without a walk");
    assert_eq!(served.freshness, Freshness::Current);

    // But the moment the view needs to WALK — the graph moved on and
    // the view is stale — the strict bounds refuse: the fallback says
    // the walk cannot answer whole instead of answering partially.
    frontier
        .apply_and_checkpoint(
            the_contains(&tenant, "evt-late", 7),
            &delivery_of(&format!("tenant-{}", tenant.account_id()), 2, "evt-late"),
        )
        .await
        .expect("the late support goes through the frontier");
    let error = strict
        .serve_impact(&tenant, &the_vulnerability(&tenant))
        .await
        .expect_err("a partial fallback is not an answer");
    assert!(
        matches!(
            error,
            hodei_risk_storage::impact_view::ImpactReadError::Unavailable(_)
        ),
        "the bound is said as unavailability, never as an empty answer: {error:?}"
    );

    // And the same stale moment, under the probe bounds: the fallback
    // walks WHOLE and says the lag it substituted.
    let answer = view
        .serve_impact(&tenant, &the_vulnerability(&tenant))
        .await
        .expect("the answer runs");
    assert!(
        matches!(answer.freshness, Freshness::Authoritative { .. }),
        "the stale view answers from the walk of NOW: {:?}",
        answer.freshness
    );
    assert!(
        answer
            .subjects
            .iter()
            .any(|subject| subject.subject.as_str() == the_artifact(&tenant, 7).to_string()),
        "the fallback reaches the new subject"
    );

    // And the advisory of another account is a closed door through the
    // native view too: a mismatch is an error, not an empty answer.
    let foreign =
        Hrn::from_string("hrn:hodei:artifacts:eu-west-1:mallory:Vulnerability/GHSA-view-0001")
            .expect("an HRN");
    let error = view
        .serve_impact(&tenant, &foreign)
        .await
        .expect_err("the door is closed");
    assert!(
        matches!(
            error,
            hodei_risk_storage::impact_view::ImpactReadError::TenantMismatch { .. }
        ),
        "the mismatch is said as such: {error:?}"
    );
}
