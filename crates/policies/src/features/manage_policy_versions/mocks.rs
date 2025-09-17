//! Mock implementations for manage_policy_versions feature testing
//!
//! This module provides mock implementations of all dependencies for unit testing.

use async_trait::async_trait;
use mockall::mock;
use std::sync::Arc;

use super::error::ManagePolicyVersionsError;
use super::ports::{PolicyVersionAuditor, PolicyVersionHistory, PolicyVersionStorage, PolicyVersionValidator};
use crate::domain::ids::PolicyId;
use crate::domain::policy::{Policy, PolicyVersion};
use shared::hrn::UserId;

// Mock for PolicyVersionValidator
mock! {
    pub PolicyVersionValidator {}

    #[async_trait]
    impl PolicyVersionValidator for PolicyVersionValidator {
        async fn validate_version_number(&self, version: i64) -> Result<(), ManagePolicyVersionsError>;
        async fn validate_version_content(&self, content: &str) -> Result<(), ManagePolicyVersionsError>;
        async fn validate_rollback_allowed(&self, policy: &Policy, target_version: i64, user_id: &UserId) -> Result<(), ManagePolicyVersionsError>;
    }
}

// Mock for PolicyVersionHistory
mock! {
    pub PolicyVersionHistory {}

    #[async_trait]
    impl PolicyVersionHistory for PolicyVersionHistory {
        async fn get_version_history(&self, policy_id: &PolicyId, limit: Option<usize>) -> Result<Vec<PolicyVersion>, ManagePolicyVersionsError>;
        async fn get_version_diff(&self, policy_id: &PolicyId, from_version: i64, to_version: i64) -> Result<String, ManagePolicyVersionsError>;
        async fn cleanup_old_versions(&self, policy_id: &PolicyId, keep_last: usize) -> Result<(), ManagePolicyVersionsError>;
    }
}

// Mock for PolicyVersionStorage
mock! {
    pub PolicyVersionStorage {}

    #[async_trait]
    impl PolicyVersionStorage for PolicyVersionStorage {
        async fn save_version(&self, version: &PolicyVersion) -> Result<(), ManagePolicyVersionsError>;
        async fn find_versions_by_policy(&self, policy_id: &PolicyId) -> Result<Vec<PolicyVersion>, ManagePolicyVersionsError>;
        async fn find_version(&self, policy_id: &PolicyId, version: i64) -> Result<Option<PolicyVersion>, ManagePolicyVersionsError>;
        async fn delete_version(&self, policy_id: &PolicyId, version: i64) -> Result<(), ManagePolicyVersionsError>;
        async fn update_current_version(&self, policy_id: &PolicyId, version: i64) -> Result<(), ManagePolicyVersionsError>;
    }
}

// Mock for PolicyVersionAuditor
mock! {
    pub PolicyVersionAuditor {}

    #[async_trait]
    impl PolicyVersionAuditor for PolicyVersionAuditor {
        async fn log_version_creation(&self, policy_id: &PolicyId, version: i64, user_id: &UserId) -> Result<(), ManagePolicyVersionsError>;
        async fn log_version_rollback(&self, policy_id: &PolicyId, from_version: i64, to_version: i64, user_id: &UserId) -> Result<(), ManagePolicyVersionsError>;
        async fn log_version_cleanup(&self, policy_id: &PolicyId, deleted_versions: Vec<i64>, user_id: &UserId) -> Result<(), ManagePolicyVersionsError>;
    }
}
