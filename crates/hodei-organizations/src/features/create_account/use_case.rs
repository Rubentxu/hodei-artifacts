use crate::features::create_account::dto::{AccountView, CreateAccountCommand};
use crate::features::create_account::error::CreateAccountError;
use crate::features::create_account::ports::{
    CreateAccountUnitOfWork, CreateAccountUnitOfWorkFactory,
};
use crate::shared::domain::account::Account;
use crate::shared::domain::events::AccountCreated;
use policies::domain::Hrn;
use shared::EventPublisher;
use shared::application::ports::event_bus::EventEnvelope;
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;

/// Use case for creating accounts with transactional guarantees
///
/// This implementation uses the UnitOfWork pattern to ensure atomic operations
/// and consistency. Events are published after successful commit to guarantee
/// eventual consistency.
pub struct CreateAccountUseCase<UWF: CreateAccountUnitOfWorkFactory> {
    uow_factory: Arc<UWF>,
    /// Partition for HRN generation (e.g., "aws", "hodei")
    partition: String,
    /// Account identifier for HRN generation (e.g., "default", account_id)
    account_id: String,
    /// Optional event publisher for domain events
    event_publisher: Option<Arc<InMemoryEventBus>>,
}

impl<UWF: CreateAccountUnitOfWorkFactory> CreateAccountUseCase<UWF> {
    pub fn new(uow_factory: Arc<UWF>, partition: String, account_id: String) -> Self {
        Self {
            uow_factory,
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
        // Create a new UnitOfWork for this operation
        let mut uow = self.uow_factory.create().await?;

        // Begin the transaction
        uow.begin().await?;

        // Execute the business logic within the transaction
        let result = self.execute_within_transaction(&command, &mut uow).await;

        // Commit or rollback based on the result
        match result {
            Ok((view, account)) => {
                uow.commit().await?;

                // Publish domain event AFTER successful commit
                // This ensures eventual consistency - if event publishing fails,
                // the account is still created
                self.publish_account_created_event(&account).await;

                Ok(view)
            }
            Err(e) => {
                // Attempt to rollback, but don't hide the original error
                if let Err(rollback_err) = uow.rollback().await {
                    tracing::error!("Failed to rollback transaction: {}", rollback_err);
                }
                Err(e)
            }
        }
    }

    async fn execute_within_transaction(
        &self,
        command: &CreateAccountCommand,
        uow: &mut UWF::UnitOfWork,
    ) -> Result<(AccountView, Account), CreateAccountError> {
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
        let account = Account::new(
            hrn.clone(),
            command.name.clone(),
            command.parent_hrn.clone(),
        );

        // Guardar la cuenta dentro de la transacción
        let account_repo = uow.accounts();
        account_repo.save(&account).await?;

        // Devolver la vista de la cuenta y el agregado para eventos
        let view = AccountView {
            hrn: account.hrn.clone(),
            name: account.name.clone(),
            parent_hrn: account.parent_hrn.clone(),
        };

        Ok((view, account))
    }

    async fn publish_account_created_event(&self, account: &Account) {
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
                // This is eventual consistency - we can retry or have a dead letter queue
            }
        }
    }
}
