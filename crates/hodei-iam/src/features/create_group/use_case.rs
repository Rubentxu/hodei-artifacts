use super::dto::{CreateGroupCommand, GroupView};
use crate::shared::{
    application::ports::GroupRepository,
    domain::{Group, events::GroupCreated},
};
use policies::shared::domain::hrn::Hrn;
use shared::EventPublisher;
use shared::application::ports::event_bus::EventEnvelope;
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
/// Use case for creating a new group
use std::sync::Arc;

pub struct CreateGroupUseCase {
    repo: Arc<dyn GroupRepository>,
    event_publisher: Option<Arc<InMemoryEventBus>>,
}

impl CreateGroupUseCase {
    pub fn new(repo: Arc<dyn GroupRepository>) -> Self {
        Self {
            repo,
            event_publisher: None,
        }
    }

    pub fn with_event_publisher(mut self, publisher: Arc<InMemoryEventBus>) -> Self {
        self.event_publisher = Some(publisher);
        self
    }

    pub async fn execute(&self, cmd: CreateGroupCommand) -> Result<GroupView, anyhow::Error> {
        // Generate a unique HRN using the type-safe constructor
        let group_id = uuid::Uuid::new_v4().to_string();
        let hrn =
            Hrn::for_entity_type::<Group>("hodei".to_string(), "default".to_string(), group_id);

        // Create the group domain entity
        let mut group = Group::new(hrn, cmd.group_name.clone());
        group.tags = cmd.tags.clone();

        // Persist the group
        self.repo.save(&group).await?;

        // Publish domain event
        if let Some(publisher) = &self.event_publisher {
            let event = GroupCreated {
                group_hrn: group.hrn.clone(),
                name: group.name.clone(),
                created_at: chrono::Utc::now(),
            };

            let envelope = EventEnvelope::new(event)
                .with_metadata("aggregate_type".to_string(), "Group".to_string());

            if let Err(e) = publisher.publish_with_envelope(envelope).await {
                tracing::warn!("Failed to publish GroupCreated event: {}", e);
                // Don't fail the use case if event publishing fails
            }
        }

        // Return the view
        Ok(GroupView {
            hrn: group.hrn.to_string(),
            name: group.name,
            tags: group.tags,
        })
    }
}
