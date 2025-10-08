//! Data Transfer Objects for list_policies feature
//!
//! This module defines the query and response DTOs for listing policies
//! with pagination support.

use kernel::Hrn;
use serde::{Deserialize, Serialize};

/// Query for listing policies with pagination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPoliciesQuery {
    /// Maximum number of items to return (1-100)
    pub limit: usize,

    /// Offset for pagination
    pub offset: usize,
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
    pub fn with_pagination(limit: usize, offset: usize) -> Self {
        Self { limit, offset }
    }

    /// Create a new query with only limit specified (offset defaults to 0)
    pub fn with_limit(limit: usize) -> Self {
        Self { limit, offset: 0 }
    }
}

/// Summary information about a policy (without content)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySummary {
    /// Policy HRN (Hierarchical Resource Name)
    pub hrn: Hrn,

    /// Policy name
    pub name: String,

    /// Optional description
    pub description: Option<String>,
}

/// Pagination information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageInfo {
    /// Total number of policies
    pub total_count: usize,

    /// Whether there are more policies beyond the current page
    pub has_next_page: bool,

    /// Whether there are previous pages
    pub has_previous_page: bool,
}

impl PageInfo {
    /// Create new page info
    pub fn new(total_count: usize, has_next_page: bool, has_previous_page: bool) -> Self {
        Self {
            total_count,
            has_next_page,
            has_previous_page,
        }
    }
}

/// Response for listing policies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPoliciesResponse {
    /// List of policy summaries
    pub policies: Vec<PolicySummary>,

    /// Total number of policies
    pub total_count: usize,

    /// Whether there are more policies beyond the current page
    pub has_next_page: bool,

    /// Whether there are previous pages
    pub has_previous_page: bool,
}

impl ListPoliciesResponse {
    /// Create a new response
    pub fn new(
        policies: Vec<PolicySummary>,
        total_count: usize,
        has_next_page: bool,
        has_previous_page: bool,
    ) -> Self {
        Self {
            policies,
            total_count,
            has_next_page,
            has_previous_page,
        }
    }
}
