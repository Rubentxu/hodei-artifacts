use super::dto::AddUserToGroupCommand;
use crate::shared::{
    application::ports::{GroupRepository, UserRepository},
    domain::events::UserAddedToGroup,
};
use policies::shared::domain::hrn::Hrn;
use shared::EventPublisher;
use shared::application::ports::event_bus::EventEnvelope;
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
/// Use case for adding a user to a group
use std::sync::Arc;

pub struct AddUserToGroupUseCase {
    user_repo: Arc<dyn UserRepository>,
    group_repo: Arc<dyn GroupRepository>,
    event_publisher: Option<Arc<InMemoryEventBus>>,
}

impl AddUserToGroupUseCase {
    pub fn new(user_repo: Arc<dyn UserRepository>, group_repo: Arc<dyn GroupRepository>) -> Self {
        Self {
            user_repo,
            group_repo,
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

        // Validate that the group exists to maintain consistency
        if self.group_repo.find_by_hrn(&group_hrn).await?.is_none() {
            return Err(anyhow::anyhow!("Group not found: {}", cmd.group_hrn));
        }

        // Load the user
        let mut user = self
            .user_repo
            .find_by_hrn(&user_hrn)
            .await?
            .ok_or_else(|| anyhow::anyhow!("User not found: {}", cmd.user_hrn))?;

        // Add user to group (domain logic handles idempotency)
        user.add_to_group(group_hrn.clone());

        // Persist the updated user
        self.user_repo.save(&user).await?;

        // Publish domain event
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
}
