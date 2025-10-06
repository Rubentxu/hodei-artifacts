use crate::internal::application::ports::{
    GroupRepository, IamPolicyProvider, IamPolicyProviderError, UserRepository,
};
use crate::internal::infrastructure::surreal::{
    SurrealGroupRepository, SurrealUserRepository, policy_repository::IamPolicyRepository,
};
use async_trait::async_trait;
use cedar_policy::PolicySet;
use kernel::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use tracing::{info, instrument, warn};

/// SurrealDB implementation of IamPolicyProvider
pub struct SurrealIamPolicyProvider {
    user_repository: SurrealUserRepository,
    group_repository: SurrealGroupRepository,
    policy_repository: IamPolicyRepository,
}

impl SurrealIamPolicyProvider {
    /// Create a new SurrealIamPolicyProvider instance
    pub fn new(db: Surreal<Any>) -> Self {
        Self {
            user_repository: SurrealUserRepository::new(db.clone()),
            group_repository: SurrealGroupRepository::new(db.clone()),
            policy_repository: IamPolicyRepository::new(db),
        }
    }
}

#[async_trait]
impl IamPolicyProvider for SurrealIamPolicyProvider {
    /// Get identity policies for a principal
    #[instrument(skip(self), fields(principal = %principal_hrn))]
    async fn get_identity_policies_for(
        &self,
        principal_hrn: &Hrn,
    ) -> Result<PolicySet, IamPolicyProviderError> {
        info!("Retrieving IAM policies for principal: {}", principal_hrn);

        // Step 1: Get the user (principal)
        let user = self
            .user_repository
            .find_by_hrn(principal_hrn)
            .await
            .map_err(|e| {
                IamPolicyProviderError::RepositoryError(format!(
                    "Failed to find user {}: {}",
                    principal_hrn, e
                ))
            })?
            .ok_or_else(|| IamPolicyProviderError::PrincipalNotFound(principal_hrn.to_string()))?;

        // Step 2: Collect all policy HRNs from user and groups
        let mut policy_hrns = Vec::new();

        // Add user's attached policies (if any - User entity doesn't have attached_policy_hrns in current implementation)
        // Note: This would require extending User entity to have attached_policy_hrns like Group

        // Add policies from all groups the user belongs to
        info!("User belongs to {} groups", user.group_hrns.len());
        for group_hrn in &user.group_hrns {
            match self.group_repository.find_by_hrn(group_hrn).await {
                Ok(Some(group)) => {
                    info!(
                        "Found group {} with {} attached policies",
                        group_hrn,
                        group.attached_policy_hrns.len()
                    );
                    policy_hrns.extend_from_slice(&group.attached_policy_hrns);
                }
                Ok(None) => {
                    warn!("Group {} not found for user {}", group_hrn, principal_hrn);
                }
                Err(e) => {
                    warn!("Failed to retrieve group {}: {}", group_hrn, e);
                }
            }
        }

        // Remove duplicates
        policy_hrns.sort();
        policy_hrns.dedup();
        info!("Found {} unique policy HRNs to retrieve", policy_hrns.len());

        // Step 3: Retrieve all policies and build PolicySet
        let mut policy_set = PolicySet::new();
        let mut successful_policies = 0;
        let mut failed_policies = 0;

        for policy_hrn in &policy_hrns {
            match self.policy_repository.find_by_hrn(policy_hrn).await {
                Ok(Some(iam_policy)) => match iam_policy.as_cedar_policy() {
                    Ok(cedar_policy) => {
                        policy_set.add_policy(cedar_policy);
                        successful_policies += 1;
                        info!("Successfully loaded policy: {}", policy_hrn);
                    }
                    Err(e) => {
                        failed_policies += 1;
                        warn!(
                            "Failed to parse policy {} as Cedar policy: {}",
                            policy_hrn, e
                        );
                    }
                },
                Ok(None) => {
                    failed_policies += 1;
                    warn!("Policy not found: {}", policy_hrn);
                }
                Err(e) => {
                    failed_policies += 1;
                    warn!("Failed to retrieve policy {}: {}", policy_hrn, e);
                }
            }
        }

        info!(
            "Successfully loaded {} policies, {} failed",
            successful_policies, failed_policies
        );

        if failed_policies > 0 {
            warn!(
                "Some policies failed to load for principal {}",
                principal_hrn
            );
        }

        Ok(policy_set)
    }
}
