// crates/iam/src/features/list_policies/dto.rs

use crate::domain::policy::{Policy, PolicyStatus};
use serde::{Serialize, Deserialize};

/// Sort criteria for policies
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PolicySortBy {
    Name,
    CreatedAt,
    UpdatedAt,
}

/// Sort order
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SortOrder {
    Asc,
    Desc,
}

/// Query for listing policies with filtering and pagination
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ListPoliciesQuery {
    pub name: Option<String>,
    pub name_contains: Option<String>,
    pub status: Option<PolicyStatus>,
    pub tags: Vec<String>,
    pub created_by: Option<String>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
    pub sort_by: Option<PolicySortBy>,
    pub sort_order: Option<SortOrder>,
}

/// Response for policy listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPoliciesResponse {
    pub policies: Vec<Policy>,
    pub total: u64,
    pub page: u32,
    pub per_page: u32,
    pub has_more: bool,
}

/// Individual policy in list response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyListItem {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub status: PolicyStatus,
    pub created_at: time::OffsetDateTime,
    pub created_by: String,
    pub tags: Vec<String>,
}

impl ListPoliciesQuery {
    /// Create a new empty query
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by name
    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    /// Filter by name containing text
    pub fn with_name_contains(mut self, name: String) -> Self {
        self.name_contains = Some(name);
        self
    }

    /// Filter by status
    pub fn with_status(mut self, status: PolicyStatus) -> Self {
        self.status = Some(status);
        self
    }

    /// Filter by tags
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Filter by creator
    pub fn with_created_by(mut self, created_by: String) -> Self {
        self.created_by = Some(created_by);
        self
    }

    /// Set pagination
    pub fn with_pagination(mut self, page: u32, per_page: u32) -> Self {
        self.page = Some(page);
        self.per_page = Some(per_page);
        self
    }

    /// Set sorting
    pub fn with_sort(mut self, sort_by: PolicySortBy, sort_order: SortOrder) -> Self {
        self.sort_by = Some(sort_by);
        self.sort_order = Some(sort_order);
        self
    }

    /// Check if query has any criteria
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.status.is_none()
            && self.name_contains.is_none()
            && self.tags.is_empty()
            && self.created_by.is_none()
    }

    /// Get effective limit (with default)
    pub fn effective_limit(&self) -> u32 {
        self.per_page.unwrap_or(50) // Default limit of 50
    }

    /// Get effective offset (with default)
    pub fn effective_offset(&self) -> u32 {
        if let (Some(page), Some(per_page)) = (self.page, self.per_page) {
            (page.saturating_sub(1)) * per_page
        } else {
            0
        }
    }
}

impl ListPoliciesResponse {
    /// Create a new policy list response
    pub fn new(policies: Vec<Policy>, total: u64) -> Self {
        let has_more = policies.len() < total as usize;
        Self {
            policies,
            total,
            page: 1,
            per_page: 20,
            has_more,
        }
    }

    /// Create a new policy list response with pagination info
    pub fn with_pagination(policies: Vec<Policy>, total: u64, page: u32, per_page: u32) -> Self {
        let has_more = (page * per_page) < total as u32;
        Self {
            policies,
            total,
            page,
            per_page,
            has_more,
        }
    }

    /// Create an empty policy list response
    pub fn empty() -> Self {
        Self {
            policies: Vec::new(),
            total: 0,
            page: 1,
            per_page: 20,
            has_more: false,
        }
    }

    /// Check if the list is empty
    pub fn is_empty(&self) -> bool {
        self.policies.is_empty()
    }

    /// Get the number of policies in this page
    pub fn len(&self) -> usize {
        self.policies.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_policies_query_builder() {
        let query = ListPoliciesQuery::new()
            .with_status(PolicyStatus::Active)
            .with_name_contains("test".to_string())
            .with_tags(vec!["engineering".to_string()])
            .with_created_by("user_123".to_string())
            .with_pagination(2, 25)
            .with_sort(PolicySortBy::Name, SortOrder::Asc);

        assert_eq!(query.status, Some(PolicyStatus::Active));
        assert_eq!(query.name_contains, Some("test".to_string()));
        assert_eq!(query.tags, vec!["engineering"]);
        assert_eq!(query.created_by, Some("user_123".to_string()));
        assert_eq!(query.page, Some(2));
        assert_eq!(query.per_page, Some(25));
        assert_eq!(query.sort_by, Some(PolicySortBy::Name));
        assert_eq!(query.sort_order, Some(SortOrder::Asc));
    }

    #[test]
    fn test_list_policies_query_defaults() {
        let query = ListPoliciesQuery::new();
        
        assert!(query.is_empty());
        assert_eq!(query.effective_limit(), 50);
        assert_eq!(query.effective_offset(), 0);
    }

    #[test]
    fn test_policy_list_response_creation() {
        let policies = vec![];
        let response = ListPoliciesResponse::new(policies, 0);
        
        assert!(response.is_empty());
        assert_eq!(response.len(), 0);
        assert_eq!(response.total, 0);
        assert!(!response.has_more);
    }

    #[test]
    fn test_policy_list_response_with_pagination() {
        let policies = vec![];
        let response = ListPoliciesResponse::with_pagination(policies, 100, 1, 20);
        
        assert!(response.has_more); // 20 < 100
        assert_eq!(response.page, 1);
        assert_eq!(response.per_page, 20);
    }

    #[test]
    fn test_policy_list_response_empty() {
        let response = ListPoliciesResponse::empty();
        
        assert!(response.is_empty());
        assert_eq!(response.len(), 0);
        assert_eq!(response.total, 0);
        assert!(!response.has_more);
    }
}