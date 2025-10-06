//! Use case for evaluating IAM policies
//!
//! This use case implements the `IamPolicyEvaluator` trait from the kernel,
//! making hodei-iam responsible for evaluating its own IAM policies.
//!
//! # Architecture
//!
//! This follows the Vertical Slice Architecture (VSA) pattern:
//! - Uses segregated ports for dependencies (PolicyFinderPort)
//! - Delegates Cedar evaluation to the policies crate engine
//! - Implements the cross-context trait from kernel
//!
//! # TODO: REFACTOR (Phase 2)
//!
//! This is a temporary stub implementation that allows compilation.
//! In Phase 2, this will be properly integrated with the refactored policies engine.

use async_trait::async_trait;
use std::sync::Arc;
use tracing::{debug, info, warn};

use kernel::application::ports::authorization::{
    AuthorizationError, EvaluationDecision, EvaluationRequest, IamPolicyEvaluator,
};

use super::ports::PolicyFinderPort;

/// Use case for evaluating IAM policies
///
/// This use case coordinates the evaluation of IAM policies to determine
/// if a principal (user, service account) has permission to perform
/// an action on a resource.
///
/// # Process
///
/// 1. Retrieve effective policies for the principal
/// 2. Parse policies into PolicySet
/// 3. Delegate evaluation to policies engine
/// 4. Return authorization decision
pub struct EvaluateIamPoliciesUseCase<P>
where
    P: PolicyFinderPort,
{
    policy_finder: Arc<P>,
}

impl<P> EvaluateIamPoliciesUseCase<P>
where
    P: PolicyFinderPort,
{
    /// Create a new instance of the use case
    ///
    /// # Arguments
    ///
    /// * `policy_finder` - Port for retrieving effective policies
    pub fn new(policy_finder: Arc<P>) -> Self {
        Self { policy_finder }
    }
}

#[async_trait]
impl<P> IamPolicyEvaluator for EvaluateIamPoliciesUseCase<P>
where
    P: PolicyFinderPort + Send + Sync,
{
    async fn evaluate_iam_policies(
        &self,
        request: EvaluationRequest,
    ) -> Result<EvaluationDecision, AuthorizationError> {
        info!(
            principal_hrn = %request.principal_hrn,
            action = %request.action_name,
            resource_hrn = %request.resource_hrn,
            "Starting IAM policy evaluation"
        );

        // Step 1: Retrieve effective IAM policies for the principal
        debug!(
            principal_hrn = %request.principal_hrn,
            "Retrieving effective policies"
        );

        let policy_set = self
            .policy_finder
            .get_effective_policies(&request.principal_hrn)
            .await
            .map_err(|e| {
                warn!(error = %e, "Failed to retrieve policies");
                AuthorizationError::EvaluationFailed(format!("Policy retrieval failed: {}", e))
            })?;

        debug!(
            policy_count = policy_set.policies().count(),
            "Retrieved policies"
        );

        // Step 2: Use policies engine to evaluate
        // TODO: In Phase 2, properly integrate with the refactored policies engine
        // For now, we'll do a simple stub evaluation

        // Check if there are any policies
        if policy_set.policies().count() == 0 {
            warn!("No policies found for principal, denying by default");
            return Ok(EvaluationDecision {
                principal_hrn: request.principal_hrn.clone(),
                action_name: request.action_name.clone(),
                resource_hrn: request.resource_hrn.clone(),
                decision: false,
                reason: "No IAM policies found for principal".to_string(),
            });
        }

        // Temporary stub: Always allow if policies exist
        // In Phase 2, this will use the actual policies engine evaluation
        info!(
            principal_hrn = %request.principal_hrn,
            action = %request.action_name,
            resource_hrn = %request.resource_hrn,
            "IAM policy evaluation completed (STUB - allowing by default)"
        );

        Ok(EvaluationDecision {
            principal_hrn: request.principal_hrn,
            action_name: request.action_name,
            resource_hrn: request.resource_hrn,
            decision: true,
            reason: "IAM policies evaluation (stub implementation)".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::evaluate_iam_policies::mocks::MockPolicyFinder;
    use cedar_policy::PolicySet;
    use kernel::Hrn;
    use std::str::FromStr;

    #[tokio::test]
    async fn test_evaluate_denies_when_no_policies() {
        // Arrange
        let mock_finder = Arc::new(MockPolicyFinder::new(PolicySet::new()));
        let use_case = EvaluateIamPoliciesUseCase::new(mock_finder);

        let request = EvaluationRequest {
            principal_hrn: Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap(),
            action_name: "read".to_string(),
            resource_hrn: Hrn::from_string("hrn:hodei:artifact::account123:artifact/doc1").unwrap(),
        };

        // Act
        let result = use_case.evaluate_iam_policies(request).await;

        // Assert
        assert!(result.is_ok());
        let decision = result.unwrap();
        assert!(!decision.decision, "Expected deny decision");
    }

    #[tokio::test]
    async fn test_evaluate_allows_when_policies_exist() {
        // Arrange
        let policy_text = r#"
            permit(
                principal,
                action,
                resource
            );
        "#;
        let policy_set = cedar_policy::Policy::parse(None, policy_text)
            .map(|p| PolicySet::from_policies([p]))
            .unwrap()
            .unwrap();
        let mock_finder = Arc::new(MockPolicyFinder::new(policy_set));
        let use_case = EvaluateIamPoliciesUseCase::new(mock_finder);

        let request = EvaluationRequest {
            principal_hrn: Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap(),
            action_name: "read".to_string(),
            resource_hrn: Hrn::from_string("hrn:hodei:artifact::account123:artifact/doc1").unwrap(),
        };

        // Act
        let result = use_case.evaluate_iam_policies(request).await;

        // Assert
        assert!(result.is_ok());
        let decision = result.unwrap();

        // With stub implementation, should allow when policies exist
        assert!(
            decision.decision,
            "Expected allow decision, got: {:?}",
            decision
        );
    }

    #[tokio::test]
    async fn test_evaluate_handles_policy_retrieval_error() {
        // Arrange
        let mock_finder = Arc::new(MockPolicyFinder::with_error(
            "Database connection failed".to_string(),
        ));
        let use_case = EvaluateIamPoliciesUseCase::new(mock_finder);

        let request = EvaluationRequest {
            principal_hrn: Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap(),
            action_name: "read".to_string(),
            resource_hrn: Hrn::from_string("hrn:hodei:artifact::account123:artifact/doc1").unwrap(),
        };

        // Act
        let result = use_case.evaluate_iam_policies(request).await;

        // Assert
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            matches!(error, AuthorizationError::EvaluationFailed(_)),
            "Expected EvaluationFailed, got: {:?}",
            error
        );
    }
}
