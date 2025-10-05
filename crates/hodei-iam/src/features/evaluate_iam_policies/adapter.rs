//! Adapters for the evaluate_iam_policies feature
//!
//! This module provides concrete implementations of the ports defined for this feature.
//! Following hexagonal architecture, adapters connect the use case to external infrastructure
//! (repositories, databases, etc.).

use async_trait::async_trait;
use kernel::application::ports::authorization::AuthorizationError;
use kernel::domain::Hrn;
use std::sync::Arc;
use tracing::{debug, warn};

use super::ports::PolicyFinderPort;

/// Adapter that uses a policy repository to find IAM policies
///
/// This adapter is responsible for:
/// 1. Querying the policy repository for policies attached to a principal
/// 2. Querying policies inherited from groups the principal belongs to
/// 3. Combining and returning all applicable policy documents
///
/// # Future Enhancements
/// - Support for role-based policies
/// - Policy caching for performance
/// - Policy filtering based on conditions
pub struct PolicyRepositoryAdapter<PR, UR, GR>
where
    PR: Send + Sync,
    UR: Send + Sync,
    GR: Send + Sync,
{
    /// Repository for IAM policy operations
    policy_repo: Arc<PR>,

    /// Repository for user operations (to get user's groups)
    user_repo: Arc<UR>,

    /// Repository for group operations (to get group policies)
    group_repo: Arc<GR>,
}

impl<PR, UR, GR> PolicyRepositoryAdapter<PR, UR, GR>
where
    PR: Send + Sync,
    UR: Send + Sync,
    GR: Send + Sync,
{
    /// Create a new policy repository adapter
    ///
    /// # Arguments
    /// * `policy_repo` - Repository for IAM policies
    /// * `user_repo` - Repository for users (to resolve group memberships)
    /// * `group_repo` - Repository for groups (to get group policies)
    pub fn new(policy_repo: Arc<PR>, user_repo: Arc<UR>, group_repo: Arc<GR>) -> Self {
        Self {
            policy_repo,
            user_repo,
            group_repo,
        }
    }
}

// Note: Full implementation requires repository traits to be defined first
// This is a placeholder showing the structure. The actual implementation will:
// 1. Query policies directly attached to the principal
// 2. If principal is a User, get their groups
// 3. Query policies attached to those groups
// 4. Combine all policies and return as Vec<String>

// Temporary simple implementation that returns empty policies
// This allows the feature to compile and be tested with mocks
impl<PR, UR, GR> PolicyRepositoryAdapter<PR, UR, GR>
where
    PR: Send + Sync,
    UR: Send + Sync,
    GR: Send + Sync,
{
    /// Get policies directly attached to a principal
    ///
    /// This is a helper method that will query the policy repository
    async fn get_direct_policies(&self, _principal_hrn: &Hrn) -> Result<Vec<String>, AuthorizationError> {
        // TODO: Implement when PolicyRepository trait is ready
        // For now, return empty to allow compilation
        debug!("Getting direct policies (not yet implemented)");
        Ok(Vec::new())
    }

    /// Get group memberships for a user
    ///
    /// This is a helper method that will query the user repository
    async fn get_user_groups(&self, _user_hrn: &Hrn) -> Result<Vec<Hrn>, AuthorizationError> {
        // TODO: Implement when UserRepository trait includes group lookup
        debug!("Getting user groups (not yet implemented)");
        Ok(Vec::new())
    }

    /// Get policies attached to groups
    ///
    /// This is a helper method that will query policies for each group
    async fn get_group_policies(&self, _group_hrns: &[Hrn]) -> Result<Vec<String>, AuthorizationError> {
        // TODO: Implement when PolicyRepository supports group queries
        debug!("Getting group policies (not yet implemented)");
        Ok(Vec::new())
    }
}

#[async_trait]
impl<PR, UR, GR> PolicyFinderPort for PolicyRepositoryAdapter<PR, UR, GR>
where
    PR: Send + Sync,
    UR: Send + Sync,
    GR: Send + Sync,
{
    async fn get_policies_for_principal(
        &self,
        principal_hrn: &Hrn,
    ) -> Result<Vec<String>, AuthorizationError> {
        debug!(
            principal = %principal_hrn,
            "Finding IAM policies for principal"
        );

        // Step 1: Get direct policies attached to the principal
        let mut all_policies = self.get_direct_policies(principal_hrn).await?;

        // Step 2: If principal is a user, get policies from groups
        if principal_hrn.service() == "iam" && principal_hrn.resource_type() == "user" {
            debug!("Principal is a user, checking group memberships");

            let groups = self.get_user_groups(principal_hrn).await?;

            if !groups.is_empty() {
                debug!(group_count = groups.len(), "User belongs to groups");
                let group_policies = self.get_group_policies(&groups).await?;
                all_policies.extend(group_policies);
            }
        }

        debug!(
            policy_count = all_policies.len(),
            principal = %principal_hrn,
            "Found IAM policies for principal"
        );

        Ok(all_policies)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock repositories for testing
    struct MockPolicyRepo;
    struct MockUserRepo;
    struct MockGroupRepo;

    #[tokio::test]
    async fn test_adapter_compiles() {
        let policy_repo = Arc::new(MockPolicyRepo);
        let user_repo = Arc::new(MockUserRepo);
        let group_repo = Arc::new(MockGroupRepo);

        let adapter = PolicyRepositoryAdapter::new(policy_repo, user_repo, group_repo);

        let hrn = Hrn::new(
            "iam".to_string(),
            "us-east-1".to_string(),
            "123456789012".to_string(),
            "user".to_string(),
            "alice".to_string(),
        );

        // Should return empty policies for now
        let result = adapter.get_policies_for_principal(&hrn).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }
}
