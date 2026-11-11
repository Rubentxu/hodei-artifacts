//! The graph of evidence: the queries, the account scope and the
//! reconstruction.
//!
//! The graph is a projection of the ledger, so the property that matters is
//! that it can be rebuilt and that the rebuilt one says the same things.
//!
//! # Guarantees
//!
//! - G1 The lineage of an artefact says what produced it, what it came from
//!   and which evidence supports it.
//! - G2 The lineage of a digest that is not there says nothing, instead of
//!   inventing a relation.
//! - G3 A relation whose evidence is not in the graph is not accepted in
//!   silence: it is refused.
//! - G4 The queries of one account never return anything of another.
//! - G5 Rebuilding the graph from the same ledger produces a graph that says
//!   the same things, and the comparison is semantic.
//! - G6 The rebuild does not alter the ledger.

use chrono::{DateTime, TimeZone, Utc};
use hodei_evidence::{
    ContentDigest, EventLedger, EvidenceGraphProjector, EvidenceId, EvidenceIngestion,
    EvidenceType, EvidenceVerification, GraphProjector, GraphQuery, GraphWriter, IngestionEnvelope,
    IngestionOutcome, IngestionRequest, LedgerCursor, Lineage, NodeKind, Normalization, Producer,
    RelationClaim, RelationType, SourceRef, rebuild_graph,
};
use hodei_evidence_storage::surreal::SurrealGraphStore;
use hodei_evidence_storage::{
    HmacEnvelopeAuthenticator, InMemoryContentStore, ProducerCredentials, SurrealEventLedger,
    SurrealEvidenceStore,
};
use kernel::Hrn;
use std::sync::Arc;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

fn account() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/web-1.0.0").expect("an HRN")
}

fn other_account() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:other:Artifact/web-1.0.0").expect("an HRN")
}

fn hrn(kind: &str, id: &str) -> Hrn {
    Hrn::from_string(&format!("hrn:hodei:artifacts:eu-west-1:acme:{kind}/{id}")).expect("an HRN")
}

fn moment(second: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(1_767_322_800 + second, 0)
        .single()
        .expect("a moment")
}

fn producer() -> Producer {
    Producer::new("connector", "pipelinek").with_version("1.2.3")
}

/// The pieces of the journey, and the relation each one claims.
fn journey() -> Vec<(String, Vec<u8>, EvidenceType, Option<RelationClaim>)> {
    let repository = hrn("Repository", "web");
    let revision = hrn("SourceRevision", "web-abc123");
    let build = hrn("Build", "web-42");
    let artifact = hrn("Artifact", "web-1.0.0");
    let deployment = hrn("Deployment", "prod-7");

    vec![
        (
            "repo-1".to_string(),
            b"the repository".to_vec(),
            EvidenceType::SourceMetadata,
            Some(RelationClaim {
                relation: RelationType::HasRevision,
                from: repository.clone(),
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
                from: revision.clone(),
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
                from: build.clone(),
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
                from: artifact.clone(),
                to: deployment.clone(),
                from_kind: NodeKind::Artifact,
                to_kind: NodeKind::Deployment,
            }),
        ),
    ]
}

/// The store, the ledger and the graph, over a throwaway database.
async fn stores() -> (
    Arc<SurrealEventLedger<Db>>,
    Arc<SurrealEvidenceStore<Db>>,
    Arc<SurrealGraphStore<Db>>,
    Arc<SurrealGraphStore<Db>>,
) {
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

    (
        Arc::new(SurrealEventLedger::new(db.clone())),
        Arc::new(SurrealEvidenceStore::new(db.clone())),
        Arc::new(SurrealGraphStore::new(db.clone())),
        Arc::new(SurrealGraphStore::named(db, "graph_rebuild")),
    )
}

/// Admits the journey and projects it into the graph.
async fn admit_journey(
    ledger: &Arc<SurrealEventLedger<Db>>,
    evidence_store: &Arc<SurrealEvidenceStore<Db>>,
    graph: &Arc<SurrealGraphStore<Db>>,
) {
    let content_store = Arc::new(InMemoryContentStore::new());
    let authenticator = Arc::new(HmacEnvelopeAuthenticator::new());
    authenticator.register(
        "pipelinek",
        ProducerCredentials {
            secret: b"the-secret-of-the-producer".to_vec(),
            accounts: vec!["acme".to_string()],
        },
    );
    let ingestion = EvidenceIngestion::new(
        content_store,
        evidence_store.clone(),
        ledger.clone(),
        authenticator.clone(),
    );

    for (external_id, content, kind, claim) in journey() {
        let mut envelope = IngestionEnvelope {
            producer: producer(),
            tenant_hrn: account(),
            subject_refs: vec![],
            evidence_type: kind,
            schema_uri: None,
            schema_version: None,
            source: SourceRef::by_external_id(&external_id),
            content_digest: ContentDigest::of_bytes(&content),
            relation_claim: claim,
            observed_at: moment(10),
            valid_from: None,
            verification: EvidenceVerification::verified("signature"),
            normalization: Normalization::new("pipelinek", "1.0.0"),
            correlation_id: None,
            signature: String::new(),
        };
        envelope.signature = authenticator
            .sign(&envelope)
            .expect("the producer is known");

        let outcome = ingestion
            .ingest(IngestionRequest { envelope, content })
            .await
            .expect("the ingestion runs");
        assert!(
            matches!(outcome, IngestionOutcome::Accepted { .. }),
            "the piece of evidence is accepted: {outcome:?}"
        );
    }

    // The projection of what was accepted.
    let projector = EvidenceGraphProjector::new();
    let mut cursor = LedgerCursor::start_of("tenant-acme");
    loop {
        let page = ledger
            .read(cursor.clone(), 100)
            .await
            .expect("the ledger is read");
        if page.events.is_empty() {
            break;
        }
        let mut changes = hodei_evidence::GraphChangeSet::empty();
        for event in &page.events {
            changes = merge(changes, projector.project(event));
        }
        if !changes.is_empty() {
            graph.apply(changes).await.expect("the graph is written");
        }
        let Some(next) = page.next_cursor else { break };
        cursor = next;
    }
}

/// The union of two change sets.
fn merge(
    mut left: hodei_evidence::GraphChangeSet,
    right: hodei_evidence::GraphChangeSet,
) -> hodei_evidence::GraphChangeSet {
    left.nodes.extend(right.nodes);
    left.relations.extend(right.relations);
    left
}

/// G1 The lineage of an artefact says what produced it.
#[tokio::test]
async fn g1_the_lineage_says_what_produced_the_artifact() {
    let (ledger, evidence_store, graph, _rebuild) = stores().await;
    admit_journey(&ledger, &evidence_store, &graph).await;

    let lineage: Lineage = graph
        .lineage(&account(), &hrn("Artifact", "web-1.0.0"))
        .await
        .expect("the lineage is read");

    assert!(lineage.artifact.is_some(), "the artefact is in the lineage");
    assert_eq!(
        lineage.builds.len(),
        1,
        "the build that produced it is in the lineage"
    );
    assert_eq!(lineage.builds[0].id, hrn("Build", "web-42"));
    assert_eq!(
        lineage.revisions.len(),
        1,
        "the revision the build used is in the lineage"
    );
    assert_eq!(lineage.revisions[0].id, hrn("SourceRevision", "web-abc123"));
    assert!(
        lineage.repository.is_some(),
        "the repository the revision belongs to is in the lineage"
    );
    assert_eq!(
        lineage.deployments.len(),
        1,
        "the deployment of the artefact is in the lineage"
    );
    assert!(
        !lineage.evidence.is_empty(),
        "the lineage says which evidence supports it"
    );
}

/// G2 The lineage of something that is not there says nothing.
#[tokio::test]
async fn g2_the_lineage_of_what_is_not_there_says_nothing() {
    let (ledger, evidence_store, graph, _rebuild) = stores().await;
    admit_journey(&ledger, &evidence_store, &graph).await;

    let lineage = graph
        .lineage(&account(), &hrn("Artifact", "not-there"))
        .await
        .expect("the lineage is read");

    assert!(lineage.artifact.is_none(), "there is no artefact");
    assert!(lineage.builds.is_empty(), "there is no build");
    assert!(lineage.revisions.is_empty(), "there is no revision");
    assert!(
        lineage.evidence.is_empty(),
        "no relation is invented for what is not there"
    );
}

/// G3 A relation whose evidence is not in the graph is refused.
#[tokio::test]
async fn g3_a_relation_without_its_evidence_is_refused() {
    let (ledger, evidence_store, graph, _rebuild) = stores().await;
    admit_journey(&ledger, &evidence_store, &graph).await;

    // A relation that claims a piece of evidence that was never accepted.
    let forged = hodei_evidence::GraphRelation::new(
        RelationType::Produced,
        hrn("Build", "web-42"),
        hrn("Artifact", "other"),
        account(),
        EvidenceId::new("evd-does-not-exist"),
        moment(10),
    );

    let mut changes = hodei_evidence::GraphChangeSet::empty();
    changes = changes.with_relation(forged);
    let outcome = graph.apply(changes).await;

    assert!(
        outcome.is_err(),
        "a relation whose evidence is not in the graph is refused: {outcome:?}"
    );
}

/// G4 The queries of one account never return anything of another.
#[tokio::test]
async fn g4_the_queries_never_cross_the_accounts() {
    let (ledger, evidence_store, graph, _rebuild) = stores().await;
    admit_journey(&ledger, &evidence_store, &graph).await;

    let mine = graph
        .nodes(&account(), NodeKind::Artifact)
        .await
        .expect("the nodes are read");
    assert!(!mine.is_empty(), "the account sees its own nodes");

    let theirs = graph
        .nodes(&other_account(), NodeKind::Artifact)
        .await
        .expect("the nodes are read");
    assert!(
        theirs.is_empty(),
        "the other account sees nothing of this one"
    );

    let relations = graph
        .relations_from(&other_account(), &hrn("Build", "web-42"))
        .await
        .expect("the relations are read");
    assert!(
        relations.is_empty(),
        "the other account sees no relation of this one"
    );
}

/// G5 Rebuilding the graph produces a graph that says the same things.
#[tokio::test]
async fn g5_the_rebuilt_graph_says_the_same_things() {
    let (ledger, evidence_store, graph, rebuilt) = stores().await;
    admit_journey(&ledger, &evidence_store, &graph).await;

    let report = rebuild_graph(
        ledger.as_ref(),
        &EvidenceGraphProjector::new(),
        rebuilt.as_ref(),
        &account(),
        Some(graph.as_ref()),
    )
    .await
    .expect("the rebuild runs");

    assert_eq!(report.events_read, 4, "the four events were read");
    assert_eq!(
        report.events_projected, 4,
        "the four events change the graph"
    );
    assert!(report.equivalent, "the graphs say the same things");
    assert!(
        report.differences.is_empty(),
        "there are no differences: {:?}",
        report.differences
    );

    // And the rebuilt graph answers the same question.
    let before = graph
        .lineage(&account(), &hrn("Artifact", "web-1.0.0"))
        .await
        .expect("the lineage is read");
    let after = rebuilt
        .lineage(&account(), &hrn("Artifact", "web-1.0.0"))
        .await
        .expect("the lineage is read");
    assert_eq!(before.builds, after.builds, "the same builds");
    assert_eq!(before.revisions, after.revisions, "the same revisions");
    assert_eq!(
        before.deployments, after.deployments,
        "the same deployments"
    );
}

/// G6 The rebuild does not alter the ledger.
#[tokio::test]
async fn g6_the_rebuild_does_not_alter_the_ledger() {
    let (ledger, evidence_store, graph, rebuilt) = stores().await;
    admit_journey(&ledger, &evidence_store, &graph).await;

    let before = ledger
        .read(LedgerCursor::start_of("tenant-acme"), 100)
        .await
        .expect("the ledger is read");

    rebuild_graph(
        ledger.as_ref(),
        &EvidenceGraphProjector::new(),
        rebuilt.as_ref(),
        &account(),
        Some(graph.as_ref()),
    )
    .await
    .expect("the rebuild runs");

    let after = ledger
        .read(LedgerCursor::start_of("tenant-acme"), 100)
        .await
        .expect("the ledger is read");

    assert_eq!(
        before.events.len(),
        after.events.len(),
        "the ledger holds the same events"
    );
    assert_eq!(
        before.events, after.events,
        "the events are the same ones, untouched"
    );
}
