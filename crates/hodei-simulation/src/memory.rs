//! The in-memory fixture: the world that conformance needs without a
//! database.
//!
//! The simulation is read against a graph, and a real graph is a
//! real graph. To check that the simulation behaves the same
//! with any adapter an adapter that is not the real one is
//! needed: this is it, and it holds the same relations with the same rules,
//! but kept in memory.
//!
//! It lives in the domain and not in the store because it is what makes the
//! conformance suite **portable**: the domain can run it without a
//! database, and a real adapter runs it with the same
//! accounts.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_evidence::domain::read_set::ReadSet;
use hodei_trust_policy::RequirementKind;
use hodei_trust_policy::TrustPolicy;
use hodei_trust_policy::decision::Outcome;
use kernel::Hrn;

use crate::conformance::{ConformanceWorld, Fixture};
use crate::diff::TheDecision;
use crate::hypothetical::{Hypothetical, HypotheticalClaim, RelationFact};
use crate::ports::{DecisionEvaluator, SimulationGraph};

/// The in-memory graph.
///
/// It holds the relations and their generation, and no more. It has no write method
/// on the simulation port, and that is why simulating against
/// it cannot change the world: the only one that can move it is whoever
/// prepares it.
pub struct InMemoryGraph {
    relations: Mutex<Vec<RelationFact>>,
    generation: Mutex<u64>,
    account: String,
}

impl InMemoryGraph {
    /// An in-memory graph, with its account.
    pub fn new(account: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            relations: Mutex::new(Vec::new()),
            generation: Mutex::new(1),
            account: account.into(),
        })
    }

    /// The relations it has right now.
    pub fn the_relations(&self) -> Vec<RelationFact> {
        self.relations
            .lock()
            .expect("el grafo no esta envenenado")
            .clone()
    }

    /// The generation it has right now.
    pub fn the_generation(&self) -> u64 {
        *self
            .generation
            .lock()
            .expect("la generacion no esta envenenada")
    }

    /// Leaves the world as the fixture says.
    pub fn set_the_world(&self, relations: Vec<RelationFact>, generation: u64) {
        *self.relations.lock().expect("el grafo no esta envenenado") = relations;
        *self
            .generation
            .lock()
            .expect("la generacion no esta envenenada") = generation;
    }
}

#[async_trait]
impl SimulationGraph for InMemoryGraph {
    async fn relations_of(&self, subject: &Hrn) -> Result<Vec<RelationFact>, String> {
        Ok(self
            .the_relations()
            .into_iter()
            .filter(|relation| &relation.from == subject || &relation.to == subject)
            .collect())
    }

    async fn relation_count(&self) -> Result<usize, String> {
        Ok(self.the_relations().len())
    }

    async fn all_relations(&self) -> Result<Vec<RelationFact>, String> {
        Ok(self.the_relations())
    }

    async fn generation(&self) -> Result<u64, String> {
        Ok(self.the_generation())
    }

    fn account(&self) -> String {
        self.account.clone()
    }
}

/// The in-memory evaluator.
///
/// It is the simplest evaluator that can be written without lying: it says
/// DENY when the policy prohibits the established risk and the subject
/// has a critical and reachable vulnerability, and ALLOW otherwise.
/// What it does not know, it does not say.
pub struct InMemoryDecisions;

#[async_trait]
impl DecisionEvaluator for InMemoryDecisions {
    async fn decide(
        &self,
        subject: &Hrn,
        policy: &TrustPolicy,
        facts: &[Hypothetical],
        _now: DateTime<Utc>,
    ) -> Result<TheDecision, String> {
        let exposed = facts.iter().any(|fact| {
            matches!(
                &fact.claim,
                HypotheticalClaim::Risk {
                    subject: who,
                    critical,
                    reachable,
                    ..
                } if who == subject && *critical && *reachable
            )
        });
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

/// The in-memory conformance world.
///
/// It is the suite's adapter against a graph that is not a real
/// graph: the same relations, the same rules and no disk.
pub struct InMemoryConformance {
    graph: Arc<InMemoryGraph>,
    tenant: Hrn,
    decisions: Arc<dyn DecisionEvaluator>,
}

impl InMemoryConformance {
    /// An account's in-memory world.
    pub fn new(account: impl Into<String>, tenant: Hrn) -> Self {
        let account = account.into();
        Self {
            graph: InMemoryGraph::new(account),
            tenant,
            decisions: Arc::new(InMemoryDecisions),
        }
    }

    /// The in-memory graph, in case it needs to be inspected.
    pub fn the_graph(&self) -> Arc<InMemoryGraph> {
        self.graph.clone()
    }
}

#[async_trait]
impl ConformanceWorld for InMemoryConformance {
    fn account(&self) -> String {
        SimulationGraph::account(&*self.graph)
    }

    fn tenant(&self) -> Hrn {
        self.tenant.clone()
    }

    async fn prepare(&self, fixture: &Fixture) -> Result<(), String> {
        self.graph
            .set_the_world(fixture.relations.clone(), fixture.generation);
        Ok(())
    }

    async fn relations(&self) -> Result<Vec<RelationFact>, String> {
        Ok(self.graph.the_relations())
    }

    async fn generation(&self) -> Result<u64, String> {
        Ok(self.graph.the_generation())
    }

    fn decisions(&self) -> Arc<dyn DecisionEvaluator> {
        self.decisions.clone()
    }
}
