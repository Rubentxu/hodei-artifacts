use super::dto::CreatePolicyCommand;
use super::error::CreatePolicyError;

use crate::domain::policy::{Policy, PolicyVersion};
use async_trait::async_trait;
use shared::hrn::{HodeiPolicyId, UserId};

/// Trait for creating policies
#[async_trait]
pub trait PolicyCreator: Send + Sync {
    async fn create_policy(&self, command: CreatePolicyCommand) -> Result<Policy, CreatePolicyError>;
}

/// Trait for policy validation during creation
#[async_trait]
pub trait PolicyCreationValidator: Send + Sync {
    async fn validate_policy_id(&self, policy_id: &HodeiPolicyId) -> Result<(), CreatePolicyError>;
    async fn validate_policy_content(&self, content: &str) -> Result<(), CreatePolicyError>;
    async fn validate_policy_syntax(&self, content: &str) -> Result<(), CreatePolicyError>;
    async fn validate_policy_semantics(&self, content: &str, policy_id: &HodeiPolicyId) -> Result<(), CreatePolicyError>;
}

/// Trait for checking policy existence
#[async_trait]
pub trait PolicyExistenceChecker: Send + Sync {
    async fn exists(&self, policy_id: &HodeiPolicyId) -> Result<bool, CreatePolicyError>;
}

/// Trait for policy storage operations during creation
#[async_trait]
pub trait PolicyCreationStorage: Send + Sync {
    async fn save(&self, policy: &Policy) -> Result<(), CreatePolicyError>;
    async fn create_version(&self, version: &PolicyVersion) -> Result<(), CreatePolicyError>;
}

/// Trait for audit logging during policy creation
#[async_trait]
pub trait PolicyCreationAuditor: Send + Sync {
    async fn log_policy_creation(&self, policy_id: &HodeiPolicyId, user_id: &UserId) -> Result<(), CreatePolicyError>;
}
