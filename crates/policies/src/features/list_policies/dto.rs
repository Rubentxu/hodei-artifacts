use crate::domain::ids::OrganizationId;
use serde::{Deserialize, Serialize};

/// Query parameters for listing policies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPoliciesQuery {
    pub organization_id: Option<OrganizationId>,
    pub name_filter: Option<String>,
    pub status_filter: Option<String>,
    pub created_by_filter: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
}

/// Response DTO for policy listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPoliciesResponse {
    pub policies: Vec<PolicySummaryDto>,
    pub total_count: usize,
    pub has_more: bool,
    pub query: ListPoliciesQuery,
}

/// Summary DTO for policies in list
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySummaryDto {
    pub policy_id: String,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub version: i64,
    pub created_at: String,
    pub updated_at: String,
    pub created_by: String,
}

/// Detailed response for policy listing with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPoliciesDetailedResponse {
    pub policies: Vec<PolicyDetailedDto>,
    pub total_count: usize,
    pub has_more: bool,
    pub query: ListPoliciesQuery,
    pub execution_time_ms: u64,
}

/// Detailed DTO for policies in list
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDetailedDto {
    pub policy_id: String,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub version: i64,
    pub content_preview: String, // First 200 chars of content
    pub created_at: String,
    pub updated_at: String,
    pub created_by: String,
    pub organization_id: String,
}

/// Statistics response for policy listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPoliciesStatsResponse {
    pub total_policies: usize,
    pub active_policies: usize,
    pub draft_policies: usize,
    pub archived_policies: usize,
    pub policies_by_organization: std::collections::HashMap<String, usize>,
    pub recent_activity: Vec<PolicyActivityDto>,
}

/// Activity DTO for recent policy activity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyActivityDto {
    pub policy_id: String,
    pub action: String, // "created", "updated", "deleted"
    pub timestamp: String,
    pub user_id: String,
}
