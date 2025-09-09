// crates/iam/src/features/get_policy/api.rs

use crate::features::get_policy::dto::{GetPolicyQuery, GetPolicyResponse};
use crate::features::get_policy::use_case::GetPolicyUseCase;
use crate::infrastructure::errors::IamError;
use std::sync::Arc;

/// API layer for get policy feature
/// This is the entry point that external systems (HTTP, gRPC, etc.) will use
pub struct GetPolicyApi {
    use_case: Arc<GetPolicyUseCase>,
}

impl GetPolicyApi {
    /// Create a new get policy API
    pub fn new(use_case: Arc<GetPolicyUseCase>) -> Self {
        Self { use_case }
    }

    /// Handle get policy request
    pub async fn get_policy(&self, query: GetPolicyQuery) -> Result<GetPolicyResponse, IamError> {
        self.use_case.execute(query).await
    }
}