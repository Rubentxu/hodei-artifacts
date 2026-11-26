//! The conformance of the simulation: the same scenarios for any
//! adapter.
//!
//! The suite lives in the domain and not in the store, and so it can
//! run against an in-memory graph and against the real SurrealDB
//! graph: what changes is where the relations come from, and what
//! is checked is always the same.
//!
//! # The fixture
//!
//! The scenarios do not invent the world: they carry a **fixture** that
//! the adapter leaves in its world. The fixture is the same for all --an
//! artifact that depends on two components-- and so the results
//! of two adapters are comparable: if the SurrealDB adapter
//! said something different from the in-memory one, the fault is the adapter's and not
//! the scenario's.
//!
//! # The scenarios
//!
//! - The **order** of the reads does not change the result.
//! - An overlay **cycle** is declared, not hidden.
//! - A **duplicate** is declared.
//! - An **addition and a removal** appear in the diff.
//! - A **stale base** is not a diff: it is an error.

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

/// The evidence that supports the first relation of the fixture.
pub const THE_FIRST_EVIDENCE: &str = "ev-conformance-1";
/// The evidence that supports the second relation of the fixture.
pub const THE_SECOND_EVIDENCE: &str = "ev-conformance-2";

/// The fixture: the minimal world, with shape.
///
/// The chain is `artifact -> lib-v1 -> lib-v2`: one subject and two
/// dependencies. It is the smallest world that has an addition, a
/// removal and a possible cycle, and so it serves for all three.
#[derive(Debug, Clone)]
pub struct Fixture {
    /// The account of the world.
    pub account: String,
    /// The tenant of the graph: the HRN of the account.
    pub tenant: Hrn,
    /// The generation of the world.
    pub generation: u64,
    /// The nodes of the world.
    pub nodes: Vec<FixtureNode>,
    /// The relations of the world.
    pub relations: Vec<RelationFact>,
    /// The subject: the artifact that depends.
    pub artefact: Hrn,
    /// The first component.
    pub lib_v1: Hrn,
    /// The second component.
    pub lib_v2: Hrn,
    /// The component that is climbed to.
    pub lib_v3: Hrn,
}

/// A node of the fixture.
#[derive(Debug, Clone)]
pub struct FixtureNode {
    /// The name of the node.
    pub hrn: Hrn,
    /// The kind of the node.
    pub kind: NodeKind,
}

impl Fixture {
    /// The fixture of an account, over its tenant.
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

/// The node of the fixture, with the name of the tenant.
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

/// The world that the conformance needs.
///
/// The world is what changes from one adapter to another: an in-memory graph
/// or the real SurrealDB graph. What is checked is always the
/// same, and so the scenarios live here and not in the adapter.
#[async_trait]
pub trait ConformanceWorld: Send + Sync {
    /// The account of the world.
    fn account(&self) -> String;
    /// The tenant of the graph: the HRN of the account.
    fn tenant(&self) -> Hrn;
    /// Prepares the fixture: leaves it in the world.
    async fn prepare(&self, fixture: &Fixture) -> Result<(), String>;
    /// The relations of the world, in the order the adapter gives them.
    async fn relations(&self) -> Result<Vec<RelationFact>, String>;
    /// The generation of the world.
    async fn generation(&self) -> Result<u64, String>;
    /// The decision evaluator of the world.
    fn decisions(&self) -> Arc<dyn DecisionEvaluator>;
}

/// The result of a scenario.
#[derive(Debug, Clone)]
pub struct ScenarioResult {
    /// The name of the scenario.
    pub name: &'static str,
    /// Whether the scenario passed.
    pub passed: bool,
    /// Why it did not pass, if it did not pass.
    pub reason: Option<String>,
}

/// The conformance report.
#[derive(Debug, Clone)]
pub struct ConformanceReport {
    /// The scenarios, in the order they were run.
    pub scenarios: Vec<ScenarioResult>,
}

impl ConformanceReport {
    /// Whether all the scenarios passed.
    pub fn passed(&self) -> bool {
        self.scenarios.iter().all(|scenario| scenario.passed)
    }

    /// The scenarios that did not pass.
    pub fn failures(&self) -> Vec<&ScenarioResult> {
        self.scenarios
            .iter()
            .filter(|scenario| !scenario.passed)
            .collect()
    }
}

/// The conformance suite: the scenarios that every simulation has to
/// meet, whichever adapter reads the world.
pub async fn run_suite(world: &dyn ConformanceWorld) -> ConformanceReport {
    let mut scenarios: Vec<ScenarioResult> = Vec::new();
    scenarios.push(the_order_of_the_reads_does_not_change_the_result(world).await);
    scenarios.push(the_cycle_of_the_overlay_is_declared(world).await);
    scenarios.push(the_duplicate_is_declared(world).await);
    scenarios.push(the_addition_and_the_elimination_are_in_the_diff(world).await);
    scenarios.push(the_stale_base_is_not_a_diff(world).await);
    ConformanceReport { scenarios }
}

/// The graph of the suite: the relations the world gives, fixed.
///
/// It is the same graph for both adapters: what changes is where
/// the relations come from, and the suite receives them from the world.
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

/// The prepared world: the fixture, the relations and the generation.
struct TheWorld {
    fixture: Fixture,
    relations: Vec<RelationFact>,
    generation: u64,
    decisions: Arc<dyn DecisionEvaluator>,
}

/// The instant of the conformance: a single one for all the scenarios.
///
/// The clock is not read from the wall: two simulations with the same base and
/// the same clock give the same result, and a conformance that depends on
/// the clock cannot be compared.
fn the_now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-01-01T12:00:00Z")
        .expect("un instante fijo")
        .with_timezone(&Utc)
}

/// The conformance policy: the one that forbids the established risk.
///
/// It is a real policy, with its requirements, and so the result
/// that is declared has to travel without it: only with its seal.
fn the_policy(version: &str) -> TrustPolicy {
    TrustPolicy::new(
        format!("policy-{version}"),
        version,
        vec![Requirement::denying(RequirementKind::RiskEstablished)],
    )
}

/// The conformance fork.
///
/// It declares the subjects of the fixture: a simulation that declares no
/// subjects is a simulation that cannot be made, and the conformance is not
/// going to check a simulation that does not exist.
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

/// The conformance label.
fn the_tag() -> SimulationTag {
    SimulationTag::new("sim-conformance", the_now())
}

/// The service of the suite, over the given relations.
fn the_service(world: &TheWorld, relations: Vec<RelationFact>) -> SimulationService {
    let graph: Arc<dyn SimulationGraph> = Arc::new(StaticGraph {
        relations,
        generation: world.generation,
        account: world.fixture.account.clone(),
    });
    SimulationService::new(graph, world.decisions.clone())
}

/// The prepared world: the fixture is left in the world and read.
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

/// The result of a scenario that passed.
fn passed(name: &'static str) -> ScenarioResult {
    ScenarioResult {
        name,
        passed: true,
        reason: None,
    }
}

/// The result of a scenario that did not pass.
fn failed(name: &'static str, reason: impl Into<String>) -> ScenarioResult {
    ScenarioResult {
        name,
        passed: false,
        reason: Some(reason.into()),
    }
}

// ───────────────────────── the order ─────────────────────────

/// The order of the reads does not change the result.
async fn the_order_of_the_reads_does_not_change_the_result(
    world: &dyn ConformanceWorld,
) -> ScenarioResult {
    const NAME: &str = "the order of the reads does not change the result";
    match the_order_scenario(world).await {
        Ok(()) => passed(NAME),
        Err(reason) => failed(NAME, reason),
    }
}

/// The order scenario: the same world read backwards gives the SAME
/// canonical result.
///
/// And the canonical result is the one compared: the structural diff and
/// the decisions already come out sorted, and the manifest is compared by its
/// canonical form and not by the order in which it was read.
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

// ───────────────────────── the cycle ─────────────────────────

/// The overlay cycle is declared.
async fn the_cycle_of_the_overlay_is_declared(world: &dyn ConformanceWorld) -> ScenarioResult {
    const NAME: &str = "the cycle of the overlay is declared";
    match the_cycle_scenario(world).await {
        Ok(()) => passed(NAME),
        Err(reason) => failed(NAME, reason),
    }
}

/// The cycle scenario: a hypothetical that closes the loop is
/// declared as a conflict.
///
/// The overlay of the fixture is `artifact -> lib-v1 -> lib-v2`, and the
/// hypothetical makes `lib-v2` go back to the artifact.
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

// ───────────────────────── the duplicate ─────────────────────────

/// The duplicate is declared.
async fn the_duplicate_is_declared(world: &dyn ConformanceWorld) -> ScenarioResult {
    const NAME: &str = "the duplicate is declared";
    match the_duplicate_scenario(world).await {
        Ok(()) => passed(NAME),
        Err(reason) => failed(NAME, reason),
    }
}

/// The duplicate scenario: two hypotheticals that say the same thing, and a
/// hypothetical that repeats what is already in the world.
///
/// Both are declared: a diff that keeps them without saying anything looks like a
/// diff that did not look.
async fn the_duplicate_scenario(world: &dyn ConformanceWorld) -> Result<(), String> {
    let world = the_world(world).await?;
    let claim = HypotheticalClaim::Relation {
        relation: RelationType::DependsOn,
        from: world.fixture.artefact.clone(),
        to: world.fixture.lib_v3.clone(),
        replaces: None,
    };

    // Two equal hypotheticals.
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

    // And a hypothetical that repeats what is already in the world.
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

// ───────────────────────── the addition and the removal ─────────────────────────

/// The addition and the removal appear in the diff.
async fn the_addition_and_the_elimination_are_in_the_diff(
    world: &dyn ConformanceWorld,
) -> ScenarioResult {
    const NAME: &str = "the addition and the elimination are in the diff";
    match the_addition_scenario(world).await {
        Ok(()) => passed(NAME),
        Err(reason) => failed(NAME, reason),
    }
}

/// The addition scenario: a dependency that is replaced leaves one
/// relation that appears and another that disappears.
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

// ───────────────────────── the stale base ─────────────────────────

/// The stale base is not a diff: it is an error.
async fn the_stale_base_is_not_a_diff(world: &dyn ConformanceWorld) -> ScenarioResult {
    const NAME: &str = "the stale base is not a diff";
    match the_stale_base_scenario(world).await {
        Ok(()) => passed(NAME),
        Err(reason) => failed(NAME, reason),
    }
}

/// The stale base scenario: a fork prepared against a
/// generation that is no longer the world's does not return a diff.
///
/// A diff computed over a world that nobody has does not tell the truth, and
/// that is said before returning it.
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
