pub(crate) use super::dto::ListPoliciesQuery;
use super::error::ListPoliciesError;
use crate::domain::policy::Policy;
use async_trait::async_trait;
use shared::hrn::UserId;

/// Trait for listing policies
#[async_trait]
pub trait PolicyLister: Send + Sync {
    async fn list_policies(&self, query: ListPoliciesQuery, user_id: &UserId) -> Result<Vec<Policy>, ListPoliciesError>;
}

/// Trait for validating list queries
#[async_trait]
pub trait ListQueryValidator: Send + Sync {
    async fn validate_query(&self, query: &ListPoliciesQuery, user_id: &UserId) -> Result<(), ListPoliciesError>;
    async fn apply_access_filter(&self, query: &ListPoliciesQuery, user_id: &UserId) -> Result<ListPoliciesQuery, ListPoliciesError>;
}

/// Trait for policy listing storage operations
#[async_trait]
pub trait PolicyListingStorage: Send + Sync {
    async fn find_all(&self, query: ListPoliciesQuery) -> Result<Vec<Policy>, ListPoliciesError>;
    async fn count(&self, query: ListPoliciesQuery) -> Result<usize, ListPoliciesError>;
}

/// Trait for audit logging during policy listing
#[async_trait]
pub trait PolicyListingAuditor: Send + Sync {
    async fn log_policy_list_access(&self, user_id: &UserId, query: &ListPoliciesQuery, result_count: usize) -> Result<(), ListPoliciesError>;
}

/// Configuration for listing behavior
#[derive(Debug, Clone)]
pub struct ListPoliciesConfig {
    pub max_limit: usize,
    pub default_limit: usize,
    pub max_offset: usize,
}

impl Default for ListPoliciesConfig {
    fn default() -> Self {
        Self {
            max_limit: 100,
            default_limit: 20,
            max_offset: 10000,
        }
    }
}
