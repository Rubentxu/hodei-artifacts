// crates/iam/src/features/update_policy/api.rs

use crate::features::update_policy::dto::{UpdatePolicyCommand, UpdatePolicyResponse};
use crate::features::update_policy::use_case::UpdatePolicyUseCase;
use crate::infrastructure::errors::IamError;
use std::sync::Arc;

/// API layer for update policy feature
/// This is the entry point that external systems (HTTP, gRPC, etc.) will use
pub struct UpdatePolicyApi {
    use_case: Arc<UpdatePolicyUseCase>,
}

impl UpdatePolicyApi {
    /// Create a new update policy API
    pub fn new(use_case: Arc<UpdatePolicyUseCase>) -> Self {
        Self { use_case }
    }

    /// Handle update policy request
    pub async fn update_policy(&self, command: UpdatePolicyCommand) -> Result<UpdatePolicyResponse, IamError> {
        self.use_case.execute(command).await
    }
}