//! Infrastructure adapters for evaluate_iam_policies feature
//!
//! This module provides concrete implementations of the PolicyFinderPort trait
//! for different storage backends.

use std::sync::Arc;
use kernel::domain::policy::{HodeiPolicy, HodeiPolicySet};
use kernel::Hrn;
use tracing::{debug, info};

use async_trait::async_trait;
use super::ports::{PolicyFinderPort, PolicyFinderError};

/// In-memory implementation of PolicyFinderPort for testing
#[derive(Debug, Clone)]
pub struct InMemoryPolicyFinderAdapter {
    policies: Arc<Vec<HodeiPolicy>>,
}

impl InMemoryPolicyFinderAdapter {
    /// Create a new in-memory policy finder with no policies
    pub fn new() -> Self {
        Self {
            policies: Arc::new(Vec::new()),
        }
    }

    /// Create a new in-memory policy finder with initial policies
    pub fn with_policies(policies: Vec<HodeiPolicy>) -> Self {
        Self {
            policies: Arc::new(policies),
        }
    }
}

impl Default for InMemoryPolicyFinderAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PolicyFinderPort for InMemoryPolicyFinderAdapter {
    async fn get_effective_policies(&self, principal_hrn: &Hrn) -> Result<HodeiPolicySet, PolicyFinderError> {
        debug!("Finding policies for principal: {}", principal_hrn);

        // For this in-memory implementation, we'll return all policies
        // In a real implementation, this would filter by principal
        let policy_set = HodeiPolicySet::new(self.policies.to_vec());
        
        info!("Found {} policies for principal: {}", policy_set.len(), principal_hrn);
        Ok(policy_set)
    }
}

/// SurrealDB implementation of PolicyFinderPort
#[derive(Debug)]
pub struct SurrealPolicyFinderAdapter {
    // TODO: Add SurrealDB connection when implemented
}

impl SurrealPolicyFinderAdapter {
    /// Create a new SurrealDB policy finder
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
    async fn get_effective_policies(&self, principal_hrn: &Hrn) -> Result<HodeiPolicySet, PolicyFinderError> {
        debug!("Finding policies for principal: {} using SurrealDB", principal_hrn);
        
        // TODO: Implement actual SurrealDB query
        // For now, return empty policy set
        let policy_set = HodeiPolicySet::new(Vec::new());
        
        info!("Found {} policies for principal {} using SurrealDB", policy_set.policies().len(), principal_hrn);
        Ok(policy_set)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::domain::policy::PolicyId;

    #[tokio::test]
    async fn test_in_memory_policy_finder_empty() {
        let finder = InMemoryPolicyFinderAdapter::new();
        let principal_hrn = Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap();
        
        let result = finder.get_effective_policies(&principal_hrn).await;
        assert!(result.is_ok());
        
        let policy_set = result.unwrap();
        assert_eq!(policy_set.policies().len(), 0);
    }

    #[tokio::test]
    async fn test_in_memory_policy_finder_with_policies() {
        let policy = HodeiPolicy::new(
            PolicyId::new("test-policy".to_string()),
            "permit(principal, action, resource);".to_string(),
        );
        
        let finder = InMemoryPolicyFinderAdapter::with_policies(vec![policy]);
        let principal_hrn = Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap();
        
        let result = finder.get_effective_policies(&principal_hrn).await;
        assert!(result.is_ok());
        
        let policy_set = result.unwrap();
        assert_eq!(policy_set.policies().len(), 1);
    }

    #[tokio::test]
    async fn test_surreal_policy_finder() {
        let finder = SurrealPolicyFinderAdapter::new();
        let principal_hrn = Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap();
        
        let result = finder.get_effective_policies(&principal_hrn).await;
        assert!(result.is_ok());
        
        let policy_set = result.unwrap();
        assert_eq!(policy_set.policies().len(), 0); // Empty for now
    }
}
