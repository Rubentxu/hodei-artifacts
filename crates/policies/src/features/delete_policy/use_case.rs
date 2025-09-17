use async_trait::async_trait;
use chrono::Utc;
use std::sync::Arc;
use tracing::{error, info, warn};

use super::dto::{DeletePolicyCommand, DeletePolicyResponse};
use super::error::DeletePolicyError;
use super::ports::{DeletionMode, PolicyDeleter, PolicyDeletionAuditor, PolicyDeletionRetriever, PolicyDeletionStorage, PolicyDeletionValidator};
use crate::domain::ids::PolicyId;
use crate::domain::policy::Policy;

/// Use case for deleting policies
pub struct DeletePolicyUseCase<PV, PR, PS, PA> {
    validator: Arc<dyn PolicyDeletionValidator>,
    retriever: Arc<dyn PolicyDeletionRetriever>,
    storage: Arc<dyn PolicyDeletionStorage>,
    auditor: Arc<dyn PolicyDeletionAuditor>,
    _phantom: std::marker::PhantomData<(PV, PR, PS, PA)>,
}

impl<PV, PR, PS, PA> DeletePolicyUseCase<PV, PR, PS, PA> {
    pub fn new(
        validator: Arc<dyn PolicyDeletionValidator>,
        retriever: Arc<dyn PolicyDeletionRetriever>,
        storage: Arc<dyn PolicyDeletionStorage>,
        auditor: Arc<dyn PolicyDeletionAuditor>,
    ) -> Self {
        Self {
            validator,
            retriever,
            storage,
            auditor,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Execute the delete policy use case
    pub async fn execute(&self, command: DeletePolicyCommand) -> Result<DeletePolicyResponse, DeletePolicyError> {
        info!("Deleting policy: {} with mode: {:?}", command.policy_id, command.deletion_mode);

        // Retrieve the policy
        let policy = self.retriever.get_policy(&command.policy_id).await?
            .ok_or_else(|| DeletePolicyError::policy_not_found(command.policy_id.clone()))?;

        // Validate deletion is allowed
        self.validator.validate_deletion_allowed(&policy, &command.deleted_by).await?;
        self.validator.check_dependencies(&policy).await?;

        // Perform deletion based on mode
        match command.deletion_mode {
            DeletionMode::Soft => {
                self.storage.soft_delete(&command.policy_id).await?;
            }
            DeletionMode::Hard => {
                // Archive versions first, then hard delete
                self.storage.archive_versions(&command.policy_id).await?;
                self.storage.hard_delete(&command.policy_id).await?;
            }
            DeletionMode::Archive => {
                // Archive versions and soft delete
                self.storage.archive_versions(&command.policy_id).await?;
                self.storage.soft_delete(&command.policy_id).await?;
            }
        }

        // Log the deletion
        self.auditor.log_policy_deletion(&command.policy_id, &command.deleted_by).await?;

        let message = match command.deletion_mode {
            DeletionMode::Soft => "Policy soft deleted successfully",
            DeletionMode::Hard => "Policy permanently deleted",
            DeletionMode::Archive => "Policy archived successfully",
        };

        info!("Policy deleted successfully: {} with mode: {:?}", command.policy_id, command.deletion_mode);

        Ok(DeletePolicyResponse {
            policy_id: command.policy_id,
            deletion_mode: command.deletion_mode,
            deleted_at: Utc::now().to_rfc3339(),
            deleted_by: command.deleted_by,
            success: true,
            message: message.to_string(),
        })
    }
}

#[async_trait]
impl<PV, PR, PS, PA> PolicyDeleter for DeletePolicyUseCase<PV, PR, PS, PA>
where
    PV: PolicyDeletionValidator + Send + Sync,
    PR: PolicyDeletionRetriever + Send + Sync,
    PS: PolicyDeletionStorage + Send + Sync,
    PA: PolicyDeletionAuditor + Send + Sync,
{
    async fn delete_policy(&self, command: DeletePolicyCommand) -> Result<(), DeletePolicyError> {
        let result = self.execute(command).await?;
        if result.success {
            Ok(())
        } else {
            Err(DeletePolicyError::deletion_failed("Deletion failed"))
        }
    }
}
