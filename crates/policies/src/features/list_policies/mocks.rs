//! Mock implementations for list_policies feature testing
//!
//! This module provides mock implementations of all dependencies for unit testing.

use async_trait::async_trait;
use mockall::mock;
use std::sync::Arc;

use super::error::ListPoliciesError;
use super::ports::{ListPoliciesQuery, ListQueryValidator, PolicyListingAuditor, PolicyListingStorage};
use crate::domain::policy::Policy;


// Mock for ListQueryValidator
mock! {
    pub ListQueryValidator {}

    #[async_trait]
    impl ListQueryValidator for ListQueryValidator {
        async fn validate_query(&self, query: &ListPoliciesQuery, user_id: &UserId) -> Result<(), ListPoliciesError>;
        async fn apply_access_filter(&self, query: &ListPoliciesQuery, user_id: &UserId) -> Result<ListPoliciesQuery, ListPoliciesError>;
    }
}

// Mock for PolicyListingStorage
mock! {
    pub PolicyListingStorage {}

    #[async_trait]
    impl PolicyListingStorage for PolicyListingStorage {
        async fn find_all(&self, query: ListPoliciesQuery) -> Result<Vec<Policy>, ListPoliciesError>;
        async fn count(&self, query: ListPoliciesQuery) -> Result<usize, ListPoliciesError>;
    }
}

// Mock for PolicyListingAuditor
mock! {
    pub PolicyListingAuditor {}

    #[async_trait]
    impl PolicyListingAuditor for PolicyListingAuditor {
        async fn log_policy_list_access(&self, user_id: &UserId, query: &ListPoliciesQuery, result_count: usize) -> Result<(), ListPoliciesError>;
    }
}
