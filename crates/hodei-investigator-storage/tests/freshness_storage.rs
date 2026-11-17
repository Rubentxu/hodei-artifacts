//! El comprobador de frescura, sobre el almacenamiento real.
//!
//! El verificador puro ya esta probado en el dominio. Aqui lo que se
//! prueba es lo de los adaptadores: que el mundo que se trae es el
//! mundo de verdad, que los deltas que se leen son los del tenant, y
//! que una cuenta ajena no se consulta.
//!
//! El material es el de SurrealDB, con su registro de deltas, porque es
//! el que corre en el despliegue.

use std::sync::Arc;

use hodei_evidence::ContentDigest;
use hodei_evidence::ContentRef;
use hodei_evidence::domain::delta::RelationDeltaLedger;
use hodei_evidence::domain::evidence::{
    EvidenceId, EvidenceRecord, EvidenceType, EvidenceVerification, Normalization, Producer,
    SourceRef,
};
use hodei_evidence::domain::graph::{
    GraphChangeSet, GraphNode, GraphRelation, GraphRelationWithdrawal, NodeKind, RelationType,
};
use hodei_evidence::ports::EvidenceStore;
use hodei_evidence::ports::graph::GraphWriter;
use hodei_evidence_storage::{SurrealEvidenceStore, SurrealGraphStore, SurrealRelationDeltaLedger};
use hodei_investigator::freshness::{Freshness, GRAPH_SOURCE};
use hodei_investigator::read_set::{ReadKind, ReadSet, ReadStamp};
use hodei_investigator_storage::freshness::LedgerFreshness;
use kernel::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

/// El tenant del grafo: el de la cuenta.
fn tenant() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/acme").expect("un HRN")
}

/// El sujeto de la investigacion.
fn subject() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app").expect("un HRN")
}

/// El otro extremo de la relacion.
fn component() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/lib").expect("un HRN")
}

/// El contenido de la evidencia: un digest, que es lo que se verifica.
fn the_digest() -> ContentDigest {
    ContentDigest::of_bytes(b"the sbom of the artefact declares the component lib-vuln")
}

/// El mundo de la prueba: su base, su grafo, su evidencia y su registro.
struct TheWorld {
    _tmp: tempfile::TempDir,
    graph: Arc<SurrealGraphStore<Db>>,
    evidence: Arc<SurrealEvidenceStore<Db>>,
    ledger: Arc<SurrealRelationDeltaLedger<Db>>,
    relation: GraphRelation,
    digest: ContentDigest,
}

impl TheWorld {
    /// Un mundo con una evidencia y una relacion, y con la causa que
    /// hace que el registro diga lo que cambio.
    async fn new() -> Self {
        let tmp = tempfile::tempdir().expect("un directorio desechable");
        let db = Arc::new(
            Surreal::new::<RocksDb>(
                tmp.path()
                    .join("evidence.rocksdb")
                    .to_string_lossy()
                    .to_string(),
            )
            .await
            .expect("la base abre"),
        );
        db.use_ns("hodei")
            .use_db("evidence")
            .await
            .expect("el espacio");

        let evidence = Arc::new(SurrealEvidenceStore::new(db.clone()));
        let graph = Arc::new(SurrealGraphStore::new(db.clone()));
        let ledger = Arc::new(SurrealRelationDeltaLedger::new(db));

        let digest = the_digest();
        let record = EvidenceRecord::builder(EvidenceId::new("ev-1"), tenant())
            .evidence_type(EvidenceType::Sbom)
            .subjects(vec![subject()])
            .source(SourceRef::by_external_id("external-1"))
            .content(ContentRef::by_digest(digest.clone()))
            .observed_at(chrono::Utc::now())
            .recorded_at(chrono::Utc::now())
            .verification(EvidenceVerification::verified("dsse"))
            .normalization(Normalization::new("cyclonedx", "1.0.0"))
            .producer(Producer::new("connector", "pipelinek").with_version("1.0.0"))
            .build();
        evidence.put(record).await.expect("la evidencia se guarda");

        let relation = GraphRelation::new(
            RelationType::Contains,
            subject(),
            component(),
            tenant(),
            EvidenceId::new("ev-1"),
            chrono::Utc::now(),
        );
        let changes = GraphChangeSet::empty()
            .with_node(GraphNode::new(
                subject(),
                tenant(),
                NodeKind::Artifact,
                chrono::Utc::now(),
            ))
            .with_node(GraphNode::new(
                component(),
                tenant(),
                NodeKind::Artifact,
                chrono::Utc::now(),
            ))
            .with_node(GraphNode::new(
                hodei_evidence::application::projector::evidence_hrn(
                    &tenant(),
                    &EvidenceId::new("ev-1"),
                ),
                tenant(),
                NodeKind::Evidence,
                chrono::Utc::now(),
            ))
            .with_relation(relation.clone())
            .with_cause("evt-1");
        graph.apply(changes).await.expect("el grafo se escribe");

        Self {
            _tmp: tmp,
            graph,
            evidence,
            ledger,
            relation,
            digest,
        }
    }

    /// El manifiesto de la lectura: lo que el informe diria haber leido.
    async fn the_report(&self) -> ReadSet {
        let version = self
            .ledger
            .version_of(&tenant())
            .await
            .expect("la version se lee");
        let mut read_set = ReadSet::empty();
        read_set.generation(GRAPH_SOURCE, version);
        read_set.read(ReadStamp::relation(self.relation.identity(), "ev-1"));
        read_set.read(ReadStamp::evidence("ev-1", self.digest.to_string()));
        read_set
    }

    /// El comprobador de esta cuenta.
    fn the_checker(&self) -> LedgerFreshness {
        LedgerFreshness::new(
            self.evidence.clone() as Arc<dyn EvidenceStore>,
            self.ledger.clone() as Arc<dyn RelationDeltaLedger>,
            tenant(),
        )
    }
}

/// Un informe cuyo mundo no se movio SIGUE VIGENTE.
#[tokio::test]
async fn a_report_whose_world_did_not_move_is_still_current() {
    let world = TheWorld::new().await;
    let report = world.the_report().await;

    let verdict = world.the_checker().freshness_of(&report, &subject()).await;
    assert_eq!(verdict, Freshness::Current);
}

/// El apoyo que el informe leyo ya no esta: el informe OBSOLETO, y dice
/// que referencia fue la que se movio.
#[tokio::test]
async fn the_support_the_report_rests_on_is_gone_and_the_report_is_stale() {
    let world = TheWorld::new().await;
    let report = world.the_report().await;

    // La evidencia retira su apoyo: la relacion se queda sin sustainer.
    let withdrawal = GraphChangeSet::empty()
        .with_withdrawal(GraphRelationWithdrawal::new(
            RelationType::Contains,
            subject(),
            component(),
            tenant(),
            EvidenceId::new("ev-1"),
            chrono::Utc::now(),
        ))
        .with_cause("evt-2");
    world
        .graph
        .apply(withdrawal)
        .await
        .expect("la retirada se escribe");

    match world.the_checker().freshness_of(&report, &subject()).await {
        Freshness::Stale { changed } => {
            assert_eq!(changed.len(), 1, "una referencia cambio: {changed:?}");
            assert_eq!(changed[0].kind, ReadKind::Relation);
            assert_eq!(changed[0].resource, world.relation.identity());
            assert_eq!(changed[0].evidence, "ev-1");
        }
        other => panic!("un apoyo que se retiro deja el informe obsoleto: {other:?}"),
    }
}

/// Un cambio AJENO al informe no lo invalida: lo que no se leyo no puede
/// caducar lo que se leyo.
#[tokio::test]
async fn a_change_away_from_the_report_does_not_invalidate_it() {
    let world = TheWorld::new().await;
    let report = world.the_report().await;

    // Otra relacion, de otro par de nodos: el grafo se movio, pero lo
    // que el informe leyo sigue igual.
    let other = GraphRelation::new(
        RelationType::Contains,
        subject(),
        Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/other").expect("un HRN"),
        tenant(),
        EvidenceId::new("ev-2"),
        chrono::Utc::now(),
    );
    let changes = GraphChangeSet::empty()
        .with_node(GraphNode::new(
            other.to.clone(),
            tenant(),
            NodeKind::Artifact,
            chrono::Utc::now(),
        ))
        .with_node(GraphNode::new(
            hodei_evidence::application::projector::evidence_hrn(
                &tenant(),
                &EvidenceId::new("ev-2"),
            ),
            tenant(),
            NodeKind::Evidence,
            chrono::Utc::now(),
        ))
        .with_relation(other)
        .with_cause("evt-3");
    world
        .graph
        .apply(changes)
        .await
        .expect("el grafo se escribe");

    assert_eq!(
        world.the_checker().freshness_of(&report, &subject()).await,
        Freshness::Current
    );
}

/// Un sujeto de otra cuenta NO se consulta: el veredicto es que no se
/// sabe, y no una respuesta sobre lo que no es suyo.
#[tokio::test]
async fn a_subject_of_another_account_is_not_consulted() {
    let world = TheWorld::new().await;
    let report = world.the_report().await;

    let foreign =
        Hrn::from_string("hrn:hodei:artifacts:eu-west-1:contoso:Artifact/app").expect("un HRN");

    match world.the_checker().freshness_of(&report, &foreign).await {
        Freshness::Unknown { reason } => {
            assert!(reason.contains("tenant"), "el motivo lo dice: {reason}")
        }
        other => panic!("una cuenta ajena no se consulta: {other:?}"),
    }
}
