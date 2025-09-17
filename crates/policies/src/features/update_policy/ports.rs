use super::dto::UpdatePolicyCommand;
use super::error::UpdatePolicyError;
use crate::domain::ids::PolicyId;
use crate::domain::policy::{Policy, PolicyVersion};
use async_trait::async_trait;
use shared::hrn::UserId;

/// Trait for updating policies
#[async_trait]
pub trait PolicyUpdater: Send + Sync {
    async fn update_policy(&self, policy_id: &PolicyId, command: UpdatePolicyCommand) -> Result<Policy, UpdatePolicyError>;
}

/// Trait for policy validation during update
#[async_trait]
pub trait PolicyUpdateValidator: Send + Sync {
    async fn validate_policy_content(&self, content: &str) -> Result<(), UpdatePolicyError>;
    async fn validate_policy_syntax(&self, content: &str) -> Result<(), UpdatePolicyError>;
    async fn validate_policy_semantics(&self, content: &str, policy_id: &PolicyId) -> Result<(), UpdatePolicyError>;
    async fn validate_update_allowed(&self, policy: &Policy, user_id: &UserId) -> Result<(), UpdatePolicyError>;
}

/// Trait for policy retrieval during update
#[async_trait]
pub trait PolicyRetriever: Send + Sync {
    async fn get_policy(&self, policy_id: &PolicyId) -> Result<Option<Policy>, UpdatePolicyError>;
}

/// Trait for policy storage operations during update
#[async_trait]
pub trait PolicyUpdateStorage: Send + Sync {
    async fn update(&self, policy: &Policy) -> Result<(), UpdatePolicyError>;
    async fn create_version(&self, version: &PolicyVersion) -> Result<(), UpdatePolicyError>;
}

/// Trait for audit logging during policy update
#[async_trait]
pub trait PolicyUpdateAuditor: Send + Sync {
    async fn log_policy_update(&self, policy_id: &PolicyId, user_id: &UserId, changes: Vec<String>) -> Result<(), UpdatePolicyError>;
}
