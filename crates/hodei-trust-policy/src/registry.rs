//! The registry of the policy sets, and the history of their activation.
//!
//! A policy set is not a file that is read when it is needed: it is
//! **activated**. Activating it says that from this moment on, this
//! version is the one that decides, and it leaves a record of the fact.
//!
//! The history is what makes a decision readable months later. A decision
//! names the policy set and its version, and the registry can say who
//! activated it and when. A policy that is changed does not rewrite the
//! decisions that were made under the previous version: those decisions
//! name the version they were made under, and that version is still there.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

use crate::policy::TrustPolicy;

/// The record of one activation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activation {
    /// The identifier of the policy set.
    pub policy_id: String,
    /// The version that was activated.
    pub version: String,
    /// The version of the schema it was written against.
    pub schema_version: String,
    /// Who activated it.
    pub activated_by: String,
    /// When it was activated.
    pub activated_at: DateTime<Utc>,
    /// Why it was activated, in the words of whoever did it.
    pub reason: String,
}

/// The registry of the policy sets.
///
/// It holds the set that is active and the history of the ones that were
/// activated before it. The set that is active is the one that decides;
/// the history is read and never decides.
#[derive(Debug, Clone, Default)]
pub struct PolicyRegistry {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Debug, Default)]
struct Inner {
    active: Vec<TrustPolicy>,
    history: Vec<Activation>,
}

impl PolicyRegistry {
    /// A registry with nothing activated.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Activates a policy set.
    ///
    /// The set that was active for that identifier stops being active, and
    /// the history keeps both. Activating the same version twice is
    /// recorded twice, because the fact that somebody activated it again
    /// is a fact.
    pub fn activate(
        &self,
        policy: TrustPolicy,
        activated_by: impl Into<String>,
        reason: impl Into<String>,
        activated_at: DateTime<Utc>,
    ) -> Activation {
        let activation = Activation {
            policy_id: policy.id.clone(),
            version: policy.version.clone(),
            schema_version: policy.schema_version.clone(),
            activated_by: activated_by.into(),
            activated_at,
            reason: reason.into(),
        };

        let mut inner = self.inner.lock().expect("the registry is not poisoned");
        inner.active.retain(|active| active.id != policy.id);
        inner.active.push(policy);
        inner.history.push(activation.clone());
        activation
    }

    /// The policy set that is active for an identifier.
    pub fn active(&self, policy_id: &str) -> Option<TrustPolicy> {
        let inner = self.inner.lock().expect("the registry is not poisoned");
        inner
            .active
            .iter()
            .find(|policy| policy.id == policy_id)
            .cloned()
    }

    /// The policy sets that are active.
    pub fn active_policies(&self) -> Vec<TrustPolicy> {
        let inner = self.inner.lock().expect("the registry is not poisoned");
        inner.active.clone()
    }

    /// The history of the activations, oldest first.
    pub fn history(&self) -> Vec<Activation> {
        let inner = self.inner.lock().expect("the registry is not poisoned");
        inner.history.clone()
    }

    /// The activation of a version, if it was ever activated.
    pub fn activation_of(&self, policy_id: &str, version: &str) -> Option<Activation> {
        let inner = self.inner.lock().expect("the registry is not poisoned");
        inner
            .history
            .iter()
            .find(|activation| activation.policy_id == policy_id && activation.version == version)
            .cloned()
    }

    /// The number of activations.
    pub fn activation_count(&self) -> usize {
        let inner = self.inner.lock().expect("the registry is not poisoned");
        inner.history.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{Requirement, RequirementKind};
    use hodei_evidence::domain::capsule::{DimensionName, DimensionState};

    fn a_policy(id: &str, version: &str) -> TrustPolicy {
        TrustPolicy::new(
            id,
            version,
            vec![Requirement::denying(RequirementKind::Dimension {
                dimension: DimensionName::Integrity,
                accepts: vec![DimensionState::Verified],
            })],
        )
    }

    #[test]
    fn activating_keeps_the_history() {
        let registry = PolicyRegistry::empty();
        let now = Utc::now();
        registry.activate(a_policy("prod", "1.0.0"), "alice", "the first one", now);
        registry.activate(a_policy("prod", "2.0.0"), "bob", "the second one", now);

        assert_eq!(registry.activation_count(), 2);
        assert_eq!(
            registry.active("prod").expect("the set is active").version,
            "2.0.0",
            "the last one activated is the one that decides"
        );
        assert_eq!(
            registry
                .activation_of("prod", "1.0.0")
                .expect("it is in the history")
                .activated_by,
            "alice",
            "and the old one is still there"
        );
    }
}
