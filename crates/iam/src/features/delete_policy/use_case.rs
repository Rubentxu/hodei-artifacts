// crates/iam/src/features/delete_policy/use_case.rs

use crate::features::delete_policy::dto::{DeletePolicyCommand, DeletePolicyResponse};
use crate::features::delete_policy::ports::{PolicyDeleteEventPublisher, PolicyDeleter};
use crate::infrastructure::errors::IamError;
use std::sync::Arc;

/// Use case for deleting policies
/// Contains pure business logic without infrastructure concerns
pub struct DeletePolicyUseCase {
    deleter: Arc<dyn PolicyDeleter>,
    event_publisher: Arc<dyn PolicyDeleteEventPublisher>,
}

impl DeletePolicyUseCase {
    /// Create a new delete policy use case
    pub fn new(
        deleter: Arc<dyn PolicyDeleter>,
        event_publisher: Arc<dyn PolicyDeleteEventPublisher>,
    ) -> Self {
        Self {
            deleter,
            event_publisher,
        }
    }

    /// Execute the delete policy use case
    pub async fn execute(
        &self,
        command: DeletePolicyCommand,
    ) -> Result<DeletePolicyResponse, IamError> {
        // 1. Validate command
        command.validate()?;

        // 2. Get existing policy for event publishing
        let existing_policy = self
            .deleter
            .get_by_id(&command.id)
            .await?
            .ok_or_else(|| IamError::PolicyNotFound(command.id.clone()))?;

        // 3. Delete policy from repository
        self.deleter.delete(&command.id).await?;

        // 4. Publish domain event
        self.event_publisher
            .publish_policy_deleted(&existing_policy)
            .await?;

        // 5. Return response
        Ok(DeletePolicyResponse::new(command.id))
    }
}
