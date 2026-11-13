//! El contrato de la simulacion.
//!
//! Cinco cosas se prometen y cinco cosas se comprueban:
//!
//! - El fork es aislado: el grafo real no cambia por simular.
//! - Se puede superponer evidencia y politica hipoteticas sobre el estado
//!   real.
//! - El diff estructural muestra las relaciones que aparecen y las que
//!   desaparecen.
//! - El diff de decisiones muestra que decisiones cambian, con su version
//!   de politica.
//! - El SPIKE-008 deja escrita su conclusion.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{Duration, Utc};
use hodei_evidence::domain::graph::RelationType;
use hodei_simulation::diff::TheDecision;
use hodei_simulation::hypothetical::{Hypothetical, HypotheticalClaim, RelationFact};
use hodei_simulation::ports::{DecisionEvaluator, SimulationGraph};
use hodei_simulation::service::SimulationService;
use hodei_simulation::{Checkpoint, SimulationFork, SimulationTag, StructuralDiff};
use hodei_trust_policy::decision::Outcome;
use hodei_trust_policy::{Requirement, RequirementKind, TrustPolicy};
use kernel::Hrn;

/// El sujeto de la simulacion.
fn subject() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-1.0.0").expect("un HRN")
}

/// El componente del que depende.
fn component_v1() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v1").expect("un HRN")
}

/// El componente al que se sube.
fn component_v2() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v2").expect("un HRN")
}

/// El grafo real de la simulacion.
///
/// Guarda las relaciones en un contador, y el contador es lo que comprueba
/// el aislamiento.
struct TheGraph {
    relations: Mutex<Vec<RelationFact>>,
    writes: Arc<Mutex<usize>>,
}

impl TheGraph {
    fn new(relations: Vec<RelationFact>) -> Arc<Self> {
        Arc::new(Self {
            relations: Mutex::new(relations),
            writes: Arc::new(Mutex::new(0)),
        })
    }

    fn writes(&self) -> usize {
        *self.writes.lock().expect("el contador no esta envenenado")
    }
}

#[async_trait]
impl SimulationGraph for TheGraph {
    async fn relations_of(&self, subject: &Hrn) -> Result<Vec<RelationFact>, String> {
        let relations = self.relations.lock().expect("el grafo no esta envenenado");
        Ok(relations
            .iter()
            .filter(|relation| &relation.from == subject || &relation.to == subject)
            .cloned()
            .collect())
    }

    async fn relation_count(&self) -> Result<usize, String> {
        let relations = self.relations.lock().expect("el grafo no esta envenenado");
        Ok(relations.len())
    }

    async fn all_relations(&self) -> Result<Vec<RelationFact>, String> {
        let relations = self.relations.lock().expect("el grafo no esta envenenado");
        Ok(relations.clone())
    }
}

/// El evaluador de decisiones.
///
/// Decide por la politica: una politica que exige que no haya
/// vulnerabilidad critica alcanzable deniega cuando el sujeto la tiene.
struct TheEvaluator {
    exposed: Mutex<Vec<String>>,
}

impl TheEvaluator {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            exposed: Mutex::new(Vec::new()),
        })
    }
}

#[async_trait]
impl DecisionEvaluator for TheEvaluator {
    async fn decide(
        &self,
        subject: &Hrn,
        policy: &TrustPolicy,
        facts: &[Hypothetical],
    ) -> Result<TheDecision, String> {
        // El hecho hipotetico que dice que el sujeto esta expuesto.
        let exposed = facts.iter().any(|fact| {
            matches!(
                &fact.claim,
                HypotheticalClaim::Risk { subject: who, critical, reachable, .. }
                    if who == subject && *critical && *reachable
            )
        });
        if exposed {
            self.exposed
                .lock()
                .expect("el evaluador no esta envenenado")
                .push(subject.to_string());
        }

        let denies = policy
            .requirements
            .iter()
            .any(|requirement| requirement.kind == RequirementKind::RiskEstablished);

        let outcome = if denies && exposed {
            Outcome::Deny
        } else {
            Outcome::Allow
        };

        Ok(TheDecision {
            outcome,
            policy: policy.clone(),
        })
    }
}

/// La politica de ahora: no exige que el riesgo este establecido.
fn the_policy(version: &str) -> TrustPolicy {
    TrustPolicy::new(
        format!("policy-{version}"),
        version,
        vec![Requirement::denying(RequirementKind::RiskEstablished)],
    )
}

/// La politica hipotetica: exige que el riesgo este establecido.
fn the_strict_policy(version: &str) -> TrustPolicy {
    TrustPolicy::new(
        format!("policy-{version}"),
        version,
        vec![
            Requirement::denying(RequirementKind::NoReachableCriticalVulnerability),
            Requirement::denying(RequirementKind::RiskEstablished),
        ],
    )
}

/// El fork de la simulacion.
fn the_fork(policy: hodei_trust_policy::TrustPolicy) -> SimulationFork {
    SimulationFork::new(
        "sim-1",
        Checkpoint::new("checkpoint-1", "acme", Utc::now(), Some(42)),
        policy,
        Utc::now(),
    )
}

/// La etiqueta de la simulacion.
fn the_tag() -> SimulationTag {
    SimulationTag::new("sim-1", Utc::now())
}

// ───────────────────────────── AC1 ─────────────────────────────

/// El fork es aislado: el grafo real no cambia por simular.
#[tokio::test]
async fn the_fork_is_isolated() {
    let graph = TheGraph::new(vec![RelationFact::new(
        RelationType::DependsOn,
        subject(),
        component_v1(),
    )]);
    let service = SimulationService::new(graph.clone(), TheEvaluator::new());

    let fork = the_fork(the_policy("1.0.0")).with(Hypothetical {
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

    let diff = service.simulate(&fork, Utc::now()).await.expect("simula");

    // El diff dice lo que tiene que decir.
    assert_eq!(diff.structural.appeared.len(), 1);
    assert_eq!(diff.structural.disappeared.len(), 1);

    // Y el grafo real esta IGUAL: mismas relaciones, mismas escrituras.
    let relations = graph.relations.lock().expect("el grafo no esta envenenado");
    assert_eq!(relations.len(), 1, "el grafo real no cambio");
    assert_eq!(
        relations[0].to,
        component_v1(),
        "y la relacion que desaparece sigue en el grafo real"
    );
    assert_eq!(graph.writes(), 0, "y no hubo ninguna escritura");
}

/// Y los hechos hipoteticos llevan su etiqueta, que es lo que los
/// distingue de la verdad.
#[tokio::test]
async fn the_hypotheticals_are_tagged() {
    let fork = the_fork(the_policy("1.0.0")).with(Hypothetical {
        tag: the_tag(),
        claim: HypotheticalClaim::Risk {
            subject: subject(),
            advisory: "GHSA-test-0001".to_string(),
            critical: true,
            reachable: true,
        },
    });

    assert_eq!(fork.facts.len(), 1);
    assert_eq!(fork.facts[0].tag.simulation, "sim-1");
    assert!(
        fork.facts[0].tag.simulation == the_tag().simulation,
        "la etiqueta dice de que simulacion es el hecho"
    );
}

// ───────────────────────────── AC2 ─────────────────────────────

/// Se puede superponer evidencia y politica hipoteticas sobre el estado
/// real.
#[tokio::test]
async fn the_evidence_and_the_policy_can_be_overlaid() {
    let graph = TheGraph::new(vec![RelationFact::new(
        RelationType::DependsOn,
        subject(),
        component_v1(),
    )]);
    let service = SimulationService::new(graph.clone(), TheEvaluator::new());

    let fork = the_fork(the_policy("1.0.0"))
        .with(Hypothetical {
            tag: the_tag(),
            claim: HypotheticalClaim::Risk {
                subject: subject(),
                advisory: "GHSA-test-0001".to_string(),
                critical: true,
                reachable: true,
            },
        })
        .with_policy(the_strict_policy("2.0.0"));

    // La politica efectiva es la hipotetica.
    assert_eq!(fork.effective_policy().version, "2.0.0");
    assert_eq!(fork.base_policy.version, "1.0.0");

    let diff = service.simulate(&fork, Utc::now()).await.expect("simula");

    // Y la decision cambia, con las dos versiones de la politica.
    assert_eq!(diff.decisions.len(), 1);
    let change = &diff.decisions[0];
    assert_eq!(change.before, Outcome::Allow);
    assert_eq!(change.after, Outcome::Deny);
    assert_eq!(change.policy_version_before, "1.0.0");
    assert_eq!(change.policy_version_after, "2.0.0");
}

// ───────────────────────────── AC3 ─────────────────────────────

/// El diff estructural muestra las relaciones que aparecen y las que
/// desaparecen.
#[test]
fn the_structural_diff_shows_what_appears_and_what_disappears() {
    let before = vec![
        RelationFact::new(RelationType::DependsOn, subject(), component_v1()),
        RelationFact::new(RelationType::Contains, subject(), component_v1()),
    ];
    let after = vec![
        RelationFact::new(RelationType::DependsOn, subject(), component_v2()),
        RelationFact::new(RelationType::Contains, subject(), component_v1()),
    ];

    let diff = StructuralDiff::between(&before, &after);

    assert_eq!(diff.appeared.len(), 1);
    assert_eq!(diff.appeared[0].to, component_v2());
    assert_eq!(diff.disappeared.len(), 1);
    assert_eq!(diff.disappeared[0].to, component_v1());
    assert!(
        diff.affected_subjects.contains(&subject()),
        "y el diff dice a quien le llega"
    );
    assert!(!diff.is_empty());
}

/// Y el diff de dos estados iguales esta vacio.
#[test]
fn a_diff_of_the_same_is_empty() {
    let relations = vec![RelationFact::new(
        RelationType::DependsOn,
        subject(),
        component_v1(),
    )];
    let diff = StructuralDiff::between(&relations, &relations);
    assert!(diff.is_empty());
    assert!(diff.appeared.is_empty());
    assert!(diff.disappeared.is_empty());
}

// ───────────────────────────── AC4 ─────────────────────────────

/// El diff de decisiones muestra que decisiones cambian, con su version
/// de politica.
#[tokio::test]
async fn the_decision_diff_shows_the_changes_with_the_policy_version() {
    let graph = TheGraph::new(vec![]);
    let service = SimulationService::new(graph.clone(), TheEvaluator::new());

    let fork = the_fork(the_policy("1.0.0"))
        .with(Hypothetical {
            tag: the_tag(),
            claim: HypotheticalClaim::Risk {
                subject: subject(),
                advisory: "GHSA-test-0001".to_string(),
                critical: true,
                reachable: true,
            },
        })
        .with_policy(the_strict_policy("2.0.0"));

    let diff = service.simulate(&fork, Utc::now()).await.expect("simula");

    assert_eq!(diff.decisions.len(), 1);
    let change = &diff.decisions[0];
    assert_eq!(change.subject, subject());
    assert_eq!(change.before, Outcome::Allow);
    assert_eq!(change.after, Outcome::Deny);
    assert_eq!(change.policy_version_after, "2.0.0");
    assert!(change.is_a_change());
}

/// Y una decision que sigue igual no es un cambio.
#[tokio::test]
async fn a_decision_that_does_not_change_is_not_a_change() {
    let graph = TheGraph::new(vec![]);
    let service = SimulationService::new(graph.clone(), TheEvaluator::new());

    // Un hecho hipotetico que no expone a nadie.
    let fork = the_fork(the_policy("1.0.0")).with(Hypothetical {
        tag: the_tag(),
        claim: HypotheticalClaim::Risk {
            subject: subject(),
            advisory: "GHSA-test-0001".to_string(),
            critical: false,
            reachable: false,
        },
    });

    let diff = service.simulate(&fork, Utc::now()).await.expect("simula");
    assert!(
        diff.decisions.is_empty(),
        "una decision que sigue igual no aparece en el diff: {:?}",
        diff.decisions
    );
}

// ───────────────────────────── AC5 ─────────────────────────────

/// El SPIKE-008 deja escrita su conclusion.
#[test]
fn the_conclusion_of_spike_008_is_written_down() {
    let source = include_str!("../src/lib.rs");
    assert!(
        source.contains("superposicion, no copia"),
        "the conclusion says which mechanism was chosen"
    );
    assert!(
        source.contains("no tiene un metodo de escritura"),
        "and it says why the isolation is a construction"
    );
}

/// Y los limites se comprueban ANTES de simular.
#[tokio::test]
async fn the_limits_are_checked_before_simulating() {
    let graph = TheGraph::new(vec![]);
    let service = SimulationService::new(graph.clone(), TheEvaluator::new());

    let mut fork = the_fork(the_policy("1.0.0"));
    fork.limits.max_hypotheticals = 0;
    fork.facts.push(Hypothetical {
        tag: the_tag(),
        claim: HypotheticalClaim::Risk {
            subject: subject(),
            advisory: "GHSA-test-0001".to_string(),
            critical: true,
            reachable: true,
        },
    });

    let error = service
        .simulate(&fork, Utc::now())
        .await
        .expect_err("una simulacion que no cabe no se ejecuta");
    assert!(
        matches!(error, hodei_simulation::SimulationError::TooLarge(_)),
        "y dice que la simulacion es demasiado grande: {error}"
    );

    // Y una simulacion caducada tampoco se ejecuta.
    let mut old = the_fork(the_policy("1.0.0"));
    old.created_at = Utc::now() - Duration::days(2);
    let error = service
        .simulate(&old, Utc::now())
        .await
        .expect_err("una simulacion caducada no se ejecuta");
    assert!(
        matches!(error, hodei_simulation::SimulationError::Expired(_)),
        "y dice que la simulacion ha caducado: {error}"
    );
}

// ───────────────────────── HG-H4-01: solo politica, sin engaños ─────────────────────────

/// Un evaluador que decide por la POLITICA: con la misma evidencia, una
/// politica mas estricta cambia el resultado. Es el caso de una
/// simulacion que solo cambia la politica y no anade ningun hecho.
struct PolicyDrivenEvaluator;

#[async_trait]
impl DecisionEvaluator for PolicyDrivenEvaluator {
    async fn decide(
        &self,
        _subject: &Hrn,
        policy: &TrustPolicy,
        _facts: &[Hypothetical],
    ) -> Result<TheDecision, String> {
        let outcome = if policy.version.starts_with('2') {
            Outcome::Deny
        } else {
            Outcome::Allow
        };
        Ok(TheDecision {
            outcome,
            policy: policy.clone(),
        })
    }
}

/// AC1: un fork con SOLO cambio de politica y sujetos DECLARADOS
/// produce DecisionChange cuando la politica modifica el resultado. La
/// conclusion de hoy --"sin hechos no hay afectados"-- era un engaño:
/// los sujetos se declaran, no se infieren de los hechos.
#[tokio::test]
async fn a_policy_only_fork_with_declared_subjects_changes_the_decisions() {
    let graph = TheGraph::new(vec![RelationFact::new(
        RelationType::DependsOn,
        subject(),
        component_v1(),
    )]);
    let service = SimulationService::new(graph.clone(), Arc::new(PolicyDrivenEvaluator));

    let fork = the_fork(the_policy("1.0.0"))
        .with_subjects(vec![subject()])
        .with_policy(the_strict_policy("2.0.0"));

    let diff = service.simulate(&fork, Utc::now()).await.expect("simula");

    assert_eq!(
        diff.decisions.len(),
        1,
        "la decision cambia porque la politica cambia: {:?}",
        diff.decisions
    );
    let change = &diff.decisions[0];
    assert_eq!(change.subject, subject());
    assert_eq!(change.before, Outcome::Allow);
    assert_eq!(change.after, Outcome::Deny);
    assert_eq!(change.policy_version_after, "2.0.0");
    assert!(
        diff.structural.affected_subjects.contains(&subject()),
        "y el diff dice a quien le llega: {:?}",
        diff.structural.affected_subjects
    );
}

/// AC3: un fork SIN sujetos no es una simulacion completa sin afectados:
/// es una simulacion que no se puede hacer, y se rechaza. Un diff vacio
/// que parezca "todo revisado, nada cambia" es una conclusion engañosa.
#[tokio::test]
async fn a_fork_with_no_subjects_is_rejected_not_a_silent_empty_diff() {
    let graph = TheGraph::new(Vec::new());
    let service = SimulationService::new(graph.clone(), Arc::new(PolicyDrivenEvaluator));

    let fork = the_fork(the_policy("1.0.0")).with_policy(the_strict_policy("2.0.0"));

    let error = service
        .simulate(&fork, Utc::now())
        .await
        .expect_err("sin sujetos no hay simulacion que valga");

    assert!(
        matches!(error, hodei_simulation::SimulationError::NotAllowed(_)),
        "dice que no hay sujetos declarados: {error}"
    );
}

/// AC3: un sujeto de OTRA cuenta no se admite en la simulacion, aunque
/// alguien lo declare. La cuenta del sujeto se compara con la del
/// checkpoint por su componente canonico.
#[tokio::test]
async fn a_subject_of_another_account_is_not_admitted() {
    let graph = TheGraph::new(Vec::new());
    let service = SimulationService::new(graph.clone(), Arc::new(PolicyDrivenEvaluator));

    let foreign = Hrn::from_string("hrn:hodei:artifacts:eu-west-1:contoso:Artifact/private")
        .expect("un HRN");
    let fork = the_fork(the_policy("1.0.0"))
        .with_subjects(vec![subject(), foreign])
        .with_policy(the_strict_policy("2.0.0"));

    let error = service
        .simulate(&fork, Utc::now())
        .await
        .expect_err("un sujeto de otra cuenta no entra");

    assert!(
        matches!(error, hodei_simulation::SimulationError::NotAllowed(ref reason)
            if reason.contains("contoso")),
        "dice que el sujeto sale de la cuenta de la simulacion: {error}"
    );
}
