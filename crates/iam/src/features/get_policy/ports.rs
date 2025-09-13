// crates/iam/src/features/get_policy/ports.rs

use crate::domain::policy::Policy;
use crate::infrastructure::errors::IamError;
use async_trait::async_trait;
use cedar_policy::PolicyId;

/// Port for policy reading operations specific to get_policy feature
#[async_trait]
pub trait PolicyReader: Send + Sync {
    /// Get a policy by its ID
    async fn get_by_id(&self, id: &PolicyId) -> Result<Option<Policy>, IamError>;
}