//! # Policy Domain Entities
//!
//! This module defines the core policy entities that are shared across bounded contexts.
//! These are the agnostic representations used by the authorization engine.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A unique identifier for a policy.
///
/// This is a value object that wraps a string ID.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PolicyId(String);

impl PolicyId {
    /// Creates a new `PolicyId` from a string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the inner string representation of the ID.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the `PolicyId` and returns the inner string.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for PolicyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for PolicyId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

impl From<&str> for PolicyId {
    fn from(id: &str) -> Self {
        Self(id.to_string())
    }
}

impl AsRef<str> for PolicyId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// An agnostic policy representation.
///
/// This is the shared kernel representation of a policy, containing only
/// the essential information needed for authorization evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HodeiPolicy {
    /// Unique identifier for this policy
    id: PolicyId,

    /// The policy content (Cedar DSL text)
    content: String,
}

impl HodeiPolicy {
    /// Creates a new `HodeiPolicy`.
    pub fn new(id: PolicyId, content: String) -> Self {
        Self { id, content }
    }

    /// Returns the policy's unique identifier.
    pub fn id(&self) -> &PolicyId {
        &self.id
    }

    /// Returns the policy's content.
    pub fn content(&self) -> &str {
        &self.content
    }
}

/// A collection of policies for evaluation.
///
/// This represents a set of policies that can be evaluated together
/// in an authorization request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HodeiPolicySet {
    policies: Vec<HodeiPolicy>,
}

impl HodeiPolicySet {
    /// Creates a new `HodeiPolicySet` from a vector of policies.
    pub fn new(policies: Vec<HodeiPolicy>) -> Self {
        Self { policies }
    }

    /// Add a policy to the set
    pub fn add(&mut self, policy: HodeiPolicy) {
        self.policies.push(policy);
    }

    /// Returns a reference to the policies in this set.
    pub fn policies(&self) -> &[HodeiPolicy] {
        &self.policies
    }

    /// Returns the number of policies in this set.
    pub fn len(&self) -> usize {
        self.policies.len()
    }

    /// Returns true if the set contains no policies.
    pub fn is_empty(&self) -> bool {
        self.policies.is_empty()
    }

    /// Returns true if the set contains the specified policy.
    pub fn contains(&self, policy: &HodeiPolicy) -> bool {
        self.policies.contains(policy)
    }
}

impl Default for HodeiPolicySet {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_id_can_be_created_and_displayed() {
        let id = PolicyId::new("test-123");
        assert_eq!(id.to_string(), "test-123");
        assert_eq!(id.as_str(), "test-123");
    }

    #[test]
    fn policy_id_can_be_converted_from_string() {
        let id: PolicyId = "test-456".into();
        assert_eq!(id.to_string(), "test-456");
    }

    #[test]
    fn hodei_policy_can_be_created() {
        let id = PolicyId::new("policy-1");
        let content = "permit(principal, action, resource);".to_string();

        let policy = HodeiPolicy::new(id.clone(), content.clone());

        assert_eq!(policy.id(), &id);
        assert_eq!(policy.content(), content);
    }

    #[test]
    fn hodei_policy_set_can_be_created() {
        let policy1 = HodeiPolicy::new(
            PolicyId::new("p1"),
            "permit(principal, action, resource);".to_string(),
        );
        let policy2 = HodeiPolicy::new(
            PolicyId::new("p2"),
            "forbid(principal, action, resource);".to_string(),
        );

        let policy_set = HodeiPolicySet::new(vec![policy1.clone(), policy2.clone()]);

        assert_eq!(policy_set.len(), 2);
        assert_eq!(policy_set.policies()[0], policy1);
        assert_eq!(policy_set.policies()[1], policy2);
    }

    #[test]
    fn hodei_policy_set_default_is_empty() {
        let policy_set = HodeiPolicySet::default();
        assert!(policy_set.is_empty());
        assert_eq!(policy_set.len(), 0);
    }
}
