use cedar_policy::PolicySet;
use serde::{Deserialize, Serialize};

/// Query to get effective IAM policies for a principal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetEffectivePoliciesQuery {
    /// HRN of the principal (user, service account, etc.)
    pub principal_hrn: String,
}

/// Response containing effective IAM policies as a Cedar PolicySet
/// This is the PUBLIC interface - does not expose internal entities
#[derive(Debug, Clone)]
pub struct EffectivePoliciesResponse {
    /// Cedar PolicySet containing all effective IAM policies
    /// This includes:
    /// - Direct policies attached to the user
    /// - Policies from all groups the user belongs to
    /// - Policies from roles assigned to the user
    pub policies: PolicySet,
    /// HRN of the principal (for logging/debugging)
    pub principal_hrn: String,
    /// Number of policies included (for observability)
    pub policy_count: usize,
}

impl EffectivePoliciesResponse {
    pub fn new(policies: PolicySet, principal_hrn: String) -> Self {
        let policy_count = policies.policies().count();
        Self {
            policies,
            principal_hrn,
            policy_count,
        }
    }
}
