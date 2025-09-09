// crates/iam/src/features/list_policies/use_case.rs

use crate::features::list_policies::dto::{ListPoliciesQuery, ListPoliciesResponse};
use crate::features::list_policies::ports::PolicyLister;
use crate::infrastructure::errors::IamError;
use std::sync::Arc;

/// Use case for listing policies
/// Contains pure business logic without infrastructure concerns
pub struct ListPoliciesUseCase {
    lister: Arc<dyn PolicyLister>,
}

impl ListPoliciesUseCase {
    /// Create a new list policies use case
    pub fn new(lister: Arc<dyn PolicyLister>) -> Self {
        Self { lister }
    }

    /// Execute the list policies use case
    pub async fn execute(&self, query: ListPoliciesQuery) -> Result<ListPoliciesResponse, IamError> {
        // 1. Validate query
        query.validate()?;

        // 2. Convert to PolicyFilter
        let filter = query.to_policy_filter();

        // 3. Get policies from repository
        let policies = self.lister.list(filter).await?;

        // 4. Return response
        Ok(ListPoliciesResponse::new(policies))
    }
}