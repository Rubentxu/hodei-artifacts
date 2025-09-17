use crate::domain::ids::{OrganizationId, HodeiPolicyId};
use serde::{Deserialize, Serialize};
use shared::hrn::UserId;

/// Command to create a new policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePolicyCommand {
    pub policy_id: HodeiPolicyId,
    pub name: String,
    pub description: Option<String>,
    pub organization_id: OrganizationId,
    pub content: String,
    pub created_by: UserId,
}

/// Response DTO for policy creation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePolicyResponse {
    pub policy_id: HodeiPolicyId,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub version: i64,
    pub organization_id: OrganizationId,
    pub created_at: String,
    pub created_by: UserId,
}
