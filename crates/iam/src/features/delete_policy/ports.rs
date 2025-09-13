// crates/iam/src/features/delete_policy/ports.rs

use crate::domain::policy::Policy;
use crate::infrastructure::errors::IamError;
use async_trait::async_trait;
use cedar_policy::PolicyId;

/// Port for policy deletion operations specific to delete_policy feature
#[async_trait]
pub trait PolicyDeleter: Send + Sync {
    /// Get a policy by its ID (needed for deletion validation)
    async fn get_by_id(&self, id: &PolicyId) -> Result<Option<Policy>, IamError>;
    
    /// Delete a policy by its ID
    async fn delete(&self, id: &PolicyId) -> Result<(), IamError>;
}

/// Port for publishing events specific to delete_policy feature
#[async_trait]
pub trait PolicyDeleteEventPublisher: Send + Sync {
    /// Publish policy deleted event
    async fn publish_policy_deleted(&self, policy: &Policy) -> Result<(), IamError>;
}