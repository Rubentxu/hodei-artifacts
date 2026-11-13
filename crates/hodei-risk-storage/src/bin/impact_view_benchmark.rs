//! The measurement of the materialized impact view (HSP-82, AC4).
//!
//! The view is worth its cost only if serving the impact from it is
//! materially cheaper than walking the graph, AND the incremental path
//! --consume the deltas, invalidate the affected keys, rebuild only
//! those-- costs a fraction of a full rebuild. The measurement says
//! both, over the SAME dataset the SPIKE-003 measured: the same
//! [`ScaleModel`], the same projection of the journey, and the same
//! risk layer of shared components.
//!
//! What is measured, each with its p50/p95/max:
//!
//! - the **baseline**: the authoritative walk of the blast radius, the
//!   answer of always;
//! - the **rebuild**: materialize the view of the advisory, one walk
//!   plus one transaction;
//! - the **serve**: read the view materialized, what a reader of the
//!   case would do;
//! - the **per-delta cost**: a new support arrives (a new artifact that
//!   holds the vulnerable component), the view consumes its delta from
//!   the ledger and recomputes ONLY the advisories it touches -- with a
//!   second advisory whose route the delta does not touch, so the
//!   selectivity is verified and not assumed;
//! - the **memory**: the resident memory of the process and the size
//!   of the base, at each stage.
//!
//! The comparison with the baseline is VERIFIABLE, not declarative: the
//! subjects of the view have to be the subjects of the walk, before and
//! after the deltas, or the measurement does not print a number.
//!
//! The measurement runs BY HAND, not in the ordinary CI: generating the
//! reference load takes minutes.
//!
//! # The command
//!
//! ```text
//! HODEI_SCALE_MODEL=custom HODEI_SCALE_ACCOUNTS=2 HODEI_SCALE_SIZE=10 \
//!   cargo run --release -p hodei-risk-storage --bin impact_view_benchmark
//! ```

use hodei_evidence::application::projection::project_stream;
use hodei_evidence::domain::delta::{RelationDeltaLedger, RelationDeltaSubscription};
use hodei_evidence::domain::graph::{
    GraphChangeSet, GraphNode, GraphRelation, NodeKind, RelationType,
};
use hodei_evidence::ports::graph::{GraphQuery, GraphWriter};
use hodei_evidence::{
    ContentDigest, EvidenceGraphProjector, EvidenceIngestion, EvidenceVerification,
    IngestionEnvelope, IngestionRequest, Normalization, Producer, SourceRef,
};
use hodei_evidence_storage::scale::ScaleModel;
use hodei_evidence_storage::surreal::delta::SurrealRelationDeltaLedger;
use hodei_evidence_storage::surreal::graph::{GRAPH_INDEXES, declare_index_statement};
use hodei_evidence_storage::{
    HmacEnvelopeAuthenticator, InMemoryContentStore, ProducerCredentials, SurrealEventLedger,
    SurrealEvidenceStore, SurrealGraphStore, SurrealProjectionCheckpoints,
};
use hodei_risk_storage::impact_view::SurrealImpactView;
use kernel::Hrn;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use surrealdb::engine::local::RocksDb;
use surrealdb::Surreal;

/// How many times each query is measured.
const ROUNDS: usize = 50;

/// The account the measurement runs on, the same of the SPIKE-003.
const ACCOUNT: &str = "acme";

/// The stream the projection reads, the same of the SPIKE-003.
const STREAM: &str = "tenant-acme";

/// How many components each artefact holds, the same of the SPIKE-003.
const COMPONENTS_PER_ARTEFACT: usize = 3;

/// How many supports the per-delta phase says, one artifact per round.
const DELTA_ROUNDS: usize = 12;

/// The percentile of a list of measurements.
fn percentile(values: &mut [Duration], fraction: f64) -> Duration {
    if values.is_empty() {
        return Duration::ZERO;
    }
    values.sort_unstable();
    let index = ((values.len() as f64) * fraction).ceil() as usize;
    values[index.saturating_sub(1).min(values.len() - 1)]
}

/// The percentiles of one set of measurements.
struct Measurement {
    values: Vec<Duration>,
}

impl Measurement {
    fn of(values: &[Duration]) -> Self {
        Self {
            values: values.to_vec(),
        }
    }

    /// (p50, p95, max), in milliseconds.
    fn millis(&self) -> (u128, u128, u128) {
        let mut values = self.values.clone();
        (
            percentile(&mut values, 0.50).as_millis(),
            percentile(&mut values, 0.95).as_millis(),
            values.last().copied().unwrap_or_default().as_millis(),
        )
    }

    /// The mean, in milliseconds, said with one decimal.
    fn mean_ms(&self) -> f64 {
        if self.values.is_empty() {
            return 0.0;
        }
        let total: Duration = self.values.iter().sum();
        total.as_secs_f64() * 1000.0 / self.values.len() as f64
    }
}

/// The HRN of a node of the account, the same way the SPIKE-003 says it.
fn hrn(kind: &str, id: &str) -> Hrn {
    Hrn::from_string(&format!("hrn:hodei:artifacts:eu-west-1:{ACCOUNT}:{kind}/{id}"))
        .expect("an HRN")
}

/// The account of the measurement.
fn account() -> Hrn {
    hrn("Artifact", "acme")
}

/// The model the measurement runs on, selected the same way the
/// SPIKE-003 selects it.
fn the_model() -> ScaleModel {
    match std::env::var("HODEI_SCALE_MODEL").as_deref() {
        Ok("small") => ScaleModel::small(),
        Ok("custom") => {
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
    }
}

/// How many artefacts hold the vulnerable component, the same share the
/// SPIKE-003 varies.
fn the_vulnerable_share() -> usize {
    std::env::var("HODEI_SPIKE003_SHARE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(50)
}

/// The risk layer of the load, the SAME shape the SPIKE-003 builds: a
/// pool of shared components --one of them vulnerable-- and a chain of
/// private components under each artefact. There is a SECOND advisory
/// about another shared component, so the selectivity of the
/// invalidation is verified over two keys and not assumed over one.
fn risk_layer(artefacts: &[Hrn]) -> GraphChangeSet {
    let mut changes = GraphChangeSet::empty();
    let vulnerability = hrn("Vulnerability", "GHSA-spike003-0001");
    let second_vulnerability = hrn("Vulnerability", "GHSA-spike003-0002");

    let share = the_vulnerable_share().min(artefacts.len());
    let shared: Vec<Hrn> = (0..share.max(2))
        .map(|component| hrn("Component", &format!("shared-{component}")))
        .collect();
    let vulnerable = shared[0].clone();
    let second = shared[1].clone();

    for component in &shared {
        changes = changes.with_node(GraphNode::new(
            component.clone(),
            account(),
            NodeKind::Component,
            chrono::Utc::now(),
        ));
    }
    for (advisory, component, evidence) in [
        (&vulnerability, &vulnerable, "ev-risk-0"),
        (&second_vulnerability, &second, "ev-risk-1"),
    ] {
        changes = changes.with_node(GraphNode::new(
            advisory.clone(),
            account(),
            NodeKind::Vulnerability,
            chrono::Utc::now(),
        ));
        changes = changes.with_relation(GraphRelation::new(
            RelationType::AffectedBy,
            component.clone(),
            advisory.clone(),
            account(),
            hodei_evidence::EvidenceId::new(evidence),
            chrono::Utc::now(),
        ));
    }

    for (index, artefact) in artefacts.iter().enumerate() {
        let evidence_id = hodei_evidence::EvidenceId::new(format!("ev-risk-{index}"));
        changes = changes.with_node(GraphNode::new(
            hodei_evidence::application::projector::evidence_hrn(&account(), &evidence_id),
            account(),
            NodeKind::Evidence,
            chrono::Utc::now(),
        ));

        let shared_of_this = &shared[index % shared.len()];
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

    changes
}

/// The changeset of ONE new support that arrives later: an artifact that
/// now holds the vulnerable component. Its cause makes the graph say
/// its delta.
fn the_new_support(cause: &str, index: usize, component: &Hrn) -> GraphChangeSet {
    let evidence_id = hodei_evidence::EvidenceId::new(format!("ev-delta-{index}"));
    let artefact = hrn("Artifact", &format!("app-late-{index}"));
    GraphChangeSet::empty()
        .with_cause(cause)
        .with_node(GraphNode::new(
            artefact.clone(),
            account(),
            NodeKind::Artifact,
            chrono::Utc::now(),
        ))
        .with_node(GraphNode::new(
            hodei_evidence::application::projector::evidence_hrn(&account(), &evidence_id),
            account(),
            NodeKind::Evidence,
            chrono::Utc::now(),
        ))
        .with_relation(GraphRelation::new(
            RelationType::Contains,
            artefact,
            component.clone(),
            account(),
            evidence_id,
            chrono::Utc::now(),
        ))
}

/// The resident memory of the process, in kB, as the kernel says it.
fn the_resident_memory() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            return rest
                .trim()
                .trim_end_matches("kB")
                .trim()
                .parse()
                .unwrap_or(0);
        }
    }
    0
}

/// The size of a directory, in bytes, walking it.
fn the_directory_size(path: &std::path::Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            if metadata.is_dir() {
                total += the_directory_size(&entry.path());
            } else {
                total += metadata.len();
            }
        }
    }
    total
}

/// The subjects of the walk, said the same way the view says them: the
/// artefacts with their depth, the components at depth zero.
fn the_walk_subjects(radius: &hodei_risk::case::BlastRadius) -> BTreeSet<(String, usize)> {
    let mut subjects: BTreeSet<(String, usize)> = radius
        .depths
        .iter()
        .map(|one| (one.subject.clone(), one.depth))
        .collect();
    for component in &radius.affected_components {
        subjects.insert((component.clone(), 0));
    }
    subjects
}

/// The number of rows of a table, said by a count.
async fn the_row_count<C: surrealdb::Connection>(
    db: &Surreal<C>,
    table: &'static str,
) -> usize {
    let mut response = db
        .query("SELECT count() FROM type::table($table) GROUP ALL")
        .bind(("table", table))
        .await
        .expect("the count runs");
    let rows: Vec<std::collections::BTreeMap<String, usize>> =
        response.take(0).expect("the count is read");
    rows.first()
        .and_then(|row| row.get("count").copied())
        .unwrap_or(0)
}

#[tokio::main]
async fn main() {
    let model = the_model();
    println!("=== The scale model of the SPIKE-003 ===");
    println!("accounts: {}", model.accounts);
    println!("pieces of evidence expected: {}", model.expected_events());
    println!("deltas per round: {DELTA_ROUNDS}");

    let tmp = tempfile::tempdir().expect("a throwaway directory");
    let path = tmp.keep();
    let db = Arc::new(
        Surreal::new::<RocksDb>(path.join("db").to_string_lossy().to_string())
            .await
            .expect("the database opens"),
    );
    db.use_ns("hodei").use_db("evidence").await.expect("the namespace");
    for (table, fields) in GRAPH_INDEXES {
        db.query(declare_index_statement(table, fields))
            .await
            .expect("the index is declared")
            .check()
            .expect("the index is declared");
    }

    // ── The load: the SAME dataset the SPIKE-003 measures ────────────────
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

    let started = Instant::now();
    let mut total = 0usize;
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
            envelope.signature = authenticator.sign(&envelope).expect("the producer is known");
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

    let started = Instant::now();
    let report = project_stream(
        ledger.as_ref(),
        &EvidenceGraphProjector::new(),
        graph.as_ref(),
        checkpoints.as_ref(),
        "impact-view",
        STREAM,
    )
    .await
    .expect("the projection runs");
    println!(
        "projection: read {}, applied {} in {:.1} s",
        report.events_read,
        report.events_applied,
        started.elapsed().as_secs_f64()
    );

    // ── The risk layer ────────────────────────────────────────────────────
    let artefacts = graph
        .nodes(&account(), NodeKind::Artifact)
        .await
        .expect("the artefacts are read");
    let started = Instant::now();
    graph
        .apply(risk_layer(
            &artefacts.iter().map(|node| node.id.clone()).collect::<Vec<_>>(),
        ))
        .await
        .expect("the risk layer is written");
    println!(
        "risk layer over {} artefacts in {:.1} s",
        artefacts.len(),
        started.elapsed().as_secs_f64()
    );

    // ── The view of the two advisories ────────────────────────────────────
    let deltas = Arc::new(SurrealRelationDeltaLedger::new(db.clone()));
    let view = SurrealImpactView::new(graph.clone(), deltas.clone());
    let vulnerability = hrn("Vulnerability", "GHSA-spike003-0001");
    let second_vulnerability = hrn("Vulnerability", "GHSA-spike003-0002");

    let after_load = the_resident_memory();

    let started = Instant::now();
    let snapshot = view
        .rebuild_advisory(&account(), &vulnerability)
        .await
        .expect("the view of the advisory is built");
    let second = view
        .rebuild_advisory(&account(), &second_vulnerability)
        .await
        .expect("the view of the second advisory is built");
    let rebuilt_ms = started.elapsed().as_secs_f64() * 1000.0;
    println!("\n=== The view materialized ===");
    println!(
        "advisory GHSA-spike003-0001: {} subjects, GHSA-spike003-0002: {} subjects",
        snapshot.subjects.len(),
        second.subjects.len()
    );
    println!("both rebuilds: {rebuilt_ms:.1} ms");

    // The comparison with the baseline is VERIFIABLE: the view says what
    // the walk says, or the measurement does not continue.
    let radius = hodei_risk_storage::blast_radius(graph.as_ref(), &account(), &vulnerability)
        .await
        .expect("the walk runs");
    let expected = the_walk_subjects(&radius);
    let seen: BTreeSet<(String, usize)> = snapshot.subjects.iter().cloned().collect();
    assert_eq!(
        seen, expected,
        "the view does not say what the walk says: the measurement would be of nothing"
    );

    // ── The queries ───────────────────────────────────────────────────────
    let mut baseline = Vec::new();
    let mut rebuild = Vec::new();
    let mut serve = Vec::new();
    for _ in 0..ROUNDS {
        let at = Instant::now();
        let radius = hodei_risk_storage::blast_radius(graph.as_ref(), &account(), &vulnerability)
            .await
            .expect("the walk runs");
        baseline.push(at.elapsed());
        assert!(!radius.affected_subjects.is_empty());

        let at = Instant::now();
        let _snapshot = view
            .rebuild_advisory(&account(), &vulnerability)
            .await
            .expect("the rebuild runs");
        rebuild.push(at.elapsed());

        let at = Instant::now();
        let read = view
            .read(&account(), &vulnerability)
            .await
            .expect("the serve runs")
            .expect("the view is there");
        serve.push(at.elapsed());
        assert_eq!(
            read.subjects.iter().cloned().collect::<BTreeSet<_>>(),
            the_walk_subjects(&radius),
            "the served view diverged from the walk"
        );
    }

    let baseline = Measurement::of(&baseline);
    let rebuild = Measurement::of(&rebuild);
    let serve = Measurement::of(&serve);
    let (b50, b95, bmax) = baseline.millis();
    let (r50, r95, rmax) = rebuild.millis();
    let (s50, s95, smax) = serve.millis();
    println!("\n=== The queries, over the same dataset ===");
    println!("baseline walk:   p50 {b50} ms, p95 {b95} ms, max {bmax} ms, mean {:.1} ms", baseline.mean_ms());
    println!("view rebuild:    p50 {r50} ms, p95 {r95} ms, max {rmax} ms, mean {:.1} ms", rebuild.mean_ms());
    println!("view serve:      p50 {s50} ms, p95 {s95} ms, max {smax} ms, mean {:.1} ms", serve.mean_ms());

    let after_rebuild = the_resident_memory();

    // ── The per-delta cost ────────────────────────────────────────────────
    // A NEW support arrives, one artifact per round. The view consumes
    // the deltas the graph said, from where it stopped, the way the
    // production consumer does. The second advisory is NOT touched by
    // these deltas: the selectivity is asserted, round by round.
    let subscription = RelationDeltaSubscription::new(
        account(),
        [
            RelationType::Contains,
            RelationType::AffectedBy,
            RelationType::DependsOn,
            RelationType::DeployedAs,
        ],
    );
    let mut cursor = snapshot.watermark;
    let mut per_delta = Vec::new();
    let mut handled = 0usize;
    let second_before = second.subjects.clone();
    let vulnerable_component = hrn("Component", "shared-0");

    println!("\n=== The per-delta phase ===");
    let started = Instant::now();
    for round in 0..DELTA_ROUNDS {
        let cause = format!("evt-delta-{round}");
        graph
            .apply(the_new_support(&cause, round, &vulnerable_component))
            .await
            .expect("the new support is written");
        let (pending, stopped) = deltas
            .pending(&subscription, cursor, 100)
            .await
            .expect("the deltas are read");
        cursor = stopped;
        for (_version, delta) in &pending {
            let at = Instant::now();
            let touched = view
                .apply_delta(delta)
                .await
                .expect("the delta is applied");
            per_delta.push(at.elapsed());
            handled += 1;
            if delta.relation == RelationType::Contains {
                assert_eq!(
                    touched,
                    vec![vulnerability.clone()],
                    "the delta of {} touched more than its advisory: {touched:?}",
                    delta.identity()
                );
            }
        }
    }
    let deltas_ms = started.elapsed().as_secs_f64() * 1000.0;
    let per_delta = Measurement::of(&per_delta);
    let (d50, d95, dmax) = per_delta.millis();
    println!("deltas handled: {handled} in {deltas_ms:.1} ms");
    println!("per delta:       p50 {d50} ms, p95 {d95} ms, max {dmax} ms, mean {:.1} ms", per_delta.mean_ms());

    // The second advisory was NOT recomputed by the deltas: its rows
    // are the ones it had.
    let second_after = view
        .read(&account(), &second_vulnerability)
        .await
        .expect("the serve runs")
        .expect("the view is there");
    assert_eq!(
        second_after.subjects, second_before,
        "the second advisory changed without a delta touching it"
    );

    // And the view of the advisory still says what the walk says, with
    // the new supports IN.
    let radius = hodei_risk_storage::blast_radius(graph.as_ref(), &account(), &vulnerability)
        .await
        .expect("the walk runs");
    let read = view
        .read(&account(), &vulnerability)
        .await
        .expect("the serve runs")
        .expect("the view is there");
    assert_eq!(
        read.subjects.iter().cloned().collect::<BTreeSet<_>>(),
        the_walk_subjects(&radius),
        "after the deltas, the view diverged from the walk"
    );
    println!(
        "after the deltas: {} subjects in the view, the walk says the same",
        read.subjects.len()
    );

    // The lag the deltas of the second advisory leave is EXPOSED, not
    // hidden: the tenant is not current until its view is rebuilt.
    let current = view.is_current(&account()).await.expect("the check runs");
    println!("tenant current without rebuilding the second advisory: {current}");
    view.rebuild_advisory(&account(), &second_vulnerability)
        .await
        .expect("the rebuild runs");
    assert!(
        view.is_current(&account()).await.expect("the check runs"),
        "rebuilt, the tenant is current"
    );

    // ── The memory ────────────────────────────────────────────────────────
    let after_deltas = the_resident_memory();
    let rows = the_row_count(db.as_ref(), "impact_view").await;
    let base_size = the_directory_size(&path);
    println!("\n=== The memory and the base ===");
    println!(
        "resident: after load {} kB, after rebuild {} kB, after deltas {} kB",
        after_load, after_rebuild, after_deltas
    );
    println!("materialized rows: {rows}");
    println!("base on disk: {:.1} MB", base_size as f64 / 1_048_576.0);

    println!("\nThe measurement is verifiable: the view said what the walk said, before and after the deltas.");
}
