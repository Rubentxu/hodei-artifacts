// crates/iam/src/features/list_policies/api.rs

use crate::features::list_policies::dto::{ListPoliciesQuery, ListPoliciesResponse};
use crate::features::list_policies::use_case::ListPoliciesUseCase;
use crate::infrastructure::errors::IamError;
use std::sync::Arc;

/// API layer for list policies feature
/// This is the entry point that external systems (HTTP, gRPC, etc.) will use
pub struct ListPoliciesApi {
    use_case: Arc<ListPoliciesUseCase>,
}

impl ListPoliciesApi {
    /// Create a new list policies API
    pub fn new(use_case: Arc<ListPoliciesUseCase>) -> Self {
        Self { use_case }
    }

    /// Handle list policies request
    pub async fn list_policies(&self, query: ListPoliciesQuery) -> Result<ListPoliciesResponse, IamError> {
        self.use_case.execute(query).await
    }
}