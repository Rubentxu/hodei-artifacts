//! Dependency Injection module for get_effective_policies_for_principal feature
//!
//! Provides factory functions to create instances of GetEffectivePoliciesForPrincipalUseCase
//! with the appropriate adapters and repositories wired together.

use super::adapter::{GroupFinderAdapter, PolicyFinderAdapter, UserFinderAdapter};
use super::use_case::GetEffectivePoliciesForPrincipalUseCase;
use crate::internal::application::ports::{GroupRepository, UserRepository};
use std::sync::Arc;

/// Create a GetEffectivePoliciesForPrincipalUseCase with in-memory repositories
///
/// This is the primary DI function for creating the use case in production.
/// It wires together all the necessary adapters and repositories.
///
/// # Arguments
/// * `user_repo` - User repository implementation
/// * `group_repo` - Group repository implementation
///
/// # Returns
/// Fully configured use case ready to execute
pub fn make_use_case<UR, GR>(
    user_repo: Arc<UR>,
    group_repo: Arc<GR>,
) -> GetEffectivePoliciesForPrincipalUseCase<
    UserFinderAdapter<UR>,
    GroupFinderAdapter<GR>,
    PolicyFinderAdapter,
>
where
    UR: UserRepository + 'static,
    GR: GroupRepository + 'static,
{
    let user_finder = Arc::new(UserFinderAdapter::new(user_repo));
    let group_finder = Arc::new(GroupFinderAdapter::new(group_repo));
    let policy_finder = Arc::new(PolicyFinderAdapter::new());

    GetEffectivePoliciesForPrincipalUseCase::new(user_finder, group_finder, policy_finder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::infrastructure::persistence::{InMemoryGroupRepository, InMemoryUserRepository};

    #[test]
    fn test_make_use_case() {
        let user_repo = Arc::new(InMemoryUserRepository::new());
        let group_repo = Arc::new(InMemoryGroupRepository::new());

        let _use_case = make_use_case(user_repo, group_repo);

        // If it compiles and constructs, the DI is working correctly
    }
}
