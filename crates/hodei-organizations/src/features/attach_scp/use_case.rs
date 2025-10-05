use crate::features::attach_scp::dto::{AttachScpCommand, AttachScpView};
use crate::features::attach_scp::error::AttachScpError;
use crate::features::attach_scp::ports::{
    AccountRepositoryPort, OuRepositoryPort, ScpRepositoryPort,
};
use crate::shared::domain::events::{ScpAttached, ScpTargetType};
use kernel::EventPublisher;
use kernel::Hrn;
use kernel::application::ports::event_bus::EventEnvelope;
use kernel::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;

/// Use case for attaching an SCP to an entity (Account or OU)
pub struct AttachScpUseCase<
    SRP: ScpRepositoryPort,
    ARP: AccountRepositoryPort,
    ORP: OuRepositoryPort,
> {
    scp_repository: SRP,
    account_repository: ARP,
    ou_repository: ORP,
    event_publisher: Option<Arc<InMemoryEventBus>>,
}

impl<SRP: ScpRepositoryPort, ARP: AccountRepositoryPort, ORP: OuRepositoryPort>
    AttachScpUseCase<SRP, ARP, ORP>
{
    /// Create a new instance of the use case
    pub fn new(scp_repository: SRP, account_repository: ARP, ou_repository: ORP) -> Self {
        Self {
            scp_repository,
            account_repository,
            ou_repository,
            event_publisher: None,
        }
    }

    pub fn with_event_publisher(mut self, publisher: Arc<InMemoryEventBus>) -> Self {
        self.event_publisher = Some(publisher);
        self
    }

    /// Execute the use case
    pub async fn execute(
        &self,
        command: AttachScpCommand,
    ) -> Result<AttachScpView, AttachScpError> {
        // Parse HRNs
        let scp_hrn = Hrn::from_string(&command.scp_hrn)
            .ok_or_else(|| AttachScpError::ScpNotFound(command.scp_hrn.clone()))?;
        let target_hrn = Hrn::from_string(&command.target_hrn)
            .ok_or_else(|| AttachScpError::TargetNotFound(command.target_hrn.clone()))?;

        // Find the SCP
        let _scp = self
            .scp_repository
            .find_scp_by_hrn(&scp_hrn)
            .await?
            .ok_or_else(|| AttachScpError::ScpNotFound(command.scp_hrn.clone()))?;

        // Attach SCP based on target entity type
        let target_type = match target_hrn.resource_type.as_str() {
            "account" => {
                let mut account = self
                    .account_repository
                    .find_account_by_hrn(&target_hrn)
                    .await?
                    .ok_or_else(|| AttachScpError::TargetNotFound(command.target_hrn.clone()))?;
                account.attach_scp(scp_hrn.clone());
                self.account_repository.save_account(account).await?;
                ScpTargetType::Account
            }
            "ou" => {
                let mut ou = self
                    .ou_repository
                    .find_ou_by_hrn(&target_hrn)
                    .await?
                    .ok_or_else(|| AttachScpError::TargetNotFound(command.target_hrn.clone()))?;
                ou.attach_scp(scp_hrn.clone());
                self.ou_repository.save_ou(ou).await?;
                ScpTargetType::OrganizationalUnit
            }
            _ => {
                return Err(AttachScpError::InvalidTargetType(
                    target_hrn.resource_type.clone(),
                ));
            }
        };

        // Publish domain event
        if let Some(publisher) = &self.event_publisher {
            let event = ScpAttached {
                scp_hrn: scp_hrn.clone(),
                target_hrn: target_hrn.clone(),
                target_type,
                attached_at: chrono::Utc::now(),
            };

            let envelope = EventEnvelope::new(event)
                .with_metadata("aggregate_type".to_string(), "Scp".to_string());

            if let Err(e) = publisher.publish_with_envelope(envelope).await {
                tracing::warn!("Failed to publish ScpAttached event: {}", e);
                // Don't fail the use case if event publishing fails
            }
        }

        // Return the attach SCP view
        Ok(AttachScpView {
            scp_hrn: scp_hrn.to_string(),
            target_hrn: target_hrn.to_string(),
        })
    }
}
