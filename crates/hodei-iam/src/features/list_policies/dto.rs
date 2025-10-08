//! Data Transfer Objects for list_policies feature
//!
//! This module defines the query and response DTOs for listing policies
//! with pagination support.

use serde::{Deserialize, Serialize};

/// Query for listing policies with pagination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPoliciesQuery {
    /// Maximum number of items to return (1-100)
    pub limit: u32,

    /// Offset for pagination
    pub offset: u32,
}

impl Default for ListPoliciesQuery {
    fn default() -> Self {
        Self {
            limit: 50,
            offset: 0,
        }
    }
}

impl ListPoliciesQuery {
    /// Create a new query with pagination parameters
    pub fn with_pagination(limit: u32, offset: u32) -> Self {
        Self { limit, offset }
    }
}

/// Summary information about a policy (without content)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySummary {
    /// Policy identifier
    pub id: String,

    /// Optional description
    pub description: Option<String>,

    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// Last update timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Pagination information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageInfo {
    /// Total number of policies
    pub total_count: u32,

    /// Whether there are more policies beyond the current page
    pub has_next_page: bool,

    /// Offset for the next page, if available
    pub next_offset: Option<u32>,
}

impl PageInfo {
    /// Create new page info
    pub fn new(total_count: u32, has_next_page: bool, next_offset: Option<u32>) -> Self {
        Self {
            total_count,
            has_next_page,
            next_offset,
        }
    }
}

/// Response for listing policies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPoliciesResponse {
    /// List of policy summaries
    pub policies: Vec<PolicySummary>,

    /// Pagination information
    pub page_info: PageInfo,
}

impl ListPoliciesResponse {
    /// Create a new response
    pub fn new(policies: Vec<PolicySummary>, page_info: PageInfo) -> Self {
        Self {
            policies,
            page_info,
        }
    }
}
