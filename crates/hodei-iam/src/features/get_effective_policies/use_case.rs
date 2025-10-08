//! Use case for getting effective IAM policies
//!
//! This use case implements the business logic for retrieving all policies that
//! apply to a given principal (user or service account), including:
//! - Direct policies attached to the principal
//! - Policies inherited from groups
//! - Policies from assumed roles (future)
//!
//! # Architecture
//!
//! This follows the Vertical Slice Architecture (VSA) pattern:
//! - Uses segregated ports for dependencies (UserFinderPort, GroupFinderPort, PolicyFinderPort)
//! - Returns policies as kernel types for strong typing
//! - Does NOT expose internal entities to consumers

use crate::features::get_effective_policies::dto::{
    EffectivePoliciesResponse, GetEffectivePoliciesQuery,
};
use crate::features::get_effective_policies::error::{
    GetEffectivePoliciesError, GetEffectivePoliciesResult,
};
use crate::features::get_effective_policies::ports::{
    GroupFinderPort, PolicyFinderPort, UserFinderPort,
};
use kernel::domain::Hrn;
use kernel::domain::policy::HodeiPolicySet;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Use case for obtaining effective IAM policies for a principal
///
/// This use case is the primary way for other crates to access IAM policies.
/// It returns policy documents as kernel types, not internal entities.
///
/// # Responsibilities
/// - Resolve the principal (User or ServiceAccount)
/// - Get groups to which the principal belongs
/// - Collect direct policies from the principal
/// - Collect policies from all groups
/// - Return all policies as a HodeiPolicySet
///
/// # Type Parameters
/// * `UF` - User finder implementation
/// * `GF` - Group finder implementation
/// * `PF` - Policy finder implementation
pub struct GetEffectivePoliciesUseCase<UF, GF, PF>
where
    UF: UserFinderPort,
    GF: GroupFinderPort,
    PF: PolicyFinderPort,
{
    user_finder: Arc<UF>,
    group_finder: Arc<GF>,
    policy_finder: Arc<PF>,
}

impl<UF, GF, PF> GetEffectivePoliciesUseCase<UF, GF, PF>
where
    UF: UserFinderPort,
    GF: GroupFinderPort,
    PF: PolicyFinderPort,
{
    /// Create a new instance of the use case
    ///
    /// # Arguments
    /// * `user_finder` - Port for finding users
    /// * `group_finder` - Port for finding groups
    /// * `policy_finder` - Port for finding policies
    pub fn new(user_finder: Arc<UF>, group_finder: Arc<GF>, policy_finder: Arc<PF>) -> Self {
        Self {
            user_finder,
            group_finder,
            policy_finder,
        }
    }

    /// Execute the use case to get effective IAM policies
    ///
    /// This is the public method that other crates should use.
    /// It does NOT expose internal entities.
    ///
    /// # Algorithm
    /// 1. Validate and parse the principal HRN
    /// 2. Find the user/service-account
    /// 3. Get groups to which the principal belongs
    /// 4. Collect direct policies from the principal
    /// 5. Collect policies from all groups
    /// 6. Return all policies as a HodeiPolicySet
    ///
    /// # Arguments
    /// * `query` - Query containing the principal HRN
    ///
    /// # Returns
    /// A response containing all effective policies as a HodeiPolicySet
    pub async fn execute(
        &self,
        query: GetEffectivePoliciesQuery,
    ) -> GetEffectivePoliciesResult<EffectivePoliciesResponse> {
        info!(
            principal = %query.principal_hrn,
            "Getting effective policies for principal"
        );

        // Step 1: Validate and parse the principal HRN
        let principal_hrn = Hrn::from_string(&query.principal_hrn).ok_or_else(|| {
            GetEffectivePoliciesError::InvalidPrincipalHrn(query.principal_hrn.clone())
        })?;

        debug!(
            service = %principal_hrn.service,
            resource_type = %principal_hrn.resource_type,
            "Parsed principal HRN"
        );

        // Validate that the resource type is valid for a principal
        let resource_type_lower = principal_hrn.resource_type.to_string().to_lowercase();
        match resource_type_lower.as_str() {
            "user" | "service-account" => {
                debug!("Valid principal type: {}", resource_type_lower);
            }
            _ => {
                warn!(
                    resource_type = %principal_hrn.resource_type,
                    "Invalid principal type"
                );
                return Err(GetEffectivePoliciesError::InvalidPrincipalType(
                    principal_hrn.resource_type.to_string(),
                ));
            }
        }

        // Step 2: Find the user (verify that it exists)
        let user = self
            .user_finder
            .find_by_hrn(&principal_hrn)
            .await
            .map_err(|e| GetEffectivePoliciesError::RepositoryError(e.to_string()))?
            .ok_or_else(|| {
                warn!(
                    principal = %query.principal_hrn,
                    "Principal not found"
                );
                GetEffectivePoliciesError::PrincipalNotFound(query.principal_hrn.clone())
            })?;

        info!(
            user_name = %user.name,
            user_hrn = %user.hrn,
            "Found principal"
        );

        // Step 3: Get groups to which the principal belongs
        let groups =
            self.group_finder
                .find_groups_by_user_hrn(&Hrn::from_string(&user.hrn).ok_or_else(|| {
                    GetEffectivePoliciesError::InvalidPrincipalHrn(user.hrn.clone())
                })?)
                .await
                .map_err(|e| GetEffectivePoliciesError::RepositoryError(e.to_string()))?;

        info!(
            group_count = groups.len(),
            "Principal belongs to {} group(s)",
            groups.len()
        );

        // Initialize the policy set
        let mut effective_policies = HodeiPolicySet::default();

        // Step 4: Collect direct policies from the principal
        let principal_policies =
            self.policy_finder
                .find_policies_by_principal(&Hrn::from_string(&user.hrn).ok_or_else(|| {
                    GetEffectivePoliciesError::InvalidPrincipalHrn(user.hrn.clone())
                })?)
                .await
                .map_err(|e| GetEffectivePoliciesError::RepositoryError(e.to_string()))?;

        debug!(
            direct_policy_count = principal_policies.len(),
            "Found direct policies for principal"
        );

        // Add principal policies to the set
        for policy in principal_policies {
            effective_policies.add(policy);
        }

        // Step 5: Collect policies from all groups
        for group in &groups {
            let group_policies = self
                .policy_finder
                .find_policies_by_principal(&Hrn::from_string(&group.hrn).ok_or_else(|| {
                    GetEffectivePoliciesError::InvalidPrincipalHrn(group.hrn.clone())
                })?)
                .await
                .map_err(|e| GetEffectivePoliciesError::RepositoryError(e.to_string()))?;

            debug!(
                group_name = %group.name,
                group_hrn = %group.hrn,
                policy_count = group_policies.len(),
                "Found policies for group"
            );

            // Add group policies to the set
            for policy in group_policies {
                effective_policies.add(policy);
            }
        }

        info!(
            principal = %query.principal_hrn,
            total_policies = effective_policies.len(),
            "Successfully collected effective policies"
        );

        Ok(EffectivePoliciesResponse::new(
            effective_policies,
            query.principal_hrn,
        ))
    }
}
