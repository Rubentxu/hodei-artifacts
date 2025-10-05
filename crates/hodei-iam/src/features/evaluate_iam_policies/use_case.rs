//! Use case for evaluating IAM policies
//!
//! This use case implements the `IamPolicyEvaluator` trait from the kernel,
//! making hodei-iam responsible for evaluating its own IAM policies.
//!
//! # Architecture
//!
//! This follows the Vertical Slice Architecture (VSA) pattern:
//! - Uses segregated ports for dependencies (PolicyFinderPort)
//! - Delegates Cedar evaluation to the policies crate
//! - Implements the cross-context trait from kernel
//!
//! # Responsibilities
//!
//! 1. Retrieve IAM policies for a principal (user, service account, etc.)
//! 2. Delegate policy evaluation to the policies crate
//! 3. Return authorization decision with reason

use async_trait::async_trait;
use std::sync::Arc;
use tracing::{debug, info, warn};

use kernel::application::ports::authorization::{
    AuthorizationError, EvaluationDecision, EvaluationRequest, IamPolicyEvaluator,
};
use policies::features::evaluate_policies::{
    Decision, EvaluatePoliciesRequest, EvaluatePoliciesUseCase,
};

use super::ports::PolicyFinderPort;

/// Use case for evaluating IAM policies
///
/// This use case is the entry point for IAM policy evaluation. It:
/// 1. Finds all IAM policies that apply to a principal
/// 2. Delegates evaluation to the policies crate
/// 3. Returns an authorization decision
///
/// # Type Parameters
/// * `PF` - Policy finder implementation (repository adapter)
pub struct EvaluateIamPoliciesUseCase<PF>
where
    PF: PolicyFinderPort,
{
    /// Port for finding policies associated with a principal
    policy_finder: Arc<PF>,

    /// Generic policy evaluator from policies crate
    policy_evaluator: Arc<EvaluatePoliciesUseCase>,
}

impl<PF> EvaluateIamPoliciesUseCase<PF>
where
    PF: PolicyFinderPort,
{
    /// Create a new instance of the use case
    ///
    /// # Arguments
    /// * `policy_finder` - Implementation of PolicyFinderPort for retrieving policies
    /// * `policy_evaluator` - Generic policy evaluator from policies crate
    pub fn new(policy_finder: Arc<PF>, policy_evaluator: Arc<EvaluatePoliciesUseCase>) -> Self {
        Self {
            policy_finder,
            policy_evaluator,
        }
    }
}

#[async_trait]
impl<PF> IamPolicyEvaluator for EvaluateIamPoliciesUseCase<PF>
where
    PF: PolicyFinderPort,
{
    /// Evaluate IAM policies for an authorization request
    ///
    /// # Algorithm
    /// 1. Find all IAM policies for the principal
    /// 2. If no policies found, return implicit deny
    /// 3. Build evaluation request for policies crate
    /// 4. Delegate evaluation to policies crate
    /// 5. Convert result to EvaluationDecision
    ///
    /// # Arguments
    /// * `request` - The evaluation request containing principal, action, and resource
    ///
    /// # Returns
    /// An evaluation decision (allow/deny with reason)
    async fn evaluate_iam_policies(
        &self,
        request: EvaluationRequest,
    ) -> Result<EvaluationDecision, AuthorizationError> {
        debug!(
            principal = %request.principal.hrn(),
            action = %request.action.name(),
            resource = %request.resource.hrn(),
            "Starting IAM policy evaluation"
        );

        // Step 1: Find all IAM policies for the principal
        let policies = self
            .policy_finder
            .get_policies_for_principal(request.principal.hrn())
            .await?;

        debug!(policy_count = policies.len(), "Retrieved IAM policies");

        // Step 2: Handle empty policy case (implicit deny)
        if policies.is_empty() {
            info!(
                principal = %request.principal.hrn(),
                "No IAM policies found for principal (implicit deny)"
            );
            return Ok(EvaluationDecision {
                principal_hrn: request.principal.hrn().clone(),
                action_name: request.action.name().to_string(),
                resource_hrn: request.resource.hrn().clone(),
                decision: false,
                reason: "No IAM policies found for principal (implicit deny)".to_string(),
            });
        }

        // Step 3: Build evaluation request for policies crate
        let eval_request = EvaluatePoliciesRequest {
            policies,
            principal: request.principal.hrn().to_string(),
            action: format!("Action::\"{}\"", request.action.name()),
            resource: request.resource.hrn().to_string(),
            context: None,    // TODO: Add context support if needed
            entities: vec![], // TODO: Add entity support if needed
        };

        // Step 4: Delegate evaluation to policies crate
        let eval_response = self
            .policy_evaluator
            .execute(eval_request)
            .await
            .map_err(|e| {
                warn!(error = %e, "IAM policy evaluation failed");
                AuthorizationError::EvaluationFailed(format!("IAM policy evaluation error: {}", e))
            })?;

        // Step 5: Convert result to EvaluationDecision
        let decision = eval_response.decision == Decision::Allow;

        info!(
            decision = decision,
            evaluation_time_us = eval_response.evaluation_time_us,
            principal = %request.principal.hrn(),
            "IAM policy evaluation completed"
        );

        Ok(EvaluationDecision {
            principal_hrn: request.principal.hrn().clone(),
            action_name: request.action.name().to_string(),
            resource_hrn: request.resource.hrn().clone(),
            decision,
            reason: eval_response.reason,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::domain::Hrn;

    // Mock implementation will be in mocks.rs
    // Tests will be in use_case_test.rs
}
