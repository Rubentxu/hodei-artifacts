// crates/iam/src/features/delete_policy/api.rs

use crate::features::delete_policy::dto::{DeletePolicyCommand, DeletePolicyResponse};
use crate::features::delete_policy::use_case::DeletePolicyUseCase;
use crate::infrastructure::errors::IamError;
use std::sync::Arc;

/// API layer for delete policy feature
/// This is the entry point that external systems (HTTP, gRPC, etc.) will use
pub struct DeletePolicyApi {
    use_case: Arc<DeletePolicyUseCase>,
}

impl DeletePolicyApi {
    /// Create a new delete policy API
    pub fn new(use_case: Arc<DeletePolicyUseCase>) -> Self {
        Self { use_case }
    }

    /// Handle delete policy request
    pub async fn delete_policy(
        &self,
        command: DeletePolicyCommand,
    ) -> Result<DeletePolicyResponse, IamError> {
        self.use_case.execute(command).await
    }
}
