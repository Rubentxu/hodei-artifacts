use crate::domain::ids::PolicyId;
use serde::{Deserialize, Serialize};
use shared::hrn::UserId;

/// Command to update an existing policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePolicyCommand {
    pub name: Option<String>,
    pub description: Option<String>,
    pub content: Option<String>,
    pub expected_version: Option<i64>,
    pub updated_by: UserId,
}

/// Response DTO for policy update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePolicyResponse {
    pub policy_id: PolicyId,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub version: i64,
    pub updated_at: String,
    pub updated_by: UserId,
}

/// Summary of changes made during update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyUpdateChanges {
    pub name_changed: bool,
    pub description_changed: bool,
    pub content_changed: bool,
    pub version_incremented: bool,
}

/// Query for policy update history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyUpdateHistoryQuery {
    pub policy_id: PolicyId,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Response for policy update history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyUpdateHistoryResponse {
    pub policy_id: PolicyId,
    pub updates: Vec<PolicyUpdateEntry>,
}

/// Entry in policy update history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyUpdateEntry {
    pub version: i64,
    pub updated_at: String,
    pub updated_by: UserId,
    pub changes: PolicyUpdateChanges,
    pub change_description: String,
}
