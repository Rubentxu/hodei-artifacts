use super::dto::{CreateUserCommand, UserView};
use crate::shared::{
    application::ports::UserRepository,
    domain::{User, events::UserCreated},
};
use policies::shared::domain::hrn::Hrn;
use shared::EventPublisher;
use shared::application::ports::event_bus::EventEnvelope;
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
/// Use case for creating a new user
use std::sync::Arc;

pub struct CreateUserUseCase {
    repo: Arc<dyn UserRepository>,
    event_publisher: Option<Arc<InMemoryEventBus>>,
}

impl CreateUserUseCase {
    pub fn new(repo: Arc<dyn UserRepository>) -> Self {
        Self {
            repo,
            event_publisher: None,
        }
    }

    pub fn with_event_publisher(mut self, publisher: Arc<InMemoryEventBus>) -> Self {
        self.event_publisher = Some(publisher);
        self
    }

    pub async fn execute(&self, cmd: CreateUserCommand) -> Result<UserView, anyhow::Error> {
        // Generate a unique HRN using the type-safe constructor
        let user_id = uuid::Uuid::new_v4().to_string();
        let hrn = Hrn::for_entity_type::<User>("hodei".to_string(), "default".to_string(), user_id);

        // Create the user domain entity
        let mut user = User::new(hrn, cmd.name.clone(), cmd.email.clone());
        user.tags = cmd.tags.clone();

        // Persist the user
        self.repo.save(&user).await?;

        // Publish domain event
        if let Some(publisher) = &self.event_publisher {
            let event = UserCreated {
                user_hrn: user.hrn.clone(),
                username: user.name.clone(),
                email: user.email.clone(),
                created_at: chrono::Utc::now(),
            };

            let envelope = EventEnvelope::new(event)
                .with_metadata("aggregate_type".to_string(), "User".to_string());

            if let Err(e) = publisher.publish_with_envelope(envelope).await {
                tracing::warn!("Failed to publish UserCreated event: {}", e);
                // Don't fail the use case if event publishing fails
            }
        }

        // Return the view
        Ok(UserView {
            hrn: user.hrn.to_string(),
            name: user.name,
            email: user.email,
            groups: Vec::new(),
            tags: user.tags,
        })
    }
}
