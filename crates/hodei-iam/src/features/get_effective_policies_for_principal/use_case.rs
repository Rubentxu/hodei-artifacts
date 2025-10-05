//! Use case for getting effective IAM policies for a principal
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
//! - Returns policies as strings to maintain decoupling from Cedar
//! - Does NOT expose internal entities (User, Group, Policy) to consumers

use crate::features::get_effective_policies_for_principal::dto::{
    EffectivePoliciesResponse, GetEffectivePoliciesQuery,
};
use crate::features::get_effective_policies_for_principal::error::{
    GetEffectivePoliciesError, GetEffectivePoliciesResult,
};
use crate::features::get_effective_policies_for_principal::ports::{
    GroupFinderPort, PolicyFinderPort, UserFinderPort,
};
use kernel::domain::Hrn;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Use case for obtaining effective IAM policies for a principal
///
/// This use case is the primary way for other crates to access IAM policies.
/// It returns policy documents as strings, NOT internal entities or Cedar types.
///
/// # Responsibilities
/// - Resolve the principal (User or ServiceAccount)
/// - Get groups to which the principal belongs
/// - Collect direct policies from the principal
/// - Collect policies from all groups
/// - Return all policies as a list of strings
///
/// # Type Parameters
/// * `UF` - User finder implementation
/// * `GF` - Group finder implementation
/// * `PF` - Policy finder implementation
pub struct GetEffectivePoliciesForPrincipalUseCase<UF, GF, PF>
where
    UF: UserFinderPort,
    GF: GroupFinderPort,
    PF: PolicyFinderPort,
{
    user_finder: Arc<UF>,
    group_finder: Arc<GF>,
    policy_finder: Arc<PF>,
}

impl<UF, GF, PF> GetEffectivePoliciesForPrincipalUseCase<UF, GF, PF>
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
    /// 6. Return all policies as strings
    ///
    /// # Arguments
    /// * `query` - Query containing the principal HRN
    ///
    /// # Returns
    /// A response containing all effective policies as strings
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
        let groups = self
            .group_finder
            .find_groups_by_user_hrn(&user.hrn)
            .await
            .map_err(|e| GetEffectivePoliciesError::RepositoryError(e.to_string()))?;

        info!(
            group_count = groups.len(),
            "Principal belongs to {} group(s)",
            groups.len()
        );

        // Step 4: Collect direct policies from the principal
        let principal_policies = self
            .policy_finder
            .find_policies_by_principal(&user.hrn)
            .await
            .map_err(|e| GetEffectivePoliciesError::RepositoryError(e.to_string()))?;

        debug!(
            direct_policy_count = principal_policies.len(),
            "Found direct policies for principal"
        );

        // Step 5: Collect policies from all groups
        let mut all_group_policies = Vec::new();
        for group in &groups {
            let group_policies = self
                .policy_finder
                .find_policies_by_principal(&group.hrn)
                .await
                .map_err(|e| GetEffectivePoliciesError::RepositoryError(e.to_string()))?;

            debug!(
                group_name = %group.name,
                group_hrn = %group.hrn,
                policy_count = group_policies.len(),
                "Found policies for group"
            );

            all_group_policies.extend(group_policies);
        }

        // Step 6: Combine all policies
        let all_policies: Vec<String> = principal_policies
            .into_iter()
            .chain(all_group_policies)
            .collect();

        info!(
            principal = %query.principal_hrn,
            total_policies = all_policies.len(),
            "Successfully collected effective policies"
        );

        Ok(EffectivePoliciesResponse::new(
            all_policies,
            query.principal_hrn,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::domain::{Group, User};
    use async_trait::async_trait;
    use std::sync::Arc;

    // Mock UserFinderPort
    struct MockUserFinder {
        users: Vec<User>,
    }

    #[async_trait]
    impl UserFinderPort for MockUserFinder {
        async fn find_by_hrn(
            &self,
            hrn: &Hrn,
        ) -> Result<Option<User>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(self.users.iter().find(|u| &u.hrn == hrn).cloned())
        }
    }

    // Mock GroupFinderPort
    struct MockGroupFinder {
        groups: Vec<Group>,
    }

    #[async_trait]
    impl GroupFinderPort for MockGroupFinder {
        async fn find_groups_by_user_hrn(
            &self,
            _user_hrn: &Hrn,
        ) -> Result<Vec<Group>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(self.groups.clone())
        }
    }

    // Mock PolicyFinderPort
    struct MockPolicyFinder {
        policies: Vec<(Hrn, Vec<String>)>,
    }

    #[async_trait]
    impl PolicyFinderPort for MockPolicyFinder {
        async fn find_policies_by_principal(
            &self,
            principal_hrn: &Hrn,
        ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(self
                .policies
                .iter()
                .find(|(hrn, _)| hrn == principal_hrn)
                .map(|(_, policies)| policies.clone())
                .unwrap_or_default())
        }
    }

    #[tokio::test]
    async fn test_execute_with_valid_user_and_policies() {
        let user_hrn = Hrn::new("iam", "user", "alice");
        let user = User {
            hrn: user_hrn.clone(),
            name: "Alice".to_string(),
            email: "alice@example.com".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let user_finder = Arc::new(MockUserFinder { users: vec![user] });
        let group_finder = Arc::new(MockGroupFinder { groups: vec![] });
        let policy_finder = Arc::new(MockPolicyFinder {
            policies: vec![(
                user_hrn.clone(),
                vec!["permit(principal, action, resource);".to_string()],
            )],
        });

        let use_case =
            GetEffectivePoliciesForPrincipalUseCase::new(user_finder, group_finder, policy_finder);

        let query = GetEffectivePoliciesQuery {
            principal_hrn: user_hrn.to_string(),
        };

        let result = use_case.execute(query).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.policy_count, 1);
        assert_eq!(response.policies.len(), 1);
    }

    #[tokio::test]
    async fn test_execute_with_user_and_group_policies() {
        let user_hrn = Hrn::new("iam", "user", "bob");
        let group_hrn = Hrn::new("iam", "group", "developers");

        let user = User {
            hrn: user_hrn.clone(),
            name: "Bob".to_string(),
            email: "bob@example.com".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let group = Group {
            hrn: group_hrn.clone(),
            name: "Developers".to_string(),
            description: Some("Developer group".to_string()),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let user_finder = Arc::new(MockUserFinder { users: vec![user] });
        let group_finder = Arc::new(MockGroupFinder {
            groups: vec![group],
        });
        let policy_finder = Arc::new(MockPolicyFinder {
            policies: vec![
                (
                    user_hrn.clone(),
                    vec!["permit(principal, action, resource);".to_string()],
                ),
                (
                    group_hrn.clone(),
                    vec!["permit(principal, action == Action::\"read\", resource);".to_string()],
                ),
            ],
        });

        let use_case =
            GetEffectivePoliciesForPrincipalUseCase::new(user_finder, group_finder, policy_finder);

        let query = GetEffectivePoliciesQuery {
            principal_hrn: user_hrn.to_string(),
        };

        let result = use_case.execute(query).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.policy_count, 2);
        assert_eq!(response.policies.len(), 2);
    }

    #[tokio::test]
    async fn test_execute_with_user_not_found() {
        let user_hrn = Hrn::new("iam", "user", "nonexistent");

        let user_finder = Arc::new(MockUserFinder { users: vec![] });
        let group_finder = Arc::new(MockGroupFinder { groups: vec![] });
        let policy_finder = Arc::new(MockPolicyFinder { policies: vec![] });

        let use_case =
            GetEffectivePoliciesForPrincipalUseCase::new(user_finder, group_finder, policy_finder);

        let query = GetEffectivePoliciesQuery {
            principal_hrn: user_hrn.to_string(),
        };

        let result = use_case.execute(query).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            GetEffectivePoliciesError::PrincipalNotFound(_) => {}
            e => panic!("Expected PrincipalNotFound error, got {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_execute_with_invalid_hrn() {
        let user_finder = Arc::new(MockUserFinder { users: vec![] });
        let group_finder = Arc::new(MockGroupFinder { groups: vec![] });
        let policy_finder = Arc::new(MockPolicyFinder { policies: vec![] });

        let use_case =
            GetEffectivePoliciesForPrincipalUseCase::new(user_finder, group_finder, policy_finder);

        let query = GetEffectivePoliciesQuery {
            principal_hrn: "invalid-hrn".to_string(),
        };

        let result = use_case.execute(query).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            GetEffectivePoliciesError::InvalidPrincipalHrn(_) => {}
            e => panic!("Expected InvalidPrincipalHrn error, got {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_execute_with_invalid_principal_type() {
        let resource_hrn = Hrn::new("s3", "bucket", "my-bucket");

        let user_finder = Arc::new(MockUserFinder { users: vec![] });
        let group_finder = Arc::new(MockGroupFinder { groups: vec![] });
        let policy_finder = Arc::new(MockPolicyFinder { policies: vec![] });

        let use_case =
            GetEffectivePoliciesForPrincipalUseCase::new(user_finder, group_finder, policy_finder);

        let query = GetEffectivePoliciesQuery {
            principal_hrn: resource_hrn.to_string(),
        };

        let result = use_case.execute(query).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            GetEffectivePoliciesError::InvalidPrincipalType(_) => {}
            e => panic!("Expected InvalidPrincipalType error, got {:?}", e),
        }
    }
}
