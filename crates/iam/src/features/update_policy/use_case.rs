// crates/iam/src/features/update_policy/use_case.rs

use crate::features::update_policy::dto::{UpdatePolicyCommand, UpdatePolicyResponse};
use crate::features::update_policy::ports::{PolicyUpdater, PolicyUpdateValidator, PolicyUpdateEventPublisher};
use crate::infrastructure::errors::IamError;
use std::sync::Arc;
use time::OffsetDateTime;

/// Use case for updating existing policies
/// Contains pure business logic without infrastructure concerns
pub struct UpdatePolicyUseCase {
    updater: Arc<dyn PolicyUpdater>,
    validator: Arc<dyn PolicyUpdateValidator>,
    event_publisher: Arc<dyn PolicyUpdateEventPublisher>,
}

impl UpdatePolicyUseCase {
    /// Create a new update policy use case
    pub fn new(
        updater: Arc<dyn PolicyUpdater>,
        validator: Arc<dyn PolicyUpdateValidator>,
        event_publisher: Arc<dyn PolicyUpdateEventPublisher>,
    ) -> Self {
        Self {
            updater,
            validator,
            event_publisher,
        }
    }

    /// Execute the update policy use case
    pub async fn execute(&self, command: UpdatePolicyCommand) -> Result<UpdatePolicyResponse, IamError> {
        // 1. Validate command
        command.validate()?;

        // 2. Get existing policy
        let mut existing_policy = self
            .updater
            .get_by_id(&command.id)
            .await?
            .ok_or_else(|| IamError::PolicyNotFound(command.id.clone()))?;

        // Store original policy for event publishing
        let original_policy = existing_policy.clone();

        // 3. Update fields if provided
        if let Some(name) = command.name {
            existing_policy.name = name;
        }

        if let Some(description) = command.description {
            existing_policy.description = Some(description);
        }

        if let Some(content) = &command.content {
            // Validate new content syntax
            let validation_result = self.validator.validate_syntax(content).await?;
            if !validation_result.is_valid {
                return Err(IamError::validation_error(
                    validation_result.first_error_message()
                        .unwrap_or("Invalid policy syntax")
                        .to_string(),
                ));
            }

            // Validate new content semantics
            self.validator.validate_semantics(content).await?;

            // Note: For compatibility validation, we could add a check here
            // to ensure the new policy doesn't break existing functionality
            // This would require additional business logic based on requirements

            existing_policy.content = content.clone();
        }

        if let Some(tags) = command.tags {
            existing_policy.metadata.tags = tags;
        }

        // 4. Update metadata
        existing_policy.metadata.updated_at = OffsetDateTime::now_utc();
        existing_policy.metadata.updated_by = command.updated_by;
        existing_policy.metadata.version += 1;

        // 5. Update policy in repository
        let updated_policy = self.updater.update(existing_policy).await?;

        // 6. Publish domain event
        self.event_publisher
            .publish_policy_updated(&original_policy, &updated_policy)
            .await?;

        // 7. Return response
        Ok(UpdatePolicyResponse::new(updated_policy))
    }
}