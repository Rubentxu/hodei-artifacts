//! The hypothetical facts of a simulation.
//!
//! A hypothetical fact is a fact that **has not happened**: it is what is
//! overlaid on the real state to ask "what would happen if". And the
//! difference between a real fact and a hypothetical one is not a convention:
//! it is a **label** that travels with the fact, and a label that can be
//! lost is a label that does not exist.
//!
//! That is why the label is part of the type, and why normal graph
//! queries never see a hypothetical fact: they are not in the
//! graph.

use chrono::{DateTime, Utc};
use hodei_evidence::domain::graph::RelationType;
use hodei_trust_policy::TrustPolicy;
use kernel::Hrn;
use serde::{Deserialize, Serialize};

/// The label that says a fact belongs to a simulation.
///
/// The label carries the simulation's identifier, so that a fact
/// of one simulation is not confused with another's, and so that it can be
/// audited later what was simulated and what.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulationTag {
    /// The simulation's identifier.
    pub simulation: String,
    /// The moment the fact was created.
    pub created_at: DateTime<Utc>,
}

impl SimulationTag {
    /// A simulation's label.
    pub fn new(simulation: impl Into<String>, created_at: DateTime<Utc>) -> Self {
        Self {
            simulation: simulation.into(),
            created_at,
        }
    }
}

/// A fact that is overlaid on the real state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hypothetical {
    /// The label: this fact belongs to a simulation.
    pub tag: SimulationTag,
    /// What it says.
    pub claim: HypotheticalClaim,
}

/// What a hypothetical fact says.
///
/// The variants have very different sizes, and the difference does not matter
/// here: a simulation has **at most a hundred hypothetical facts** (the
/// limit is declared and checked), so the waste
/// would be a few kilobytes. Boxing them all
/// would make the code worse without fixing anything.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HypotheticalClaim {
    /// A relation that would exist.
    Relation {
        /// Which relation it is.
        relation: RelationType,
        /// The node it starts from.
        from: Hrn,
        /// The node it arrives at.
        to: Hrn,
        /// The relation it REPLACES, if it replaces any.
        ///
        /// Replacing is what makes a relation disappear: without
        /// this, a dependency change would only add and the diff would
        /// say nothing about what disappears.
        replaces: Option<RelationFact>,
    },
    /// A risk that there would be, with what is known about it.
    ///
    /// The risk is a fact about the subject: a vulnerability that
    /// reaches it. And what the policy decides is not the vulnerability's
    /// identifier, but whether it is critical and whether it is reachable.
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
    ///
    /// Everything observed inside the window ceases to be
    /// verified: it is what happens when a builder is compromised
    /// for a while, and the pieces of evidence from that time are
    /// precisely the ones that are worthless.
    ///
    /// The window **names the subjects** it reaches, because without
    /// them the simulation would not know whom to look at: a window is a
    /// time range, and a time range does not say whose
    /// fault it is.
    UntrustedWindow {
        /// The start of the window.
        from: DateTime<Utc>,
        /// The end of the window.
        to: DateTime<Utc>,
        /// The subjects the window reaches.
        subjects: Vec<Hrn>,
    },
    /// A policy that would be in force.
    ///
    /// The policy is boxed because it is much larger than the
    /// other variants, and such a large size difference turns
    /// every hypothetical fact into wasted memory.
    Policy {
        /// The policy.
        policy: Box<TrustPolicy>,
    },
}

impl HypotheticalClaim {
    /// The subjects the fact touches.
    ///
    /// They are the subjects whose decision may change, and therefore those
    /// the decision diff has to look at.
    pub fn subjects(&self) -> Vec<Hrn> {
        match self {
            Self::Relation { from, to, .. } => {
                let mut subjects = vec![from.clone()];
                if !subjects.contains(to) {
                    subjects.push(to.clone());
                }
                subjects
            }
            Self::Risk { subject, .. } => vec![subject.clone()],
            Self::UntrustedWindow { subjects, .. } => subjects.clone(),
            Self::Policy { .. } => Vec::new(),
        }
    }
}

/// A relation, with the shape the diff needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationFact {
    /// Which relation it is.
    pub relation: RelationType,
    /// The node it starts from.
    pub from: Hrn,
    /// The node it arrives at.
    pub to: Hrn,
    /// The evidence that supports it, if known.
    ///
    /// It is not part of the relation's IDENTITY, which is the type and the
    /// two ends: two relations with the same identity are the same
    /// relation, and the structural diff speaks of relations and not of
    /// supports. But it is part of its VALUE: a relation whose
    /// support is changed is not the same relation that was read before, and
    /// comparing only by identity would hide it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

impl RelationFact {
    /// The relation.
    pub fn new(relation: RelationType, from: Hrn, to: Hrn) -> Self {
        Self {
            relation,
            from,
            to,
            evidence: None,
        }
    }

    /// The relation, with the evidence that supports it.
    pub fn with_evidence(mut self, evidence: impl Into<String>) -> Self {
        self.evidence = Some(evidence.into());
        self
    }

    /// The relation's identity: the one that says whether it is the same or not.
    ///
    /// The identity does NOT carry the evidence: the type and the two ends
    /// say which relation it is, and who supports it is compared separately.
    pub fn identity(&self) -> String {
        format!("{}|{}|{}", self.relation.as_str(), self.from, self.to)
    }

    /// Whether it is the SAME relation as another: the one with the same
    /// identity.
    ///
    /// It is the comparison the diff uses, because the diff speaks of
    /// relations and not of supports.
    pub fn is_the_same_as(&self, other: &Self) -> bool {
        self.identity() == other.identity()
    }
}

impl From<&HypotheticalClaim> for RelationFact {
    fn from(claim: &HypotheticalClaim) -> Self {
        match claim {
            HypotheticalClaim::Relation {
                relation, from, to, ..
            } => RelationFact::new(relation.clone(), from.clone(), to.clone()),
            _ => unreachable!("una hipotetica que no es una relacion no tiene relacion"),
        }
    }
}
