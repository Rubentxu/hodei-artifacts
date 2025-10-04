use cedar_policy::PolicySet;
use serde::{Deserialize, Serialize};

/// Query to get effective SCPs for a resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetEffectiveScpsQuery {
    /// HRN of the target entity (Account or OU)
    pub resource_hrn: String,
}

/// Response containing effective SCPs as a Cedar PolicySet
/// This is the PUBLIC interface - does not expose internal entities
#[derive(Debug, Clone)]
pub struct EffectiveScpsResponse {
    /// Cedar PolicySet containing all effective SCPs
    /// This can be directly used by the authorization engine
    pub policies: PolicySet,
    /// HRN of the target entity (for logging/debugging)
    pub target_hrn: String,
}

impl EffectiveScpsResponse {
    pub fn new(policies: PolicySet, target_hrn: String) -> Self {
        Self {
            policies,
            target_hrn,
        }
    }
}
