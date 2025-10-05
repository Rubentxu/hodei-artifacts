//! Dependency Injection configuration for evaluate_iam_policies feature
//!
//! This module provides factory functions to wire up the use case with its dependencies.
//! Following the Dependency Inversion Principle, it creates concrete implementations
//! and injects them into the use case.

use std::sync::Arc;

use kernel::application::ports::authorization::IamPolicyEvaluator;
use policies::features::evaluate_policies::EvaluatePoliciesUseCase;

use super::{adapter::PolicyRepositoryAdapter, use_case::EvaluateIamPoliciesUseCase};

/// Create the evaluate_iam_policies use case with all dependencies
///
/// This function wires up the use case with:
/// - Policy repository adapter (for finding IAM policies)
/// - Policy evaluator from policies crate (for Cedar evaluation)
///
/// # Type Parameters
/// * `PR` - Policy repository type
/// * `UR` - User repository type
/// * `GR` - Group repository type
///
/// # Arguments
/// * `policy_repo` - Repository for IAM policies
/// * `user_repo` - Repository for users (to resolve group memberships)
/// * `group_repo` - Repository for groups (to get group policies)
///
/// # Returns
/// An Arc-wrapped implementation of IamPolicyEvaluator
///
/// # Example
/// ```ignore
/// use std::sync::Arc;
/// use hodei_iam::features::evaluate_iam_policies::di::make_iam_policy_evaluator;
///
/// let policy_repo = Arc::new(SurrealPolicyRepository::new(db.clone()));
/// let user_repo = Arc::new(SurrealUserRepository::new(db.clone()));
/// let group_repo = Arc::new(SurrealGroupRepository::new(db.clone()));
///
/// let evaluator = make_iam_policy_evaluator(policy_repo, user_repo, group_repo);
/// ```
pub fn make_iam_policy_evaluator<PR, UR, GR>(
    policy_repo: Arc<PR>,
    user_repo: Arc<UR>,
    group_repo: Arc<GR>,
) -> Arc<dyn IamPolicyEvaluator>
where
    PR: Send + Sync + 'static,
    UR: Send + Sync + 'static,
    GR: Send + Sync + 'static,
{
    // Create policy finder adapter
    let policy_finder = Arc::new(PolicyRepositoryAdapter::new(
        policy_repo,
        user_repo,
        group_repo,
    ));

    // Create policy evaluator from policies crate (stateless, reusable)
    let policy_evaluator = Arc::new(EvaluatePoliciesUseCase::new());

    // Create and return the use case
    Arc::new(EvaluateIamPoliciesUseCase::new(
        policy_finder,
        policy_evaluator,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock repositories for testing DI
    struct MockPolicyRepo;
    struct MockUserRepo;
    struct MockGroupRepo;

    #[test]
    fn test_make_iam_policy_evaluator_creates_evaluator() {
        // Arrange
        let policy_repo = Arc::new(MockPolicyRepo);
        let user_repo = Arc::new(MockUserRepo);
        let group_repo = Arc::new(MockGroupRepo);

        // Act
        let evaluator = make_iam_policy_evaluator(policy_repo, user_repo, group_repo);

        // Assert
        // Just verify it compiles and creates the evaluator
        assert!(Arc::strong_count(&evaluator) == 1);
    }
}
