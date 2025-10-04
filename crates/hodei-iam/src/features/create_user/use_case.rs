use super::dto::{CreateUserCommand, UserView};
use super::ports::CreateUserUnitOfWork;
use crate::shared::domain::{User, events::UserCreated};
use policies::shared::domain::hrn::Hrn;
use shared::EventPublisher;
use shared::application::ports::event_bus::EventEnvelope;
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;

/// Use case for creating a new user with transactional integrity
pub struct CreateUserUseCase<U: CreateUserUnitOfWork> {
    uow: Arc<U>,
    event_publisher: Option<Arc<InMemoryEventBus>>,
}

impl<U: CreateUserUnitOfWork> CreateUserUseCase<U> {
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

    pub async fn execute(&self, cmd: CreateUserCommand) -> Result<UserView, anyhow::Error> {
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
                    let user_hrn = Hrn::from_string(&view.hrn).expect("Invalid HRN in view");

                    let event = UserCreated {
                        user_hrn,
                        username: view.name.clone(),
                        email: view.email.clone(),
                        created_at: chrono::Utc::now(),
                    };

                    let envelope = EventEnvelope::new(event)
                        .with_metadata("aggregate_type".to_string(), "User".to_string());

                    if let Err(e) = publisher.publish_with_envelope(envelope).await {
                        tracing::warn!("Failed to publish UserCreated event: {}", e);
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
        cmd: &CreateUserCommand,
    ) -> Result<UserView, anyhow::Error> {
        let repos = self.uow.repositories();

        // Generate a unique HRN using the type-safe constructor
        let user_id = uuid::Uuid::new_v4().to_string();
        let hrn = Hrn::for_entity_type::<User>("hodei".to_string(), "default".to_string(), user_id);

        // Create the user domain entity
        let mut user = User::new(hrn, cmd.name.clone(), cmd.email.clone());
        user.tags = cmd.tags.clone();

        // Persist the user
        repos.user_repository.save(&user).await?;

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
