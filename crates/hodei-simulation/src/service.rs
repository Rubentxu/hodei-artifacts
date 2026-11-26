//! The service that runs the simulation.
//!
//! The walk-through is that of a "what would happen if" question:
//!
//! 1. The **limits** of the simulation are checked, before doing
//!    anything: a simulation that does not fit is not run.
//! 2. The **base is pinned**: the account, the generation of the graph, the
//!    relations that will be read and the evidence consumed by the
//!    decision, with their seals.
//! 3. The real state of the graph is read, **read-only**.
//! 4. The **structural diff** is computed: the relations of the real state
//!    against those of the fork.
//! 5. The **decisions** from before and after are computed, for the
//!    subjects that the simulation touches, and only those that change are kept.
//! 6. The **base is checked again**: if the world has moved, the
//!    simulation declares itself stale instead of returning a diff on a
//!    world that no longer exists.
//!
//! And at no point is anything written. The real graph is checked at the end:
//! what was read is read again, compared with what was evaluated and,
//! if the world moved, it is said with an error that says so. That is not a
//! promise but a check, and it has its honest limit: what is read
//! after the last seal is left out, because a check
//! without a transaction has a window and it cannot be closed.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use hodei_evidence::domain::read_set::ReadSet;

use crate::base::{SimulationBase, the_evidence_fingerprint, the_seal_of};
use crate::diff::{DecisionChange, SimulationDiff, StructuralDiff};
use crate::fork::SimulationFork;
use crate::hypothetical::RelationFact;
use crate::ports::{DecisionEvaluator, SimulationGraph};
use crate::result::SimulationResult;

/// The simulation done: the diff and what was read.
///
/// What was read does not go in the diff --the diff speaks of what appears and
/// of what disappears-- but it does go in the result, which separates the
/// observed from the hypothetical.
struct TheSimulation {
    /// The diff of the simulation.
    diff: SimulationDiff,
    /// The relations that were read from the real graph.
    observed: Vec<RelationFact>,
    /// How many relations the account has: what the coverage says.
    relations_in_the_account: usize,
}

/// Why it could not be simulated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimulationError {
    /// The simulation is bigger than what is allowed.
    TooLarge(String),
    /// The simulation has lived longer than what is allowed.
    Expired(String),
    /// The real state could not be read.
    Unavailable(String),
    /// The simulation asks for what it cannot: subjects of another account or an
    /// empty scope that would give a misleading conclusion.
    NotAllowed(String),
    /// A READ that the decision needs failed. It is not replaced by
    /// an empty collection or by a result: without what has to be read,
    /// the decision is not taken.
    ReadFailed(String),
    /// The base moved while it was being evaluated: the world against which it
    /// was read is no longer the world of now.
    ///
    /// It is not a failure of the simulation: it is its answer. A diff
    /// computed on a world that nobody has is a diff that does not tell
    /// the truth, and that is said instead of returning it.
    StaleBase {
        /// What moved: the generation of the graph, the relations read,
        /// the number of relations or the evidence.
        source: String,
        /// What was expected.
        expected: String,
        /// What was found.
        found: String,
    },
    /// The base does not say against which version of the world it was evaluated.
    ///
    /// Without a declared version there is nothing to check: a base without a
    /// version is not stale, it is UNCHECKABLE, and an
    /// uncheckable base cannot support an answer. It is said instead of
    /// simulating on a world that nobody has said what it was.
    NoVersion(String),
}

impl std::fmt::Display for SimulationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge(message) => write!(formatter, "the simulation is too large: {message}"),
            Self::Expired(message) => write!(formatter, "the simulation expired: {message}"),
            Self::Unavailable(message) => {
                write!(formatter, "the state could not be read: {message}")
            }
            Self::NotAllowed(message) => {
                write!(formatter, "the simulation is not allowed: {message}")
            }
            Self::ReadFailed(message) => {
                write!(
                    formatter,
                    "a read that the decision needs failed: {message}"
                )
            }
            Self::StaleBase {
                source,
                expected,
                found,
            } => write!(
                formatter,
                "the base of the simulation is stale: {source} was {expected} and it is now {found}"
            ),
            Self::NoVersion(message) => write!(
                formatter,
                "the base of the simulation declares no version: {message}"
            ),
        }
    }
}

impl std::error::Error for SimulationError {}

/// The simulation service.
pub struct SimulationService {
    graph: Arc<dyn SimulationGraph>,
    decisions: Arc<dyn DecisionEvaluator>,
}

impl SimulationService {
    /// The service over a graph and an evaluator.
    pub fn new(graph: Arc<dyn SimulationGraph>, decisions: Arc<dyn DecisionEvaluator>) -> Self {
        Self { graph, decisions }
    }

    /// The relations of the real graph.
    ///
    /// It is exposed by the service so that it can be checked that the graph has not
    /// changed: a simulation that leaves no trace is what is wanted,
    /// and "leaves no trace" is checked, not promised.
    pub async fn graph_relations(&self) -> Result<Vec<RelationFact>, SimulationError> {
        let mut response = self
            .graph
            .all_relations()
            .await
            .map_err(SimulationError::Unavailable)?;
        response.sort_by_key(|relation| relation.identity());
        Ok(response)
    }

    /// Simulates and returns the diff.
    ///
    /// `now` is the clock of the simulation: the same instant for
    /// the whole walk-through, and the one that reaches the decision core. The wall
    /// clock is not read anywhere, and for that reason the same base and
    /// the same facts give the same diff.
    pub async fn simulate(
        &self,
        fork: &SimulationFork,
        now: DateTime<Utc>,
    ) -> Result<SimulationDiff, SimulationError> {
        Ok(self.the_simulation(fork, now).await?.diff)
    }

    /// Simulates and returns the **result**: the versioned contract.
    ///
    /// The result is the diff plus what the diff does not say: what has been
    /// observed, what has been hypothesized, which policies intervened,
    /// what has been covered and what conflicts the scenario has. And it travels
    /// without policy objects: only with their seals.
    pub async fn simulate_result(
        &self,
        fork: &SimulationFork,
        now: DateTime<Utc>,
    ) -> Result<SimulationResult, SimulationError> {
        let simulation = self.the_simulation(fork, now).await?;
        Ok(SimulationResult::of(
            &simulation.diff,
            fork,
            &simulation.observed,
            simulation.relations_in_the_account,
        ))
    }

    /// The simulation done: the diff and what was read.
    ///
    /// The diff does not carry the observed relations --only those that appear
    /// and those that disappear-- and the result does carry them: that is why the
    /// walk-through keeps both things.
    async fn the_simulation(
        &self,
        fork: &SimulationFork,
        now: DateTime<Utc>,
    ) -> Result<TheSimulation, SimulationError> {
        // 1. The limits, before anything else.
        fork.check_the_limits(now).map_err(|error| match error {
            crate::SimulationError::TooLarge(message) => SimulationError::TooLarge(message),
            crate::SimulationError::Expired(message) => SimulationError::Expired(message),
            other => other,
        })?;

        // 2. The account: the base of ANOTHER account is not a base. The graph
        // belongs to one account and the fork declares another, and a simulation cannot
        // be of both at once.
        let account = self.graph.account();
        if account != fork.base.account {
            return Err(SimulationError::NotAllowed(format!(
                "the base is about account {} and the graph is about account {account}",
                fork.base.account
            )));
        }

        // 3. The generation: the fork has to say against which version
        // it was prepared, and that version has to be that of now. A fork
        // without a version cannot be checked, and a base that cannot be
        // checked does not support an answer.
        let generation = self
            .graph
            .generation()
            .await
            .map_err(SimulationError::Unavailable)?;
        let declared = fork.base.generation.ok_or_else(|| {
            SimulationError::NoVersion(
                "the fork does not say which generation of the graph it was prepared against"
                    .to_string(),
            )
        })?;
        if declared != generation {
            return Err(SimulationError::StaleBase {
                source: "graph".to_string(),
                expected: declared.to_string(),
                found: generation.to_string(),
            });
        }

        // 4. The number of relations from before: isolation is checked
        // against it.
        let relations_before = self
            .graph
            .relation_count()
            .await
            .map_err(SimulationError::Unavailable)?;

        // 5. The subjects, in canonical order. The decision diff has
        // to speak of the same subjects in the same order as the
        // structural diff, and the declaration order of a fork is not
        // an order: it is the order in which someone happened to think of
        // declaring them.
        let subjects = {
            let mut subjects = fork.affected_subjects();
            subjects.sort_by_key(|subject| subject.to_string());
            subjects
        };

        // 6. The structural diff.
        let real = self.the_relations_of(&subjects).await?;

        // And the base, with what has been read: the manifest of what was read and
        // the seal of its content.
        let mut base = SimulationBase::new(account, now)
            .with_generation("graph", generation)
            .with_relations(&real)
            .with_policy(&fork.base_policy);
        // And the policy that IS in force in the fork, which is the one that decides
        // the "after". Sealing only the one from before would leave the manifest
        // half of what produced the decision.
        base = base.with_policy(fork.effective_policy());
        // And the ledger position that declares the starting point, if
        // known. It is a DECLARED position, not a verified one: the
        // simulation has no ledger port against which to
        // check it, and a position that cannot be verified is not
        // presented as verified.
        base.declared_ledger_position = fork.base.ledger_position;
        let seal_before = base.seal.clone();

        let forked = fork.relations_with(&real);
        let mut structural = StructuralDiff::between(&real, &forked);

        // The affected subjects are those of the fork and those of the diff: a
        // simulation that only changes the policy changes no
        // relation, and even so there are subjects it affects.
        for subject in &subjects {
            if !structural.affected_subjects.contains(subject) {
                structural.affected_subjects.push(subject.clone());
            }
        }
        // The HRN has no natural order, and the stable order is by its
        // shape: the diff does not depend on the order in which it was read.
        structural
            .affected_subjects
            .sort_by_key(|subject| subject.to_string());

        // And the deployments of the subjects that the change reaches.
        structural.affected_deployments = forked
            .iter()
            .filter(|relation| {
                relation.relation == hodei_evidence::domain::graph::RelationType::DeployedAs
                    && structural.affected_subjects.contains(&relation.from)
            })
            .map(|relation| relation.to.clone())
            .collect();
        // Without repeats and in canonical order: a deployment that appears twice
        // says no more than one that appears once, and the order cannot
        // depend on the order in which the graph gave the relations.
        structural
            .affected_deployments
            .sort_by_key(|deployment| deployment.to_string());
        structural.affected_deployments.dedup();

        // 7. The decisions: those from before with the policy from before, and those
        // from after with that of the fork. The instant is that of the
        // simulation, not that of the wall clock.
        //
        // And each decision brings what it read: that read enters the
        // base, because a base that does not say what evidence was consumed does not
        // say on which world the answer was computed.
        let mut decisions: Vec<DecisionChange> = Vec::new();
        for subject in &subjects {
            let before = self
                .decisions
                .decide(subject, &fork.base_policy, &[], now)
                .await
                .map_err(SimulationError::ReadFailed)?;
            let after = self
                .decisions
                .decide(subject, fork.effective_policy(), &fork.facts, now)
                .await
                .map_err(SimulationError::ReadFailed)?;
            base = base
                .with_reads(&before.read_set)
                .with_reads(&after.read_set);
            let change = DecisionChange {
                subject: subject.clone(),
                before: before.outcome,
                after: after.outcome,
                policy_version_before: before.policy.version.clone(),
                policy_version_after: after.policy.version.clone(),
            };
            if change.is_a_change() {
                decisions.push(change);
            }
        }

        // 8. The final check: the world has not moved.
        //
        // The generation says whether the graph's history has advanced, and the
        // content seal says whether what was read is the same. Both are
        // needed: the number of relations does not replace them, because
        // a relation replaced by another leaves the number the same.
        //
        // And the "after" state is taken in INVERSE order to the "before":
        // first the coarse markers and at the end the fine ones.
        // The seal goes last because it is the one that distinguishes the
        // smallest change --a relation whose evidence changes-- and because
        // it does not depend on the change going through the delta ledger. What
        // is read after the seal is outside the check, and
        // is a window that cannot be closed without a transactional
        // read: it is said here so that no more is promised than
        // is delivered.
        let generation_after = self
            .graph
            .generation()
            .await
            .map_err(SimulationError::Unavailable)?;
        if base.is_the_generation_stale(generation_after) {
            return Err(SimulationError::StaleBase {
                source: "graph".to_string(),
                expected: generation.to_string(),
                found: generation_after.to_string(),
            });
        }

        let relations_after = self
            .graph
            .relation_count()
            .await
            .map_err(SimulationError::Unavailable)?;
        if relations_after != relations_before {
            return Err(SimulationError::StaleBase {
                source: "relation count".to_string(),
                expected: relations_before.to_string(),
                found: relations_after.to_string(),
            });
        }

        let real_after = self.the_relations_of(&subjects).await?;
        let seal_after = the_seal_of(&real_after);
        if base.is_the_content_stale(&seal_after) {
            return Err(SimulationError::StaleBase {
                source: "relations".to_string(),
                expected: seal_before,
                found: seal_after,
            });
        }

        // And the evidence: what the decision consumes. It does not touch the graph,
        // so the three guards above do not see it, and without this
        // check evidence that enters in the middle of the
        // simulation would be attributed to the hypothetical fact.
        let mut reads_after = ReadSet::empty();
        for subject in &subjects {
            let reads = self
                .decisions
                .the_reads_of(subject)
                .await
                .map_err(SimulationError::ReadFailed)?;
            for stamp in &reads.stamps {
                reads_after.read(stamp.clone());
            }
        }
        if base.is_the_evidence_stale(&reads_after) {
            return Err(SimulationError::StaleBase {
                source: "evidence".to_string(),
                expected: the_evidence_fingerprint(&base.read_set),
                found: the_evidence_fingerprint(&reads_after),
            });
        }

        Ok(TheSimulation {
            diff: SimulationDiff {
                structural,
                decisions,
                base,
            },
            observed: real,
            relations_in_the_account: relations_before,
        })
    }

    /// The relations of the subjects, in canonical order.
    async fn the_relations_of(
        &self,
        subjects: &[kernel::Hrn],
    ) -> Result<Vec<RelationFact>, SimulationError> {
        let mut relations: Vec<RelationFact> = Vec::new();
        for subject in subjects {
            for relation in self
                .graph
                .relations_of(subject)
                .await
                .map_err(SimulationError::Unavailable)?
            {
                match relations
                    .iter_mut()
                    .find(|one| one.is_the_same_as(&relation))
                {
                    // The same relation with different support: the one with the
                    // SMALLEST evidence wins. The result cannot
                    // depend on the order in which the store returned the
                    // rows, because a seal that depends on order is not
                    // a seal of the world.
                    Some(known) => {
                        if the_evidence_of(&relation) < the_evidence_of(known) {
                            *known = relation;
                        }
                    }
                    None => relations.push(relation),
                }
            }
        }
        relations.sort_by_key(|relation| relation.identity());
        Ok(relations)
    }
}

/// The evidence that supports a relation, as text.
///
/// It is what is compared when two relations have the same identity and
/// different support.
fn the_evidence_of(relation: &RelationFact) -> String {
    relation.evidence.clone().unwrap_or_default()
}
