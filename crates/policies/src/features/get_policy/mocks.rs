//! Mock implementations for get_policy feature testing
//!
//! This module provides mock implementations of all dependencies for unit testing.

use async_trait::async_trait;
use mockall::mock;
use std::sync::Arc;

use super::error::GetPolicyError;
use super::ports::{PolicyAccessValidator, PolicyRetrievalAuditor, PolicyRetrievalStorage};
use crate::domain::ids::{PolicyId};
use crate::domain::policy::Policy;

// Mock for PolicyAccessValidator
mock! {
    pub PolicyAccessValidator {}

    #[async_trait]
    impl PolicyAccessValidator for PolicyAccessValidator {
        async fn validate_access(&self, policy: &Policy, user_id: &UserId) -> Result<(), GetPolicyError>;
        async fn can_read_policy(&self, policy_id: &PolicyId, user_id: &UserId) -> Result<bool, GetPolicyError>;
    }
}

// Mock for PolicyRetrievalStorage
mock! {
    pub PolicyRetrievalStorage {}

    #[async_trait]
    impl PolicyRetrievalStorage for PolicyRetrievalStorage {
        async fn find_by_id(&self, policy_id: &PolicyId) -> Result<Option<Policy>, GetPolicyError>;
        async fn find_by_id_with_versions(&self, policy_id: &PolicyId) -> Result<Option<Policy>, GetPolicyError>;
    }
}

// Mock for PolicyRetrievalAuditor
mock! {
    pub PolicyRetrievalAuditor {}

    #[async_trait]
    impl PolicyRetrievalAuditor for PolicyRetrievalAuditor {
        async fn log_policy_access(&self, policy_id: &PolicyId, user_id: &UserId) -> Result<(), GetPolicyError>;
    }
}
