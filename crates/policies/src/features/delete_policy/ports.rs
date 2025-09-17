use super::dto::DeletePolicyCommand;
use super::error::DeletePolicyError;
use crate::domain::ids::PolicyId;
use crate::domain::policy::Policy;
use async_trait::async_trait;
use shared::hrn::UserId;

/// Trait for deleting policies
#[async_trait]
pub trait PolicyDeleter: Send + Sync {
    async fn delete_policy(&self, command: DeletePolicyCommand) -> Result<(), DeletePolicyError>;
}

/// Trait for validating policy deletion
#[async_trait]
pub trait PolicyDeletionValidator: Send + Sync {
    async fn validate_deletion_allowed(&self, policy: &Policy, user_id: &UserId) -> Result<(), DeletePolicyError>;
    async fn check_dependencies(&self, policy: &Policy) -> Result<(), DeletePolicyError>;
}

/// Trait for policy retrieval during deletion
#[async_trait]
pub trait PolicyDeletionRetriever: Send + Sync {
    async fn get_policy(&self, policy_id: &PolicyId) -> Result<Option<Policy>, DeletePolicyError>;
}

/// Trait for policy storage operations during deletion
#[async_trait]
pub trait PolicyDeletionStorage: Send + Sync {
    async fn soft_delete(&self, policy_id: &PolicyId) -> Result<(), DeletePolicyError>;
    async fn hard_delete(&self, policy_id: &PolicyId) -> Result<(), DeletePolicyError>;
    async fn archive_versions(&self, policy_id: &PolicyId) -> Result<(), DeletePolicyError>;
}

/// Trait for audit logging during policy deletion
#[async_trait]
pub trait PolicyDeletionAuditor: Send + Sync {
    async fn log_policy_deletion(&self, policy_id: &PolicyId, user_id: &UserId) -> Result<(), DeletePolicyError>;
}

/// Configuration for deletion behavior
#[derive(Debug, Clone)]
pub enum DeletionMode {
    Soft,  // Mark as deleted but keep data
    Hard,  // Permanently remove data
    Archive, // Move to archive with versions
}
