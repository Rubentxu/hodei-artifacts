//! Dependency Injection configuration for evaluate_iam_policies feature
//!
//! This module provides factory functions to construct the use case with its dependencies.
//! Following the Dependency Inversion Principle, dependencies are injected as trait objects.
//!
//! # TODO: REFACTOR (Phase 2)
//!
//! This is a temporary implementation. In Phase 2, when we properly integrate
//! with repositories, this will be updated to inject real infrastructure adapters.

use std::sync::Arc;

use super::adapter::{InMemoryPolicyFinderAdapter, SurrealPolicyFinderAdapter};
use super::ports::PolicyFinderPort;
use super::use_case::EvaluateIamPoliciesUseCase;

/// Create the evaluate_iam_policies use case with in-memory dependencies
///
/// This factory is useful for testing and development.
///
/// # Returns
///
/// A configured `EvaluateIamPoliciesUseCase` with in-memory adapters
///
/// # Example
///
/// ```rust,ignore
/// use hodei_iam::features::evaluate_iam_policies::di;
///
/// let use_case = di::make_in_memory_use_case();
/// ```
pub fn make_in_memory_use_case() -> EvaluateIamPoliciesUseCase<InMemoryPolicyFinderAdapter> {
    let policy_finder = Arc::new(InMemoryPolicyFinderAdapter::new());
    EvaluateIamPoliciesUseCase::new(policy_finder)
}

/// Create the evaluate_iam_policies use case with a custom policy finder
///
/// This factory allows injecting a custom implementation of PolicyFinderPort,
/// which is useful for testing with mocks or for providing alternative implementations.
///
/// # Arguments
///
/// * `policy_finder` - Implementation of PolicyFinderPort to use
///
/// # Returns
///
/// A configured `EvaluateIamPoliciesUseCase` with the provided adapter
///
/// # Example
///
/// ```rust,ignore
/// use hodei_iam::features::evaluate_iam_policies::{di, mocks::MockPolicyFinder};
/// use cedar_policy::PolicySet;
///
/// let mock_finder = Arc::new(MockPolicyFinder::empty());
/// let use_case = di::make_use_case_with_finder(mock_finder);
/// ```
pub fn make_use_case_with_finder<P>(policy_finder: Arc<P>) -> EvaluateIamPoliciesUseCase<P>
where
    P: PolicyFinderPort,
{
    EvaluateIamPoliciesUseCase::new(policy_finder)
}

/// Create the evaluate_iam_policies use case with SurrealDB dependencies
///
/// This factory creates the use case with adapters that connect to SurrealDB.
///
/// # TODO: IMPLEMENTATION
///
/// In Phase 2, this will accept a SurrealDB connection pool as a parameter
/// and inject it into the adapters.
///
/// # Returns
///
/// A configured `EvaluateIamPoliciesUseCase` with SurrealDB adapters
///
/// # Example (Future)
///
/// ```rust,ignore
/// use hodei_iam::features::evaluate_iam_policies::di;
///
/// let db_pool = /* SurrealDB connection pool */;
/// let use_case = di::make_surreal_use_case(db_pool);
/// ```
pub fn make_surreal_use_case() -> EvaluateIamPoliciesUseCase<SurrealPolicyFinderAdapter> {
    let policy_finder = Arc::new(SurrealPolicyFinderAdapter::new());
    EvaluateIamPoliciesUseCase::new(policy_finder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::Hrn;
    use kernel::application::ports::authorization::{EvaluationRequest, IamPolicyEvaluator};

    #[tokio::test]
    async fn test_make_in_memory_use_case() {
        let use_case = make_in_memory_use_case();

        let request = EvaluationRequest {
            principal_hrn: Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap(),
            action_name: "read".to_string(),
            resource_hrn: Hrn::from_string("hrn:hodei:artifact::account123:artifact/doc1").unwrap(),
        };

        // Should not panic and should return a decision
        let result = use_case.evaluate_iam_policies(request).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_make_use_case_with_custom_finder() {
        use super::super::mocks::MockPolicyFinder;

        let mock_finder = Arc::new(MockPolicyFinder::empty());
        let use_case = make_use_case_with_finder(mock_finder);

        let request = EvaluationRequest {
            principal_hrn: Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap(),
            action_name: "read".to_string(),
            resource_hrn: Hrn::from_string("hrn:hodei:artifact::account123:artifact/doc1").unwrap(),
        };

        let result = use_case.evaluate_iam_policies(request).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_make_surreal_use_case() {
        let use_case = make_surreal_use_case();

        let request = EvaluationRequest {
            principal_hrn: Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap(),
            action_name: "read".to_string(),
            resource_hrn: Hrn::from_string("hrn:hodei:artifact::account123:artifact/doc1").unwrap(),
        };

        // Should not panic and should return a decision
        let result = use_case.evaluate_iam_policies(request).await;
        assert!(result.is_ok());
    }
}
