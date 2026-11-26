//! The fork: an isolated state for asking "what would happen if".
//!
//! The fork **does not copy** the real state: it **overlays** it. The real state
//! is read and not touched, and the fork is the hypothetical facts and nothing else.
//! That is the decision of SPIKE-008, and its reason is that a copy costs
//! what the state costs (156 s of projection and 656.6 s of
//! rebuild at the reference scale) whereas an overlay
//! costs what the hypothetical facts cost, which are few.
//!
//! And the overlay is **isolated by construction**: the fork is a
//! VALUE. There is no write port that reaches it, and that is why
//! simulating cannot change the real graph.

use chrono::{DateTime, Utc};
use hodei_evidence::domain::graph::RelationType;
use hodei_trust_policy::TrustPolicy;
use kernel::Hrn;
use serde::{Deserialize, Serialize};

use crate::hypothetical::{Hypothetical, HypotheticalClaim, RelationFact};

/// The point of the real state the fork comes from.
///
/// The checkpoint says **where** to start from, and does not carry the state: the
/// state is read from the real store, which is what is wanted (the truth of
/// now, not a copy of the truth from a while ago).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    /// The checkpoint's identifier.
    pub id: String,
    /// The account it belongs to.
    pub account: String,
    /// The checkpoint's moment.
    pub at: DateTime<Utc>,
    /// The number of ledger events at that point, if known.
    ///
    /// It is the DECLARED position the fork starts from, and it travels in the
    /// base's manifest so the response says where it comes from.
    /// It is not verified against the ledger: the simulation has no ledger
    /// port, and a position that cannot be checked is not
    /// presented as checked.
    pub ledger_position: Option<u64>,
    /// The graph generation at that point, if known.
    ///
    /// It is the version the fork was PREPARED against, and the one that is
    /// checked when simulating: if the graph is no longer at that generation,
    /// the fork was made against a world that no longer exists, and the
    /// simulation is declared stale instead of answering for the world of
    /// now as if it were that of back then.
    ///
    /// A checkpoint WITHOUT a generation cannot be simulated: a base that does not
    /// say which version it is evaluated against cannot be checked.
    #[serde(default)]
    pub generation: Option<u64>,
}

impl Checkpoint {
    /// A checkpoint, prepared against a graph generation.
    ///
    /// The generation is MANDATORY: a fork that does not say which
    /// version of the world it was prepared against cannot be checked, and the
    /// simulation rejects it instead of answering for a world nobody
    /// has said which one was.
    pub fn new(
        id: impl Into<String>,
        account: impl Into<String>,
        at: DateTime<Utc>,
        ledger_position: Option<u64>,
        generation: u64,
    ) -> Self {
        Self {
            id: id.into(),
            account: account.into(),
            at,
            ledger_position,
            generation: Some(generation),
        }
    }

    /// The checkpoint prepared against another graph generation.
    pub fn at_generation(mut self, generation: u64) -> Self {
        self.generation = Some(generation);
        self
    }
}

/// The limits of a simulation.
///
/// A simulation is a resource, and a resource without a limit eats the
/// machine. The limits are **declared**: whoever simulates knows in
/// advance how much it may spend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulationLimits {
    /// How many hypothetical facts the simulation may have.
    pub max_hypotheticals: usize,
    /// How many subjects it may touch.
    pub max_affected_subjects: usize,
    /// How long the simulation lives.
    pub max_lifetime: chrono::Duration,
}

impl Default for SimulationLimits {
    fn default() -> Self {
        Self {
            max_hypotheticals: 100,
            max_affected_subjects: 1_000,
            max_lifetime: chrono::Duration::hours(24),
        }
    }
}

/// The isolated fork.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulationFork {
    /// The simulation's identifier.
    pub id: String,
    /// Where it starts from.
    pub base: Checkpoint,
    /// The hypothetical facts, overlaid on the real state.
    pub facts: Vec<Hypothetical>,
    /// The DECLARED subjects the simulation touches.
    ///
    /// A simulation that only changes the policy brings no facts, and the
    /// subjects are not inferred from what is not there: they are DECLARED, and whoever
    /// declares answers for being able to simulate them.
    #[serde(default)]
    pub subjects: Vec<Hrn>,
    /// The policy that would be in force, if the simulation changes it.
    pub policy: Option<TrustPolicy>,
    /// The policy in force now, in the real state.
    ///
    /// It is the one that decides "before", and without it the decision diff would
    /// have nothing to compare against.
    pub base_policy: TrustPolicy,
    /// The limits.
    pub limits: SimulationLimits,
    /// When it was created.
    pub created_at: DateTime<Utc>,
}

impl SimulationFork {
    /// An empty fork, without hypothetical facts.
    pub fn new(
        id: impl Into<String>,
        base: Checkpoint,
        base_policy: TrustPolicy,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: id.into(),
            base,
            facts: Vec::new(),
            subjects: Vec::new(),
            policy: None,
            base_policy,
            limits: SimulationLimits::default(),
            created_at,
        }
    }

    /// Declares the subjects the simulation touches.
    ///
    /// It is the EXPLICIT selection: the subjects that are simulated are those
    /// declared plus those brought by the facts, and a simulation that
    /// only changes the policy needs declared subjects, because nothing can be inferred from
    /// facts that are not there.
    pub fn with_subjects(mut self, subjects: Vec<Hrn>) -> Self {
        for subject in subjects {
            if !self.subjects.contains(&subject) {
                self.subjects.push(subject);
            }
        }
        self
    }

    /// The policy that would be in force in the fork.
    ///
    /// It is the fork's if the simulation changes the policy, and the real
    /// state's if not: a simulation that does not touch the policy is evaluated with
    /// the current policy.
    pub fn effective_policy(&self) -> &TrustPolicy {
        self.policy.as_ref().unwrap_or(&self.base_policy)
    }

    /// Overlays a hypothetical fact.
    pub fn with(mut self, fact: Hypothetical) -> Self {
        self.facts.push(fact);
        self
    }

    /// Overlays a hypothetical policy.
    pub fn with_policy(mut self, policy: TrustPolicy) -> Self {
        self.policy = Some(policy);
        self
    }

    /// The subjects the simulation touches.
    ///
    /// They are those whose decision may change, and therefore those the
    /// decision diff has to look at.
    pub fn affected_subjects(&self) -> Vec<Hrn> {
        let mut subjects: Vec<Hrn> = self.subjects.clone();
        for fact in &self.facts {
            for subject in fact.claim.subjects() {
                if !subjects.contains(&subject) {
                    subjects.push(subject);
                }
            }
        }
        subjects
    }

    /// The relations the fork adds to the real state.
    pub fn added_relations(&self) -> Vec<RelationFact> {
        self.facts
            .iter()
            .filter_map(|fact| match &fact.claim {
                HypotheticalClaim::Relation {
                    relation, from, to, ..
                } => Some(RelationFact::new(
                    relation.clone(),
                    from.clone(),
                    to.clone(),
                )),
                _ => None,
            })
            .collect()
    }

    /// The relations the fork makes disappear from the real state.
    pub fn removed_relations(&self) -> Vec<RelationFact> {
        self.facts
            .iter()
            .filter_map(|fact| match &fact.claim {
                HypotheticalClaim::Relation { replaces, .. } => replaces.clone(),
                _ => None,
            })
            .collect()
    }

    /// The relation that is queried for a subject.
    ///
    /// The fork's is the real state's PLUS the hypothetical ones MINUS those the
    /// fork replaces. And the substitution is checked by identity:
    /// two relations with the same identity are the same relation.
    pub fn relations_with(&self, real: &[RelationFact]) -> Vec<RelationFact> {
        let removed: Vec<String> = self
            .removed_relations()
            .iter()
            .map(RelationFact::identity)
            .collect();
        let mut relations: Vec<RelationFact> = real
            .iter()
            .filter(|relation| !removed.contains(&relation.identity()))
            .cloned()
            .collect();
        for added in self.added_relations() {
            if !relations
                .iter()
                .any(|one| one.identity() == added.identity())
            {
                relations.push(added);
            }
        }
        relations
    }

    /// Checks that the simulation fits within its limits.
    pub fn check_the_limits(&self, now: DateTime<Utc>) -> Result<(), crate::SimulationError> {
        if self.facts.len() > self.limits.max_hypotheticals {
            return Err(crate::SimulationError::TooLarge(format!(
                "the simulation has {} hypotheticals and the limit is {}",
                self.facts.len(),
                self.limits.max_hypotheticals
            )));
        }
        let subjects = self.affected_subjects();
        if subjects.len() > self.limits.max_affected_subjects {
            return Err(crate::SimulationError::TooLarge(format!(
                "the simulation touches {} subjects and the limit is {}",
                subjects.len(),
                self.limits.max_affected_subjects
            )));
        }
        if now > self.created_at + self.limits.max_lifetime {
            return Err(crate::SimulationError::Expired(format!(
                "the simulation was created at {} and lives {}",
                self.created_at, self.limits.max_lifetime
            )));
        }

        // Admission of the subjects: those of ANOTHER account do not enter, and the
        // comparison is by canonical component of the HRN, not by text.
        // A fork with NO subject is not a complete simulation without
        // those affected: it is a misleading conclusion, and it is not produced.
        let subjects = self.affected_subjects();
        if subjects.is_empty() {
            return Err(crate::SimulationError::NotAllowed(
                "the simulation declares no subjects: a fork that touches nobody is not a \
                 complete simulation, it is a misleading answer"
                    .to_string(),
            ));
        }
        for subject in &subjects {
            if subject.account_id() != self.base.account {
                return Err(crate::SimulationError::NotAllowed(format!(
                    "the subject {subject} belongs to account {} and the simulation is about {}",
                    subject.account_id(),
                    self.base.account
                )));
            }
        }
        Ok(())
    }

    /// A subject's relation, by its relation.
    pub fn relations_of(&self, subject: &Hrn, real: &[RelationFact]) -> Vec<RelationFact> {
        self.relations_with(real)
            .into_iter()
            .filter(|relation| &relation.from == subject || &relation.to == subject)
            .collect()
    }

    /// The relations of a type, from the fork.
    pub fn relations_of_kind(
        &self,
        real: &[RelationFact],
        relation: RelationType,
    ) -> Vec<RelationFact> {
        self.relations_with(real)
            .into_iter()
            .filter(|one| one.relation == relation)
            .collect()
    }
}
