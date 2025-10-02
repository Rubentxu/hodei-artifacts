// crates/iam/src/features/update_policy/ports.rs

use crate::domain::policy::Policy;
use crate::domain::validation::ValidationResult;
use crate::infrastructure::errors::IamError;
use async_trait::async_trait;
use cedar_policy::PolicyId;

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

    /// Validate Cedar policy semantics against schema
    async fn validate_semantics(&self, content: &str) -> Result<(), IamError>;
}

/// Port for semantic policy validation specific to update_policy feature
#[async_trait]
pub trait UpdatePolicySemanticValidator: Send + Sync {
    /// Validate policy semantics against Cedar schema
    async fn validate_semantics(&self, policy: &str) -> Result<(), IamError>;

    /// Validate update compatibility between old and new policy
    async fn validate_update_compatibility(
        &self,
        old_policy: &str,
        new_policy: &str,
    ) -> Result<(), IamError>;
}

/// Port for publishing events specific to update_policy feature
#[async_trait]
pub trait PolicyUpdateEventPublisher: Send + Sync {
    /// Publish policy updated event
    async fn publish_policy_updated(
        &self,
        old_policy: &Policy,
        new_policy: &Policy,
    ) -> Result<(), IamError>;
}
