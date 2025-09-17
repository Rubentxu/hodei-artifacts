pub(crate) use shared::hrn::{Hrn, OrganizationId};

// Reuse shared kernel ID types to avoid duplication.
pub use shared::hrn::HodeiPolicyId;

/// Helper to create a PolicyId from organization and policy name
pub fn make_policy_id(org_id: &OrganizationId, name: &str) -> Result<HodeiPolicyId, Box<dyn std::error::Error + Send + Sync>> {
    HodeiPolicyId::new(org_id, name)
}