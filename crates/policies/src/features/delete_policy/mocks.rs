//! Mock implementations for delete_policy feature testing
//!
//! This module provides mock implementations of all dependencies for unit testing.

use async_trait::async_trait;
use mockall::mock;
use std::sync::Arc;

use super::error::DeletePolicyError;
use super::ports::{PolicyDeletionAuditor, PolicyDeletionRetriever, PolicyDeletionStorage, PolicyDeletionValidator};
use crate::domain::ids::{PolicyId};
use crate::domain::policy::Policy;

// Mock for PolicyDeletionValidator
mock! {
    pub PolicyDeletionValidator {}

    #[async_trait]
    impl PolicyDeletionValidator for PolicyDeletionValidator {
        async fn validate_deletion_allowed(&self, policy: &Policy, user_id: &UserId) -> Result<(), DeletePolicyError>;
        async fn check_dependencies(&self, policy: &Policy) -> Result<(), DeletePolicyError>;
    }
}

// Mock for PolicyDeletionRetriever
mock! {
    pub PolicyDeletionRetriever {}

    #[async_trait]
    impl PolicyDeletionRetriever for PolicyDeletionRetriever {
        async fn get_policy(&self, policy_id: &PolicyId) -> Result<Option<Policy>, DeletePolicyError>;
    }
}

// Mock for PolicyDeletionStorage
mock! {
    pub PolicyDeletionStorage {}

    #[async_trait]
    impl PolicyDeletionStorage for PolicyDeletionStorage {
        async fn soft_delete(&self, policy_id: &PolicyId) -> Result<(), DeletePolicyError>;
        async fn hard_delete(&self, policy_id: &PolicyId) -> Result<(), DeletePolicyError>;
        async fn archive_versions(&self, policy_id: &PolicyId) -> Result<(), DeletePolicyError>;
    }
}

// Mock for PolicyDeletionAuditor
mock! {
    pub PolicyDeletionAuditor {}

    #[async_trait]
    impl PolicyDeletionAuditor for PolicyDeletionAuditor {
        async fn log_policy_deletion(&self, policy_id: &PolicyId, user_id: &UserId) -> Result<(), DeletePolicyError>;
    }
}
