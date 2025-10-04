use super::dto::{CreateGroupCommand, GroupView};
use super::ports::CreateGroupUnitOfWork;
use crate::shared::domain::{Group, events::GroupCreated};
use policies::shared::domain::hrn::Hrn;
use shared::EventPublisher;
use shared::application::ports::event_bus::EventEnvelope;
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;

/// Use case for creating a new group with transactional integrity
pub struct CreateGroupUseCase<U: CreateGroupUnitOfWork> {
    uow: Arc<U>,
    event_publisher: Option<Arc<InMemoryEventBus>>,
}

impl<U: CreateGroupUnitOfWork> CreateGroupUseCase<U> {
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

    pub async fn execute(&self, cmd: CreateGroupCommand) -> Result<GroupView, anyhow::Error> {
        // Begin transaction
        self.uow
            .begin()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to begin transaction: {}", e))?;

        // Execute business logic within transaction
        let result = self.execute_in_transaction(&cmd).await;

        // Handle transaction outcome
        match result {
            Ok(view) => {
                self.uow
                    .commit()
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to commit transaction: {}", e))?;

                // Publish domain event after successful commit
                if let Some(publisher) = &self.event_publisher {
                    let group_hrn = Hrn::from_string(&view.hrn).expect("Invalid HRN in view");

                    let event = GroupCreated {
                        group_hrn,
                        name: view.name.clone(),
                        created_at: chrono::Utc::now(),
                    };

                    let envelope = EventEnvelope::new(event)
                        .with_metadata("aggregate_type".to_string(), "Group".to_string());

                    if let Err(e) = publisher.publish_with_envelope(envelope).await {
                        tracing::warn!("Failed to publish GroupCreated event: {}", e);
                        // Don't fail the use case if event publishing fails
                    }
                }

                Ok(view)
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
        cmd: &CreateGroupCommand,
    ) -> Result<GroupView, anyhow::Error> {
        let repos = self.uow.repositories();

        // Generate a unique HRN using the type-safe constructor
        let group_id = uuid::Uuid::new_v4().to_string();
        let hrn =
            Hrn::for_entity_type::<Group>("hodei".to_string(), "default".to_string(), group_id);

        // Create the group domain entity
        let mut group = Group::new(hrn, cmd.group_name.clone());
        group.tags = cmd.tags.clone();

        // Persist the group
        repos.group_repository.save(&group).await?;

        // Return the view
        Ok(GroupView {
            hrn: group.hrn.to_string(),
            name: group.name,
            tags: group.tags,
        })
    }
}
