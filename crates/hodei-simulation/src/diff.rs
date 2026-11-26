//! The diff: what appears, what disappears and what changes.
//!
//! The diff has to look like a git diff: it shows which relations
//! appear and which relations disappear, and which decisions change, without
//! having to deploy anything to find out.

use hodei_evidence::domain::read_set::ReadSet;
use hodei_trust_policy::TrustPolicy;
use hodei_trust_policy::decision::Outcome;
use kernel::Hrn;
use serde::{Deserialize, Serialize};

use crate::hypothetical::RelationFact;

/// The diff of the graph relations.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StructuralDiff {
    /// The relations that appear.
    pub appeared: Vec<RelationFact>,
    /// The relations that disappear.
    pub disappeared: Vec<RelationFact>,
    /// The subjects the change reaches.
    pub affected_subjects: Vec<Hrn>,
    /// The deployments of those subjects.
    ///
    /// A deployment is where the change hurts: an artifact that stops being
    /// promotable hurts at the deployment that runs it, and the diff
    /// has to say so.
    pub affected_deployments: Vec<Hrn>,
}

impl StructuralDiff {
    /// An empty diff.
    pub fn empty() -> Self {
        Self::default()
    }

    /// The diff between two sets of relations.
    ///
    /// Relations are compared by their IDENTITY (the type and the two
    /// endpoints): two relations with the same identity are the same
    /// relation, and comparing them by their fields would be comparing the
    /// same row twice.
    ///
    /// And the diff comes out in CANONICAL ORDER: the relations that appear and
    /// those that disappear are sorted by their identity, not in the
    /// order in which each side gave them. A diff whose order depends on the
    /// input order cannot be compared with another diff, and a diff
    /// that cannot be compared cannot be audited.
    pub fn between(before: &[RelationFact], after: &[RelationFact]) -> Self {
        let before_ids: Vec<String> = before.iter().map(RelationFact::identity).collect();
        let after_ids: Vec<String> = after.iter().map(RelationFact::identity).collect();

        let mut appeared: Vec<RelationFact> = after
            .iter()
            .filter(|relation| !before_ids.contains(&relation.identity()))
            .cloned()
            .collect();
        appeared.sort_by_key(|relation| relation.identity());
        let mut disappeared: Vec<RelationFact> = before
            .iter()
            .filter(|relation| !after_ids.contains(&relation.identity()))
            .cloned()
            .collect();
        disappeared.sort_by_key(|relation| relation.identity());

        let mut affected: Vec<Hrn> = Vec::new();
        for relation in appeared.iter().chain(disappeared.iter()) {
            for subject in [&relation.from, &relation.to] {
                if !affected.contains(subject) {
                    affected.push(subject.clone());
                }
            }
        }
        affected.sort_by_key(|subject| subject.to_string());

        Self {
            appeared,
            disappeared,
            affected_subjects: affected,
            affected_deployments: Vec::new(),
        }
    }

    /// Whether the diff says nothing.
    pub fn is_empty(&self) -> bool {
        self.appeared.is_empty() && self.disappeared.is_empty()
    }
}

/// A decision that changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionChange {
    /// The subject whose decision changes.
    pub subject: Hrn,
    /// What was decided before.
    pub before: Outcome,
    /// What would be decided now.
    pub after: Outcome,
    /// The version of the previous policy.
    pub policy_version_before: String,
    /// The version of the current policy.
    pub policy_version_after: String,
}

impl DecisionChange {
    /// The decision really changes.
    ///
    /// A decision that stays the same, even if it was evaluated again, is not
    /// a change: and a diff full of noise is not a diff.
    pub fn is_a_change(&self) -> bool {
        self.before != self.after
    }
}

/// The complete diff of the simulation.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SimulationDiff {
    /// The diff of the relations.
    pub structural: StructuralDiff,
    /// The decisions that change.
    pub decisions: Vec<DecisionChange>,
    /// The base on which it was computed: the world that the diff describes.
    ///
    /// Without it the diff is an answer without the world it refers to:
    /// it does not say on which version of the truth it was computed, and therefore it
    /// cannot be audited.
    #[serde(default)]
    pub base: crate::base::SimulationBase,
}

impl SimulationDiff {
    /// An empty diff.
    pub fn empty() -> Self {
        Self::default()
    }

    /// The diff says that nothing changes.
    pub fn is_empty(&self) -> bool {
        self.structural.is_empty() && self.decisions.iter().all(|change| !change.is_a_change())
    }

    /// The canonical form of the diff: what is compared between two
    /// simulations of the same world.
    ///
    /// The diff carries the read manifest, and that manifest carries the
    /// order in which it was read. Two simulations of the same world read in
    /// different order and therefore their manifests are equal but not
    /// identical: the canonical form is the one that is compared.
    pub fn canonical(&self) -> crate::compare::CanonicalDiff {
        crate::compare::canonical(self)
    }
}

/// The decision of a subject, with the policy that produced it.
///
/// And with what was read to decide: a decision without its read manifest is
/// a decision that does not say on which evidence it was taken, and
/// a simulation base that does not keep it cannot know whether the
/// evidence changed while it was being evaluated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TheDecision {
    /// What was decided.
    pub outcome: Outcome,
    /// The policy that decided it.
    pub policy: TrustPolicy,
    /// What was read to decide.
    pub read_set: ReadSet,
}
