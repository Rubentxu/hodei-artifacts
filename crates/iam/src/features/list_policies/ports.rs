// crates/iam/src/features/list_policies/ports.rs

use crate::application::ports::{PolicyFilter, PolicyList};
use crate::infrastructure::errors::IamError;
use async_trait::async_trait;

/// Port for policy listing operations specific to list_policies feature
#[async_trait]
pub trait PolicyLister: Send + Sync {
    /// List policies with filtering and pagination
    async fn list(&self, filter: PolicyFilter) -> Result<PolicyList, IamError>;
    
    /// Count policies matching the filter
    async fn count(&self, filter: PolicyFilter) -> Result<u64, IamError>;
}