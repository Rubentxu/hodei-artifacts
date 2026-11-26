//! The versioned contract of a simulation's result.
//!
//! The simulation diff says which relations appear, which relations
//! disappear and which decisions change. That is the **interior** of the
//! result. The result is the outer contract: what a consumer
//! --a console, a report, a second service-- receives, and that is why it
//! has to be versioned, separate the observed from the hypothetical, and
//! say what it has covered and what it has not.
//!
//! # The three separations
//!
//! The contract separates three things that are very easily confused:
//!
//! - **The observed**: the real state, as it was read. The relations
//!   that were in the graph, the policy that was in force and the
//!   evidence that was consumed to decide.
//! - **The hypothetical**: the scenarios that are overlaid. They are marked
//!   as hypothetical, and are **not published as graph facts**: a
//!   hypothetical confused with a fact is a lie formatted
//!   as data.
//! - **What has not been covered**: coverage and conflicts. A
//!   result that does not say what it has not looked at seems complete.
//!
//! # The contract travels without policy objects
//!
//! The FULL policy does not travel: it is a large object, shaped like a
//! policy, and a contract that carries it stops being portable. What
//! travels is the policy's **stamp** --its identifier and its
//! version--, which is what is needed to know which rules were
//! applied. The comparison and normalization helpers work with
//! stamps and never with policy objects.

use chrono::{DateTime, Utc};
use hodei_evidence::domain::graph::RelationType;
use hodei_evidence::domain::read_set::{ReadKind, ReadSet};
use hodei_trust_policy::TrustPolicy;
use kernel::Hrn;
use serde::{Deserialize, Serialize};

use crate::base::SimulationBase;
use crate::compare;
use crate::diff::{DecisionChange, SimulationDiff, StructuralDiff};
use crate::fork::SimulationFork;
use crate::hypothetical::{Hypothetical, HypotheticalClaim, RelationFact};

/// The contract version.
///
/// The contract changes, and when it changes it is declared with ANOTHER number: a
/// consumer reading version `1.0.0` knows what to expect from
/// it, and a consumer reading another version knows it has to look.
pub const SIMULATION_RESULT_VERSION: &str = "1.0.0";

/// A policy's stamp: its identifier and its version.
///
/// It is what travels instead of the policy: what is needed to know
/// which rules were applied, without dragging the whole object along.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PolicyStamp {
    /// The ruleset's identifier.
    pub id: String,
    /// The ruleset's version.
    pub version: String,
}

impl PolicyStamp {
    /// A policy's stamp.
    pub fn of(policy: &TrustPolicy) -> Self {
        Self {
            id: policy.id.clone(),
            version: policy.version.clone(),
        }
    }
}

/// An evidence's stamp: its identifier and the digest of its
/// content.
///
/// The digest is what allows checking the origin: if the evidence
/// changes content, the digest changes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EvidenceStamp {
    /// The evidence's identifier.
    pub id: String,
    /// The digest of its content.
    pub digest: String,
}

/// The version of a read source.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceStamp {
    /// The source: the graph, the evidence, the policy.
    pub source: String,
    /// The version that was read from that source.
    pub version: u64,
}

/// The base's stamp: which world the result was computed over.
///
/// It is what makes the result auditable: without it, an answer says
/// what would happen and does not say over which version of the truth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseStamp {
    /// The account the simulation belongs to.
    pub account: String,
    /// The instant it was evaluated.
    pub evaluated_at: DateTime<Utc>,
    /// The graph generation it was evaluated against.
    pub generation: Option<u64>,
    /// The stamp of the content read: the digest of the relations read.
    pub seal: String,
    /// The fingerprint of the read manifest.
    pub fingerprint: String,
    /// The ledger position that declares the starting point, if
    /// known. It is a DECLARED position, not a verified one.
    pub declared_ledger_position: Option<u64>,
    /// The version of each source that was read.
    pub sources: Vec<SourceStamp>,
}

/// The OBSERVED: the real state, as it was read.
///
/// What is here was in the world. What is not here, was not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedState {
    /// The observed relations, in canonical order and without duplicates.
    pub relations: Vec<RelationFact>,
    /// The policy that was in force.
    pub policy: PolicyStamp,
    /// The evidence that was consumed to decide.
    pub evidence: Vec<EvidenceStamp>,
}

/// A hypothetical scenario, in portable form.
///
/// It is a hypothetical and says so: it travels in its own field and is never
/// mixed with the observed relations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hypothesis {
    /// The simulation the hypothetical belongs to.
    pub simulation: String,
    /// The moment the hypothetical was created.
    pub created_at: DateTime<Utc>,
    /// What the hypothetical says.
    pub kind: HypothesisKind,
    /// The subjects the hypothetical touches.
    pub subjects: Vec<Hrn>,
}

/// What a hypothetical says.
///
/// And in none of the variants does the full policy travel: the
/// policy hypothetical carries its stamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HypothesisKind {
    /// A relation that would exist.
    Relation {
        /// Which relation it is.
        relation: RelationType,
        /// The node it starts from.
        from: Hrn,
        /// The node it arrives at.
        to: Hrn,
        /// The identity of the relation it REPLACES, if it replaces
        /// any.
        replaces: Option<String>,
    },
    /// A risk that there would be.
    Risk {
        /// The subject it reaches.
        subject: Hrn,
        /// The vulnerability.
        advisory: String,
        /// Whether it is critical.
        critical: bool,
        /// Whether it is reachable.
        reachable: bool,
    },
    /// A window in which a builder is not trusted.
    UntrustedWindow {
        /// The start of the window.
        from: DateTime<Utc>,
        /// The end of the window.
        to: DateTime<Utc>,
        /// The subjects the window reaches.
        subjects: Vec<Hrn>,
    },
    /// A policy that would be in force.
    Policy {
        /// The policy's stamp, not the policy.
        policy: PolicyStamp,
    },
}

impl Hypothesis {
    /// The canonical key: two hypotheticals with the same key are the same
    /// hypothetical.
    ///
    /// The key is what orders the result, and that is why it does not depend on the
    /// order in which the facts were declared.
    pub fn the_key(&self) -> String {
        match &self.kind {
            HypothesisKind::Relation {
                relation,
                from,
                to,
                replaces,
            } => format!(
                "relation|{}|{}|{}|{}",
                relation.as_str(),
                from,
                to,
                replaces.clone().unwrap_or_default()
            ),
            HypothesisKind::Risk {
                subject,
                advisory,
                critical,
                reachable,
            } => format!("risk|{subject}|{advisory}|{critical}|{reachable}"),
            HypothesisKind::UntrustedWindow { from, to, subjects } => format!(
                "window|{}|{}|{}",
                from.to_rfc3339(),
                to.to_rfc3339(),
                subjects
                    .iter()
                    .map(|subject| subject.to_string())
                    .collect::<Vec<String>>()
                    .join(",")
            ),
            HypothesisKind::Policy { policy } => format!("policy|{}|{}", policy.id, policy.version),
        }
    }
}

/// Coverage: what has been looked at and what has not.
///
/// A result that does not say what it has not seen seems complete, and a
/// result that seems complete gets used as if it were.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coverage {
    /// The DECLARED subjects: those someone named.
    pub declared_subjects: Vec<Hrn>,
    /// The EVALUATED subjects: the declared ones and those brought by the facts.
    pub evaluated_subjects: Vec<Hrn>,
    /// The relations that were read from the real graph.
    pub relations_read: usize,
    /// The account's relations that were NOT read.
    ///
    /// The simulation reads the relations of the subjects it touches, not those
    /// of the whole account. The number says what has not been seen.
    pub relations_not_read: usize,
    /// What was left out, with its reason.
    pub omitted: Vec<Omission>,
}

/// What was left out of coverage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Omission {
    /// The subject that was left out, if it is known which one it is.
    pub subject: Option<Hrn>,
    /// The reason.
    pub reason: String,
}

/// A conflict that the simulation declares.
///
/// A conflict is not a failure: it is a contradiction in the scenario that is
/// stated instead of being hidden. An overlay that closes a cycle and presents
/// itself as a normal overlay is an overlay nobody can read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    /// The conflict's kind.
    pub kind: ConflictKind,
    /// The detail, as text.
    pub detail: String,
    /// The subjects the conflict touches.
    pub subjects: Vec<Hrn>,
}

/// The conflict kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    /// The overlay closes a cycle.
    Cycle,
    /// The same relation is stated twice.
    Duplicate,
    /// Two hypotheticals contradict each other.
    Contradiction,
}

/// The versioned contract of a simulation's result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulationResult {
    /// The contract version.
    pub version: String,
    /// The simulation the result belongs to.
    pub simulation: String,
    /// The simulation's account.
    pub account: String,
    /// The base's stamp: which world it was computed over.
    pub base: BaseStamp,
    /// What was observed.
    pub observed: ObservedState,
    /// The hypothetical scenarios, in canonical order.
    pub hypotheses: Vec<Hypothesis>,
    /// The relations diff.
    pub structural: StructuralDiff,
    /// The decisions that change, in canonical order.
    pub decisions: Vec<DecisionChange>,
    /// The coverage.
    pub coverage: Coverage,
    /// The conflicts the scenario declares.
    pub conflicts: Vec<Conflict>,
}

impl SimulationResult {
    /// The result built from the diff, the fork and what was observed.
    ///
    /// The construction is **pure**: it does not read, does not write and does not decide. What
    /// it is given is what is already known, and what comes out is the contract.
    ///
    /// The account's relation count is the graph total, not that
    /// of the touched subjects: coverage says what has not been read, and
    /// for that the total is needed.
    pub fn of(
        diff: &SimulationDiff,
        fork: &SimulationFork,
        observed: &[RelationFact],
        relations_in_the_account: usize,
    ) -> Self {
        let observed = compare::normalize_relations(observed);
        let overlay = fork.relations_with(&observed);
        Self {
            version: SIMULATION_RESULT_VERSION.to_string(),
            simulation: fork.id.clone(),
            account: diff.base.account.clone(),
            base: the_base_stamp_of(&diff.base),
            observed: ObservedState {
                relations: observed.clone(),
                policy: PolicyStamp::of(&fork.base_policy),
                evidence: the_evidence_of(&diff.base.read_set),
            },
            hypotheses: the_hypotheses_of(fork),
            structural: diff.structural.clone(),
            decisions: compare::normalize_decisions(&diff.decisions),
            coverage: the_coverage_of(fork, &observed, relations_in_the_account),
            conflicts: the_conflicts_of(fork, &observed, &overlay),
        }
    }

    /// Whether the result says nothing.
    ///
    /// A result without differences, without decisions and without conflicts
    /// says nothing: and a result that says nothing is not a result.
    pub fn is_empty(&self) -> bool {
        self.structural.is_empty()
            && self.decisions.is_empty()
            && self.conflicts.is_empty()
            && self.hypotheses.is_empty()
    }
}

/// The stamp of a diff's base.
fn the_base_stamp_of(base: &SimulationBase) -> BaseStamp {
    let mut sources: Vec<SourceStamp> = base
        .read_set
        .generations
        .iter()
        .map(|(source, version)| SourceStamp {
            source: source.clone(),
            version: *version,
        })
        .collect();
    sources.sort();
    BaseStamp {
        account: base.account.clone(),
        evaluated_at: base.evaluated_at,
        generation: base
            .read_set
            .generations
            .iter()
            .find(|(source, _)| source == "graph")
            .map(|(_, version)| *version),
        seal: base.seal.clone(),
        fingerprint: base.fingerprint(),
        declared_ledger_position: base.declared_ledger_position,
        sources,
    }
}

/// The evidence that was read to decide, in canonical order.
fn the_evidence_of(read_set: &ReadSet) -> Vec<EvidenceStamp> {
    let mut evidence: Vec<EvidenceStamp> = read_set
        .stamps
        .iter()
        .filter(|stamp| stamp.kind == ReadKind::Evidence)
        .map(|stamp| EvidenceStamp {
            id: stamp.resource.clone(),
            digest: stamp.verified_by.clone(),
        })
        .collect();
    evidence.sort();
    evidence.dedup();
    evidence
}

/// The fork's hypothetical scenarios, in canonical order.
///
/// The fork's hypothetical policy is one more hypothetical: it does not travel in
/// the list of facts --because it is a field of the fork and not a fact-- but
/// it is as hypothetical as the rest, and if it is not declared the consumer does not
/// know which rules were applied.
fn the_hypotheses_of(fork: &SimulationFork) -> Vec<Hypothesis> {
    let mut hypotheses: Vec<Hypothesis> = fork.facts.iter().map(the_hypothesis_of).collect();
    if let Some(policy) = &fork.policy {
        hypotheses.push(Hypothesis {
            simulation: fork.id.clone(),
            created_at: fork.created_at,
            kind: HypothesisKind::Policy {
                policy: PolicyStamp::of(policy),
            },
            subjects: Vec::new(),
        });
    }
    hypotheses.sort_by_key(|hypothesis| hypothesis.the_key());
    hypotheses
}

/// A hypothetical fact, projected to portable form.
fn the_hypothesis_of(fact: &Hypothetical) -> Hypothesis {
    let kind = match &fact.claim {
        HypotheticalClaim::Relation {
            relation,
            from,
            to,
            replaces,
        } => HypothesisKind::Relation {
            relation: relation.clone(),
            from: from.clone(),
            to: to.clone(),
            replaces: replaces.as_ref().map(RelationFact::identity),
        },
        HypotheticalClaim::Risk {
            subject,
            advisory,
            critical,
            reachable,
        } => HypothesisKind::Risk {
            subject: subject.clone(),
            advisory: advisory.clone(),
            critical: *critical,
            reachable: *reachable,
        },
        HypotheticalClaim::UntrustedWindow { from, to, subjects } => {
            HypothesisKind::UntrustedWindow {
                from: *from,
                to: *to,
                subjects: subjects.clone(),
            }
        }
        HypotheticalClaim::Policy { policy } => HypothesisKind::Policy {
            policy: PolicyStamp::of(policy),
        },
    };
    let mut subjects = fact.claim.subjects();
    subjects.sort_by_key(|subject| subject.to_string());
    subjects.dedup();
    Hypothesis {
        simulation: fact.tag.simulation.clone(),
        created_at: fact.tag.created_at,
        kind,
        subjects,
    }
}

/// The simulation's coverage.
fn the_coverage_of(
    fork: &SimulationFork,
    observed: &[RelationFact],
    relations_in_the_account: usize,
) -> Coverage {
    let mut declared = fork.subjects.clone();
    declared.sort_by_key(|subject| subject.to_string());
    declared.dedup();
    let mut evaluated = fork.affected_subjects();
    evaluated.sort_by_key(|subject| subject.to_string());
    evaluated.dedup();

    let relations_read = observed.len();
    let relations_not_read = relations_in_the_account.saturating_sub(relations_read);
    let mut omitted: Vec<Omission> = Vec::new();
    if relations_not_read > 0 {
        omitted.push(Omission {
            subject: None,
            reason: format!(
                "the simulation reads the relations of the subjects it touches: {relations_not_read} \
                 relations of the account were not read"
            ),
        });
    }

    Coverage {
        declared_subjects: declared,
        evaluated_subjects: evaluated,
        relations_read,
        relations_not_read,
        omitted,
    }
}

/// The conflicts the scenario declares.
///
/// All three belong to the overlay and none to the real world: a cycle that was already
/// there is not the simulation's, a relation that already existed is not
/// introduced by the hypothetical and a contradiction between two hypotheticals is
/// the fault of whoever declared them.
fn the_conflicts_of(
    fork: &SimulationFork,
    observed: &[RelationFact],
    overlay: &[RelationFact],
) -> Vec<Conflict> {
    let added = fork.added_relations();
    let removed = fork.removed_relations();
    let hypothetical: Vec<String> = added.iter().map(RelationFact::identity).collect();

    let mut conflicts: Vec<Conflict> = Vec::new();
    for cycle in compare::the_cycles_introduced_by(overlay, &hypothetical) {
        conflicts.push(Conflict {
            kind: ConflictKind::Cycle,
            detail: format!(
                "the overlay closes a cycle: {}",
                cycle.relations.join(" -> ")
            ),
            subjects: cycle.subjects,
        });
    }
    for identity in compare::the_duplicates_of(&added, observed) {
        conflicts.push(Conflict {
            kind: ConflictKind::Duplicate,
            detail: format!("the relation {identity} is said twice"),
            subjects: compare::the_subjects_of(&identity, observed),
        });
    }
    for identity in compare::the_contradictions_of(&added, &removed) {
        conflicts.push(Conflict {
            kind: ConflictKind::Contradiction,
            detail: format!("the relation {identity} is added and replaced at the same time"),
            subjects: compare::the_subjects_of(&identity, observed),
        });
    }
    conflicts
}
