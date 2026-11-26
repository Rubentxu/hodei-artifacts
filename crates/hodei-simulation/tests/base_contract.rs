//! The contract of the simulation base.
//!
//! The simulation says over which world it was computed, and that world is pinned
//! beforehand and checked afterwards. What is checked:
//!
//! - A world that moves while being simulated makes the simulation
//!   declare itself STALE, rather than return a diff over a world that no longer
//!   exists.
//! - The concurrent change is detected by CONTENT, not by
//!   count: one relation replaced by another leaves the number the same.
//! - The evidence the decision consumes is also checked: it does not touch
//!   the graph, so without its own check it would be invisible.
//! - The clock is the one received by whoever simulates, not the system's: the
//!   same base and the same facts give the same result.
//! - A base from another account, without a version or prepared against another
//!   generation is rejected.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use hodei_evidence::domain::graph::RelationType;
use hodei_evidence::domain::read_set::{ReadSet, ReadStamp};
use hodei_simulation::diff::TheDecision;
use hodei_simulation::hypothetical::{Hypothetical, HypotheticalClaim, RelationFact};
use hodei_simulation::ports::{DecisionEvaluator, SimulationGraph};
use hodei_simulation::service::SimulationService;
use hodei_simulation::{Checkpoint, SimulationFork, SimulationTag};
use hodei_trust_policy::decision::Outcome;
use hodei_trust_policy::{Requirement, RequirementKind, TrustPolicy};
use kernel::Hrn;

/// The subject of the simulation.
fn subject() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-1.0.0").expect("un HRN")
}

/// The other subject of the simulation.
fn other_subject() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/api-2.0.0").expect("un HRN")
}

/// The component it depends on.
fn component_v1() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v1").expect("un HRN")
}

/// The component upgraded to in the concurrent change.
fn component_v2() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v2").expect("un HRN")
}

/// The generation of the fake graph.
const THE_GENERATION: u64 = 7;

/// The real graph, with the ability to move while being simulated.
struct TheGraph {
    relations: Mutex<Vec<RelationFact>>,
    reads: Mutex<usize>,
    movement: Mutex<Option<Movement>>,
    generation: Mutex<u64>,
}

/// A change of the world, scheduled to happen on a read.
struct Movement {
    at_read: usize,
    relations: Vec<RelationFact>,
    moves_the_generation: bool,
}

impl TheGraph {
    fn new(relations: Vec<RelationFact>) -> Arc<Self> {
        Arc::new(Self {
            relations: Mutex::new(relations),
            reads: Mutex::new(0),
            movement: Mutex::new(None),
            generation: Mutex::new(THE_GENERATION),
        })
    }

    /// The graph moves on read number `at_read`.
    fn moving_at(&self, at_read: usize, relations: Vec<RelationFact>, moves_the_generation: bool) {
        *self
            .movement
            .lock()
            .expect("el movimiento no esta envenenado") = Some(Movement {
            at_read,
            relations,
            moves_the_generation,
        });
    }

    fn a_read_happened(&self) {
        let mut reads = self.reads.lock().expect("el contador no esta envenenado");
        *reads += 1;
        let read = *reads;

        let movement = self
            .movement
            .lock()
            .expect("el movimiento no esta envenenado");
        let Some(movement) = movement.as_ref() else {
            return;
        };
        if movement.at_read != read {
            return;
        }
        *self.relations.lock().expect("el grafo no esta envenenado") = movement.relations.clone();
        if movement.moves_the_generation {
            let mut generation = self
                .generation
                .lock()
                .expect("la generacion no esta envenenada");
            *generation += 1;
        }
    }

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
        self.a_read_happened();
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

/// The evaluator that RECORDS the instant it receives.
///
/// It is the way to check that the decision core does not read the wall
/// clock: it receives the instant and decides with it.
struct TheEvaluator {
    seen: Mutex<Vec<DateTime<Utc>>>,
    /// What the evaluator has read: what it says it consumes.
    reads: Mutex<ReadSet>,
    /// The evidence that enters while simulating.
    ///
    /// It does not touch the graph: it changes neither the number of relations nor the
    /// generation. Only those who read the evidence see it.
    arrived: Mutex<Option<String>>,
}

impl TheEvaluator {
    fn new() -> Arc<Self> {
        let mut reads = ReadSet::empty();
        reads.read(ReadStamp::evidence("evd-1", "sha256:aaa"));
        Arc::new(Self {
            seen: Mutex::new(Vec::new()),
            reads: Mutex::new(reads),
            arrived: Mutex::new(None),
        })
    }

    /// The evidence enters AFTER the decision consumed it.
    fn the_evidence_arrives(&self, id: &str) {
        *self
            .arrived
            .lock()
            .expect("el evaluador no esta envenenado") = Some(id.to_string());
    }

    fn the_instants(&self) -> Vec<DateTime<Utc>> {
        self.seen
            .lock()
            .expect("el evaluador no esta envenenado")
            .clone()
    }

    /// What the evaluator reads now: what was already there, plus what has
    /// come in.
    fn the_reads_now(&self) -> ReadSet {
        let mut reads = self
            .reads
            .lock()
            .expect("el evaluador no esta envenenado")
            .clone();
        if let Some(arrived) = self
            .arrived
            .lock()
            .expect("el evaluador no esta envenenado")
            .as_ref()
        {
            reads.read(ReadStamp::evidence(arrived.clone(), "sha256:zzz"));
        }
        reads
    }
}

#[async_trait]
impl DecisionEvaluator for TheEvaluator {
    async fn decide(
        &self,
        _subject: &Hrn,
        policy: &TrustPolicy,
        _facts: &[Hypothetical],
        now: DateTime<Utc>,
    ) -> Result<TheDecision, String> {
        self.seen
            .lock()
            .expect("el evaluador no esta envenenado")
            .push(now);
        Ok(TheDecision {
            outcome: Outcome::Allow,
            policy: policy.clone(),
            read_set: self
                .reads
                .lock()
                .expect("el evaluador no esta envenenado")
                .clone(),
        })
    }

    async fn the_reads_of(&self, _subject: &Hrn) -> Result<ReadSet, String> {
        Ok(self.the_reads_now())
    }
}

/// Today's policy.
fn the_policy(version: &str) -> TrustPolicy {
    TrustPolicy::new(
        format!("policy-{version}"),
        version,
        vec![Requirement::denying(RequirementKind::RiskEstablished)],
    )
}

/// The simulation fork.
fn the_fork() -> SimulationFork {
    SimulationFork::new(
        "sim-1",
        Checkpoint::new("checkpoint-1", "acme", Utc::now(), Some(42), THE_GENERATION),
        the_policy("1.0.0"),
        Utc::now(),
    )
    .with_subjects(vec![subject()])
}

/// The simulation instant: the clock is set by whoever simulates.
fn the_moment() -> DateTime<Utc> {
    DateTime::from_timestamp(1_700_000_000, 0).expect("un instante")
}

// ───────────────────────────── AC1 ─────────────────────────────

/// AC1: a concurrent change that REPLACES one relation with another
/// keeping the SAME count is detected as a stale base.
///
/// The number of relations does not tell them apart: it is still one. And the
/// generation does not either, if the change does not move it. What does tell them apart is
/// the content read, which is what the simulation stamped.
#[tokio::test]
async fn a_concurrent_swap_with_the_same_count_is_a_stale_base() {
    let graph = TheGraph::new(vec![RelationFact::new(
        RelationType::DependsOn,
        subject(),
        component_v1(),
    )]);
    // The change happens on the second read, which is the check one:
    // one relation for another, and the number of relations is still one.
    graph.moving_at(
        2,
        vec![RelationFact::new(
            RelationType::DependsOn,
            subject(),
            component_v2(),
        )],
        false,
    );

    let service = SimulationService::new(graph.clone(), TheEvaluator::new());
    let error = service
        .simulate(&the_fork(), the_moment())
        .await
        .expect_err("un mundo que se mueve hace obsoleta la simulacion");

    match error {
        hodei_simulation::SimulationError::StaleBase { source, .. } => {
            assert_eq!(
                source, "relations",
                "y dice que lo que se movio son las relaciones leidas"
            );
        }
        other => panic!("y dice que la base esta obsoleta, no otra cosa: {other}"),
    }
}

/// And a change that moves the graph's GENERATION also makes the
/// simulation stale, even if the content read stays the same.
///
/// The world moves on the FIRST relations read, which is the one
/// that comes AFTER reading the generation and the count: the change falls
/// between pinning the base and checking the base, which is exactly the window the
/// final check exists to close.
#[tokio::test]
async fn a_concurrent_change_that_moves_the_generation_is_a_stale_base() {
    let graph = TheGraph::new(vec![RelationFact::new(
        RelationType::DependsOn,
        subject(),
        component_v1(),
    )]);
    graph.moving_at(
        1,
        vec![RelationFact::new(
            RelationType::DependsOn,
            subject(),
            component_v1(),
        )],
        true,
    );

    let service = SimulationService::new(graph.clone(), TheEvaluator::new());
    let error = service
        .simulate(&the_fork(), the_moment())
        .await
        .expect_err("una generacion nueva hace obsoleta la simulacion");

    match error {
        hodei_simulation::SimulationError::StaleBase { source, .. } => {
            assert_eq!(source, "graph", "y dice que lo que se movio es el grafo");
        }
        other => panic!("y dice que la base esta obsoleta, no otra cosa: {other}"),
    }
}

/// And a simulation whose world does NOT move completes and says over which
/// world it was computed.
#[tokio::test]
async fn a_simulation_over_a_still_world_says_which_world_it_was_over() {
    let graph = TheGraph::new(vec![RelationFact::new(
        RelationType::DependsOn,
        subject(),
        component_v1(),
    )]);
    let service = SimulationService::new(graph.clone(), TheEvaluator::new());

    let diff = service
        .simulate(&the_fork(), the_moment())
        .await
        .expect("un mundo quieto se simula");

    assert_eq!(diff.base.account, "acme");
    assert_eq!(diff.base.evaluated_at, the_moment());
    assert!(
        diff.base
            .read_set
            .is_consistent_with(&[("graph".to_string(), THE_GENERATION)]),
        "y dice la generacion contra la que se evaluo"
    );
    assert!(
        diff.base
            .read_set
            .stamps
            .iter()
            .any(|stamp| stamp.resource.contains("depends_on")),
        "y sello la relacion que leyo"
    );
    assert!(!diff.base.seal.is_empty(), "y el sello del contenido leido");
}

/// AC1, in the variant the REAL STORE can produce: the same
/// relation with OTHER evidence.
///
/// Replacing one relation with another with different endpoints leaves the
/// number of rows increased in the real store (the old row is not
/// deleted), so that case is detected by the count. What the real store
/// does with the SAME count is change the support of a relation:
/// same identity, same row, other evidence. And that case is only seen by
/// the content stamp, because the generation does not move when the
/// change does not go through the deltas ledger.
#[tokio::test]
async fn a_relation_that_changes_its_evidence_is_a_stale_base() {
    let graph = TheGraph::new(vec![
        RelationFact::new(RelationType::DependsOn, subject(), component_v1())
            .with_evidence("evd-1"),
    ]);
    // The change happens on the second read: the same relation, with other
    // evidence. The number of relations is still one.
    graph.moving_at(
        2,
        vec![
            RelationFact::new(RelationType::DependsOn, subject(), component_v1())
                .with_evidence("evd-2"),
        ],
        false,
    );

    let service = SimulationService::new(graph.clone(), TheEvaluator::new());
    let error = service
        .simulate(&the_fork(), the_moment())
        .await
        .expect_err("una relacion con otro apoyo hace obsoleta la simulacion");

    match error {
        hodei_simulation::SimulationError::StaleBase { source, .. } => {
            assert_eq!(
                source, "relations",
                "y dice que lo que se movio es lo que se leyo"
            );
        }
        other => panic!("y dice que la base esta obsoleta, no otra cosa: {other}"),
    }
}

/// And the manifest stamps the policy that is IN FORCE in the fork, which
/// is the one that produces the "after" decision: a manifest that covers only part of
/// what produced the decision does not verify the decision.
#[tokio::test]
async fn the_base_seals_the_policy_that_produced_the_decision() {
    let graph = TheGraph::new(vec![]);
    let service = SimulationService::new(graph, TheEvaluator::new());

    let fork = the_fork().with_policy(hodei_trust_policy::TrustPolicy::new(
        "policy-2.0.0",
        "2.0.0",
        vec![],
    ));
    let diff = service
        .simulate(&fork, the_moment())
        .await
        .expect("la simulacion");

    let policies: Vec<String> = diff
        .base
        .read_set
        .stamps
        .iter()
        .filter(|stamp| stamp.kind == hodei_evidence::domain::read_set::ReadKind::Policy)
        .map(|stamp| stamp.resource.clone())
        .collect();

    assert!(
        policies.contains(&"policy-1.0.0".to_string()),
        "y sella la politica de antes: {policies:?}"
    );
    assert!(
        policies.contains(&"policy-2.0.0".to_string()),
        "y la hipotetica, que es la que decide el despues: {policies:?}"
    );
}

/// And when the hypothetical is the SAME policy with ANOTHER version, both
/// versions are in the manifest.
///
/// They are the same resource verified in two ways, and the manifest
/// kept only the first: the diff said "policy 2.0.0 produced the
/// after" while the manifest only spoke of 1.0.0. A
/// manifest that does not contain what produced the decision does not verify
/// the decision.
#[tokio::test]
async fn the_base_keeps_both_versions_of_the_same_policy() {
    let graph = TheGraph::new(vec![]);
    let service = SimulationService::new(graph, TheEvaluator::new());

    let fork = the_fork().with_policy(hodei_trust_policy::TrustPolicy::new(
        "policy-1.0.0",
        "2.0.0",
        vec![],
    ));
    let diff = service
        .simulate(&fork, the_moment())
        .await
        .expect("la simulacion");

    let versions: Vec<String> = diff
        .base
        .read_set
        .stamps
        .iter()
        .filter(|stamp| stamp.kind == hodei_evidence::domain::read_set::ReadKind::Policy)
        .map(|stamp| stamp.verified_by.clone())
        .collect();

    assert!(
        versions.contains(&"1.0.0".to_string()) && versions.contains(&"2.0.0".to_string()),
        "las dos versiones de la politica estan en el manifiesto: {versions:?}"
    );
}

// ───────────────────────────── the evidence that decides ─────────────────────────────

/// The base keeps the evidence that the decision CONSUMED.
///
/// Without this the base would say over which world it was computed without saying which
/// evidence decided it, and evidence that enters in the middle of the
/// simulation would be attributed to the hypothetical fact.
#[tokio::test]
async fn the_base_keeps_the_evidence_that_the_decision_consumed() {
    let graph = TheGraph::new(vec![]);
    let service = SimulationService::new(graph, TheEvaluator::new());

    let diff = service
        .simulate(&the_fork(), the_moment())
        .await
        .expect("la simulacion");

    let evidence: Vec<String> = diff
        .base
        .read_set
        .stamps
        .iter()
        .filter(|stamp| stamp.kind == hodei_evidence::domain::read_set::ReadKind::Evidence)
        .map(|stamp| stamp.resource.clone())
        .collect();

    assert!(
        evidence.contains(&"evd-1".to_string()),
        "la evidencia que consumo la decision esta en la base: {evidence:?}"
    );
}

/// And evidence that ENTERS while simulating makes the base stale.
///
/// It does not touch the graph: it changes neither the number of relations nor the
/// generation. The graph's three guards do not see it, and without their own
/// check the change of decision would be attributed to the
/// hypothetical fact.
#[tokio::test]
async fn an_evidence_that_arrives_during_the_simulation_is_a_stale_base() {
    let graph = TheGraph::new(vec![]);
    let evaluator = TheEvaluator::new();
    // The evidence enters AFTER the decision consumed it.
    evaluator.the_evidence_arrives("evd-nueva");
    let service = SimulationService::new(graph, evaluator);

    let error = service
        .simulate(&the_fork(), the_moment())
        .await
        .expect_err("una evidencia que entra hace obsoleta la simulacion");

    match error {
        hodei_simulation::SimulationError::StaleBase { source, .. } => {
            assert_eq!(
                source, "evidence",
                "y dice que lo que se movio es la evidencia"
            );
        }
        other => panic!("y dice que la base esta obsoleta, no otra cosa: {other}"),
    }
}

// ───────────────────────────── AC2 ─────────────────────────────

/// AC2: with the same base, the same clock and the same facts, two
/// evaluations give the same result and the same diff.
#[tokio::test]
async fn the_same_base_clock_and_facts_give_the_same_diff() {
    let graph = TheGraph::new(vec![RelationFact::new(
        RelationType::DependsOn,
        subject(),
        component_v1(),
    )]);

    let first = SimulationService::new(graph.clone(), TheEvaluator::new())
        .simulate(&the_fork(), the_moment())
        .await
        .expect("la primera simulacion");
    let second = SimulationService::new(graph.clone(), TheEvaluator::new())
        .simulate(&the_fork(), the_moment())
        .await
        .expect("la segunda simulacion");

    assert_eq!(first.structural, second.structural);
    assert_eq!(first.decisions, second.decisions);
    assert_eq!(first.base, second.base);
    assert_eq!(first.base.fingerprint(), second.base.fingerprint());
}

/// And the diff does not depend on the ORDER in which the subjects are declared.
///
/// The declaration order of a fork is the order in which it occurred to someone to
/// declare them, and a diff that changes with that order cannot be
/// compared with another diff.
#[tokio::test]
async fn the_diff_does_not_depend_on_the_order_of_the_declared_subjects() {
    let graph = TheGraph::new(vec![]);

    let one = SimulationService::new(graph.clone(), TheEvaluator::new())
        .simulate(
            &the_fork().with_subjects(vec![subject(), other_subject()]),
            the_moment(),
        )
        .await
        .expect("la primera simulacion");
    let other = SimulationService::new(graph.clone(), TheEvaluator::new())
        .simulate(
            &the_fork().with_subjects(vec![other_subject(), subject()]),
            the_moment(),
        )
        .await
        .expect("la segunda simulacion");

    assert_eq!(one.decisions, other.decisions);
    assert_eq!(one.structural, other.structural);
}

/// And the content stamp does not depend on the ORDER in which the graph gives its
/// relations.
///
/// The graph may give the same relation twice with different support, and
/// the stamp cannot depend on which of the two is kept: a stamp that
/// depends on the store's order is not a stamp of the world.
#[tokio::test]
async fn the_seal_does_not_depend_on_the_order_of_the_store() {
    let one = RelationFact::new(RelationType::DependsOn, subject(), component_v1())
        .with_evidence("evd-b");
    let other = RelationFact::new(RelationType::DependsOn, subject(), component_v1())
        .with_evidence("evd-a");

    let first = TheGraph::new(vec![one.clone(), other.clone()]);
    let second = TheGraph::new(vec![other, one]);

    let one = SimulationService::new(first, TheEvaluator::new())
        .simulate(&the_fork(), the_moment())
        .await
        .expect("la primera simulacion");
    let other = SimulationService::new(second, TheEvaluator::new())
        .simulate(&the_fork(), the_moment())
        .await
        .expect("la segunda simulacion");

    assert_eq!(one.base.seal, other.base.seal);
}

// ───────────────────────────── AC3 ─────────────────────────────

/// AC3: the decision core receives the simulation instant, and not
/// the wall clock's.
#[tokio::test]
async fn the_decision_core_uses_the_clock_of_the_simulation() {
    let graph = TheGraph::new(vec![]);
    let evaluator = TheEvaluator::new();
    let service = SimulationService::new(graph, evaluator.clone());

    let moment = the_moment();
    service
        .simulate(&the_fork(), moment)
        .await
        .expect("la simulacion");

    let instants = evaluator.the_instants();
    assert!(!instants.is_empty(), "el evaluador decidio algo");
    assert!(
        instants.iter().all(|seen| *seen == moment),
        "y decidio con el instante de la simulacion, no con otro: {instants:?}"
    );
}

/// And two simulations with DIFFERENT clocks are not confused: the clock
/// is part of the base, and that is why it travels in the diff.
#[tokio::test]
async fn the_clock_travels_with_the_diff() {
    let graph = TheGraph::new(vec![]);
    let service = SimulationService::new(graph, TheEvaluator::new());

    let first = service
        .simulate(&the_fork(), the_moment())
        .await
        .expect("la primera simulacion");
    let later = service
        .simulate(&the_fork(), the_moment() + Duration::hours(1))
        .await
        .expect("la segunda simulacion");

    assert_eq!(first.base.evaluated_at, the_moment());
    assert_eq!(later.base.evaluated_at, the_moment() + Duration::hours(1));
    assert_ne!(first.base.evaluated_at, later.base.evaluated_at);
}

// ───────────────────────────── AC4 ─────────────────────────────

/// AC4: a fork prepared against ANOTHER generation is rejected: the world
/// is no longer the one the fork was prepared against.
#[tokio::test]
async fn a_fork_prepared_against_another_generation_is_rejected() {
    let graph = TheGraph::new(vec![]);
    let service = SimulationService::new(graph, TheEvaluator::new());

    let fork = the_fork();
    let fork = SimulationFork::new(
        fork.id.clone(),
        fork.base.clone().at_generation(THE_GENERATION - 1),
        fork.base_policy.clone(),
        fork.created_at,
    )
    .with_subjects(vec![subject()]);

    let error = service
        .simulate(&fork, the_moment())
        .await
        .expect_err("una base de otra generacion no se simula");

    match error {
        hodei_simulation::SimulationError::StaleBase {
            source,
            expected,
            found,
        } => {
            assert_eq!(source, "graph");
            assert_eq!(expected, "6");
            assert_eq!(found, "7");
        }
        other => panic!("y dice que la base esta obsoleta: {other}"),
    }
}

/// And a fork prepared against the generation of NOW is simulated.
#[tokio::test]
async fn a_fork_prepared_against_the_current_generation_is_simulated() {
    let graph = TheGraph::new(vec![]);
    let service = SimulationService::new(graph, TheEvaluator::new());

    service
        .simulate(&the_fork(), the_moment())
        .await
        .expect("la base vigente se simula");
}

/// And a fork WITHOUT a declared version is rejected.
///
/// A base that does not say against which version of the world it was evaluated cannot
/// be checked, and a base that cannot be checked does not support
/// an answer.
#[tokio::test]
async fn a_fork_without_a_declared_version_is_rejected() {
    let graph = TheGraph::new(vec![]);
    let service = SimulationService::new(graph, TheEvaluator::new());

    let fork = the_fork();
    let mut checkpoint = fork.base.clone();
    checkpoint.generation = None;
    let fork = SimulationFork::new(
        fork.id.clone(),
        checkpoint,
        fork.base_policy.clone(),
        fork.created_at,
    )
    .with_subjects(vec![subject()]);

    let error = service
        .simulate(&fork, the_moment())
        .await
        .expect_err("una base sin version no se simula");

    assert!(
        matches!(error, hodei_simulation::SimulationError::NoVersion(_)),
        "y dice que la base no declara version: {error}"
    );
}

/// And a base from ANOTHER account is rejected: a base from another account is not
/// a base, it is an answer about someone else's world.
#[tokio::test]
async fn a_base_of_another_account_is_rejected() {
    let graph = TheGraph::new(vec![]);
    let service = SimulationService::new(graph, TheEvaluator::new());

    let fork = SimulationFork::new(
        "sim-1",
        Checkpoint::new(
            "checkpoint-1",
            "contoso",
            Utc::now(),
            Some(42),
            THE_GENERATION,
        ),
        the_policy("1.0.0"),
        Utc::now(),
    )
    .with_subjects(vec![subject()]);

    let error = service
        .simulate(&fork, the_moment())
        .await
        .expect_err("una base de otra cuenta no se simula");

    assert!(
        matches!(error, hodei_simulation::SimulationError::NotAllowed(ref reason)
            if reason.contains("contoso")),
        "y dice que la base es de otra cuenta: {error}"
    );
}

/// And the graph's generation is checked AFTER evaluating: a world
/// that moves between the read and the end of the evaluation is also
/// declared stale.
#[tokio::test]
async fn a_world_that_moves_after_the_reads_is_also_stale() {
    let graph = TheGraph::new(vec![RelationFact::new(
        RelationType::DependsOn,
        subject(),
        component_v1(),
    )]);
    // The change happens on the first relations read, which is the one
    // that comes after reading the generation and the count: one more
    // relation, and the graph is no longer the one that was evaluated.
    graph.moving_at(
        1,
        vec![
            RelationFact::new(RelationType::DependsOn, subject(), component_v1()),
            RelationFact::new(RelationType::Contains, subject(), component_v1()),
        ],
        false,
    );

    let service = SimulationService::new(graph.clone(), TheEvaluator::new());
    let error = service
        .simulate(&the_fork(), the_moment())
        .await
        .expect_err("el mundo se movio durante la evaluacion");

    match error {
        hodei_simulation::SimulationError::StaleBase { source, .. } => {
            assert_eq!(
                source, "relation count",
                "y dice que el grafo cambio de tamano"
            );
        }
        other => panic!("y dice que la base esta obsoleta: {other}"),
    }
}

/// The hypothetical fact carries its label: what is superimposed on the real
/// world does not belong to the real world.
#[test]
fn the_hypothetical_carries_its_tag() {
    let tag = SimulationTag::new("sim-1", the_moment());
    let fork = the_fork().with(Hypothetical {
        tag: tag.clone(),
        claim: HypotheticalClaim::Risk {
            subject: subject(),
            advisory: "GHSA-test-0001".to_string(),
            critical: true,
            reachable: true,
        },
    });

    assert_eq!(fork.facts[0].tag.simulation, "sim-1");
}
