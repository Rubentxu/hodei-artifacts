//! La UAT de M7: la simulacion responde "que pasaria si" sin tocar la
//! verdad.
//!
//! La UAT corre sobre el grafo REAL, con la evidencia real y la decision
//! real: el fork se superpone al estado de verdad, el diff se calcula
//! sobre el, y al terminar el grafo real esta igual.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use hodei_evidence::domain::evidence::EvidenceType;
use hodei_evidence::domain::graph::{GraphNode, GraphRelation, NodeKind, RelationType};
use hodei_evidence::{
    ContentDigest, EvidenceIngestion, EvidenceVerification, IngestionEnvelope, IngestionOutcome,
    IngestionRequest, Normalization, Producer, SourceRef,
};
use hodei_evidence_storage::surreal::graph::SurrealGraphStore;
use hodei_evidence_storage::{
    HmacEnvelopeAuthenticator, InMemoryContentStore, ProducerCredentials, SurrealEventLedger,
    SurrealEvidenceStore,
};
use hodei_simulation::hypothetical::{Hypothetical, HypotheticalClaim, RelationFact};
use hodei_simulation::ports::{DecisionEvaluator, SimulationGraph};
use hodei_simulation::service::SimulationService;
use hodei_simulation::{Checkpoint, SimulationFork, SimulationTag};
use hodei_simulation_storage::{CapsuleDecisionEvaluator, SurrealSimulationGraph};
use hodei_trust_policy::decision::Outcome;
use hodei_trust_policy::{Requirement, RequirementKind, TrustPolicy};
use kernel::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

/// La cuenta de la UAT.
const ACCOUNT: &str = "acme";

/// El resultado de un escenario, tal como queda registrado.
///
/// Un escenario que falla FALLA el test. Antes solo se imprimia, y un
/// `cargo test` en verde no decia nada del resultado de los escenarios:
/// el verde de la suite era un verde de mentira. La UAT es una
/// aceptacion, y una aceptacion que no puede fallar no es una
/// aceptacion.
fn record(scenario: &str, outcome: Result<(), String>) {
    match outcome {
        Ok(()) => println!("{scenario}: PASSED"),
        Err(reason) => panic!("{scenario}: FAILED -- {reason}"),
    }
}

/// El sujeto de la UAT.
fn subject() -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{ACCOUNT}:Artifact/app-1.0.0"
    ))
    .expect("un HRN")
}

/// El otro sujeto de la UAT.
fn other_subject() -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{ACCOUNT}:Artifact/api-2.0.0"
    ))
    .expect("un HRN")
}

/// El despliegue del sujeto.
fn deployment() -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{ACCOUNT}:Deployment/production"
    ))
    .expect("un HRN")
}

/// El componente del que depende el sujeto.
fn component_v1() -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{ACCOUNT}:Component/lib-v1"
    ))
    .expect("un HRN")
}

/// El componente al que se sube.
fn component_v2() -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{ACCOUNT}:Component/lib-v2"
    ))
    .expect("un HRN")
}

/// La cuenta de la simulacion.
fn account_hrn() -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{ACCOUNT}:Artifact/{ACCOUNT}"
    ))
    .expect("un HRN")
}

/// El mundo de la UAT: el grafo, la evidencia y las decisiones.
struct World {
    db: Arc<Surreal<Db>>,
    graph: Arc<SurrealGraphStore<Db>>,
    evidence: Arc<SurrealEvidenceStore<Db>>,
    ingestion: Arc<EvidenceIngestion>,
    authenticator: Arc<HmacEnvelopeAuthenticator>,
}

impl World {
    /// El mundo con su grafo y su almacen de evidencia.
    async fn build(path: &std::path::Path) -> Self {
        let db = Arc::new(
            Surreal::new::<RocksDb>(path.join("db").to_string_lossy().to_string())
                .await
                .expect("la base de datos abre"),
        );
        db.use_ns("hodei")
            .use_db("evidence")
            .await
            .expect("el namespace");

        let ledger = Arc::new(SurrealEventLedger::new(db.clone()));
        let evidence = Arc::new(SurrealEvidenceStore::new(db.clone()));
        let graph = Arc::new(SurrealGraphStore::new(db.clone()));
        let content = Arc::new(InMemoryContentStore::new());
        let authenticator = Arc::new(HmacEnvelopeAuthenticator::new());
        authenticator.register(
            "pipelinek",
            ProducerCredentials {
                secret: b"el-secreto-del-productor".to_vec(),
                accounts: vec![ACCOUNT.to_string()],
            },
        );
        let ingestion =
            EvidenceIngestion::new(content, evidence.clone(), ledger, authenticator.clone());

        Self {
            db,
            graph,
            evidence,
            ingestion: Arc::new(ingestion),
            authenticator,
        }
    }

    /// Admite una pieza de evidencia del sujeto.
    async fn admit(&self, external_id: &str, kind: EvidenceType, observed_at: DateTime<Utc>) {
        let content = format!("the content of {external_id}").into_bytes();
        let mut envelope = IngestionEnvelope {
            producer: Producer::new("connector", "pipelinek").with_version("1.2.3"),
            tenant_hrn: account_hrn(),
            subject_refs: vec![subject()],
            evidence_type: kind,
            schema_uri: None,
            schema_version: None,
            source: SourceRef::by_external_id(external_id),
            content_digest: ContentDigest::of_bytes(&content),
            relation_claim: None,
            observed_at,
            valid_from: None,
            verification: EvidenceVerification::verified("signature"),
            normalization: Normalization::new("pipelinek", "1.0.0"),
            correlation_id: None,
            signature: String::new(),
        };
        envelope.signature = self
            .authenticator
            .sign(&envelope)
            .expect("el productor es conocido");

        let outcome = self
            .ingestion
            .ingest(IngestionRequest { envelope, content })
            .await
            .expect("la admision corre");
        assert!(
            matches!(outcome, IngestionOutcome::Accepted { .. }),
            "la evidencia se admite: {outcome:?}"
        );
    }

    /// La capa de riesgo: los artefactos, sus componentes y el despliegue.
    async fn the_risk_layer(&self) {
        let now = Utc::now();
        let evidence = hodei_evidence::EvidenceId::new("ev-m7-1");

        let changes = hodei_evidence::domain::graph::GraphChangeSet::empty()
            .with_node(GraphNode::new(
                subject(),
                account_hrn(),
                NodeKind::Artifact,
                now,
            ))
            .with_node(GraphNode::new(
                other_subject(),
                account_hrn(),
                NodeKind::Artifact,
                now,
            ))
            .with_node(GraphNode::new(
                component_v1(),
                account_hrn(),
                NodeKind::Component,
                now,
            ))
            .with_node(GraphNode::new(
                deployment(),
                account_hrn(),
                NodeKind::Deployment,
                now,
            ))
            .with_node(GraphNode::new(
                hodei_evidence::application::projector::evidence_hrn(&account_hrn(), &evidence),
                account_hrn(),
                NodeKind::Evidence,
                now,
            ))
            .with_relation(GraphRelation::new(
                RelationType::DependsOn,
                subject(),
                component_v1(),
                account_hrn(),
                evidence.clone(),
                now,
            ))
            .with_relation(GraphRelation::new(
                RelationType::DeployedAs,
                subject(),
                deployment(),
                account_hrn(),
                evidence,
                now,
            ));

        use hodei_evidence::ports::graph::GraphWriter;
        self.graph.apply(changes).await.expect("la capa se escribe");
    }

    /// El servicio de la simulacion sobre el mundo real.
    fn the_service(&self) -> SimulationService {
        let graph: Arc<dyn SimulationGraph> = Arc::new(SurrealSimulationGraph::new(
            self.graph.clone(),
            account_hrn(),
        ));
        let decisions: Arc<dyn DecisionEvaluator> = Arc::new(CapsuleDecisionEvaluator::new(
            self.evidence.clone(),
            ACCOUNT,
        ));
        SimulationService::new(graph, decisions)
    }

    /// Cuantos eventos tiene el ledger.
    async fn the_ledger_count(&self) -> usize {
        let response = self
            .db
            .query("SELECT count() AS total FROM type::table($table) GROUP ALL")
            .bind(("table", "event_ledger"))
            .await
            .expect("el ledger se lee");
        let mut response = response;
        let rows: Vec<TotalRow> = response.take(0).expect("el ledger se lee");
        rows.first().map(|row| row.total).unwrap_or(0) as usize
    }
}

/// La fila de la cuenta.
#[derive(Debug, Clone, serde::Deserialize)]
struct TotalRow {
    /// El total.
    total: i64,
}

/// La politica de ahora: la que permite mientras el riesgo no este
/// establecido.
fn the_policy() -> TrustPolicy {
    TrustPolicy::new(
        "production",
        "1.0.0",
        vec![Requirement::denying(
            RequirementKind::NoReachableCriticalVulnerability,
        )],
    )
}

/// La politica que exige la trazabilidad de la construccion.
fn the_provenance_policy() -> TrustPolicy {
    TrustPolicy::new(
        "production",
        "1.0.0",
        vec![Requirement::denying(RequirementKind::Dimension {
            dimension: hodei_evidence::domain::capsule::DimensionName::BuildProvenance,
            accepts: vec![hodei_evidence::domain::capsule::DimensionState::Verified],
        })],
    )
}

/// La politica hipotetica: la que exige que no haya vulnerabilidad
/// critica alcanzable.
fn the_strict_policy() -> TrustPolicy {
    TrustPolicy::new(
        "production",
        "2.0.0",
        vec![
            Requirement::denying(RequirementKind::NoReachableCriticalVulnerability),
            Requirement::denying(RequirementKind::RiskEstablished),
        ],
    )
}

/// El fork de la simulacion.
fn the_fork(policy: TrustPolicy) -> SimulationFork {
    SimulationFork::new(
        "sim-m7",
        Checkpoint::new("checkpoint-m7", ACCOUNT, Utc::now(), Some(0)),
        policy,
        Utc::now(),
    )
}

/// La etiqueta de la simulacion.
fn the_tag() -> SimulationTag {
    SimulationTag::new("sim-m7", Utc::now())
}

// ───────────────────────────── M7-UAT-01 ─────────────────────────────

/// M7-UAT-01 El radio de impacto de una politica propuesta.
///
/// Given production graph at checkpoint C and active policy P1, when
/// simulation forks C and activates hypothetical P2, then output lists
/// decisions that change, reasons and affected deployments while
/// production policy remains P1.
#[tokio::test]
async fn m7_uat_01_the_proposed_policy_changes_the_decisions() {
    let result: Result<(), String> = async {
        let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");
        let path = tmp.keep();
        let world = World::build(&path).await;
        world.the_risk_layer().await;
        world.admit("sbom-1", EvidenceType::Sbom, Utc::now()).await;
        let service = world.the_service();

        // La simulacion: la politica P2 y una vulnerabilidad que llega.
        let fork = the_fork(the_policy())
            .with_policy(the_strict_policy())
            .with(Hypothetical {
                tag: the_tag(),
                claim: HypotheticalClaim::Risk {
                    subject: subject(),
                    advisory: "GHSA-m7-0001".to_string(),
                    critical: true,
                    reachable: true,
                },
            });

        let diff = service
            .simulate(&fork, Utc::now())
            .await
            .map_err(|e| e.to_string())?;

        // La decision cambia, con las dos versiones de la politica.
        assert_eq!(
            diff.decisions.len(),
            1,
            "la decision que cambia: {:?}",
            diff.decisions
        );
        let change = &diff.decisions[0];
        assert_eq!(change.subject, subject());
        assert_eq!(change.before, Outcome::Allow);
        assert_eq!(change.after, Outcome::Deny);
        assert_eq!(change.policy_version_before, "1.0.0");
        assert_eq!(change.policy_version_after, "2.0.0");

        // Y el diff dice los despliegues a los que el cambio llega.
        assert!(
            diff.structural.affected_deployments.contains(&deployment()),
            "el despliegue afectado: {:?}",
            diff.structural.affected_deployments
        );

        // Y la politica de produccion SIGUE siendo P1: la simulacion no
        // ha escrito ninguna decision, porque el puerto de decision no
        // tiene por donde escribir.
        let mut decisions = world
            .db
            .query("SELECT count() AS total FROM type::table($table) GROUP ALL")
            .bind(("table", "promotion_decision"))
            .await
            .map_err(|e| e.to_string())?;
        let rows: Vec<TotalRow> = decisions.take(0).map_err(|e| e.to_string())?;
        assert!(
            rows.first().map(|row| row.total).unwrap_or(0) == 0,
            "la simulacion no ha escrito ninguna decision: {:?}",
            rows
        );
        Ok(())
    }
    .await;

    record("M7-UAT-01", result);
}

// ───────────────────────────── M7-UAT-02 ─────────────────────────────

/// M7-UAT-02 La subida de una dependencia.
///
/// Given artifact depends on vulnerable package v1, when simulation
/// substitutes proposed v2 plus supplied dependency/evidence changes, then
/// diff shows removed/added dependency/risk relations and new
/// TrustDecision outcome.
#[tokio::test]
async fn m7_uat_02_the_dependency_upgrade_is_in_the_diff() {
    let result: Result<(), String> = async {
        let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");
        let path = tmp.keep();
        let world = World::build(&path).await;
        world.the_risk_layer().await;
        let service = world.the_service();

        // La simulacion: la dependencia v1 se sustituye por la v2.
        let fork = the_fork(the_policy()).with(Hypothetical {
            tag: the_tag(),
            claim: HypotheticalClaim::Relation {
                relation: RelationType::DependsOn,
                from: subject(),
                to: component_v2(),
                replaces: Some(RelationFact::new(
                    RelationType::DependsOn,
                    subject(),
                    component_v1(),
                )),
            },
        });

        let diff = service
            .simulate(&fork, Utc::now())
            .await
            .map_err(|e| e.to_string())?;

        // El diff muestra la relacion que aparece y la que desaparece.
        assert_eq!(
            diff.structural.appeared.len(),
            1,
            "aparece: {:?}",
            diff.structural.appeared
        );
        assert_eq!(diff.structural.appeared[0].to, component_v2());
        assert_eq!(
            diff.structural.disappeared.len(),
            1,
            "desaparece: {:?}",
            diff.structural.disappeared
        );
        assert_eq!(diff.structural.disappeared[0].to, component_v1());
        Ok(())
    }
    .await;

    record("M7-UAT-02", result);
}

// ───────────────────────────── M7-UAT-03 ─────────────────────────────

/// M7-UAT-03 La ventana de un constructor comprometido.
///
/// Given builder B suspected compromised during T1..T2, when
/// simulation/hypothesis marks builds in window untrusted, then diff
/// enumerates artifacts/deployments whose trust changes.
#[tokio::test]
async fn m7_uat_03_the_compromised_builder_window_is_in_the_diff() {
    let result: Result<(), String> = async {
        let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");
        let path = tmp.keep();
        let world = World::build(&path).await;
        world.the_risk_layer().await;

        // La evidencia del sujeto, observada DENTRO de la ventana.
        let observed_at = Utc::now() - Duration::hours(2);
        world
            .admit("provenance-window", EvidenceType::Provenance, observed_at)
            .await;

        let service = world.the_service();
        let now = Utc::now();

        // La simulacion: el constructor no es de fiar entre las dos horas.
        let fork = the_fork(the_provenance_policy()).with(Hypothetical {
            tag: the_tag(),
            claim: HypotheticalClaim::UntrustedWindow {
                from: observed_at - Duration::minutes(30),
                to: observed_at + Duration::minutes(30),
                subjects: vec![subject()],
            },
        });

        let diff = service
            .simulate(&fork, now)
            .await
            .map_err(|e| e.to_string())?;

        // La decision del sujeto cambia: la evidencia de dentro de la
        // ventana deja de estar verificada, y la politica exige que la
        // trazabilidad de la construccion lo este.
        assert_eq!(
            diff.decisions.len(),
            1,
            "la decision que cambia: {:?}",
            diff.decisions
        );
        assert_eq!(diff.decisions[0].subject, subject());
        assert_eq!(diff.decisions[0].before, Outcome::Allow);
        assert_eq!(diff.decisions[0].after, Outcome::Deny);
        Ok(())
    }
    .await;

    record("M7-UAT-03", result);
}

// ───────────────────────────── M7-UAT-04 ─────────────────────────────

/// M7-UAT-04 El aislamiento.
///
/// After all simulations, production ledger event count/state is unchanged
/// except optional audit record that simulation was run. Hypothetical
/// facts are never visible in normal graph queries.
#[tokio::test]
async fn m7_uat_04_the_production_state_is_untouched() {
    let result: Result<(), String> = async {
        let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");
        let path = tmp.keep();
        let world = World::build(&path).await;
        world.the_risk_layer().await;
        world.admit("sbom-1", EvidenceType::Sbom, Utc::now()).await;

        let events_before = world.the_ledger_count().await;
        let relations_before = world
            .the_service()
            .graph_relations()
            .await
            .map_err(|e| e.to_string())?;

        let service = world.the_service();
        let fork = the_fork(the_policy())
            .with_policy(the_strict_policy())
            .with(Hypothetical {
                tag: the_tag(),
                claim: HypotheticalClaim::Relation {
                    relation: RelationType::DependsOn,
                    from: subject(),
                    to: component_v2(),
                    replaces: Some(RelationFact::new(
                        RelationType::DependsOn,
                        subject(),
                        component_v1(),
                    )),
                },
            })
            .with(Hypothetical {
                tag: the_tag(),
                claim: HypotheticalClaim::Risk {
                    subject: subject(),
                    advisory: "GHSA-m7-0001".to_string(),
                    critical: true,
                    reachable: true,
                },
            });

        let diff = service
            .simulate(&fork, Utc::now())
            .await
            .map_err(|e| e.to_string())?;
        assert!(!diff.is_empty(), "la simulacion dice algo");

        // El ledger tiene LOS MISMOS eventos.
        let events_after = world.the_ledger_count().await;
        assert_eq!(
            events_before, events_after,
            "el ledger no ha cambiado: {events_before} antes y {events_after} despues"
        );

        // Y el grafo tiene LAS MISMAS relaciones: el hecho hipotetico no
        // es visible en una consulta normal.
        let relations_after = world
            .the_service()
            .graph_relations()
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(
            relations_before,
            relations_after,
            "el grafo no ha cambiado: {} antes y {} despues",
            relations_before.len(),
            relations_after.len()
        );
        assert!(
            !relations_after
                .iter()
                .any(|relation| relation.to == component_v2()),
            "y la relacion hipotetica NO existe en el grafo real"
        );
        Ok(())
    }
    .await;

    record("M7-UAT-04", result);
}

// ───────────────────────────── M7-UAT-05 ─────────────────────────────

/// M7-UAT-05 La reproducibilidad.
///
/// Given same base checkpoint, same hypothetical inputs and same
/// deterministic behavior/policy versions, then repeated simulation
/// yields same structural/decision diff.
#[tokio::test]
async fn m7_uat_05_the_same_inputs_give_the_same_diff() {
    let result: Result<(), String> = async {
        let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");
        let path = tmp.keep();
        let world = World::build(&path).await;
        world.the_risk_layer().await;
        world.admit("sbom-1", EvidenceType::Sbom, Utc::now()).await;
        let service = world.the_service();

        let the_input = || {
            the_fork(the_policy())
                .with_policy(the_strict_policy())
                .with(Hypothetical {
                    tag: the_tag(),
                    claim: HypotheticalClaim::Risk {
                        subject: subject(),
                        advisory: "GHSA-m7-0001".to_string(),
                        critical: true,
                        reachable: true,
                    },
                })
        };

        let first = service
            .simulate(&the_input(), Utc::now())
            .await
            .map_err(|e| e.to_string())?;
        let second = service
            .simulate(&the_input(), Utc::now())
            .await
            .map_err(|e| e.to_string())?;

        assert_eq!(
            first.structural, second.structural,
            "el diff estructural es el mismo"
        );
        assert_eq!(
            first.decisions, second.decisions,
            "y el diff de decisiones tambien"
        );
        Ok(())
    }
    .await;

    record("M7-UAT-05", result);
}

// ───────────────────────── HG-H4-01: solo politica, sin engaños ─────────────────────────

/// Un lector de evidencia que FALLA al leer: el error no puede
/// convertirse en una capsula vacia ni en un ALLOW.
struct FailingEvidenceStore {
    inner: Arc<SurrealEvidenceStore<Db>>,
}

#[async_trait]
impl hodei_evidence::ports::EvidenceStore for FailingEvidenceStore {
    async fn put(
        &self,
        evidence: hodei_evidence::domain::evidence::EvidenceRecord,
    ) -> Result<hodei_evidence::EvidenceId, hodei_evidence::ports::StoreError> {
        self.inner.put(evidence).await
    }
    async fn get(
        &self,
        id: &hodei_evidence::EvidenceId,
    ) -> Result<
        Option<hodei_evidence::domain::evidence::EvidenceRecord>,
        hodei_evidence::ports::StoreError,
    > {
        self.inner.get(id).await
    }
    async fn find_by_external_id(
        &self,
        producer: &hodei_evidence::domain::evidence::Producer,
        external_id: &str,
    ) -> Result<
        Option<hodei_evidence::domain::evidence::EvidenceRecord>,
        hodei_evidence::ports::StoreError,
    > {
        self.inner.find_by_external_id(producer, external_id).await
    }
    async fn evidence_of_subject(
        &self,
        _account: &str,
        _subject: &kernel::Hrn,
    ) -> Result<
        Vec<hodei_evidence::domain::evidence::EvidenceRecord>,
        hodei_evidence::ports::StoreError,
    > {
        Err(hodei_evidence::ports::StoreError::Unavailable(
            "the evidence store is down".to_string(),
        ))
    }
}

/// M7-UAT-06 La simulacion de SOLO politica, con sujetos declarados.
///
/// Given a base policy that allows everything and no risk facts at all,
/// when simulation forks with a stricter hypothetical policy and
/// EXPLICITLY declared subjects, then the decisions that change are
/// listed -- y no se infiere que "sin hechos no hay afectados".
#[tokio::test]
async fn m7_uat_06_the_policy_only_simulation_with_declared_subjects_changes_the_decisions() {
    let result: Result<(), String> = async {
        let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");
        let path = tmp.keep();
        let world = World::build(&path).await;
        let service = world.the_service();

        // La politica de ahora lo permite todo; la hipotetica exige la
        // trazabilidad verificada de la construccion, que este mundo no
        // tiene: NINGUN hecho de riesgo o relacion adicional.
        let base = TrustPolicy::new("production", "1.0.0", Vec::new());
        let fork = the_fork(base)
            .with_subjects(vec![subject(), other_subject()])
            .with_policy(the_provenance_policy());

        let diff = service
            .simulate(&fork, Utc::now())
            .await
            .map_err(|e| e.to_string())?;

        assert_eq!(
            diff.decisions.len(),
            2,
            "los DOS sujetos declarados se evaluan: {:?}",
            diff.decisions
        );
        for change in &diff.decisions {
            assert_eq!(change.before, Outcome::Allow, "la base lo permitia");
            assert_ne!(
                change.before, change.after,
                "y la politica hipotetica cambia el resultado: {:?}",
                change
            );
            assert_eq!(change.policy_version_after, "1.0.0");
        }
        assert!(
            diff.structural.affected_subjects.contains(&subject()),
            "el diff dice a quien le llega: {:?}",
            diff.structural.affected_subjects
        );
        Ok(())
    }
    .await;

    record("M7-UAT-06", result);
}

/// M7-UAT-07 Un fallo de lectura de evidencia NO se convierte en
/// decision: el error es tipado y llega, no se disfraza de capsula
/// vacia ni de diff silencioso.
#[tokio::test]
async fn m7_uat_07_a_failed_evidence_read_is_not_an_allow() {
    let result: Result<(), String> = async {
        let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");
        let path = tmp.keep();
        let world = World::build(&path).await;
        let graph: Arc<dyn SimulationGraph> = Arc::new(SurrealSimulationGraph::new(
            world.graph.clone(),
            account_hrn(),
        ));
        let decisions: Arc<dyn DecisionEvaluator> = Arc::new(CapsuleDecisionEvaluator::new(
            Arc::new(FailingEvidenceStore {
                inner: world.evidence.clone(),
            }),
            ACCOUNT,
        ));
        let service = SimulationService::new(graph, decisions);

        let fork = the_fork(the_policy())
            .with_subjects(vec![subject()])
            .with_policy(the_strict_policy());

        let error = service
            .simulate(&fork, Utc::now())
            .await
            .expect_err("el fallo de lectura llega, no se disfraza");

        assert!(
            matches!(error, hodei_simulation::SimulationError::ReadFailed(ref reason)
                if reason.contains("the evidence store is down")),
            "el error es tipado y dice el por que: {error}"
        );
        Ok(())
    }
    .await;

    record("M7-UAT-07", result);
}
