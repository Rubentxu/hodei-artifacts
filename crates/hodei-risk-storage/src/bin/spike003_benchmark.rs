//! The measurement of the SPIKE-003: is a second store worth it?
//!
//! The spike asks whether an **analytic projection** --a second store, one
//! that is built for the queries of several hops-- improves materially the
//! multi-hop and the dependency queries, enough to justify the permanent
//! operational cost of another store.
//!
//! The condition of the spike is explicit: compare **only** when there is a
//! reference load. And the comparison that matters is not "the projection
//! against an example": it is the **transactional graph with its indexes**
//! against **the same graph without them**, because that is the
//! difference between what the deployment has and what it could have with
//! a second store.
//!
//! The measurement runs BY HAND, not in the ordinary CI: generating the
//! reference load takes minutes.
//!
//! # The command
//!
//! ```text
//! HODEI_SCALE_MODEL=custom HODEI_SCALE_ACCOUNTS=2 HODEI_SCALE_SIZE=10 \
//!   cargo run --release -p hodei-risk-storage --bin spike003_benchmark
//! ```
//!
//! And with the indexes of the graph absent, which is the other half of
//! the comparison:
//!
//! ```text
//! HODEI_SCALE_INDEX=off cargo run --release -p hodei-evidence-storage --bin spike003_benchmark
//! ```

use hodei_evidence::application::projection::project_stream;
use hodei_evidence::domain::graph::{
    GraphChangeSet, GraphNode, GraphRelation, NodeKind, RelationType,
};
use hodei_evidence::ports::graph::{GraphQuery, GraphWriter};
use hodei_evidence::{
    ContentDigest, EvidenceGraphProjector, EvidenceIngestion, EvidenceVerification,
    IngestionEnvelope, IngestionRequest, Normalization, Producer, SourceRef,
};
use hodei_evidence_storage::scale::ScaleModel;
use hodei_evidence_storage::surreal::graph::{GRAPH_INDEXES, declare_index_statement};
use hodei_evidence_storage::{
    HmacEnvelopeAuthenticator, InMemoryContentStore, ProducerCredentials, SurrealEventLedger,
    SurrealEvidenceStore, SurrealGraphStore, SurrealProjectionCheckpoints,
};
use kernel::Hrn;
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};
use surrealdb::Surreal;
use surrealdb::engine::local::RocksDb;

/// How many times each query is measured.
const ROUNDS: usize = 50;

/// The account the measurement runs on.
const ACCOUNT: &str = "acme";

/// The stream the projection reads.
const STREAM: &str = "tenant-acme";

/// How many components each artefact holds.
///
/// The number is not arbitrary: it is the shape of an artefact with a
/// dependency tree of a realistic size, and the spike compares the cost
/// of the walk with the cost of the load.
const COMPONENTS_PER_ARTEFACT: usize = 3;

/// How deep the chain of dependencies goes.
///
/// A chain is the shape that makes the walk long: the blast radius of a
/// vulnerability at the bottom of the chain crosses every artefact that
/// holds it.
const DEPENDENCY_DEPTH: usize = 2;

/// The target of the queries: the percentil 95 of the blast radius.
///
/// The target is written **before** the measurement and it is not adjusted
/// after seeing the number. The lineage answers in a p95 of 3.29 ms at
/// the reference scale with the indexes, and the blast radius is a walk
/// of a few hops over a bounded set of nodes: a target of 50 ms is
/// fifteen times the lineage and it is the number that a second store
/// would have to beat to be worth its cost.
const TARGET_P95_MS: u128 = 50;

/// The percentile of a list of measurements.
fn percentile(values: &mut [Duration], fraction: f64) -> Duration {
    if values.is_empty() {
        return Duration::ZERO;
    }
    values.sort_unstable();
    let index = ((values.len() as f64) * fraction).ceil() as usize;
    values[index.saturating_sub(1).min(values.len() - 1)]
}

/// The measurement of a set of queries.
struct Measurement {
    /// The samples.
    values: Vec<Duration>,
}

impl Measurement {
    /// The measurement of a set of times.
    fn of(values: &mut [Duration]) -> Self {
        Self {
            values: values.to_vec(),
        }
    }

    /// The percentiles, in milliseconds.
    fn millis(&self) -> (u128, u128, u128) {
        let mut values = self.values.clone();
        (
            percentile(&mut values, 0.50).as_millis(),
            percentile(&mut values, 0.95).as_millis(),
            values.last().copied().unwrap_or_default().as_millis(),
        )
    }
}

/// The HRN of a node.
fn hrn(kind: &str, id: &str) -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{ACCOUNT}:{kind}/{id}"
    ))
    .expect("an HRN")
}

/// The account of the measurement.
fn account() -> Hrn {
    hrn("Artifact", "acme")
}

/// The model the measurement runs on.
fn the_model() -> ScaleModel {
    let model = match std::env::var("HODEI_SCALE_MODEL").as_deref() {
        Ok("small") => ScaleModel::small(),
        Ok("custom") => {
            // The accounts change and the volumes of the shape do not: the
            // shape of the reference is what makes the load
            // representative.
            let accounts = std::env::var("HODEI_SCALE_ACCOUNTS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(2);
            let size = std::env::var("HODEI_SCALE_SIZE")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(10);
            ScaleModel::of_size(accounts, size)
        }
        _ => ScaleModel::reference(),
    };
    println!("=== The reference scale model ===");
    println!("accounts: {}", model.accounts);
    println!("pieces of evidence expected: {}", model.expected_events());
    model
}

/// How many artefacts hold the vulnerable component.
///
/// The blast radius of a vulnerability is the set of artefacts that hold
/// the vulnerable component, and a vulnerable component that everybody
/// holds is a blast radius that is the deployment entero. The number is
/// a fraction of the load, because that is the shape of a real
/// dependency: a libreria popular esta en muchas partes y una que no
/// lo esta en ninguna.
fn the_vulnerable_share() -> usize {
    std::env::var("HODEI_SPIKE003_SHARE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(50)
}

/// The risk layer of the load.
///
/// It is the layer of the grafo de riesgo: a pool of shared components
/// --the librerias que muchas partes tienen-- and a chain of private
/// components under each artefact. The vulnerability is about one of the
/// shared ones, so the blast radius is the set of artefacts that hold it.
fn risk_layer(artefacts: &[Hrn]) -> GraphChangeSet {
    let mut changes = GraphChangeSet::empty();
    let vulnerability = hrn("Vulnerability", "GHSA-spike003-0001");

    // The shared components: one of them is the vulnerable one.
    let share = the_vulnerable_share().min(artefacts.len());
    let shared: Vec<Hrn> = (0..share)
        .map(|component| hrn("Component", &format!("shared-{component}")))
        .collect();
    let vulnerable = shared
        .first()
        .cloned()
        .unwrap_or_else(|| hrn("Component", "shared-0"));

    for component in &shared {
        changes = changes.with_node(GraphNode::new(
            component.clone(),
            account(),
            NodeKind::Component,
            chrono::Utc::now(),
        ));
    }
    changes = changes.with_node(GraphNode::new(
        vulnerability.clone(),
        account(),
        NodeKind::Vulnerability,
        chrono::Utc::now(),
    ));

    for (index, artefact) in artefacts.iter().enumerate() {
        let evidence_id = hodei_evidence::EvidenceId::new(format!("ev-risk-{index}"));
        changes = changes.with_node(GraphNode::new(
            hodei_evidence::application::projector::evidence_hrn(&account(), &evidence_id),
            account(),
            NodeKind::Evidence,
            chrono::Utc::now(),
        ));

        // The artefact holds a shared component and a chain of its own.
        let shared_of_this = &shared[index % shared.len().max(1)];
        let private: Vec<Hrn> = (0..COMPONENTS_PER_ARTEFACT)
            .map(|component| hrn("Component", &format!("c-{index}-{component}")))
            .collect();
        for component in &private {
            changes = changes.with_node(GraphNode::new(
                component.clone(),
                account(),
                NodeKind::Component,
                chrono::Utc::now(),
            ));
        }

        changes = changes.with_relation(GraphRelation::new(
            RelationType::Contains,
            artefact.clone(),
            shared_of_this.clone(),
            account(),
            evidence_id.clone(),
            chrono::Utc::now(),
        ));

        // And the chain of private components hangs from it.
        let mut previous = shared_of_this.clone();
        for component in &private {
            changes = changes.with_relation(GraphRelation::new(
                RelationType::DependsOn,
                previous.clone(),
                component.clone(),
                account(),
                evidence_id.clone(),
                chrono::Utc::now(),
            ));
            previous = component.clone();
        }
    }

    // And the vulnerability is about the shared component that the first
    // group of artefacts holds. It cites the evidence of the inventory
    // of the first artefact, which is a node of the graph like any
    // other.
    changes = changes.with_relation(GraphRelation::new(
        RelationType::AffectedBy,
        vulnerable,
        vulnerability,
        account(),
        hodei_evidence::EvidenceId::new("ev-risk-0"),
        chrono::Utc::now(),
    ));

    changes
}

/// The dataset of the graph, as the export writes it.
struct TheDataset {
    /// The nodes.
    nodes: Vec<(String, String)>,
    /// The edges.
    edges: Vec<(String, String, String)>,
}

/// The graph, read kind by kind and relation by relation.
async fn the_dataset<C: surrealdb::Connection>(
    graph: &hodei_evidence_storage::SurrealGraphStore<C>,
    tenant: &kernel::Hrn,
) -> TheDataset {
    use hodei_evidence::ports::graph::GraphQuery;

    let mut nodes = Vec::new();
    for kind in [
        hodei_evidence::domain::graph::NodeKind::Repository,
        hodei_evidence::domain::graph::NodeKind::SourceRevision,
        hodei_evidence::domain::graph::NodeKind::Build,
        hodei_evidence::domain::graph::NodeKind::Artifact,
        hodei_evidence::domain::graph::NodeKind::Deployment,
        hodei_evidence::domain::graph::NodeKind::Evidence,
        hodei_evidence::domain::graph::NodeKind::Component,
        hodei_evidence::domain::graph::NodeKind::Vulnerability,
    ] {
        for node in graph
            .nodes(tenant, kind.clone())
            .await
            .expect("the nodes are read")
        {
            nodes.push((the_label_of(kind.clone()), node.id.to_string()));
        }
    }

    let mut response = graph
        .db()
        .query("SELECT from, to, relation_type FROM type::table($table) WHERE tenant_hrn = $tenant")
        .bind(("table", graph.relation_table_name()))
        .bind(("tenant", tenant.to_string()))
        .await
        .expect("the relations are read");
    let rows: Vec<RelationRow> = response.take(0).expect("the relations are read");

    let mut edges = Vec::new();
    for row in rows {
        let Some(relation) = hodei_evidence::domain::graph::RelationType::parse(&row.relation_type)
        else {
            continue;
        };
        edges.push((the_label_of_relation(relation), row.from, row.to));
    }

    TheDataset { nodes, edges }
}

/// The label of a kind of node.
fn the_label_of(kind: hodei_evidence::domain::graph::NodeKind) -> String {
    use hodei_evidence::domain::graph::NodeKind;
    match kind {
        NodeKind::Repository => "Repository".to_string(),
        NodeKind::SourceRevision => "SourceRevision".to_string(),
        NodeKind::Build => "Build".to_string(),
        NodeKind::Artifact => "Artifact".to_string(),
        NodeKind::Deployment => "Deployment".to_string(),
        NodeKind::Evidence => "Evidence".to_string(),
        NodeKind::Component => "Component".to_string(),
        NodeKind::Vulnerability => "Vulnerability".to_string(),
    }
}

/// The label of a kind of relation.
fn the_label_of_relation(relation: hodei_evidence::domain::graph::RelationType) -> String {
    use hodei_evidence::domain::graph::RelationType;
    match relation {
        RelationType::HasRevision => "HasRevision".to_string(),
        RelationType::Triggered => "Triggered".to_string(),
        RelationType::UsedSource => "UsedSource".to_string(),
        RelationType::Produced => "Produced".to_string(),
        RelationType::DeployedAs => "DeployedAs".to_string(),
        RelationType::DescribedBy => "DescribedBy".to_string(),
        RelationType::Contains => "Contains".to_string(),
        RelationType::DependsOn => "DependsOn".to_string(),
        RelationType::AffectedBy => "AffectedBy".to_string(),
    }
}

/// The relation of a row.
#[derive(Debug, Clone, serde::Deserialize)]
struct RelationRow {
    /// The node the relation starts at.
    from: String,
    /// The node the relation ends at.
    to: String,
    /// The kind of the relation.
    relation_type: String,
}

#[tokio::main]
async fn main() {
    let model = the_model();

    let tmp = tempfile::tempdir().expect("a throwaway directory");
    let path = tmp.keep();
    let db = Arc::new(
        Surreal::new::<RocksDb>(path.join("db").to_string_lossy().to_string())
            .await
            .expect("the database opens"),
    );
    db.use_ns("hodei")
        .use_db("evidence")
        .await
        .expect("the namespace");

    // The indexes of the graph are declared by an operator command, and
    // the measurement says what they are worth: the same measurement
    // without them is the deployment that has no indexes, and with them
    // is the deployment that has them.
    let with_indexes = std::env::var("HODEI_SCALE_INDEX").as_deref() != Ok("off");
    if with_indexes {
        for (table, fields) in GRAPH_INDEXES {
            db.query(declare_index_statement(table, fields))
                .await
                .expect("the index is declared")
                .check()
                .expect("the index is declared");
        }
    }
    println!(
        "indexes of the graph: {}",
        if with_indexes { "declared" } else { "absent" }
    );

    let ledger = Arc::new(SurrealEventLedger::new(db.clone()));
    let evidence_store = Arc::new(SurrealEvidenceStore::new(db.clone()));
    let graph = Arc::new(SurrealGraphStore::new(db.clone()));
    let checkpoints = Arc::new(SurrealProjectionCheckpoints::new(db.clone()));
    let content_store = Arc::new(InMemoryContentStore::new());
    let authenticator = Arc::new(HmacEnvelopeAuthenticator::new());
    authenticator.register(
        "pipelinek",
        ProducerCredentials {
            secret: b"the-secret-of-the-producer".to_vec(),
            accounts: vec![ACCOUNT.to_string()],
        },
    );
    let ingestion = EvidenceIngestion::new(
        content_store,
        evidence_store,
        ledger.clone(),
        authenticator.clone(),
    );

    // ── The admission ─────────────────────────────────────────────────────
    let mut total = 0usize;
    let started = Instant::now();
    for index in 0..model.accounts {
        for (external_id, content, kind, claim) in model.journey(ACCOUNT, index) {
            let mut envelope = IngestionEnvelope {
                producer: Producer::new("connector", "pipelinek").with_version("1.2.3"),
                tenant_hrn: account(),
                subject_refs: vec![],
                evidence_type: kind,
                schema_uri: None,
                schema_version: None,
                source: SourceRef::by_external_id(&external_id),
                content_digest: ContentDigest::of_bytes(&content),
                relation_claim: claim,
                observed_at: chrono::Utc::now(),
                valid_from: None,
                verification: EvidenceVerification::verified("signature"),
                normalization: Normalization::new("pipelinek", "1.0.0"),
                correlation_id: None,
                signature: String::new(),
            };
            envelope.signature = authenticator
                .sign(&envelope)
                .expect("the producer is known");
            ingestion
                .ingest(IngestionRequest { envelope, content })
                .await
                .expect("the ingestion runs");
            total += 1;
        }
    }
    println!("\n=== The load ===");
    println!("pieces admitted: {total}");
    println!("time: {:.1} s", started.elapsed().as_secs_f64());

    // ── The projection ────────────────────────────────────────────────────
    let started = Instant::now();
    let report = project_stream(
        ledger.as_ref(),
        &EvidenceGraphProjector::new(),
        graph.as_ref(),
        checkpoints.as_ref(),
        "spike003",
        STREAM,
    )
    .await
    .expect("the projection runs");
    println!("\n=== The projection ===");
    println!(
        "events read: {}, applied: {}",
        report.events_read, report.events_applied
    );
    println!("time: {:.1} s", started.elapsed().as_secs_f64());

    // ── The risk layer ────────────────────────────────────────────────────
    let artefacts = graph
        .nodes(&account(), NodeKind::Artifact)
        .await
        .expect("the artefacts are read");
    println!("\n=== The risk layer ===");
    println!("artefacts: {}", artefacts.len());
    println!(
        "components per artefact: {COMPONENTS_PER_ARTEFACT}, depth of the chain: {DEPENDENCY_DEPTH}"
    );

    let started = Instant::now();
    let changes = risk_layer(
        &artefacts
            .iter()
            .map(|node| node.id.clone())
            .collect::<Vec<_>>(),
    );
    graph
        .apply(changes)
        .await
        .expect("the risk layer is written");
    println!("time: {:.1} s", started.elapsed().as_secs_f64());

    let nodes = graph
        .nodes(&account(), NodeKind::Component)
        .await
        .expect("the components are read");
    println!("components in the graph: {}", nodes.len());

    // ── The queries ───────────────────────────────────────────────────────
    let vulnerability = hrn("Vulnerability", "GHSA-spike003-0001");

    // The blast radius: the walk of several hops.
    let mut blast = Vec::new();
    let mut reached = 0usize;
    for _ in 0..ROUNDS {
        let at = Instant::now();
        let radius = hodei_risk_storage::blast_radius(&graph, &account(), &vulnerability)
            .await
            .expect("the blast radius is computed");
        reached = radius.affected_subjects.len();
        blast.push(at.elapsed());
    }
    // The measurement has to be of a walk that reaches something: a walk
    // that reaches nothing measures the empty, and the empty is fast.
    assert!(
        reached > 0,
        "the blast radius reached nothing: the measurement would be of the empty"
    );
    println!("subjects reached by the vulnerability: {reached}");
    let measurement = Measurement::of(&mut blast);
    let (p50, p95, max) = measurement.millis();

    // The lineage: the walk that the SPIKE-012 measured.
    let mut lineage = Vec::new();
    if let Some(artefact) = artefacts.first() {
        for _ in 0..ROUNDS {
            let at = Instant::now();
            let _ = graph
                .lineage(&account(), &artefact.id)
                .await
                .expect("the lineage is computed");
            lineage.push(at.elapsed());
        }
    }
    let measurement = Measurement::of(&mut lineage);
    let (lineage_p50, lineage_p95, lineage_max) = measurement.millis();

    println!("\n=== The queries ===");
    println!("blast radius: p50 {p50} ms, p95 {p95} ms, max {max} ms");
    println!("lineage: p50 {lineage_p50} ms, p95 {lineage_p95} ms, max {lineage_max} ms");
    println!("target of the blast radius: p95 {TARGET_P95_MS} ms");

    // ── The export ────────────────────────────────────────────────────────
    // The dataset leaves for the other engine, together with the numbers
    // of this one, so that the comparison is of the SAME data and not of
    // two similar datasets.
    if let Ok(path) = std::env::var("HODEI_SPIKE003_EXPORT") {
        let started = Instant::now();
        let dataset = the_dataset(graph.as_ref(), &account()).await;
        let file = std::fs::File::create(&path).expect("the export file opens");
        let mut writer = std::io::BufWriter::new(file);
        let metadata = serde_json::json!({
            "kind": "metadata",
            "vulnerability": vulnerability.to_string(),
            "surrealdb": {
                "blast_p50_ms": p50,
                "blast_p95_ms": p95,
                "blast_max_ms": max,
                "lineage_p50_ms": lineage_p50,
                "lineage_p95_ms": lineage_p95,
                "lineage_max_ms": lineage_max,
                "subjects": reached,
            },
        });
        writeln!(writer, "{}", metadata).expect("the metadata is written");
        for node in &dataset.nodes {
            writeln!(
                writer,
                "{}",
                serde_json::json!({"kind": "node", "label": node.0, "id": node.1})
            )
            .expect("the node is written");
        }
        for edge in &dataset.edges {
            writeln!(
                writer,
                "{}",
                serde_json::json!({
                    "kind": "edge",
                    "label": edge.0,
                    "from": edge.1,
                    "to": edge.2,
                })
            )
            .expect("the edge is written");
        }
        println!(
            "\n=== The export ===\n{} nodes and {} edges exported to {path} in {:.1} s",
            dataset.nodes.len(),
            dataset.edges.len(),
            started.elapsed().as_secs_f64()
        );
    }

    println!("\n=== The verdict ===");
    if p95 <= TARGET_P95_MS {
        println!(
            "the blast radius met the target with a p95 of {p95} ms: the transactional graph \
             answers the multi-hop question, and a second store would have to beat it to be \
             worth its cost"
        );
    } else {
        println!(
            "the blast radius did NOT meet the target: a p95 of {p95} ms against a target of \
             {TARGET_P95_MS} ms, and a second store would have to be measured"
        );
    }
}
