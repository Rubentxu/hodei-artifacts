use super::error::GetPolicyError;
use crate::domain::ids::PolicyId;
use crate::domain::policy::Policy;
use async_trait::async_trait;
use shared::hrn::UserId;

/// Trait for getting policies
#[async_trait]
pub trait PolicyGetter: Send + Sync {
    async fn get_policy(&self, policy_id: &PolicyId) -> Result<Option<Policy>, GetPolicyError>;
    async fn get_policy_details(&self, policy_id: &PolicyId) -> Result<Option<Policy>, GetPolicyError>;
}

/// Trait for validating policy access
#[async_trait]
pub trait PolicyAccessValidator: Send + Sync {
    async fn validate_access(&self, policy: &Policy, user_id: &UserId) -> Result<(), GetPolicyError>;
    async fn can_read_policy(&self, policy_id: &PolicyId, user_id: &UserId) -> Result<bool, GetPolicyError>;
}

/// Trait for policy retrieval from storage
#[async_trait]
pub trait PolicyRetrievalStorage: Send + Sync {
    async fn find_by_id(&self, policy_id: &PolicyId) -> Result<Option<Policy>, GetPolicyError>;
    async fn find_by_id_with_versions(&self, policy_id: &PolicyId) -> Result<Option<Policy>, GetPolicyError>;
}

/// Trait for audit logging during policy retrieval
#[async_trait]
pub trait PolicyRetrievalAuditor: Send + Sync {
    async fn log_policy_access(&self, policy_id: &PolicyId, user_id: &UserId) -> Result<(), GetPolicyError>;
}
