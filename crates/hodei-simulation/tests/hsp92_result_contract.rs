//! The contract of a simulation result.
//!
//! The result is what is delivered: a **versioned** contract that
//! separates what was observed from what is hypothetical, says which policies
//! took part, says what has been covered and what has not, and declares the
//! scenario's conflicts instead of hiding them.
//!
//! And it does so without dragging policy objects along: what travels is the
//! seal (identifier and version), because a contract that carries the
//! whole policy stops being portable.

use chrono::{DateTime, Utc};
use hodei_evidence::domain::graph::RelationType;
use hodei_simulation::hypothetical::{Hypothetical, HypotheticalClaim, RelationFact};
use hodei_simulation::memory::{InMemoryDecisions, InMemoryGraph};
use hodei_simulation::result::{
    ConflictKind, HypothesisKind, SIMULATION_RESULT_VERSION, SimulationResult,
};
use hodei_simulation::service::SimulationService;
use hodei_simulation::{Checkpoint, SimulationFork, SimulationTag};
use hodei_trust_policy::{Requirement, RequirementKind, TrustPolicy};
use kernel::Hrn;

/// The instant of the tests: only one.
fn the_now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-01-01T12:00:00Z")
        .expect("un instante fijo")
        .with_timezone(&Utc)
}

/// The subject of the simulation.
fn subject() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-1.0.0").expect("un HRN")
}

/// The component it depends on.
fn component_v1() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v1").expect("un HRN")
}

/// The second component.
fn component_v2() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v2").expect("un HRN")
}

/// The component being upgraded to.
fn component_v3() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v3").expect("un HRN")
}

/// A subject that the simulation does not touch: its relation is not read.
fn other() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/other-1.0.0").expect("un HRN")
}

/// Another subject that the simulation does not touch.
fn another() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/another-1.0.0").expect("un HRN")
}

/// The current policy: the one that forbids nothing.
fn the_policy() -> TrustPolicy {
    TrustPolicy::new("production", "1.0.0", Vec::new())
}

/// The hypothetical policy: the one that forbids the established risk.
fn the_strict_policy() -> TrustPolicy {
    TrustPolicy::new(
        "production",
        "2.0.0",
        vec![Requirement::denying(RequirementKind::RiskEstablished)],
    )
}

/// The world graph: two dependencies of the subject and one that is not its own.
fn the_graph() -> std::sync::Arc<InMemoryGraph> {
    let graph = InMemoryGraph::new("acme");
    graph.set_the_world(
        vec![
            RelationFact::new(RelationType::DependsOn, subject(), component_v1())
                .with_evidence("evd-1"),
            RelationFact::new(RelationType::DependsOn, component_v1(), component_v2())
                .with_evidence("evd-2"),
            RelationFact::new(RelationType::DependsOn, other(), another()).with_evidence("evd-3"),
        ],
        7,
    );
    graph
}

/// The simulation service over the graph.
fn the_service(graph: std::sync::Arc<InMemoryGraph>) -> SimulationService {
    SimulationService::new(graph, std::sync::Arc::new(InMemoryDecisions))
}

/// The simulation fork.
fn the_fork() -> SimulationFork {
    SimulationFork::new(
        "sim-1",
        Checkpoint::new("checkpoint-1", "acme", the_now(), Some(42), 7),
        the_policy(),
        the_now(),
    )
}

/// The simulation label.
fn the_tag() -> SimulationTag {
    SimulationTag::new("sim-1", the_now())
}

/// The risk hypothetical: the subject is left exposed.
fn the_risk() -> Hypothetical {
    Hypothetical {
        tag: the_tag(),
        claim: HypotheticalClaim::Risk {
            subject: subject(),
            advisory: "GHSA-hsp92-0001".to_string(),
            critical: true,
            reachable: true,
        },
    }
}

/// The result of a simulation with the risk and the strict policy.
async fn the_result() -> SimulationResult {
    let graph = the_graph();
    let service = the_service(graph);
    let fork = the_fork().with(the_risk()).with_policy(the_strict_policy());
    service
        .simulate_result(&fork, the_now())
        .await
        .expect("la simulacion se hace")
}

// ───────────────────────────── the version ─────────────────────────────

/// The result declares the contract version.
///
/// A contract that does not state its version is a contract that cannot
/// evolve: the consumer does not know whether what it reads is what it expects.
#[tokio::test]
async fn the_result_declares_its_version() {
    let result = the_result().await;

    assert_eq!(result.version, SIMULATION_RESULT_VERSION);
    assert_eq!(result.version, "1.0.0");

    // And the JSON that is carried says the same.
    let json = serde_json::to_string(&result).expect("el resultado se serializa");
    assert!(
        json.contains("\"version\":\"1.0.0\""),
        "el JSON declara la version: {json}"
    );
}

// ───────────────────────────── the observed and the hypothetical ─────────────────────────────

/// The result separates what was observed from what is hypothetical.
///
/// And the hypothetical is **not published as a graph fact**: the
/// observed relations are those that were there, and the hypothetical travels in
/// its own field.
#[tokio::test]
async fn the_result_separates_the_observed_from_the_hypothetical() {
    let result = the_result().await;

    // The observed: the relations that were in the world.
    assert_eq!(
        result.observed.relations.len(),
        1,
        "la relacion del sujeto: {:?}",
        result.observed.relations
    );
    assert!(
        !result
            .observed
            .relations
            .iter()
            .any(|relation| relation.to == component_v3()),
        "y la relacion hipotetica NO esta entre las observadas"
    );

    // The hypothetical: the scenarios, marked as such. There are two: the
    // risk and the hypothetical policy.
    assert_eq!(
        result.hypotheses.len(),
        2,
        "la hipotetica del riesgo y la politica: {:?}",
        result.hypotheses
    );
    let hypothesis = result
        .hypotheses
        .iter()
        .find(|hypothesis| matches!(hypothesis.kind, HypothesisKind::Risk { .. }))
        .expect("la hipotetica del riesgo esta declarada");
    assert_eq!(hypothesis.simulation, "sim-1");
    assert!(matches!(
        hypothesis.kind,
        HypothesisKind::Risk {
            ref advisory,
            critical: true,
            reachable: true,
            ..
        } if advisory == "GHSA-hsp92-0001"
    ));
    assert_eq!(hypothesis.subjects, vec![subject()]);

    // And the hypothetical relation, if there is one, is not among the
    // observed ones either.
    let graph = the_graph();
    let service = the_service(graph);
    let fork = the_fork().with(Hypothetical {
        tag: the_tag(),
        claim: HypotheticalClaim::Relation {
            relation: RelationType::DependsOn,
            from: subject(),
            to: component_v3(),
            replaces: None,
        },
    });
    let result = service
        .simulate_result(&fork, the_now())
        .await
        .expect("la simulacion se hace");
    assert!(
        !result
            .observed
            .relations
            .iter()
            .any(|relation| relation.to == component_v3()),
        "la relacion hipotetica no se publica como hecho del grafo"
    );
    assert!(
        result
            .hypotheses
            .iter()
            .any(|hypothesis| matches!(&hypothesis.kind, HypothesisKind::Relation { to, .. } if *to == component_v3())),
        "y si aparece, es como hipotetica: {:?}",
        result.hypotheses
    );
}

// ───────────────────────────── the policies ─────────────────────────────

/// The result says which policies took part.
///
/// The previous one, the later one and the hypothetical one: with their identifier and
/// version, and without the whole object.
#[tokio::test]
async fn the_result_says_which_policies_intervened() {
    let result = the_result().await;

    // The policy that was in force.
    assert_eq!(result.observed.policy.id, "production");
    assert_eq!(result.observed.policy.version, "1.0.0");

    // And the two versions of each decision that changes.
    assert_eq!(result.decisions.len(), 1, "la decision que cambia");
    let change = &result.decisions[0];
    assert_eq!(change.policy_version_before, "1.0.0");
    assert_eq!(change.policy_version_after, "2.0.0");

    // And the hypothetical policy travels as hypothetical, with its seal.
    let hypothetical_policy = result
        .hypotheses
        .iter()
        .find(|hypothesis| matches!(hypothesis.kind, HypothesisKind::Policy { .. }))
        .expect("la politica hipotetica esta declarada");
    match &hypothetical_policy.kind {
        HypothesisKind::Policy { policy } => {
            assert_eq!(policy.id, "production");
            assert_eq!(policy.version, "2.0.0");
        }
        other => panic!("no es una politica: {other:?}"),
    }
}

/// And the contract does NOT carry the whole policy: only its seal.
///
/// A contract that drags the policy object stops being
/// portable, and what is needed to know which rules were applied
/// is the version.
#[tokio::test]
async fn the_result_does_not_carry_a_policy_object() {
    let result = the_result().await;

    let json = serde_json::to_string(&result).expect("el resultado se serializa");
    assert!(
        !json.contains("risk_established"),
        "el resultado no lleva los requisitos de la politica: {json}"
    );
    assert!(
        !json.contains("requirements"),
        "ni una lista de requisitos: {json}"
    );
}

// ───────────────────────────── the base ─────────────────────────────

/// The result carries the base's seal: over which world it was computed.
#[tokio::test]
async fn the_result_carries_the_stamp_of_the_base() {
    let result = the_result().await;

    assert_eq!(result.account, "acme");
    assert_eq!(result.simulation, "sim-1");
    assert_eq!(result.base.account, "acme");
    assert_eq!(result.base.evaluated_at, the_now());
    assert_eq!(result.base.generation, Some(7));
    assert_eq!(result.base.declared_ledger_position, Some(42));
    assert!(
        !result.base.seal.is_empty(),
        "el sello del contenido leido no esta vacio"
    );
    assert!(
        !result.base.fingerprint.is_empty(),
        "la huella del manifiesto no esta vacia"
    );
    assert_eq!(
        result.base.sources,
        vec![hodei_simulation::result::SourceStamp {
            source: "graph".to_string(),
            version: 7,
        }],
        "y la version de la fuente leida: {:?}",
        result.base.sources
    );
}

// ───────────────────────────── the coverage ─────────────────────────────

/// The result says what has been covered and what has not.
///
/// The other subject's relation is not read, and the result says so: a
/// result that does not say what it has not seen looks complete.
#[tokio::test]
async fn the_result_carries_the_coverage() {
    let result = the_result().await;

    assert_eq!(result.coverage.evaluated_subjects, vec![subject()]);
    assert!(
        result.coverage.declared_subjects.is_empty(),
        "no se declararon sujetos: {:?}",
        result.coverage.declared_subjects
    );
    assert_eq!(
        result.coverage.relations_read, 1,
        "la relacion del sujeto, que es lo unico que toca"
    );
    assert_eq!(
        result.coverage.relations_not_read, 2,
        "y las otras dos de la cuenta, que no se leen"
    );
    assert_eq!(
        result.coverage.omitted.len(),
        1,
        "y se dice que no se ha leido: {:?}",
        result.coverage.omitted
    );
    assert!(
        result.coverage.omitted[0]
            .reason
            .contains("2 relations of the account were not read"),
        "con su motivo: {:?}",
        result.coverage.omitted[0]
    );
}

// ───────────────────────────── the conflicts ─────────────────────────────

/// The overlay cycle is declared.
///
/// An overlay that closes a cycle and presents itself as a normal overlay
/// is an overlay that nobody can read.
#[tokio::test]
async fn the_cycle_of_the_overlay_is_declared() {
    let graph = the_graph();
    let service = the_service(graph);
    let fork = the_fork().with(Hypothetical {
        tag: the_tag(),
        claim: HypotheticalClaim::Relation {
            relation: RelationType::DependsOn,
            from: component_v2(),
            to: subject(),
            replaces: None,
        },
    });

    let result = service
        .simulate_result(&fork, the_now())
        .await
        .expect("la simulacion se hace");

    let conflict = result
        .conflicts
        .iter()
        .find(|conflict| conflict.kind == ConflictKind::Cycle)
        .expect("el ciclo se declara");
    assert!(
        conflict.subjects.contains(&subject()),
        "y dice a quien le llega: {:?}",
        conflict.subjects
    );
}

/// The duplicate is declared: two hypotheticals that say the same thing.
#[tokio::test]
async fn the_duplicate_of_the_hypothesis_is_declared() {
    let graph = the_graph();
    let service = the_service(graph);
    let claim = HypotheticalClaim::Relation {
        relation: RelationType::DependsOn,
        from: subject(),
        to: component_v3(),
        replaces: None,
    };
    let fork = the_fork()
        .with(Hypothetical {
            tag: the_tag(),
            claim: claim.clone(),
        })
        .with(Hypothetical {
            tag: the_tag(),
            claim,
        });

    let result = service
        .simulate_result(&fork, the_now())
        .await
        .expect("la simulacion se hace");

    assert!(
        result
            .conflicts
            .iter()
            .any(|conflict| conflict.kind == ConflictKind::Duplicate),
        "el duplicado se declara: {:?}",
        result.conflicts
    );
}

/// The contradiction is declared: a relation that is added and
/// replaced at the same time.
#[tokio::test]
async fn the_contradiction_of_the_hypotheses_is_declared() {
    let graph = the_graph();
    let service = the_service(graph);
    let fork = the_fork().with(Hypothetical {
        tag: the_tag(),
        claim: HypotheticalClaim::Relation {
            relation: RelationType::DependsOn,
            from: subject(),
            to: component_v3(),
            replaces: Some(RelationFact::new(
                RelationType::DependsOn,
                subject(),
                component_v3(),
            )),
        },
    });

    let result = service
        .simulate_result(&fork, the_now())
        .await
        .expect("la simulacion se hace");

    assert!(
        result
            .conflicts
            .iter()
            .any(|conflict| conflict.kind == ConflictKind::Contradiction),
        "la contradiccion se declara: {:?}",
        result.conflicts
    );
}

// ───────────────────────────── the reproducibility ─────────────────────────────

/// The same simulation gives the same result.
///
/// The clock is injected, the result is canonical and therefore two
/// simulations of the same world can be compared.
#[tokio::test]
async fn the_same_simulation_gives_the_same_result() {
    let first = the_result().await;
    let second = the_result().await;

    assert_eq!(first, second, "el resultado es el mismo");
}
