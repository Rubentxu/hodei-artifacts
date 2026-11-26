//! The simulation contract.
//!
//! Five things are promised and five things are checked:
//!
//! - The fork is isolated: the real graph does not change by simulating.
//! - Hypothetical evidence and policy can be superimposed on the real
//!   state.
//! - The structural diff shows the relations that appear and those that
//!   disappear.
//! - The decision diff shows which decisions change, with their policy
//!   version.
//! - SPIKE-008 leaves its conclusion written.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{Duration, Utc};
use hodei_evidence::domain::graph::RelationType;
use hodei_evidence::domain::read_set::ReadSet;
use hodei_simulation::diff::TheDecision;
use hodei_simulation::hypothetical::{Hypothetical, HypotheticalClaim, RelationFact};
use hodei_simulation::ports::{DecisionEvaluator, SimulationGraph};
use hodei_simulation::service::SimulationService;
use hodei_simulation::{Checkpoint, SimulationFork, SimulationTag, StructuralDiff};
use hodei_trust_policy::decision::Outcome;
use hodei_trust_policy::{Requirement, RequirementKind, TrustPolicy};
use kernel::Hrn;

/// The subject of the simulation.
fn subject() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-1.0.0").expect("un HRN")
}

/// The component it depends on.
fn component_v1() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v1").expect("un HRN")
}

/// The component upgraded to.
fn component_v2() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v2").expect("un HRN")
}

/// The real graph of the simulation.
///
/// It keeps the relations and their generation, and no more: the change of the world
/// while simulating is tested in the base contract, which is where
/// that case lives.
struct TheGraph {
    relations: Mutex<Vec<RelationFact>>,
    /// The generation of the graph.
    generation: Mutex<u64>,
}

impl TheGraph {
    fn new(relations: Vec<RelationFact>) -> Arc<Self> {
        Arc::new(Self {
            relations: Mutex::new(relations),
            generation: Mutex::new(1),
        })
    }

    /// The relations it has right now.
    fn the_relations(&self) -> Vec<RelationFact> {
        self.relations
            .lock()
            .expect("el grafo no esta envenenado")
            .clone()
    }

    /// The generation it has right now.
    fn the_generation(&self) -> u64 {
        *self
            .generation
            .lock()
            .expect("la generacion no esta envenenada")
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

    async fn generation(&self) -> Result<u64, String> {
        Ok(self.the_generation())
    }

    fn account(&self) -> String {
        "acme".to_string()
    }
}

/// The decision evaluator.
///
/// It decides by policy: a policy that requires no reachable
/// critical vulnerability denies when the subject has one.
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
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<TheDecision, String> {
        // The hypothetical fact that says the subject is exposed.
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
            read_set: ReadSet::empty(),
        })
    }

    async fn the_reads_of(&self, _subject: &Hrn) -> Result<ReadSet, String> {
        Ok(ReadSet::empty())
    }
}

/// Today's policy: it does not require the risk to be established.
fn the_policy(version: &str) -> TrustPolicy {
    TrustPolicy::new(
        format!("policy-{version}"),
        version,
        vec![Requirement::denying(RequirementKind::RiskEstablished)],
    )
}

/// The hypothetical policy: it requires the risk to be established.
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

/// The simulation fork.
fn the_fork(policy: hodei_trust_policy::TrustPolicy) -> SimulationFork {
    SimulationFork::new(
        "sim-1",
        Checkpoint::new("checkpoint-1", "acme", Utc::now(), Some(42), 1),
        policy,
        Utc::now(),
    )
}

/// The simulation label.
fn the_tag() -> SimulationTag {
    SimulationTag::new("sim-1", Utc::now())
}

// ───────────────────────────── AC1 ─────────────────────────────

/// The fork is isolated: the real graph does not change by simulating.
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

    // The diff says what it has to say.
    assert_eq!(diff.structural.appeared.len(), 1);
    assert_eq!(diff.structural.disappeared.len(), 1);

    // And the real graph is the SAME: same relations, same generation.
    let relations = graph.the_relations();
    assert_eq!(relations.len(), 1, "el grafo real no cambio");
    assert_eq!(
        relations[0].to,
        component_v1(),
        "y la relacion que desaparece sigue en el grafo real"
    );
    assert_eq!(
        graph.the_generation(),
        1,
        "y la generacion del grafo no se movio"
    );
}

/// And the hypothetical facts carry their label, which is what
/// distinguishes them from the truth.
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

/// Hypothetical evidence and policy can be superimposed on the real
/// state.
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

    // The effective policy is the hypothetical one.
    assert_eq!(fork.effective_policy().version, "2.0.0");
    assert_eq!(fork.base_policy.version, "1.0.0");

    let diff = service.simulate(&fork, Utc::now()).await.expect("simula");

    // And the decision changes, with the two policy versions.
    assert_eq!(diff.decisions.len(), 1);
    let change = &diff.decisions[0];
    assert_eq!(change.before, Outcome::Allow);
    assert_eq!(change.after, Outcome::Deny);
    assert_eq!(change.policy_version_before, "1.0.0");
    assert_eq!(change.policy_version_after, "2.0.0");
}

// ───────────────────────────── AC3 ─────────────────────────────

/// The structural diff shows the relations that appear and those that
/// disappear.
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

/// And the diff of two equal states is empty.
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

/// The decision diff shows which decisions change, with their policy
/// version.
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

/// And a decision that stays the same is not a change.
#[tokio::test]
async fn a_decision_that_does_not_change_is_not_a_change() {
    let graph = TheGraph::new(vec![]);
    let service = SimulationService::new(graph.clone(), TheEvaluator::new());

    // A hypothetical fact that exposes nobody.
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

/// SPIKE-008 leaves its conclusion written.
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

/// And the limits are checked BEFORE simulating.
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

    // And an expired simulation is not run either.
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

// ───────────────────────── HG-H4-01: policy only, no deception ─────────────────────────

/// An evaluator that decides by POLICY: with the same evidence, a
/// stricter policy changes the result. It is the case of a
/// simulation that only changes the policy and adds no fact.
struct PolicyDrivenEvaluator;

#[async_trait]
impl DecisionEvaluator for PolicyDrivenEvaluator {
    async fn decide(
        &self,
        _subject: &Hrn,
        policy: &TrustPolicy,
        _facts: &[Hypothetical],
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<TheDecision, String> {
        let outcome = if policy.version.starts_with('2') {
            Outcome::Deny
        } else {
            Outcome::Allow
        };
        Ok(TheDecision {
            outcome,
            policy: policy.clone(),
            read_set: ReadSet::empty(),
        })
    }

    async fn the_reads_of(&self, _subject: &Hrn) -> Result<ReadSet, String> {
        Ok(ReadSet::empty())
    }
}

/// AC1: a fork with ONLY a policy change and DECLARED subjects
/// produces a DecisionChange when the policy modifies the result. Today's
/// conclusion --"without facts there are no affected"-- was a deception:
/// subjects are declared, not inferred from the facts.
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

/// AC3: a fork WITHOUT subjects is not a complete simulation without affected:
/// it is a simulation that cannot be done, and it is rejected. An empty diff
/// that looks like "everything reviewed, nothing changes" is a misleading conclusion.
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

/// AC3: a subject from ANOTHER account is not admitted in the simulation, even if
/// someone declares it. The subject's account is compared with the
/// checkpoint's by its canonical component.
#[tokio::test]
async fn a_subject_of_another_account_is_not_admitted() {
    let graph = TheGraph::new(Vec::new());
    let service = SimulationService::new(graph.clone(), Arc::new(PolicyDrivenEvaluator));

    let foreign =
        Hrn::from_string("hrn:hodei:artifacts:eu-west-1:contoso:Artifact/private").expect("un HRN");
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
