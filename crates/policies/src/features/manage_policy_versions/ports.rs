use super::error::ManagePolicyVersionsError;
use crate::domain::ids::PolicyId;
use crate::domain::policy::{Policy, PolicyVersion};
use async_trait::async_trait;
use shared::hrn::UserId;


/// Trait for managing policy versions
#[async_trait]
pub trait PolicyVersionManager: Send + Sync {
    async fn create_version(&self, policy_id: &PolicyId, content: String, user_id: &UserId) -> Result<PolicyVersion, ManagePolicyVersionsError>;
    async fn get_versions(&self, policy_id: &PolicyId) -> Result<Vec<PolicyVersion>, ManagePolicyVersionsError>;
    async fn get_version(&self, policy_id: &PolicyId, version: i64) -> Result<Option<PolicyVersion>, ManagePolicyVersionsError>;
    async fn rollback_to_version(&self, policy_id: &PolicyId, version: i64, user_id: &UserId) -> Result<(), ManagePolicyVersionsError>;
}

/// Trait for validating version operations
#[async_trait]
pub trait PolicyVersionValidator: Send + Sync {
    async fn validate_version_number(&self, version: i64) -> Result<(), ManagePolicyVersionsError>;
    async fn validate_version_content(&self, content: &str) -> Result<(), ManagePolicyVersionsError>;
    async fn validate_rollback_allowed(&self, policy: &Policy, target_version: i64, user_id: &UserId) -> Result<(), ManagePolicyVersionsError>;
}

/// Trait for version history management
#[async_trait]
pub trait PolicyVersionHistory: Send + Sync {
    async fn get_version_history(&self, policy_id: &PolicyId, limit: Option<usize>) -> Result<Vec<PolicyVersion>, ManagePolicyVersionsError>;
    async fn get_version_diff(&self, policy_id: &PolicyId, from_version: i64, to_version: i64) -> Result<String, ManagePolicyVersionsError>;
    async fn cleanup_old_versions(&self, policy_id: &PolicyId, keep_last: usize) -> Result<(), ManagePolicyVersionsError>;
}

/// Trait for version storage operations
#[async_trait]
pub trait PolicyVersionStorage: Send + Sync {
    async fn save_version(&self, version: &PolicyVersion) -> Result<(), ManagePolicyVersionsError>;
    async fn find_versions_by_policy(&self, policy_id: &PolicyId) -> Result<Vec<PolicyVersion>, ManagePolicyVersionsError>;
    async fn find_version(&self, policy_id: &PolicyId, version: i64) -> Result<Option<PolicyVersion>, ManagePolicyVersionsError>;
    async fn delete_version(&self, policy_id: &PolicyId, version: i64) -> Result<(), ManagePolicyVersionsError>;
    async fn update_current_version(&self, policy_id: &PolicyId, version: i64) -> Result<(), ManagePolicyVersionsError>;
}

/// Trait for audit logging during version operations
#[async_trait]
pub trait PolicyVersionAuditor: Send + Sync {
    async fn log_version_creation(&self, policy_id: &PolicyId, version: i64, user_id: &UserId) -> Result<(), ManagePolicyVersionsError>;
    async fn log_version_rollback(&self, policy_id: &PolicyId, from_version: i64, to_version: i64, user_id: &UserId) -> Result<(), ManagePolicyVersionsError>;
    async fn log_version_cleanup(&self, policy_id: &PolicyId, deleted_versions: Vec<i64>, user_id: &UserId) -> Result<(), ManagePolicyVersionsError>;
}
