// crates/iam/src/application/ports.rs

use crate::domain::policy::{Policy, PolicyStatus};

/// Filter criteria for policy queries
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PolicyFilter {
    pub name: Option<String>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
    pub sort_by: Option<PolicySortBy>,
    pub sort_order: Option<SortOrder>,
    // Legacy fields for compatibility
    pub status: Option<PolicyStatus>,
    pub name_contains: Option<String>,
    pub tags: Vec<String>,
    pub created_by: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// Sort criteria for policies
#[derive(Debug, Clone, PartialEq)]
pub enum PolicySortBy {
    Name,
    CreatedAt,
    UpdatedAt,
}

/// Sort order
#[derive(Debug, Clone, PartialEq)]
pub enum SortOrder {
    Asc,
    Desc,
}

/// Paginated list of policies
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PolicyList {
    pub policies: Vec<Policy>,
    pub total: u64,
    pub page: u32,
    pub per_page: u32,
    // Legacy fields for compatibility
    pub total_count: u64,
    pub has_more: bool,
}

// ValidationResult is now defined in domain/validation.rs
pub use crate::domain::validation::ValidationResult;

// Legacy interfaces removed - now using segregated interfaces per feature

impl PolicyFilter {
    /// Create a new empty filter
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by name
    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
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

    /// Filter by status (legacy)
    pub fn with_status(mut self, status: PolicyStatus) -> Self {
        self.status = Some(status);
        self
    }

    /// Filter by name containing text (legacy)
    pub fn with_name_contains(mut self, name: String) -> Self {
        self.name_contains = Some(name);
        self
    }

    /// Filter by tags (legacy)
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Filter by creator (legacy)
    pub fn with_created_by(mut self, created_by: String) -> Self {
        self.created_by = Some(created_by);
        self
    }

    /// Set pagination limit (legacy)
    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Set pagination offset (legacy)
    pub fn with_offset(mut self, offset: u32) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Check if filter has any criteria
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.status.is_none()
            && self.name_contains.is_none()
            && self.tags.is_empty()
            && self.created_by.is_none()
    }

    /// Get effective limit (with default)
    pub fn effective_limit(&self) -> u32 {
        self.per_page.or(self.limit).unwrap_or(50) // Default limit of 50
    }

    /// Get effective offset (with default)
    pub fn effective_offset(&self) -> u32 {
        if let (Some(page), Some(per_page)) = (self.page, self.per_page) {
            (page.saturating_sub(1)) * per_page
        } else {
            self.offset.unwrap_or(0)
        }
    }
}

impl PolicyList {
    /// Create a new policy list
    pub fn new(policies: Vec<Policy>, total: u64) -> Self {
        let total_count = total;
        let has_more = policies.len() < total as usize;
        Self {
            policies,
            total,
            page: 1,
            per_page: 20,
            total_count,
            has_more,
        }
    }

    /// Create a new policy list with pagination info
    pub fn with_pagination(policies: Vec<Policy>, total: u64, page: u32, per_page: u32) -> Self {
        let total_count = total;
        let has_more = (page * per_page) < total as u32;
        Self {
            policies,
            total,
            page,
            per_page,
            total_count,
            has_more,
        }
    }

    /// Create an empty policy list
    pub fn empty() -> Self {
        Self {
            policies: Vec::new(),
            total: 0,
            page: 1,
            per_page: 20,
            total_count: 0,
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

// ValidationResult implementation is now in domain/validation.rs

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_filter_builder() {
        let filter = PolicyFilter::new()
            .with_status(PolicyStatus::Active)
            .with_name_contains("test".to_string())
            .with_tags(vec!["engineering".to_string()])
            .with_created_by("user_123".to_string())
            .with_limit(10)
            .with_offset(20);

        assert_eq!(filter.status, Some(PolicyStatus::Active));
        assert_eq!(filter.name_contains, Some("test".to_string()));
        assert_eq!(filter.tags, vec!["engineering"]);
        assert_eq!(filter.created_by, Some("user_123".to_string()));
        assert_eq!(filter.limit, Some(10));
        assert_eq!(filter.offset, Some(20));
    }

    #[test]
    fn test_policy_filter_defaults() {
        let filter = PolicyFilter::new();

        assert!(filter.is_empty());
        assert_eq!(filter.effective_limit(), 50);
        assert_eq!(filter.effective_offset(), 0);
    }

    #[test]
    fn test_policy_filter_effective_values() {
        let filter = PolicyFilter::new().with_limit(100).with_offset(25);

        assert_eq!(filter.effective_limit(), 100);
        assert_eq!(filter.effective_offset(), 25);
    }

    #[test]
    fn test_policy_list_creation() {
        let policies = vec![];
        let list = PolicyList::new(policies, 0);

        assert!(list.is_empty());
        assert_eq!(list.len(), 0);
        assert_eq!(list.total_count, 0);
        assert!(!list.has_more);
    }

    #[test]
    fn test_policy_list_empty() {
        let list = PolicyList::empty();

        assert!(list.is_empty());
        assert_eq!(list.len(), 0);
        assert_eq!(list.total_count, 0);
        assert!(!list.has_more);
    }

    // ValidationResult tests are now in domain/validation.rs
}
