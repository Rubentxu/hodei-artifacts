// crates/iam/src/features/list_policies/ports.rs

use crate::features::list_policies::dto::{ListPoliciesQuery, ListPoliciesResponse};
use crate::features::list_policies::error::ListPoliciesError;
use async_trait::async_trait;

/// Port for policy listing operations specific to list_policies feature
#[async_trait]
pub trait PolicyLister: Send + Sync {
    /// List policies with filtering and pagination
    async fn list(
        &self,
        query: ListPoliciesQuery,
    ) -> Result<ListPoliciesResponse, ListPoliciesError>;

    /// Count policies matching the query
    async fn count(&self, query: ListPoliciesQuery) -> Result<u64, ListPoliciesError>;
}
