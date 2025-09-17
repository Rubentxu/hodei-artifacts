use crate::domain::ids::PolicyId;
use serde::{Deserialize, Serialize};

/// Query to get a policy by ID
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPolicyQuery {
    pub policy_id: PolicyId,
    pub include_versions: bool,
}

/// Response DTO for policy retrieval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPolicyResponse {
    pub policy_id: PolicyId,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub version: i64,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
    pub created_by: String,
}

/// Detailed response with version information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPolicyDetailsResponse {
    pub policy_id: PolicyId,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub version: i64,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
    pub created_by: String,
    pub versions_count: i64,
    pub last_modified_by: Option<String>,
}

/// Response when policy is not found
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyNotFoundResponse {
    pub policy_id: PolicyId,
    pub message: String,
    pub requested_at: String,
}
