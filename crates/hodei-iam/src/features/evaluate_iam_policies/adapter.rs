//! Adapter implementations for the evaluate_iam_policies feature
//!
//! This module provides concrete implementations of the ports defined for
//! IAM policy evaluation.
//!
//! # TODO: REFACTOR (Phase 2)
//!
//! This is a temporary stub implementation that allows compilation.
//! In Phase 2, this will be properly integrated with the repository layer
//! to retrieve actual policies from storage.

use async_trait::async_trait;
use cedar_policy::PolicySet;
use kernel::Hrn;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::ports::{PolicyFinderError, PolicyFinderPort};

/// In-memory adapter for policy finding (for testing and development)
///
/// This adapter stores policies in memory and returns them for any principal.
/// It's intended for testing and will be replaced with a real repository adapter
/// in Phase 2.
///
/// # Architecture
///
/// This adapter lives in the infrastructure layer and implements the
/// PolicyFinderPort defined in the application layer (ports.rs).
pub struct InMemoryPolicyFinderAdapter {
    /// Map of principal HRN to PolicySet
    policies: Arc<RwLock<HashMap<String, PolicySet>>>,
}

impl InMemoryPolicyFinderAdapter {
    /// Create a new in-memory adapter with no policies
    pub fn new() -> Self {
        Self {
            policies: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add policies for a specific principal
    ///
    /// # Arguments
    ///
    /// * `principal_hrn` - The HRN of the principal
    /// * `policy_set` - The PolicySet to associate with this principal
    pub fn add_policies_for_principal(&self, principal_hrn: String, policy_set: PolicySet) {
        let mut policies = self.policies.write().unwrap();
        policies.insert(principal_hrn, policy_set);
    }

    /// Clear all stored policies
    pub fn clear(&self) {
        let mut policies = self.policies.write().unwrap();
        policies.clear();
    }
}

impl Default for InMemoryPolicyFinderAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PolicyFinderPort for InMemoryPolicyFinderAdapter {
    async fn get_effective_policies(
        &self,
        principal_hrn: &Hrn,
    ) -> Result<PolicySet, PolicyFinderError> {
        let policies = self.policies.read().unwrap();

        // Try to find policies for this specific principal
        if let Some(policy_set) = policies.get(&principal_hrn.to_string()) {
            return Ok(policy_set.clone());
        }

        // Return empty policy set if no policies found
        Ok(PolicySet::new())
    }
}

/// SurrealDB adapter for policy finding
///
/// This adapter retrieves policies from SurrealDB storage.
///
/// # TODO: IMPLEMENTATION
///
/// This is a stub that will be implemented in Phase 2 when we integrate
/// with the actual repository layer.
pub struct SurrealPolicyFinderAdapter {
    // TODO: Add SurrealDB connection pool
}

impl SurrealPolicyFinderAdapter {
    /// Create a new SurrealDB adapter
    ///
    /// # TODO
    ///
    /// Accept SurrealDB connection pool as parameter
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for SurrealPolicyFinderAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PolicyFinderPort for SurrealPolicyFinderAdapter {
    async fn get_effective_policies(
        &self,
        _principal_hrn: &Hrn,
    ) -> Result<PolicySet, PolicyFinderError> {
        // TODO: Implement actual database query
        // For now, return empty policy set
        Ok(PolicySet::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[tokio::test]
    async fn test_in_memory_adapter_returns_empty_when_no_policies() {
        let adapter = InMemoryPolicyFinderAdapter::new();
        let principal_hrn = Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap();

        let result = adapter.get_effective_policies(&principal_hrn).await;

        assert!(result.is_ok());
        let policy_set = result.unwrap();
        assert_eq!(policy_set.policies().count(), 0);
    }

    #[tokio::test]
    async fn test_in_memory_adapter_returns_configured_policies() {
        let adapter = InMemoryPolicyFinderAdapter::new();
        let principal_hrn = Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap();

        let policy_text = "permit(principal, action, resource);";
        let policy_set = PolicySet::from_str(policy_text).unwrap();

        adapter.add_policies_for_principal(principal_hrn.to_string(), policy_set.clone());

        let result = adapter.get_effective_policies(&principal_hrn).await;

        assert!(result.is_ok());
        let returned_set = result.unwrap();
        assert_eq!(returned_set.policies().count(), 1);
    }

    #[tokio::test]
    async fn test_in_memory_adapter_clear() {
        let adapter = InMemoryPolicyFinderAdapter::new();
        let principal_hrn = Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap();

        let policy_text = "permit(principal, action, resource);";
        let policy_set = PolicySet::from_str(policy_text).unwrap();
        adapter.add_policies_for_principal(principal_hrn.to_string(), policy_set);

        adapter.clear();

        let result = adapter.get_effective_policies(&principal_hrn).await;
        assert!(result.is_ok());
        let returned_set = result.unwrap();
        assert_eq!(returned_set.policies().count(), 0);
    }
}
