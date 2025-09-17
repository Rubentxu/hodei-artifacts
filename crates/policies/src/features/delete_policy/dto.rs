use super::ports::DeletionMode;
use crate::domain::ids::PolicyId;
use serde::{Deserialize, Serialize};
use shared::hrn::UserId;

/// Command to delete a policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletePolicyCommand {
    pub policy_id: PolicyId,
    pub deleted_by: UserId,
    pub deletion_mode: DeletionMode,
    pub reason: Option<String>,
}

/// Response DTO for policy deletion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletePolicyResponse {
    pub policy_id: PolicyId,
    pub deletion_mode: DeletionMode,
    pub deleted_at: String,
    pub deleted_by: UserId,
    pub success: bool,
    pub message: String,
}

/// Query for deleted policies (for recovery purposes)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletedPoliciesQuery {
    pub deleted_by: Option<UserId>,
    pub deletion_date_from: Option<String>,
    pub deletion_date_to: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Response for deleted policies query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletedPoliciesResponse {
    pub policies: Vec<DeletedPolicySummary>,
    pub total_count: usize,
}

/// Summary of a deleted policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletedPolicySummary {
    pub policy_id: PolicyId,
    pub name: String,
    pub deletion_mode: DeletionMode,
    pub deleted_at: String,
    pub deleted_by: UserId,
    pub reason: Option<String>,
}

/// Command to restore a soft-deleted policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestorePolicyCommand {
    pub policy_id: PolicyId,
    pub restored_by: UserId,
    pub reason: Option<String>,
}

/// Response for policy restoration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestorePolicyResponse {
    pub policy_id: PolicyId,
    pub restored_at: String,
    pub restored_by: UserId,
    pub success: bool,
    pub message: String,
}
