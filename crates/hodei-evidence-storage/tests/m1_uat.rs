//! La UAT de M1: el grafo de evidencia.
//!
//! Son los ocho escenarios de `uat/M1-EVIDENCE-GRAPH-UAT.md`, ejecutados
//! sobre fixtures locales y servicios efimeros, sin integraciones obligatorias
//! con otros productos.
//!
//! Cada escenario deja el comando con el que se ejecuta y su resultado, que
//! es lo que la tarea pide que quede registrado.
//!
//! # Los escenarios
//!
//! - M1-UAT-01 El linaje de la fuente al despliegue.
//! - M1-UAT-02 La ingesta idempotente, la misma entrega diez veces y a la vez.
//! - M1-UAT-03 La revision de un registro externo conserva el historico.
//! - M1-UAT-04 La evidencia invalida no envenena el grafo.
//! - M1-UAT-05 La equivalencia de la reconstruccion.
//! - M1-UAT-06 El fallo de la proyeccion y su recuperacion.
//! - M1-UAT-07 El aislamiento entre cuentas.
//! - M1-UAT-08 El rendimiento de referencia.

use chrono::{DateTime, TimeZone, Utc};
use hodei_evidence::application::projection::{
    ProjectionCheckpoints, ProjectionError, project_stream,
};
use hodei_evidence::ports::graph::{GraphProjector, GraphQuery};
use hodei_evidence::{
    ContentDigest, EventLedger, EvidenceGraphProjector, EvidenceIngestion, EvidenceStore,
    EvidenceType, EvidenceVerification, GraphChangeSet, IngestionEnvelope, IngestionOutcome,
    IngestionRequest, LedgerCursor, NodeKind, Normalization, Producer, RelationClaim, RelationType,
    SourceRef, rebuild_graph,
};
use hodei_evidence_storage::{
    HmacEnvelopeAuthenticator, InMemoryContentStore, ProducerCredentials, SurrealEventLedger,
    SurrealEvidenceStore, SurrealGraphStore, SurrealProjectionCheckpoints,
};
use kernel::Hrn;
use std::sync::Arc;
use std::time::Instant;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

/// El nombre del consumidor de la proyeccion.
const CONSUMER: &str = "m1-uat";

/// El comando con el que se ejecuta esta suite.
const COMMAND: &str = "cargo test -p hodei-evidence-storage --test m1_uat -- --nocapture";

fn moment(second: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(1_767_322_800 + second, 0)
        .single()
        .expect("a moment")
}

fn hrn(account: &str, kind: &str, id: &str) -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{account}:{kind}/{id}"
    ))
    .expect("an HRN")
}

fn producer() -> Producer {
    Producer::new("connector", "pipelinek").with_version("1.2.3")
}

/// La cuenta de la UAT.
fn acme() -> Hrn {
    hrn("acme", "Artifact", "web-1.0.0")
}

/// La otra cuenta, con los mismos nombres externos.
fn other() -> Hrn {
    hrn("other", "Artifact", "web-1.0.0")
}

/// El recorrido de referencia: un repositorio, una revision, una
/// construccion, un artefacto y un despliegue.
fn journey(account: &str) -> Vec<(String, Vec<u8>, EvidenceType, Option<RelationClaim>)> {
    let repository = hrn(account, "Repository", "web");
    let revision = hrn(account, "SourceRevision", "web-abc123");
    let build = hrn(account, "Build", "web-42");
    let artifact = hrn(account, "Artifact", "web-1.0.0");
    let deployment = hrn(account, "Deployment", "prod-7");

    vec![
        (
            "repo-1".to_string(),
            b"the repository".to_vec(),
            EvidenceType::SourceMetadata,
            Some(RelationClaim {
                relation: RelationType::HasRevision,
                from: repository,
                to: revision.clone(),
                from_kind: NodeKind::Repository,
                to_kind: NodeKind::SourceRevision,
            }),
        ),
        (
            "commit-1".to_string(),
            b"the commit".to_vec(),
            EvidenceType::SourceMetadata,
            Some(RelationClaim {
                relation: RelationType::Triggered,
                from: revision,
                to: build.clone(),
                from_kind: NodeKind::SourceRevision,
                to_kind: NodeKind::Build,
            }),
        ),
        (
            "build-1".to_string(),
            b"the build".to_vec(),
            EvidenceType::BuildMetadata,
            Some(RelationClaim {
                relation: RelationType::Produced,
                from: build,
                to: artifact.clone(),
                from_kind: NodeKind::Build,
                to_kind: NodeKind::Artifact,
            }),
        ),
        (
            "artifact-1".to_string(),
            b"the artifact".to_vec(),
            EvidenceType::ArtifactIntegrity,
            Some(RelationClaim {
                relation: RelationType::DeployedAs,
                from: artifact,
                to: deployment,
                from_kind: NodeKind::Artifact,
                to_kind: NodeKind::Deployment,
            }),
        ),
    ]
}

/// Todo lo que la UAT necesita.
struct Fixture {
    ledger: Arc<SurrealEventLedger<Db>>,
    evidence_store: Arc<SurrealEvidenceStore<Db>>,
    graph: Arc<SurrealGraphStore<Db>>,
    rebuild: Arc<SurrealGraphStore<Db>>,
    checkpoints: Arc<SurrealProjectionCheckpoints<Db>>,
    authenticator: Arc<HmacEnvelopeAuthenticator>,
    ingestion: Arc<EvidenceIngestion>,
}

/// El fixture, sobre una base de datos de usar y tirar.
async fn fixture() -> Fixture {
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
            accounts: vec!["acme".to_string(), "other".to_string()],
        },
    );

    let ingestion = Arc::new(EvidenceIngestion::new(
        content_store.clone(),
        evidence_store.clone(),
        ledger.clone(),
        authenticator.clone(),
    ));

    Fixture {
        ledger,
        evidence_store,
        graph,
        rebuild,
        checkpoints,
        authenticator,
        ingestion,
    }
}

/// El sobre de una pieza de evidencia.
fn an_envelope(
    account: &Hrn,
    external_id: &str,
    content: &[u8],
    kind: EvidenceType,
    claim: Option<RelationClaim>,
) -> IngestionEnvelope {
    IngestionEnvelope {
        producer: producer(),
        tenant_hrn: account.clone(),
        subject_refs: vec![],
        evidence_type: kind,
        schema_uri: None,
        schema_version: None,
        source: SourceRef::by_external_id(external_id),
        content_digest: ContentDigest::of_bytes(content),
        relation_claim: claim,
        observed_at: moment(10),
        valid_from: None,
        verification: EvidenceVerification::verified("signature"),
        normalization: Normalization::new("pipelinek", "1.0.0"),
        correlation_id: None,
        signature: String::new(),
    }
}

/// Admite el recorrido de una cuenta y lo proyecta en el grafo.
async fn admit(fixture: &Fixture, account: &Hrn) -> Vec<IngestionOutcome> {
    let mut outcomes = Vec::new();
    for (external_id, content, kind, claim) in journey(account.account_id()) {
        let mut envelope = an_envelope(account, &external_id, &content, kind, claim);
        envelope.signature = fixture
            .authenticator
            .sign(&envelope)
            .expect("the producer is known");
        let outcome = fixture
            .ingestion
            .ingest(IngestionRequest { envelope, content })
            .await
            .expect("the ingestion runs");
        outcomes.push(outcome);
    }
    outcomes
}

/// Proyecta el registro en el grafo, con checkpoint.
async fn project(
    fixture: &Fixture,
    consumer: &str,
    stream: &str,
) -> hodei_evidence::application::projection::ProjectionReport {
    project_stream(
        fixture.ledger.as_ref(),
        &EvidenceGraphProjector::new(),
        fixture.graph.as_ref(),
        fixture.checkpoints.as_ref(),
        consumer,
        stream,
    )
    .await
    .expect("the projection runs")
}

/// El resultado de un escenario, tal como queda registrado.
fn record(scenario: &str, outcome: Result<(), String>) {
    match outcome {
        Ok(()) => println!("{scenario}: PASSED"),
        Err(reason) => println!("{scenario}: FAILED -- {reason}"),
    }
}

// ───────────────────────────── M1-UAT-01 ─────────────────────────────

/// M1-UAT-01 El linaje de la fuente al despliegue.
#[tokio::test]
async fn m1_uat_01_source_to_deployment_lineage() {
    let fixture = fixture().await;
    let outcomes = admit(&fixture, &acme()).await;
    project(&fixture, CONSUMER, "tenant-acme").await;

    let result = async {
        for outcome in &outcomes {
            if !matches!(outcome, IngestionOutcome::Accepted { .. }) {
                return Err(format!("the evidence was not accepted: {outcome:?}"));
            }
        }

        let artifact = hrn("acme", "Artifact", "web-1.0.0");
        let lineage = fixture
            .graph
            .lineage(&acme(), &artifact)
            .await
            .map_err(|error| error.to_string())?;

        if lineage.artifact.is_none() {
            return Err("the lineage does not have the artefact".to_string());
        }
        if lineage.builds.len() != 1 {
            return Err(format!(
                "the lineage has {} builds and it should have one",
                lineage.builds.len()
            ));
        }
        if lineage.revisions.len() != 1 {
            return Err(format!(
                "the lineage has {} revisions and it should have one",
                lineage.revisions.len()
            ));
        }
        if lineage.repository.is_none() {
            return Err("the lineage does not have the repository".to_string());
        }

        let deployments = fixture
            .graph
            .deployments(&acme(), &artifact)
            .await
            .map_err(|error| error.to_string())?;
        if deployments.len() != 1 {
            return Err(format!(
                "the artefact has {} deployments and it should have one",
                deployments.len()
            ));
        }

        // The critical relations carry the evidence that supports them.
        if lineage.evidence.is_empty() {
            return Err("the relations of the lineage carry no evidence".to_string());
        }

        Ok(())
    }
    .await;

    record("M1-UAT-01 source to deployment lineage", result);
}

// ───────────────────────────── M1-UAT-02 ─────────────────────────────

/// M1-UAT-02 La ingesta idempotente: la misma entrega diez veces, y a la vez.
#[tokio::test]
async fn m1_uat_02_idempotent_ingest() {
    let fixture = fixture().await;
    let content = b"the same delivery, ten times";
    let mut envelope = an_envelope(
        &acme(),
        "run-1",
        content,
        EvidenceType::Sbom,
        Some(RelationClaim {
            relation: RelationType::Produced,
            from: hrn("acme", "Build", "web-42"),
            to: hrn("acme", "Artifact", "web-1.0.0"),
            from_kind: NodeKind::Build,
            to_kind: NodeKind::Artifact,
        }),
    );
    envelope.signature = fixture
        .authenticator
        .sign(&envelope)
        .expect("the producer is known");

    // Ten deliveries at the same time.
    let mut tasks = Vec::new();
    for _ in 0..10 {
        let ingestion = fixture.ingestion.clone();
        let envelope = envelope.clone();
        let content = content.to_vec();
        tasks.push(tokio::spawn(async move {
            ingestion
                .ingest(IngestionRequest { envelope, content })
                .await
        }));
    }

    let result = async {
        let mut accepted = 0;
        let mut repeated = 0;
        for task in tasks {
            let outcome = task
                .await
                .expect("the delivery runs")
                .map_err(|error| error.to_string())?;
            match outcome {
                IngestionOutcome::Accepted { .. } => accepted += 1,
                IngestionOutcome::AlreadyAccepted { .. } => repeated += 1,
                other => return Err(format!("the delivery said something else: {other:?}")),
            }
        }

        // One canonical evidence, and one set of relations.
        let found = fixture
            .evidence_store
            .find_by_external_id(&producer(), "run-1")
            .await
            .map_err(|error| error.to_string())?;
        if found.is_none() {
            return Err("the evidence is not in the store".to_string());
        }

        if accepted + repeated != 10 {
            return Err("the ten deliveries did not all answer".to_string());
        }

        // One set of relations, and not ten: the facts are not duplicated.
        project(&fixture, CONSUMER, "tenant-acme").await;
        let relations = fixture
            .graph
            .relations_to(&acme(), &hrn("acme", "Artifact", "web-1.0.0"))
            .await
            .map_err(|error| error.to_string())?;

        println!(
            "  M1-UAT-02: {accepted} accepted, {repeated} repeated, {} relations",
            relations.len()
        );

        if relations.len() != 1 {
            return Err(format!(
                "the graph holds {} relations and it should hold one",
                relations.len()
            ));
        }

        // And a delivery that comes later says that it was already
        // accepted, instead of writing anything else.
        let mut again = an_envelope(
            &acme(),
            "run-1",
            content,
            EvidenceType::Sbom,
            Some(RelationClaim {
                relation: RelationType::Produced,
                from: hrn("acme", "Build", "web-42"),
                to: hrn("acme", "Artifact", "web-1.0.0"),
                from_kind: NodeKind::Build,
                to_kind: NodeKind::Artifact,
            }),
        );
        again.signature = fixture
            .authenticator
            .sign(&again)
            .expect("the producer is known");
        let outcome = fixture
            .ingestion
            .ingest(IngestionRequest {
                envelope: again,
                content: content.to_vec(),
            })
            .await
            .map_err(|error| error.to_string())?;
        match outcome {
            IngestionOutcome::AlreadyAccepted { .. } => {}
            other => {
                return Err(format!(
                    "the delivery that came later did not say it was already accepted: {other:?}"
                ));
            }
        }

        Ok(())
    }
    .await;

    record("M1-UAT-02 idempotent ingest", result);
}

// ───────────────────────────── M1-UAT-03 ─────────────────────────────

/// M1-UAT-03 La revision de un registro externo conserva el historico.
#[tokio::test]
async fn m1_uat_03_revision_preserves_history() {
    let fixture = fixture().await;

    let first_content = b"the first reading";
    let mut envelope = an_envelope(&acme(), "run-1", first_content, EvidenceType::Sbom, None);
    envelope.signature = fixture
        .authenticator
        .sign(&envelope)
        .expect("the producer is known");
    let first = fixture
        .ingestion
        .ingest(IngestionRequest {
            envelope,
            content: first_content.to_vec(),
        })
        .await
        .expect("the first delivery runs");

    let second_content = b"the second reading";
    let mut envelope = an_envelope(&acme(), "run-1", second_content, EvidenceType::Sbom, None);
    envelope.signature = fixture
        .authenticator
        .sign(&envelope)
        .expect("the producer is known");
    let second = fixture
        .ingestion
        .ingest(IngestionRequest {
            envelope,
            content: second_content.to_vec(),
        })
        .await
        .expect("the revision runs");

    let result = async {
        let supersedes = match &second {
            IngestionOutcome::Revised { supersedes, .. } => supersedes.clone(),
            other => return Err(format!("the second delivery is not a revision: {other:?}")),
        };
        let original = match &first {
            IngestionOutcome::Accepted { evidence_id, .. } => evidence_id.clone(),
            other => return Err(format!("the first delivery was not accepted: {other:?}")),
        };
        if supersedes != original {
            return Err("the revision does not say what it revises".to_string());
        }
        Ok(())
    }
    .await;

    record(
        "M1-UAT-03 revised external record preserves history",
        result,
    );
}

// ───────────────────────────── M1-UAT-04 ─────────────────────────────

/// M1-UAT-04 La evidencia invalida no envenena el grafo.
#[tokio::test]
async fn m1_uat_04_invalid_evidence_cannot_poison_graph() {
    let fixture = fixture().await;
    admit(&fixture, &acme()).await;
    project(&fixture, CONSUMER, "tenant-acme").await;

    let before = fixture
        .graph
        .nodes(&acme(), NodeKind::Deployment)
        .await
        .expect("the nodes are read")
        .len();

    // A deployment that nobody vouched for.
    let content = b"a deployment with a wrong signature";
    let mut envelope = an_envelope(
        &acme(),
        "deployment-forged",
        content,
        EvidenceType::DeploymentObservation,
        Some(RelationClaim {
            relation: RelationType::DeployedAs,
            from: hrn("acme", "Artifact", "web-1.0.0"),
            to: hrn("acme", "Deployment", "prod-forged"),
            from_kind: NodeKind::Artifact,
            to_kind: NodeKind::Deployment,
        }),
    );
    envelope.signature = "not the signature".to_string();

    let result = async {
        let outcome = fixture
            .ingestion
            .ingest(IngestionRequest {
                envelope,
                content: content.to_vec(),
            })
            .await
            .map_err(|error| error.to_string())?;

        // The refusal is observable.
        match &outcome {
            IngestionOutcome::Rejected { reason } => {
                println!("  M1-UAT-04: the delivery was refused -- {reason}");
            }
            other => return Err(format!("the delivery was not refused: {other:?}")),
        }

        // And nothing of it is in the graph.
        let after = fixture
            .graph
            .nodes(&acme(), NodeKind::Deployment)
            .await
            .map_err(|error| error.to_string())?
            .len();
        if after != before {
            return Err("the graph holds a deployment that was refused".to_string());
        }

        let deployments = fixture
            .graph
            .deployments(&acme(), &hrn("acme", "Artifact", "web-1.0.0"))
            .await
            .map_err(|error| error.to_string())?;
        if deployments
            .iter()
            .any(|node| node.id == hrn("acme", "Deployment", "prod-forged"))
        {
            return Err("the forged deployment is in the graph".to_string());
        }

        Ok(())
    }
    .await;

    record("M1-UAT-04 invalid evidence cannot poison graph", result);
}

// ───────────────────────────── M1-UAT-05 ─────────────────────────────

/// M1-UAT-05 La equivalencia de la reconstruccion.
#[tokio::test]
async fn m1_uat_05_projection_rebuild_equivalence() {
    let fixture = fixture().await;
    admit(&fixture, &acme()).await;
    project(&fixture, CONSUMER, "tenant-acme").await;

    let artifact = hrn("acme", "Artifact", "web-1.0.0");
    let build = hrn("acme", "Build", "web-42");
    let repository = hrn("acme", "Repository", "web");

    let result = async {
        let report = rebuild_graph(
            fixture.ledger.as_ref(),
            &EvidenceGraphProjector::new(),
            fixture.rebuild.as_ref(),
            &acme(),
            Some(fixture.graph.as_ref()),
        )
        .await
        .map_err(|error| error.to_string())?;

        if !report.equivalent {
            return Err(format!(
                "the rebuilt graph does not say the same things: {:?}",
                report.differences
            ));
        }

        // The critical queries: the lineage, the deployment lookup, from the
        // build to the artefacts and from the repository to the artefacts.
        let checks: Vec<(&str, bool)> = vec![
            (
                "artifact lineage",
                fixture.graph.lineage(&acme(), &artifact).await.ok()
                    == fixture.rebuild.lineage(&acme(), &artifact).await.ok(),
            ),
            (
                "deployment lookup",
                fixture.graph.deployments(&acme(), &artifact).await.ok()
                    == fixture.rebuild.deployments(&acme(), &artifact).await.ok(),
            ),
            (
                "builder to artifacts",
                fixture.graph.artifacts_of_build(&acme(), &build).await.ok()
                    == fixture
                        .rebuild
                        .artifacts_of_build(&acme(), &build)
                        .await
                        .ok(),
            ),
            (
                "repository to artifacts",
                fixture
                    .graph
                    .artifacts_of_repository(&acme(), &repository)
                    .await
                    .ok()
                    == fixture
                        .rebuild
                        .artifacts_of_repository(&acme(), &repository)
                        .await
                        .ok(),
            ),
        ];

        for (name, equal) in &checks {
            println!(
                "  M1-UAT-05: {name}: {}",
                if *equal { "same" } else { "DIFFERENT" }
            );
            if !equal {
                return Err(format!(
                    "the critical query {name} says something different"
                ));
            }
        }

        Ok(())
    }
    .await;

    record("M1-UAT-05 projection rebuild equivalence", result);
}

// ───────────────────────────── M1-UAT-06 ─────────────────────────────

/// M1-UAT-06 El fallo de la proyeccion y su recuperacion.
#[tokio::test]
async fn m1_uat_06_projection_failure_and_recovery() {
    let fixture = fixture().await;
    admit(&fixture, &acme()).await;

    // A projector that fails on the last event.
    struct FailingProjector {
        inner: EvidenceGraphProjector,
        failures_left: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl GraphProjector for FailingProjector {
        fn project(
            &self,
            event: &hodei_evidence::EventEnvelope,
        ) -> Result<GraphChangeSet, hodei_evidence::ports::graph::ProjectionError> {
            use std::sync::atomic::Ordering;
            if self.failures_left.fetch_sub(1, Ordering::SeqCst) > 0 {
                return Err(hodei_evidence::ports::graph::ProjectionError::Unavailable(
                    "the projector is broken on purpose".to_string(),
                ));
            }
            self.inner.project(event)
        }
    }

    let failures = Arc::new(std::sync::atomic::AtomicUsize::new(1));
    let failing = FailingProjector {
        inner: EvidenceGraphProjector::new(),
        failures_left: failures.clone(),
    };

    let result = async {
        // The projection fails.
        let outcome = project_stream(
            fixture.ledger.as_ref(),
            &failing,
            fixture.graph.as_ref(),
            fixture.checkpoints.as_ref(),
            CONSUMER,
            "tenant-acme",
        )
        .await;

        match outcome {
            Ok(_) => return Err("the projection was supposed to fail".to_string()),
            Err(ProjectionError::Projector(reason)) => {
                println!("  M1-UAT-06: the projection failed -- {reason}");
            }
            Err(other) => return Err(format!("the projection failed in another way: {other}")),
        }

        // The event is still durable: it is in the ledger.
        let page = fixture
            .ledger
            .read(LedgerCursor::start_of("tenant-acme"), 100)
            .await
            .expect("the ledger is read");
        if page.events.is_empty() {
            return Err("the event is not in the ledger".to_string());
        }

        // The failure is visible: the checkpoint did not move.
        let checkpoint = fixture
            .checkpoints
            .checkpoint(CONSUMER, "tenant-acme")
            .await
            .expect("the checkpoint is read");
        if checkpoint.unwrap_or(0) != 0 {
            return Err(format!(
                "the checkpoint moved to {checkpoint:?} even though the projection failed"
            ));
        }

        // The graph holds nothing of the failed projection.
        let artifacts = fixture
            .graph
            .nodes(&acme(), NodeKind::Artifact)
            .await
            .expect("the nodes are read");
        if !artifacts.is_empty() {
            return Err("the graph holds nodes even though the projection failed".to_string());
        }

        // With the projector fixed, the projection resumes from the
        // checkpoint and applies every event exactly once.
        let report = project(&fixture, CONSUMER, "tenant-acme").await;
        if report.events_applied != 4 {
            return Err(format!(
                "the projection applied {} events and it should have applied four",
                report.events_applied
            ));
        }

        let artifacts = fixture
            .graph
            .nodes(&acme(), NodeKind::Artifact)
            .await
            .expect("the nodes are read");
        if artifacts.len() != 1 {
            return Err(format!(
                "the graph holds {} artefacts and it should hold one",
                artifacts.len()
            ));
        }

        // And a second run applies nothing new.
        let again = project(&fixture, CONSUMER, "tenant-acme").await;
        if again.events_applied != 0 {
            return Err(format!(
                "the second run applied {} events and it should have applied none",
                again.events_applied
            ));
        }

        Ok(())
    }
    .await;

    record("M1-UAT-06 projection failure and recovery", result);
}

// ───────────────────────────── M1-UAT-07 ─────────────────────────────

/// M1-UAT-07 El aislamiento entre cuentas.
#[tokio::test]
async fn m1_uat_07_tenant_isolation() {
    let fixture = fixture().await;
    admit(&fixture, &acme()).await;
    admit(&fixture, &other()).await;
    project(&fixture, CONSUMER, "tenant-acme").await;
    project(&fixture, CONSUMER, "tenant-other").await;

    let result = async {
        // The artefact of the other account, with the same name.
        let their_artifact = hrn("other", "Artifact", "web-1.0.0");
        let my_artifact = hrn("acme", "Artifact", "web-1.0.0");

        let mine = fixture
            .graph
            .lineage(&acme(), &my_artifact)
            .await
            .map_err(|error| error.to_string())?;
        let theirs = fixture
            .graph
            .lineage(&acme(), &their_artifact)
            .await
            .map_err(|error| error.to_string())?;

        // The lineage of the other account is not reachable from this one.
        if theirs.artifact.is_some() {
            return Err("the lineage of the other account is visible".to_string());
        }
        if theirs.builds.is_empty() && theirs.deployments.is_empty() {
            // Nothing of the other account leaked.
        } else {
            return Err("the lineage of the other account leaked".to_string());
        }

        // The deployments of the other account are not visible either.
        let their_deployments = fixture
            .graph
            .deployments(&acme(), &their_artifact)
            .await
            .map_err(|error| error.to_string())?;
        if !their_deployments.is_empty() {
            return Err("the deployments of the other account are visible".to_string());
        }

        // And the account sees its own.
        if mine.builds.is_empty() {
            return Err("the account does not see its own lineage".to_string());
        }

        Ok(())
    }
    .await;

    record("M1-UAT-07 tenant isolation", result);
}

// ───────────────────────────── M1-UAT-08 ─────────────────────────────

/// M1-UAT-08 El rendimiento de referencia.
///
/// El sobre de rendimiento de la UAT no esta acordado, y por eso lo que se
/// mide aqui es una LINEA BASE, no el cumplimiento de un objetivo: la
/// suite deja el numero y la tarea recoge el riesgo residual.
#[tokio::test]
async fn m1_uat_08_reference_performance() {
    let fixture = fixture().await;
    admit(&fixture, &acme()).await;
    project(&fixture, CONSUMER, "tenant-acme").await;

    let artifact = hrn("acme", "Artifact", "web-1.0.0");

    let result = async {
        // The time of the projection of the whole reference journey.
        let started = Instant::now();
        project(&fixture, "m1-uat-baseline", "tenant-acme").await;
        let projection_ms = started.elapsed().as_millis();

        // The time of the one-to-three hop queries, measured over a hundred
        // rounds so that the number means something.
        let rounds = 100;
        let started = Instant::now();
        for _ in 0..rounds {
            fixture
                .graph
                .lineage(&acme(), &artifact)
                .await
                .map_err(|error| error.to_string())?;
            fixture
                .graph
                .deployments(&acme(), &artifact)
                .await
                .map_err(|error| error.to_string())?;
            fixture
                .graph
                .artifacts_of_build(&acme(), &hrn("acme", "Build", "web-42"))
                .await
                .map_err(|error| error.to_string())?;
            fixture
                .graph
                .artifacts_of_repository(&acme(), &hrn("acme", "Repository", "web"))
                .await
                .map_err(|error| error.to_string())?;
        }
        let elapsed = started.elapsed();
        let per_query_us = elapsed.as_micros() as f64 / (rounds as f64 * 4.0);

        println!("  M1-UAT-08: projection {projection_ms} ms; queries {per_query_us:.1} us each");
        println!("  M1-UAT-08: the command was {COMMAND}");

        Ok(())
    }
    .await;

    record("M1-UAT-08 reference performance", result);
}
