//! El contrato de la base de la simulacion.
//!
//! La simulacion dice sobre que mundo se calculo, y ese mundo se fija
//! antes y se comprueba despues. Lo que se comprueba:
//!
//! - Un mundo que se mueve mientras se simula hace que la simulacion se
//!   diga OBSOLETA, y no que devuelva un diff sobre un mundo que ya no
//!   existe.
//! - El cambio concurrente se detecta por el CONTENIDO, no por el
//!   recuento: una relacion sustituida por otra deja el numero igual.
//! - El reloj es el que recibe quien simula, no el del sistema: la
//!   misma base y los mismos hechos dan el mismo resultado.
//! - Una base de otra cuenta o preparada contra otra generacion se
//!   rechaza.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use hodei_evidence::domain::graph::RelationType;
use hodei_simulation::diff::TheDecision;
use hodei_simulation::hypothetical::{Hypothetical, HypotheticalClaim, RelationFact};
use hodei_simulation::ports::{DecisionEvaluator, SimulationGraph};
use hodei_simulation::service::SimulationService;
use hodei_simulation::{Checkpoint, SimulationFork, SimulationTag};
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

/// El componente al que se sube en el cambio concurrente.
fn component_v2() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v2").expect("un HRN")
}

/// El grafo real, con la capacidad de moverse mientras se simula.
struct TheGraph {
    relations: Mutex<Vec<RelationFact>>,
    reads: Mutex<usize>,
    movement: Mutex<Option<Movement>>,
    generation: Mutex<u64>,
}

/// Un cambio del mundo, programado para ocurrir en una lectura.
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
            generation: Mutex::new(7),
        })
    }

    /// El grafo se mueve en la lectura numero `at_read`.
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

/// El evaluador que APUNTA el instante que recibe.
///
/// Es la forma de comprobar que el nucleo de decision no lee el reloj de
/// pared: recibe el instante y decide con el.
struct TheEvaluator {
    seen: Mutex<Vec<DateTime<Utc>>>,
}

impl TheEvaluator {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            seen: Mutex::new(Vec::new()),
        })
    }

    fn the_instants(&self) -> Vec<DateTime<Utc>> {
        self.seen
            .lock()
            .expect("el evaluador no esta envenenado")
            .clone()
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
        })
    }
}

/// La politica de ahora.
fn the_policy(version: &str) -> TrustPolicy {
    TrustPolicy::new(
        format!("policy-{version}"),
        version,
        vec![Requirement::denying(RequirementKind::RiskEstablished)],
    )
}

/// El fork de la simulacion.
fn the_fork() -> SimulationFork {
    SimulationFork::new(
        "sim-1",
        Checkpoint::new("checkpoint-1", "acme", Utc::now(), Some(42)),
        the_policy("1.0.0"),
        Utc::now(),
    )
    .with_subjects(vec![subject()])
}

/// El instante de la simulacion: el reloj lo fija quien simula.
fn the_moment() -> DateTime<Utc> {
    DateTime::from_timestamp(1_700_000_000, 0).expect("un instante")
}

// ───────────────────────────── AC1 ─────────────────────────────

/// AC1: un cambio concurrente que SUSTITUYE una relacion por otra
/// manteniendo el MISMO recuento se detecta como base obsoleta.
///
/// El numero de relaciones no lo distingue: sigue siendo una. Y la
/// generacion tampoco, si el cambio no la mueve. Lo que lo distingue es
/// el contenido leido, que es lo que la simulacion sello.
#[tokio::test]
async fn a_concurrent_swap_with_the_same_count_is_a_stale_base() {
    let graph = TheGraph::new(vec![RelationFact::new(
        RelationType::DependsOn,
        subject(),
        component_v1(),
    )]);
    // El cambio ocurre en la segunda lectura, que es la de comprobacion:
    // una relacion por otra, y el numero de relaciones sigue siendo uno.
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

/// Y un cambio que mueve la GENERACION del grafo tambien hace obsoleta
/// la simulacion, aunque el contenido leido siga igual.
#[tokio::test]
async fn a_concurrent_change_that_moves_the_generation_is_a_stale_base() {
    let graph = TheGraph::new(vec![RelationFact::new(
        RelationType::DependsOn,
        subject(),
        component_v1(),
    )]);
    graph.moving_at(
        2,
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

/// Y una simulacion cuyo mundo NO se mueve se completa y dice sobre que
/// mundo se calculo.
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
            .is_consistent_with(&[("graph".to_string(), 7)]),
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

// ───────────────────────────── AC2 ─────────────────────────────

/// AC2: con la misma base, el mismo reloj y los mismos hechos, dos
/// evaluaciones dan el mismo resultado y el mismo diff.
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

// ───────────────────────────── AC3 ─────────────────────────────

/// AC3: el nucleo de decision recibe el instante de la simulacion, y no
/// el del reloj de pared.
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

/// Y dos simulaciones con relojes DISTINTOS no se confunden: el reloj
/// es parte de la base, y por eso viaja en el diff.
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

/// AC4: un fork preparado contra OTRA generacion se rechaza: el mundo
/// ya no es el que el fork mio.
#[tokio::test]
async fn a_fork_prepared_against_another_generation_is_rejected() {
    let graph = TheGraph::new(vec![]);
    let service = SimulationService::new(graph, TheEvaluator::new());

    let fork = the_fork();
    let fork = SimulationFork::new(
        fork.id.clone(),
        fork.base.clone().at_generation(6),
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

/// Y el fork preparado contra la generacion de AHORA si se simula.
#[tokio::test]
async fn a_fork_prepared_against_the_current_generation_is_simulated() {
    let graph = TheGraph::new(vec![]);
    let service = SimulationService::new(graph, TheEvaluator::new());

    let fork = the_fork();
    let fork = SimulationFork::new(
        fork.id.clone(),
        fork.base.clone().at_generation(7),
        fork.base_policy.clone(),
        fork.created_at,
    )
    .with_subjects(vec![subject()]);

    service
        .simulate(&fork, the_moment())
        .await
        .expect("la base vigente se simula");
}

/// Y una base de OTRA cuenta se rechaza: una base de otra cuenta no es
/// una base, es una respuesta sobre el mundo de otro.
#[tokio::test]
async fn a_base_of_another_account_is_rejected() {
    let graph = TheGraph::new(vec![]);
    let service = SimulationService::new(graph, TheEvaluator::new());

    let fork = SimulationFork::new(
        "sim-1",
        Checkpoint::new("checkpoint-1", "contoso", Utc::now(), Some(42)),
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

/// Y la generacion del grafo se comprueba DESPUES de evaluar: un mundo
/// que se mueve entre la lectura y el final de la evaluacion tambien se
/// dice obsoleto.
#[tokio::test]
async fn a_world_that_moves_after_the_reads_is_also_stale() {
    let graph = TheGraph::new(vec![RelationFact::new(
        RelationType::DependsOn,
        subject(),
        component_v1(),
    )]);
    // El cambio ocurre en la segunda lectura, la de comprobacion: una
    // relacion mas, y el grafo ya no es el que se evaluo.
    graph.moving_at(
        2,
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

/// El hecho hipotetico lleva su etiqueta: lo que se superpone al mundo
/// real no es del mundo real.
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
