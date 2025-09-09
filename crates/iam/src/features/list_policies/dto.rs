// crates/iam/src/features/list_policies/dto.rs

use crate::application::ports::{PolicyFilter, PolicyList};
use crate::domain::policy::PolicyStatus;
use crate::infrastructure::errors::IamError;
use serde::{Deserialize, Serialize};

/// Query to list policies with filtering and pagination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPoliciesQuery {
    /// Status filter (optional)
    pub status: Option<PolicyStatus>,
    /// Name contains filter (optional)
    pub name_contains: Option<String>,
    /// Tags filter (optional)
    pub tags: Option<Vec<String>>,
    /// Created by filter (optional)
    pub created_by: Option<String>,
    /// Pagination limit (optional)
    pub limit: Option<u32>,
    /// Pagination offset (optional)
    pub offset: Option<u32>,
}

/// Response containing the list of policies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPoliciesResponse {
    /// The list of policies with metadata
    pub policies: PolicyList,
}

impl ListPoliciesQuery {
    /// Create a new list policies query
    pub fn new() -> Self {
        Self {
            status: None,
            name_contains: None,
            tags: None,
            created_by: None,
            limit: None,
            offset: None,
        }
    }

    /// Set status filter
    pub fn with_status(mut self, status: PolicyStatus) -> Self {
        self.status = Some(status);
        self
    }

    /// Set name contains filter
    pub fn with_name_contains(mut self, name_contains: String) -> Self {
        self.name_contains = Some(name_contains);
        self
    }

    /// Set tags filter
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = Some(tags);
        self
    }

    /// Set created by filter
    pub fn with_created_by(mut self, created_by: String) -> Self {
        self.created_by = Some(created_by);
        self
    }

    /// Set pagination
    pub fn with_pagination(mut self, limit: Option<u32>, offset: Option<u32>) -> Self {
        self.limit = limit;
        self.offset = offset;
        self
    }

    /// Convert to PolicyFilter
    pub fn to_policy_filter(&self) -> PolicyFilter {
        let mut filter = PolicyFilter::new();

        if let Some(status) = &self.status {
            filter = filter.with_status(status.clone());
        }

        if let Some(name_contains) = &self.name_contains {
            filter = filter.with_name_contains(name_contains.clone());
        }

        if let Some(tags) = &self.tags {
            filter = filter.with_tags(tags.clone());
        }

        if let Some(created_by) = &self.created_by {
            filter = filter.with_created_by(created_by.clone());
        }

        if let Some(limit) = self.limit {
            filter = filter.with_limit(limit);
        }

        if let Some(offset) = self.offset {
            filter = filter.with_offset(offset);
        }

        filter
    }

    /// Validate the query
    pub fn validate(&self) -> Result<(), IamError> {
        // Validate limit
        if let Some(limit) = self.limit {
            if limit == 0 {
                return Err(IamError::InvalidInput("Limit must be greater than 0".to_string()));
            }
            if limit > 1000 {
                return Err(IamError::InvalidInput("Limit cannot exceed 1000".to_string()));
            }
        }

        // Validate tags
        if let Some(tags) = &self.tags {
            if tags.is_empty() {
                return Err(IamError::InvalidInput("Tags filter cannot be empty".to_string()));
            }
            for tag in tags {
                if tag.trim().is_empty() {
                    return Err(IamError::InvalidInput("Tag cannot be empty".to_string()));
                }
            }
        }

        // Validate name_contains
        if let Some(name_contains) = &self.name_contains {
            if name_contains.trim().is_empty() {
                return Err(IamError::InvalidInput("Name contains filter cannot be empty".to_string()));
            }
        }

        // Validate created_by
        if let Some(created_by) = &self.created_by {
            if created_by.trim().is_empty() {
                return Err(IamError::InvalidInput("Created by filter cannot be empty".to_string()));
            }
        }

        Ok(())
    }
}

impl Default for ListPoliciesQuery {
    fn default() -> Self {
        Self::new()
    }
}

impl ListPoliciesResponse {
    /// Create a new list policies response
    pub fn new(policies: PolicyList) -> Self {
        Self { policies }
    }
}