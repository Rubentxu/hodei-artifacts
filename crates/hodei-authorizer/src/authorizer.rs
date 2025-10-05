use crate::ports::{AuthorizationError, IamPolicyProvider, OrganizationBoundaryProvider};

use kernel::Hrn;

/// Authorizer service that combines IAM policies and SCPs to make authorization decisions
pub struct AuthorizerService<IAM: IamPolicyProvider, ORG: OrganizationBoundaryProvider> {
    iam_provider: IAM,
    org_provider: ORG,
    policy_evaluator: PolicyEvaluator,
}

impl<IAM: IamPolicyProvider, ORG: OrganizationBoundaryProvider> AuthorizerService<IAM, ORG> {
    /// Create a new instance of the authorizer service
    pub fn new(iam_provider: IAM, org_provider: ORG, policy_evaluator: PolicyEvaluator) -> Self {
        Self {
            iam_provider,
            org_provider,
            policy_evaluator,
        }
    }

    /// Check if a principal is authorized to perform an action on a resource
    pub async fn is_authorized(
        &self,
        request: AuthorizationRequest,
    ) -> Result<AuthorizationResponse, AuthorizationError> {
        // Get IAM policies for the principal
        let iam_policies = self
            .iam_provider
            .get_identity_policies_for(&request.principal)
            .await?;

        // Get effective SCPs for the principal's account
        let effective_scps = self
            .org_provider
            .get_effective_scps_for(&request.principal)
            .await?;

        // Combine IAM policies and SCPs
        let mut combined_policies = iam_policies;
        for scp in effective_scps {
            combined_policies.add_policy(scp.policy.clone());
        }

        // Evaluate the combined policies
        let response = self.policy_evaluator.evaluate(&combined_policies, &request);

        Ok(response)
    }
}
