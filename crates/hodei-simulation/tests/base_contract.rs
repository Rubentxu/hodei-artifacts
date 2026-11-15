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
//! - La evidencia que consume la decision tambien se comprueba: no toca
//!   el grafo, asi que sin su propia comprobacion seria invisible.
//! - El reloj es el que recibe quien simula, no el del sistema: la
//!   misma base y los mismos hechos dan el mismo resultado.
//! - Una base de otra cuenta, sin version o preparada contra otra
//!   generacion se rechaza.

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

/// El sujeto de la simulacion.
fn subject() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-1.0.0").expect("un HRN")
}

/// El otro sujeto de la simulacion.
fn other_subject() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/api-2.0.0").expect("un HRN")
}

/// El componente del que depende.
fn component_v1() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v1").expect("un HRN")
}

/// El componente al que se sube en el cambio concurrente.
fn component_v2() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v2").expect("un HRN")
}

/// La generacion del grafo de mentira.
const THE_GENERATION: u64 = 7;

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
            generation: Mutex::new(THE_GENERATION),
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
    /// Lo que el evaluador ha leido: lo que dice que consume.
    reads: Mutex<ReadSet>,
    /// La evidencia que entra mientras se simula.
    ///
    /// No toca el grafo: no cambia el numero de relaciones ni mueve la
    /// generacion. Solo la ven los que leen la evidencia.
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

    /// La evidencia entra DESPUES de que la decision la consumio.
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

    /// Lo que el evaluador lee ahora: lo que ya estaba, mas lo que ha
    /// entrado.
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
        Checkpoint::new("checkpoint-1", "acme", Utc::now(), Some(42), THE_GENERATION),
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
///
/// El mundo se mueve en la PRIMERA lectura de relaciones, que es la
/// que viene DESPUES de leer la generacion y el recuento: el cambio cae
/// entre fijar la base y comprobar la base, que es justo la ventana que
/// la comprobacion final existe para cerrar.
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

/// AC1, en la variante que el ALMACEN REAL puede producir: la misma
/// relacion con OTRA evidencia.
///
/// Sustituir una relacion por otra de extremos distintos deja el
/// numero de filas increased en el almacen real (la fila vieja no se
/// borra), asi que ese caso lo detecta el recuento. Lo que el almacen
/// real hace con el recuento IGUAL es cambiar el apoyo de una relacion:
/// misma identidad, misma fila, otra evidencia. Y ese caso solo lo ve
/// el sello de contenido, porque la generacion no se mueve cuando el
/// cambio no pasa por el ledger de deltas.
#[tokio::test]
async fn a_relation_that_changes_its_evidence_is_a_stale_base() {
    let graph = TheGraph::new(vec![
        RelationFact::new(RelationType::DependsOn, subject(), component_v1())
            .with_evidence("evd-1"),
    ]);
    // El cambio ocurre en la segunda lectura: la misma relacion, con otra
    // evidencia. El numero de relaciones sigue siendo uno.
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

/// Y el manifiesto sella la politica que ESTA en vigor en el fork, que
/// es la que produce la decision de "despues": un manifiesto a medias de
/// lo que produjo la decision no verifica la decision.
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

/// Y cuando la hipotetica es la MISMA politica con OTRA version, las dos
/// versiones estan en el manifiesto.
///
/// Son el mismo recurso verificado de dos maneras, y el manifiesto
/// guardaba solo la primera: el diff decia "la politica 2.0.0 produjo el
/// despues" mientras el manifiesto solo hablaba de la 1.0.0. Un
/// manifiesto que no contiene lo que produjo la decision no verifica
/// la decision.
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

// ───────────────────────────── la evidencia que decide ─────────────────────────────

/// La base guarda la evidencia que CONSUMIO la decision.
///
/// Sin esto la base diria sobre que mundo se calculo sin decir que
/// evidencia lo decidio, y una evidencia que entra en mitad de la
/// simulacion se atribuiria al hecho hipotetico.
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

/// Y una evidencia que ENTRA mientras se simula hace obsoleta la base.
///
/// No toca el grafo: no cambia el numero de relaciones ni mueve la
/// generacion. Los tres guards del grafo no la ven, y sin su propia
/// comprobacion el cambio de decision se atribuiria al hecho
/// hipotetico.
#[tokio::test]
async fn an_evidence_that_arrives_during_the_simulation_is_a_stale_base() {
    let graph = TheGraph::new(vec![]);
    let evaluator = TheEvaluator::new();
    // La evidencia entra DESPUES de que la decision la consumio.
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

/// Y el diff no depende del ORDEN en que se declaran los sujetos.
///
/// El orden de declaracion de un fork es el orden en que a alguien se le
/// ocurrio declararlos, y un diff que cambia con ese orden no se puede
/// comparar con otro diff.
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

/// Y el sello del contenido no depende del ORDEN en que el grafo da sus
/// relaciones.
///
/// El grafo puede dar la misma relacion dos veces con distinto apoyo, y
/// el sello no puede depender de cual de las dos se queda: un sello que
/// depende del orden del almacen no es un sello del mundo.
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

/// Y el fork preparado contra la generacion de AHORA si se simula.
#[tokio::test]
async fn a_fork_prepared_against_the_current_generation_is_simulated() {
    let graph = TheGraph::new(vec![]);
    let service = SimulationService::new(graph, TheEvaluator::new());

    service
        .simulate(&the_fork(), the_moment())
        .await
        .expect("la base vigente se simula");
}

/// Y un fork SIN version declarada se rechaza.
///
/// Una base que no dice contra que version del mundo se evaluo no se
/// puede comprobar, y una base que no se puede comprobar no sostiene
/// una respuesta.
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

/// Y una base de OTRA cuenta se rechaza: una base de otra cuenta no es
/// una base, es una respuesta sobre el mundo de otro.
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
    // El cambio ocurre en la primera lectura de relaciones, que es la
    // que viene despues de leer la generacion y el recuento: una
    // relacion mas, y el grafo ya no es el que se evaluo.
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
