//! La conformidad de la simulacion: los mismos escenarios para cualquier
//! adaptador.
//!
//! La suite vive en el dominio y no en el almacen, y por eso puede
//! correr contra un grafo en memoria y contra el grafo real de
//! SurrealDB: lo que cambia es de donde salen las relaciones, y lo que
//! se comprueba es siempre lo mismo.
//!
//! # El fixture
//!
//! Los escenarios no se inventan el mundo: se llevan un **fixture** que
//! el adaptador deja en su mundo. El fixture es el mismo para todos --un
//! artefacto que depende de dos componentes-- y por eso los resultados
//! de dos adaptadores son comparables: si el adaptador de SurrealDB
//! dijera otra cosa que el de memoria, el fallo es del adaptador y no
//! del escenario.
//!
//! # Los escenarios
//!
//! - El **orden** de las lecturas no cambia el resultado.
//! - Un **ciclo** del overlay se declara, no se esconde.
//! - Un **duplicado** se declara.
//! - Una **adicion y una eliminacion** aparecen en el diff.
//! - Una **base obsoleta** no es un diff: es un error.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_evidence::domain::graph::{NodeKind, RelationType};
use hodei_trust_policy::{Requirement, RequirementKind, TrustPolicy};
use kernel::Hrn;

use crate::hypothetical::{Hypothetical, HypotheticalClaim, RelationFact};
use crate::ports::{DecisionEvaluator, SimulationGraph};
use crate::result::ConflictKind;
use crate::service::{SimulationError, SimulationService};
use crate::{Checkpoint, SimulationFork, SimulationTag};

/// La evidencia que sostiene la primera relacion del fixture.
pub const THE_FIRST_EVIDENCE: &str = "ev-conformance-1";
/// La evidencia que sostiene la segunda relacion del fixture.
pub const THE_SECOND_EVIDENCE: &str = "ev-conformance-2";

/// El fixture: el mundo minimo y con forma.
///
/// La cadena es `artefacto -> lib-v1 -> lib-v2`: un sujeto y dos
/// dependencias. Es el mundo mas pequeno que tiene una adicion, una
/// eliminacion y un ciclo posible, y por eso sirve para las tres cosas.
#[derive(Debug, Clone)]
pub struct Fixture {
    /// La cuenta del mundo.
    pub account: String,
    /// El tenant del grafo: el HRN de la cuenta.
    pub tenant: Hrn,
    /// La generacion del mundo.
    pub generation: u64,
    /// Los nodos del mundo.
    pub nodes: Vec<FixtureNode>,
    /// Las relaciones del mundo.
    pub relations: Vec<RelationFact>,
    /// El sujeto: el artefacto que depende.
    pub artefact: Hrn,
    /// El primer componente.
    pub lib_v1: Hrn,
    /// El segundo componente.
    pub lib_v2: Hrn,
    /// El componente al que se sube.
    pub lib_v3: Hrn,
}

/// Un nodo del fixture.
#[derive(Debug, Clone)]
pub struct FixtureNode {
    /// El nombre del nodo.
    pub hrn: Hrn,
    /// La clase del nodo.
    pub kind: NodeKind,
}

impl Fixture {
    /// El fixture de una cuenta, sobre su tenant.
    pub fn of(account: &str, tenant: Hrn) -> Self {
        let artefact = the_node(&tenant, "Artifact", "app-1.0.0");
        let lib_v1 = the_node(&tenant, "Component", "lib-v1");
        let lib_v2 = the_node(&tenant, "Component", "lib-v2");
        let lib_v3 = the_node(&tenant, "Component", "lib-v3");
        Self {
            account: account.to_string(),
            tenant: tenant.clone(),
            generation: 1,
            nodes: vec![
                FixtureNode {
                    hrn: artefact.clone(),
                    kind: NodeKind::Artifact,
                },
                FixtureNode {
                    hrn: lib_v1.clone(),
                    kind: NodeKind::Component,
                },
                FixtureNode {
                    hrn: lib_v2.clone(),
                    kind: NodeKind::Component,
                },
                FixtureNode {
                    hrn: lib_v3.clone(),
                    kind: NodeKind::Component,
                },
            ],
            relations: vec![
                RelationFact::new(RelationType::DependsOn, artefact.clone(), lib_v1.clone())
                    .with_evidence(THE_FIRST_EVIDENCE),
                RelationFact::new(RelationType::DependsOn, lib_v1.clone(), lib_v2.clone())
                    .with_evidence(THE_SECOND_EVIDENCE),
            ],
            artefact,
            lib_v1,
            lib_v2,
            lib_v3,
        }
    }
}

/// El nodo del fixture, con el nombre del tenant.
fn the_node(tenant: &Hrn, resource_type: &str, resource_id: &str) -> Hrn {
    Hrn::with_region(
        tenant.partition().to_string(),
        tenant.service().to_string(),
        tenant.region().to_string(),
        tenant.account_id().to_string(),
        resource_type.to_string(),
        resource_id.to_string(),
    )
}

/// El mundo que la conformidad necesita.
///
/// El mundo es lo que cambia de un adaptador a otro: un grafo en memoria
/// o el grafo real de SurrealDB. Lo que se comprueba es siempre lo
/// mismo, y por eso los escenarios viven aqui y no en el adaptador.
#[async_trait]
pub trait ConformanceWorld: Send + Sync {
    /// La cuenta del mundo.
    fn account(&self) -> String;
    /// El tenant del grafo: el HRN de la cuenta.
    fn tenant(&self) -> Hrn;
    /// Prepara el fixture: lo deja en el mundo.
    async fn prepare(&self, fixture: &Fixture) -> Result<(), String>;
    /// Las relaciones del mundo, en el orden que las da el adaptador.
    async fn relations(&self) -> Result<Vec<RelationFact>, String>;
    /// La generacion del mundo.
    async fn generation(&self) -> Result<u64, String>;
    /// El evaluador de decisiones del mundo.
    fn decisions(&self) -> Arc<dyn DecisionEvaluator>;
}

/// El resultado de un escenario.
#[derive(Debug, Clone)]
pub struct ScenarioResult {
    /// El nombre del escenario.
    pub name: &'static str,
    /// Si el escenario paso.
    pub passed: bool,
    /// Por que no paso, si no paso.
    pub reason: Option<String>,
}

/// El informe de la conformidad.
#[derive(Debug, Clone)]
pub struct ConformanceReport {
    /// Los escenarios, en el orden en que se corrieron.
    pub scenarios: Vec<ScenarioResult>,
}

impl ConformanceReport {
    /// Si todos los escenarios pasaron.
    pub fn passed(&self) -> bool {
        self.scenarios.iter().all(|scenario| scenario.passed)
    }

    /// Los escenarios que no pasaron.
    pub fn failures(&self) -> Vec<&ScenarioResult> {
        self.scenarios
            .iter()
            .filter(|scenario| !scenario.passed)
            .collect()
    }
}

/// La suite de conformidad: los escenarios que toda simulacion tiene que
/// cumplir, sea cual sea el adaptador que lee el mundo.
pub async fn run_suite(world: &dyn ConformanceWorld) -> ConformanceReport {
    let mut scenarios: Vec<ScenarioResult> = Vec::new();
    scenarios.push(the_order_of_the_reads_does_not_change_the_result(world).await);
    scenarios.push(the_cycle_of_the_overlay_is_declared(world).await);
    scenarios.push(the_duplicate_is_declared(world).await);
    scenarios.push(the_addition_and_the_elimination_are_in_the_diff(world).await);
    scenarios.push(the_stale_base_is_not_a_diff(world).await);
    ConformanceReport { scenarios }
}

/// El grafo de la suite: las relaciones que da el mundo, fijas.
///
/// Es el mismo grafo para los dos adaptadores: lo que cambia es de donde
/// salen las relaciones, y la suite las recibe del mundo.
struct StaticGraph {
    relations: Vec<RelationFact>,
    generation: u64,
    account: String,
}

#[async_trait]
impl SimulationGraph for StaticGraph {
    async fn relations_of(&self, subject: &Hrn) -> Result<Vec<RelationFact>, String> {
        Ok(self
            .relations
            .iter()
            .filter(|relation| &relation.from == subject || &relation.to == subject)
            .cloned()
            .collect())
    }

    async fn relation_count(&self) -> Result<usize, String> {
        Ok(self.relations.len())
    }

    async fn all_relations(&self) -> Result<Vec<RelationFact>, String> {
        Ok(self.relations.clone())
    }

    async fn generation(&self) -> Result<u64, String> {
        Ok(self.generation)
    }

    fn account(&self) -> String {
        self.account.clone()
    }
}

/// El mundo preparado: el fixture, las relaciones y la generacion.
struct TheWorld {
    fixture: Fixture,
    relations: Vec<RelationFact>,
    generation: u64,
    decisions: Arc<dyn DecisionEvaluator>,
}

/// El instante de la conformidad: uno solo para todos los escenarios.
///
/// El reloj no se lee de la pared: dos simulaciones con la misma base y
/// el mismo reloj dan el mismo resultado, y una conformidad que depende
/// del reloj no se puede comparar.
fn the_now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-01-01T12:00:00Z")
        .expect("un instante fijo")
        .with_timezone(&Utc)
}

/// La politica de la conformidad: la que prohibe el riesgo establecido.
///
/// Es una politica de verdad, con sus requisitos, y por eso el resultado
/// que se declara tiene que viajar sin ella: solo con su sello.
fn the_policy(version: &str) -> TrustPolicy {
    TrustPolicy::new(
        format!("policy-{version}"),
        version,
        vec![Requirement::denying(RequirementKind::RiskEstablished)],
    )
}

/// El fork de la conformidad.
///
/// Declara los sujetos del fixture: una simulacion que no declara
/// sujetos es una simulacion que no se puede hacer, y la conformidad no
/// va a comprobar una simulacion que no existe.
fn the_fork(fixture: &Fixture, generation: u64) -> SimulationFork {
    SimulationFork::new(
        "sim-conformance",
        Checkpoint::new(
            "checkpoint-conformance",
            fixture.account.clone(),
            the_now(),
            Some(0),
            generation,
        ),
        the_policy("1.0.0"),
        the_now(),
    )
    .with_subjects(vec![fixture.artefact.clone(), fixture.lib_v1.clone()])
}

/// La etiqueta de la conformidad.
fn the_tag() -> SimulationTag {
    SimulationTag::new("sim-conformance", the_now())
}

/// El servicio de la suite, sobre las relaciones dadas.
fn the_service(world: &TheWorld, relations: Vec<RelationFact>) -> SimulationService {
    let graph: Arc<dyn SimulationGraph> = Arc::new(StaticGraph {
        relations,
        generation: world.generation,
        account: world.fixture.account.clone(),
    });
    SimulationService::new(graph, world.decisions.clone())
}

/// El mundo preparado: el fixture se deja en el mundo y se lee.
async fn the_world(world: &dyn ConformanceWorld) -> Result<TheWorld, String> {
    let fixture = Fixture::of(&world.account(), world.tenant());
    world.prepare(&fixture).await?;
    let relations = world.relations().await?;
    let generation = world.generation().await?;
    Ok(TheWorld {
        fixture,
        relations,
        generation,
        decisions: world.decisions(),
    })
}

/// El resultado de un escenario que ha pasado.
fn passed(name: &'static str) -> ScenarioResult {
    ScenarioResult {
        name,
        passed: true,
        reason: None,
    }
}

/// El resultado de un escenario que no ha pasado.
fn failed(name: &'static str, reason: impl Into<String>) -> ScenarioResult {
    ScenarioResult {
        name,
        passed: false,
        reason: Some(reason.into()),
    }
}

// ───────────────────────── el orden ─────────────────────────

/// El orden de las lecturas no cambia el resultado.
async fn the_order_of_the_reads_does_not_change_the_result(
    world: &dyn ConformanceWorld,
) -> ScenarioResult {
    const NAME: &str = "the order of the reads does not change the result";
    match the_order_scenario(world).await {
        Ok(()) => passed(NAME),
        Err(reason) => failed(NAME, reason),
    }
}

/// El escenario del orden: el mismo mundo leido al reves da el MISMO
/// resultado canonico.
///
/// Y el resultado canonico es el que se compara: el diff estructural y
/// las decisiones ya salen ordenados, y el manifiesto se compara por su
/// forma canonica y no por el orden en que se leyo.
async fn the_order_scenario(world: &dyn ConformanceWorld) -> Result<(), String> {
    let world = the_world(world).await?;
    if world.relations.len() < 2 {
        return Err(
            "the fixture has fewer than two relations: there is no order to change".to_string(),
        );
    }
    let mut reversed = world.relations.clone();
    reversed.reverse();
    if reversed == world.relations {
        return Err(
            "the reversed order is the same order: the scenario would prove nothing".to_string(),
        );
    }

    let fork = the_fork(&world.fixture, world.generation);
    let first = the_service(&world, world.relations.clone())
        .simulate(&fork, the_now())
        .await
        .map_err(|error| error.to_string())?;
    let second = the_service(&world, reversed.clone())
        .simulate(&fork, the_now())
        .await
        .map_err(|error| error.to_string())?;

    if first.canonical() != second.canonical() {
        return Err(format!(
            "the canonical diffs are different:\n  first:  {:?}\n  second: {:?}",
            first.canonical(),
            second.canonical()
        ));
    }
    Ok(())
}

// ───────────────────────── el ciclo ─────────────────────────

/// El ciclo del overlay se declara.
async fn the_cycle_of_the_overlay_is_declared(world: &dyn ConformanceWorld) -> ScenarioResult {
    const NAME: &str = "the cycle of the overlay is declared";
    match the_cycle_scenario(world).await {
        Ok(()) => passed(NAME),
        Err(reason) => failed(NAME, reason),
    }
}

/// El escenario del ciclo: una hipotetica que cierra la vuelta se
/// declara como conflicto.
///
/// El overlay del fixture es `artefacto -> lib-v1 -> lib-v2`, y la
/// hipotetica hace que `lib-v2` vuelva al artefacto.
async fn the_cycle_scenario(world: &dyn ConformanceWorld) -> Result<(), String> {
    let world = the_world(world).await?;
    let fork = the_fork(&world.fixture, world.generation).with(Hypothetical {
        tag: the_tag(),
        claim: HypotheticalClaim::Relation {
            relation: RelationType::DependsOn,
            from: world.fixture.lib_v2.clone(),
            to: world.fixture.artefact.clone(),
            replaces: None,
        },
    });

    let result = the_service(&world, world.relations.clone())
        .simulate_result(&fork, the_now())
        .await
        .map_err(|error| error.to_string())?;

    if !result
        .conflicts
        .iter()
        .any(|conflict| conflict.kind == ConflictKind::Cycle)
    {
        return Err(format!(
            "the cycle of the overlay is not declared: {:?}",
            result.conflicts
        ));
    }
    Ok(())
}

// ───────────────────────── el duplicado ─────────────────────────

/// El duplicado se declara.
async fn the_duplicate_is_declared(world: &dyn ConformanceWorld) -> ScenarioResult {
    const NAME: &str = "the duplicate is declared";
    match the_duplicate_scenario(world).await {
        Ok(()) => passed(NAME),
        Err(reason) => failed(NAME, reason),
    }
}

/// El escenario del duplicado: dos hipoteticas que dicen lo mismo, y una
/// hipotetica que repite lo que ya esta en el mundo.
///
/// Las dos se declaran: un diff que las guarda sin decir nada parece un
/// diff que no ha mirado.
async fn the_duplicate_scenario(world: &dyn ConformanceWorld) -> Result<(), String> {
    let world = the_world(world).await?;
    let claim = HypotheticalClaim::Relation {
        relation: RelationType::DependsOn,
        from: world.fixture.artefact.clone(),
        to: world.fixture.lib_v3.clone(),
        replaces: None,
    };

    // Dos hipoteticas iguales.
    let fork = the_fork(&world.fixture, world.generation)
        .with(Hypothetical {
            tag: the_tag(),
            claim: claim.clone(),
        })
        .with(Hypothetical {
            tag: the_tag(),
            claim: claim.clone(),
        });
    let result = the_service(&world, world.relations.clone().clone())
        .simulate_result(&fork, the_now())
        .await
        .map_err(|error| error.to_string())?;
    if !result
        .conflicts
        .iter()
        .any(|conflict| conflict.kind == ConflictKind::Duplicate)
    {
        return Err(format!(
            "the duplicated hypothesis is not declared: {:?}",
            result.conflicts
        ));
    }

    // Y una hipotetica que repite lo que ya esta en el mundo.
    let fork = the_fork(&world.fixture, world.generation).with(Hypothetical {
        tag: the_tag(),
        claim: HypotheticalClaim::Relation {
            relation: RelationType::DependsOn,
            from: world.fixture.artefact.clone(),
            to: world.fixture.lib_v1.clone(),
            replaces: None,
        },
    });
    let result = the_service(&world, world.relations.clone().clone())
        .simulate_result(&fork, the_now())
        .await
        .map_err(|error| error.to_string())?;
    if !result
        .conflicts
        .iter()
        .any(|conflict| conflict.kind == ConflictKind::Duplicate)
    {
        return Err(format!(
            "the hypothesis that repeats the world is not declared: {:?}",
            result.conflicts
        ));
    }
    Ok(())
}

// ───────────────────────── la adicion y la eliminacion ─────────────────────────

/// La adicion y la eliminacion aparecen en el diff.
async fn the_addition_and_the_elimination_are_in_the_diff(
    world: &dyn ConformanceWorld,
) -> ScenarioResult {
    const NAME: &str = "the addition and the elimination are in the diff";
    match the_addition_scenario(world).await {
        Ok(()) => passed(NAME),
        Err(reason) => failed(NAME, reason),
    }
}

/// El escenario de la adicion: una dependencia que se sustituye deja una
/// relacion que aparece y otra que desaparece.
async fn the_addition_scenario(world: &dyn ConformanceWorld) -> Result<(), String> {
    let world = the_world(world).await?;
    let fork = the_fork(&world.fixture, world.generation).with(Hypothetical {
        tag: the_tag(),
        claim: HypotheticalClaim::Relation {
            relation: RelationType::DependsOn,
            from: world.fixture.artefact.clone(),
            to: world.fixture.lib_v3.clone(),
            replaces: Some(RelationFact::new(
                RelationType::DependsOn,
                world.fixture.artefact.clone(),
                world.fixture.lib_v1.clone(),
            )),
        },
    });

    let result = the_service(&world, world.relations.clone())
        .simulate_result(&fork, the_now())
        .await
        .map_err(|error| error.to_string())?;

    if result.structural.appeared.len() != 1
        || result.structural.appeared[0].to != world.fixture.lib_v3
    {
        return Err(format!(
            "the relation that appears is not the new one: {:?}",
            result.structural.appeared
        ));
    }
    if result.structural.disappeared.len() != 1
        || result.structural.disappeared[0].to != world.fixture.lib_v1
    {
        return Err(format!(
            "the relation that disappears is not the replaced one: {:?}",
            result.structural.disappeared
        ));
    }
    Ok(())
}

// ───────────────────────── la base obsoleta ─────────────────────────

/// La base obsoleta no es un diff: es un error.
async fn the_stale_base_is_not_a_diff(world: &dyn ConformanceWorld) -> ScenarioResult {
    const NAME: &str = "the stale base is not a diff";
    match the_stale_base_scenario(world).await {
        Ok(()) => passed(NAME),
        Err(reason) => failed(NAME, reason),
    }
}

/// El escenario de la base obsoleta: un fork preparado contra una
/// generacion que ya no es la del mundo no devuelve un diff.
///
/// Un diff calculado sobre un mundo que nadie tiene no dice la verdad, y
/// se dice antes de devolverlo.
async fn the_stale_base_scenario(world: &dyn ConformanceWorld) -> Result<(), String> {
    let world = the_world(world).await?;
    let stale = if world.generation == 0 {
        1
    } else {
        world.generation - 1
    };
    let fork = the_fork(&world.fixture, stale);

    let error = the_service(&world, world.relations.clone())
        .simulate(&fork, the_now())
        .await
        .expect_err("a base that does not match the world does not give a diff");

    match error {
        SimulationError::StaleBase { ref source, .. } if source == "graph" => Ok(()),
        other => Err(format!(
            "the error is not a stale base of the graph: {other}"
        )),
    }
}
