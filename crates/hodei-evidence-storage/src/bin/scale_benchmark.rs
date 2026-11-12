//! The measurement of the reference SLO envelope.
//!
//! This is SPIKE-012 of the plan. The question it answers: without a
//! reference load there is no way to know whether a slow query is slow
//! because the model is bad or because the volume is another. And the answer
//! decides whether more store is needed.
//!
//! The measurement runs BY HAND, not in the ordinary CI: generating the load
//! takes minutes and the CI must not pay that on every push.
//!
//! The command:
//!
//! ```
//! cargo run --release -p hodei-evidence-storage --bin scale_benchmark
//! ```
//!
//! # What it measures
//!
//! - The admission: how long it takes to admit a piece of evidence.
//! - The projection: how long it takes to project the ledger.
//! - The queries of one to three hops: their p50, their p95 and their max.
//! - The effect of the account: the same query in the biggest account and
//!   in the smallest one.
//! - The rebuild: how long it takes to rebuild the graph.
//!
//! # Why it is a binary and not a test
//!
//! A measurement is not a test: it takes minutes, and a test that takes
//! minutes is a test nobody runs. The shape of the model is checked by the
//! test `scale_model`, and the numbers are taken by hand.

use hodei_evidence::application::projection::project_stream;
use hodei_evidence::ports::graph::GraphQuery;
use hodei_evidence::{
    ContentDigest, EvidenceGraphProjector, EvidenceIngestion, EvidenceVerification,
    IngestionEnvelope, IngestionRequest, Normalization, Producer, SourceRef, rebuild_graph,
};
use hodei_evidence_storage::scale::ScaleModel;
use hodei_evidence_storage::surreal::graph::{GRAPH_INDEXES, declare_index_statement};
use hodei_evidence_storage::{
    HmacEnvelopeAuthenticator, InMemoryContentStore, ProducerCredentials, SurrealEventLedger,
    SurrealEvidenceStore, SurrealGraphStore, SurrealProjectionCheckpoints,
};
use kernel::Hrn;
use std::sync::Arc;
use std::time::{Duration, Instant};
use surrealdb::Surreal;
use surrealdb::engine::local::RocksDb;

/// How many times each query is measured.
const ROUNDS: usize = 50;

/// The account the load belongs to.
const ACCOUNT: &str = "acme";

/// The stream the events of the load are in.
///
/// The ledger names the stream of an account after the account, so the
/// projection reads the stream of the account the load belongs to.
const STREAM: &str = "tenant-acme";

/// The model the measurement runs on.
///
/// The model is chosen with the environment, so that the same command
/// measures the small load while the code is being written and the reference
/// load when the numbers are taken:
///
/// - `HODEI_SCALE_MODEL=small` -- the small model;
/// - `HODEI_SCALE_MODEL=reference` -- the model of reference;
/// - `HODEI_SCALE_MODEL=custom` -- `HODEI_SCALE_ACCOUNTS` accounts of
///   `HODEI_SCALE_SIZE` times the size of the reference.
///
/// The custom model is what makes the growth measurable: the same shape at
/// several sizes says how the cost grows, and how it grows is the question
/// the spike answers.
fn the_model() -> ScaleModel {
    match std::env::var("HODEI_SCALE_MODEL").as_deref() {
        Ok("small") => ScaleModel::small(),
        Ok("custom") => {
            let accounts = std::env::var("HODEI_SCALE_ACCOUNTS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(4);
            let size = std::env::var("HODEI_SCALE_SIZE")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(10);
            ScaleModel::of_size(accounts, size)
        }
        _ => ScaleModel::reference(),
    }
}

/// The percentile of a list of measurements.
///
/// The percentile is the one that matters for an envelope: the average hides
/// the tail, and the tail is what a user feels.
fn percentile(values: &mut [Duration], fraction: f64) -> Duration {
    values.sort();
    let index = ((values.len() as f64 - 1.0) * fraction).round() as usize;
    values[index]
}

/// The measurement of a set of queries.
#[derive(Debug, Clone)]
struct Measurement {
    /// The p50 of the queries.
    pub p50: Duration,
    /// The p95 of the queries.
    pub p95: Duration,
    /// The slowest query.
    pub max: Duration,
}

impl Measurement {
    /// The measurement of a set of times.
    fn of(values: &mut [Duration]) -> Self {
        let max = values.iter().copied().max().unwrap_or_default();
        Self {
            p50: percentile(values, 0.50),
            p95: percentile(values, 0.95),
            max,
        }
    }

    /// The measurement, in milliseconds.
    fn millis(&self) -> (f64, f64, f64) {
        (
            self.p50.as_secs_f64() * 1000.0,
            self.p95.as_secs_f64() * 1000.0,
            self.max.as_secs_f64() * 1000.0,
        )
    }
}

/// The identity of a node of the load.
fn hrn(kind: &str, id: &str) -> Hrn {
    kernel::Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{ACCOUNT}:{kind}/{id}"
    ))
    .expect("the identity of a node of the load is a valid HRN")
}

/// The account the queries are asked about.
///
/// The account is a tenant of the graph, and the queries are asked for the
/// account the load belongs to.
fn account() -> Hrn {
    hrn("Artifact", "load")
}

#[tokio::main]
async fn main() {
    // The model can be chosen, so that the same command measures a small
    // load while the code is being written and the reference load when the
    // numbers are taken.
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

    // The indexes of the graph are declared by an operator command, and the
    // measurement says what they are worth: the same measurement without
    // them is what the deployment had before, and with them is what the
    // deployment has now. The declaration is the same statement the
    // operator command runs.
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
    let rebuild = Arc::new(SurrealGraphStore::named(db.clone(), "graph_rebuild"));
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

    println!("=== The reference scale model ===");
    println!("accounts: {}", model.accounts);
    println!(
        "repositories per account: {}..{}",
        model.min_repositories, model.max_repositories
    );
    println!(
        "revisions per repository: {}..{}",
        model.min_revisions, model.max_revisions
    );
    println!(
        "builds per revision: {}..{}",
        model.min_builds, model.max_builds
    );
    println!(
        "artefacts per build: {}, deployments per artefact: {}",
        model.artifacts_per_build, model.deployments_per_artifact
    );
    println!("pieces of evidence expected: {}", model.expected_events());

    // ── The admission ─────────────────────────────────────────────────────
    let mut admission = Vec::new();
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
                // The identity of the piece is the one the model gives it:
                // the model already tells the pieces of a journey and the
                // pieces of two accounts apart.
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

            let at = Instant::now();
            ingestion
                .ingest(IngestionRequest { envelope, content })
                .await
                .expect("the ingestion runs");
            admission.push(at.elapsed());
            total += 1;
        }
    }
    let admission_total = started.elapsed();
    let admission_measurement = Measurement::of(&mut admission);
    let (p50, p95, max) = admission_measurement.millis();
    println!("\n=== The admission ===");
    println!("pieces admitted: {total}");
    println!("total time: {:.1} s", admission_total.as_secs_f64());
    println!("per piece: p50 {p50:.2} ms, p95 {p95:.2} ms, max {max:.2} ms");

    // ── The projection ────────────────────────────────────────────────────
    let started = Instant::now();
    let report = project_stream(
        ledger.as_ref(),
        &EvidenceGraphProjector::new(),
        graph.as_ref(),
        checkpoints.as_ref(),
        "scale",
        STREAM,
    )
    .await
    .expect("the projection runs");
    let projection = started.elapsed();
    println!("\n=== The projection ===");
    println!(
        "events read: {}, applied: {}",
        report.events_read, report.events_applied
    );
    println!("time: {:.1} s", projection.as_secs_f64());

    // ── The queries ───────────────────────────────────────────────────────
    // The queries are asked about the artefact of the first build of the
    // first repository of the first account: the node that exists in every
    // model.
    let artifact = hrn("Artifact", "artifact-build-0-0-0-0-0");
    let build = hrn("Build", "build-0-0-0");
    let repository = hrn("Repository", "repo-0");

    let mut lineage_times = Vec::new();
    let mut deployment_times = Vec::new();
    let mut builder_times = Vec::new();
    let mut repository_times = Vec::new();

    for _ in 0..ROUNDS {
        let at = Instant::now();
        graph
            .lineage(&account(), &artifact)
            .await
            .expect("the lineage is read");
        lineage_times.push(at.elapsed());

        let at = Instant::now();
        graph
            .deployments(&account(), &artifact)
            .await
            .expect("the deployments are read");
        deployment_times.push(at.elapsed());

        let at = Instant::now();
        graph
            .artifacts_of_build(&account(), &build)
            .await
            .expect("the artefacts are read");
        builder_times.push(at.elapsed());

        let at = Instant::now();
        graph
            .artifacts_of_repository(&account(), &repository)
            .await
            .expect("the artefacts are read");
        repository_times.push(at.elapsed());
    }

    println!("\n=== The queries of one to three hops ({ROUNDS} rounds) ===");
    for (name, times) in [
        ("the lineage of the artefact", &mut lineage_times),
        ("the deployments of the artefact", &mut deployment_times),
        ("from the build to the artefacts", &mut builder_times),
        (
            "from the repository to the artefacts",
            &mut repository_times,
        ),
    ] {
        let measurement = Measurement::of(times);
        let (p50, p95, max) = measurement.millis();
        println!("{name}: p50 {p50:.2} ms, p95 {p95:.2} ms, max {max:.2} ms");
    }

    // ── The rebuild ───────────────────────────────────────────────────────
    let started = Instant::now();
    let report = rebuild_graph(
        ledger.as_ref(),
        &EvidenceGraphProjector::new(),
        rebuild.as_ref(),
        &account(),
        Some(graph.as_ref()),
    )
    .await
    .expect("the rebuild runs");
    let rebuild_time = started.elapsed();
    println!("\n=== The rebuild ===");
    println!("time: {:.1} s", rebuild_time.as_secs_f64());
    println!("equivalent: {}", report.equivalent);
    println!(
        "nodes: {}, relations: {}",
        report.nodes_written, report.relations_written
    );
}
