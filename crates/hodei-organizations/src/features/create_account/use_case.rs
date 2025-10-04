use crate::features::create_account::dto::{AccountView, CreateAccountCommand};
use crate::features::create_account::error::CreateAccountError;
use crate::features::create_account::ports::AccountPersister;
use crate::shared::domain::account::Account;
use crate::shared::domain::events::AccountCreated;
use policies::domain::Hrn;
use shared::EventPublisher;
use shared::application::ports::event_bus::EventEnvelope;
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;

pub struct CreateAccountUseCase<AP: AccountPersister> {
    persister: Arc<AP>,
    /// Partition for HRN generation (e.g., "aws", "hodei")
    partition: String,
    /// Account identifier for HRN generation (e.g., "default", account_id)
    account_id: String,
    /// Optional event publisher for domain events
    event_publisher: Option<Arc<InMemoryEventBus>>,
}

impl<AP: AccountPersister> CreateAccountUseCase<AP> {
    pub fn new(persister: Arc<AP>, partition: String, account_id: String) -> Self {
        Self {
            persister,
            partition,
            account_id,
            event_publisher: None,
        }
    }

    pub fn with_event_publisher(mut self, publisher: Arc<InMemoryEventBus>) -> Self {
        self.event_publisher = Some(publisher);
        self
    }

    pub async fn execute(
        &self,
        command: CreateAccountCommand,
    ) -> Result<AccountView, CreateAccountError> {
        // Validar el nombre de la cuenta
        if command.name.is_empty() {
            return Err(CreateAccountError::InvalidAccountName);
        }

        // Generar HRN para la nueva cuenta (centralized generation)
        // Format: hrn:partition:organizations:account_id:account/account_name
        let hrn = Hrn::new(
            self.partition.clone(),
            "organizations".to_string(),
            self.account_id.clone(),
            "account".to_string(),
            command.name.clone(),
        );

        // Crear la cuenta
        let account = Account::new(hrn.clone(), command.name.clone(), command.parent_hrn);

        // Guardar la cuenta
        self.persister.save(account.clone()).await?;

        // Publish domain event
        if let Some(publisher) = &self.event_publisher {
            let event = AccountCreated {
                account_hrn: account.hrn.clone(),
                name: account.name.clone(),
                parent_hrn: account.parent_hrn.clone(),
                created_at: chrono::Utc::now(),
            };

            let envelope = EventEnvelope::new(event)
                .with_metadata("aggregate_type".to_string(), "Account".to_string());

            if let Err(e) = publisher.publish_with_envelope(envelope).await {
                tracing::warn!("Failed to publish AccountCreated event: {}", e);
                // Don't fail the use case if event publishing fails
            }
        }

        // Devolver la vista de la cuenta
        Ok(AccountView {
            hrn: account.hrn,
            name: account.name,
            parent_hrn: account.parent_hrn,
        })
    }
}
