use crate::domain::ids::PolicyId;
use serde::{Deserialize, Serialize};
use shared::hrn::UserId;

/// Command to create a new policy version
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePolicyVersionCommand {
    pub policy_id: PolicyId,
    pub content: String,
    pub created_by: UserId,
}

/// Command to rollback policy to specific version
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackPolicyVersionCommand {
    pub policy_id: PolicyId,
    pub target_version: i64,
    pub rollback_by: UserId,
    pub reason: Option<String>,
}

/// Query for policy versions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPolicyVersionsQuery {
    pub policy_id: PolicyId,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Response DTO for version creation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePolicyVersionResponse {
    pub policy_id: PolicyId,
    pub version: i64,
    pub content: String,
    pub created_at: String,
    pub created_by: UserId,
}

/// Response DTO for version rollback
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackPolicyVersionResponse {
    pub policy_id: PolicyId,
    pub from_version: i64,
    pub to_version: i64,
    pub rolled_back_at: String,
    pub rolled_back_by: UserId,
    pub success: bool,
    pub message: String,
}

/// Response for listing policy versions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPolicyVersionsResponse {
    pub policy_id: PolicyId,
    pub versions: Vec<PolicyVersionSummaryDto>,
    pub total_count: usize,
}

/// Summary DTO for policy version
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyVersionSummaryDto {
    pub version: i64,
    pub created_at: String,
    pub created_by: UserId,
    pub content_length: usize,
    pub is_current: bool,
}

/// Detailed DTO for policy version
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyVersionDetailDto {
    pub policy_id: PolicyId,
    pub version: i64,
    pub content: String,
    pub created_at: String,
    pub created_by: UserId,
    pub is_current: bool,
}

/// Response for version comparison
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparePolicyVersionsResponse {
    pub policy_id: PolicyId,
    pub from_version: i64,
    pub to_version: i64,
    pub diff: String,
    pub from_content: String,
    pub to_content: String,
}

/// Command to clean up old versions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupPolicyVersionsCommand {
    pub policy_id: PolicyId,
    pub keep_last: usize,
    pub cleanup_by: UserId,
}

/// Response for version cleanup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupPolicyVersionsResponse {
    pub policy_id: PolicyId,
    pub deleted_versions: Vec<i64>,
    pub kept_versions: Vec<i64>,
    pub cleanup_at: String,
    pub cleanup_by: UserId,
}
