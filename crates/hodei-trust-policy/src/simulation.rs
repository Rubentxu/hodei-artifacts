//! The simulation of a policy that is not yet active.
//!
//! A deployment wants to know what a policy would do **before** it is
//! activated: which releases would change, and which way. That question
//! has an answer that does not require activating anything, and the answer
//! is worth having: activating a policy that changes forty releases
//! without having said so is how a deployment loses trust in its own
//! decisions.
//!
//! The simulation **does not activate**. The active policy stays what it
//! was, and the report says so.

use serde::{Deserialize, Serialize};

use crate::context::TrustContext;
use crate::decision::{evaluate, Outcome};
use crate::policy::TrustPolicy;

/// What happens to one release under a candidate policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AffectedRelease {
    /// The release.
    pub subject: String,
    /// What the active policy says about it.
    pub current: Outcome,
    /// What the candidate says about it.
    pub candidate: Outcome,
    /// Whether the two disagree.
    pub changed: bool,
}

/// The report of a simulation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulationReport {
    /// The identifier of the candidate.
    pub candidate_policy_id: String,
    /// The version of the candidate.
    pub candidate_policy_version: String,
    /// The version that is active, which the simulation did not change.
    pub active_policy_version: Option<String>,
    /// What happens to each release.
    pub affected: Vec<AffectedRelease>,
    /// The releases whose result changes.
    pub changed: Vec<String>,
    /// Whether anything was activated.
    ///
    /// It is always false, and it is written down so that a reader of the
    /// report does not have to take it on faith.
    pub activated: bool,
}

impl SimulationReport {
    /// The number of releases whose result changes.
    pub fn changed_count(&self) -> usize {
        self.changed.len()
    }
}

/// Simulates a candidate policy against the current contexts.
///
/// The candidate is **not** activated and nothing is written: the report is
/// a reading of what would happen, and the policy that decides stays the
/// one that is active.
pub fn simulate(
    active: Option<&TrustPolicy>,
    candidate: &TrustPolicy,
    contexts: &[(String, TrustContext)],
) -> SimulationReport {
    let mut affected = Vec::with_capacity(contexts.len());
    let mut changed = Vec::new();

    for (subject, context) in contexts {
        let current = match active {
            Some(policy) => evaluate(context, policy).outcome,
            None => Outcome::Indeterminate,
        };
        let candidate_outcome = evaluate(context, candidate).outcome;
        let is_changed = current != candidate_outcome;
        if is_changed {
            changed.push(subject.clone());
        }
        affected.push(AffectedRelease {
            subject: subject.clone(),
            current,
            candidate: candidate_outcome,
            changed: is_changed,
        });
    }

    SimulationReport {
        candidate_policy_id: candidate.id.clone(),
        candidate_policy_version: candidate.version.clone(),
        active_policy_version: active.map(|policy| policy.version.clone()),
        affected,
        changed,
        activated: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_simulation_never_activates() {
        let report = simulate(None, &TrustPolicy::new("p", "2.0.0", Vec::new()), &[]);
        assert!(!report.activated);
        assert_eq!(report.candidate_policy_version, "2.0.0");
    }
}
