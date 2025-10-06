use super::dto::AddUserToGroupCommand;
use super::ports::AddUserToGroupUnitOfWork;
use crate::internal::domain::events::UserAddedToGroup;
use kernel::EventPublisher;
use kernel::Hrn;
use kernel::application::ports::event_bus::EventEnvelope;
use kernel::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;

/// Use case for adding a user to a group with transactional integrity
pub struct AddUserToGroupUseCase<U: AddUserToGroupUnitOfWork> {
    uow: Arc<U>,
    event_publisher: Option<Arc<InMemoryEventBus>>,
}

impl<U: AddUserToGroupUnitOfWork> AddUserToGroupUseCase<U> {
    pub fn new(uow: Arc<U>) -> Self {
        Self {
            uow,
            event_publisher: None,
        }
    }

    pub fn with_event_publisher(mut self, publisher: Arc<InMemoryEventBus>) -> Self {
        self.event_publisher = Some(publisher);
        self
    }

    pub async fn execute(&self, cmd: AddUserToGroupCommand) -> Result<(), anyhow::Error> {
        // Parse HRNs
        let user_hrn = Hrn::from_string(&cmd.user_hrn)
            .ok_or_else(|| anyhow::anyhow!("Invalid user HRN: {}", cmd.user_hrn))?;
        let group_hrn = Hrn::from_string(&cmd.group_hrn)
            .ok_or_else(|| anyhow::anyhow!("Invalid group HRN: {}", cmd.group_hrn))?;

        // Begin transaction
        self.uow
            .begin()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to begin transaction: {}", e))?;

        // Execute business logic within transaction
        let result = self.execute_in_transaction(&user_hrn, &group_hrn).await;

        // Handle transaction outcome
        match result {
            Ok(_) => {
                self.uow
                    .commit()
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to commit transaction: {}", e))?;

                // Publish domain event after successful commit
                if let Some(publisher) = &self.event_publisher {
                    let event = UserAddedToGroup {
                        user_hrn: user_hrn.clone(),
                        group_hrn: group_hrn.clone(),
                        added_at: chrono::Utc::now(),
                    };

                    let envelope = EventEnvelope::new(event)
                        .with_metadata("aggregate_type".to_string(), "Group".to_string());

                    if let Err(e) = publisher.publish_with_envelope(envelope).await {
                        tracing::warn!("Failed to publish UserAddedToGroup event: {}", e);
                        // Don't fail the use case if event publishing fails
                    }
                }

                Ok(())
            }
            Err(e) => {
                if let Err(rollback_err) = self.uow.rollback().await {
                    tracing::error!("Failed to rollback transaction: {}", rollback_err);
                }
                Err(e)
            }
        }
    }

    async fn execute_in_transaction(
        &self,
        user_hrn: &Hrn,
        group_hrn: &Hrn,
    ) -> Result<(), anyhow::Error> {
        let repos = self.uow.repositories();

        // Validate that the group exists to maintain consistency
        if repos
            .group_repository
            .find_by_hrn(group_hrn)
            .await?
            .is_none()
        {
            return Err(anyhow::anyhow!("Group not found: {}", group_hrn));
        }

        // Load the user
        let mut user = repos
            .user_repository
            .find_by_hrn(user_hrn)
            .await?
            .ok_or_else(|| anyhow::anyhow!("User not found: {}", user_hrn))?;

        // Add user to group (domain logic handles idempotency)
        user.add_to_group(group_hrn.clone());

        // Persist the updated user
        repos.user_repository.save(&user).await?;

        Ok(())
    }
}
