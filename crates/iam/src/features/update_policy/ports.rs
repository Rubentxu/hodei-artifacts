// crates/iam/src/features/update_policy/ports.rs

use crate::domain::policy::Policy;
use crate::domain::validation::ValidationResult;
use crate::infrastructure::errors::IamError;
use async_trait::async_trait;
use shared::hrn::PolicyId;

/// Port for policy update operations specific to update_policy feature
#[async_trait]
pub trait PolicyUpdater: Send + Sync {
    /// Get a policy by its ID (needed for update validation)
    async fn get_by_id(&self, id: &PolicyId) -> Result<Option<Policy>, IamError>;
    
    /// Update an existing policy
    async fn update(&self, policy: Policy) -> Result<Policy, IamError>;
    
    /// Check if a policy exists (for validation)
    async fn exists(&self, id: &PolicyId) -> Result<bool, IamError>;
}

/// Port for policy validation specific to update_policy feature
#[async_trait]
pub trait PolicyUpdateValidator: Send + Sync {
    /// Validate Cedar policy syntax
    async fn validate_syntax(&self, content: &str) -> Result<ValidationResult, IamError>;
}

/// Port for publishing events specific to update_policy feature
#[async_trait]
pub trait PolicyUpdateEventPublisher: Send + Sync {
    /// Publish policy updated event
    async fn publish_policy_updated(&self, old_policy: &Policy, new_policy: &Policy) -> Result<(), IamError>;
}