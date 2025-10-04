use crate::features::attach_scp::dto::{AttachScpCommand, AttachScpView};
use crate::features::attach_scp::error::AttachScpError;
use crate::features::attach_scp::ports::{ScpRepositoryPort, AccountRepositoryPort, OuRepositoryPort};
use policies::domain::Hrn;

/// Use case for attaching an SCP to an entity (Account or OU)
pub struct AttachScpUseCase<SRP: ScpRepositoryPort, ARP: AccountRepositoryPort, ORP: OuRepositoryPort> {
    scp_repository: SRP,
    account_repository: ARP,
    ou_repository: ORP,
}

impl<SRP: ScpRepositoryPort, ARP: AccountRepositoryPort, ORP: OuRepositoryPort> AttachScpUseCase<SRP, ARP, ORP> {
    /// Create a new instance of the use case
    pub fn new(scp_repository: SRP, account_repository: ARP, ou_repository: ORP) -> Self {
        Self {
            scp_repository,
            account_repository,
            ou_repository,
        }
    }

    /// Execute the use case
    pub async fn execute(&self, command: AttachScpCommand) -> Result<AttachScpView, AttachScpError> {
        // Parse HRNs
        let scp_hrn = Hrn::from_string(&command.scp_hrn)
            .ok_or_else(|| AttachScpError::ScpNotFound(command.scp_hrn.clone()))?;
        let target_hrn = Hrn::from_string(&command.target_hrn)
            .ok_or_else(|| AttachScpError::TargetNotFound(command.target_hrn.clone()))?;

        // Find the SCP
        let _scp = self.scp_repository.find_scp_by_hrn(&scp_hrn).await?
            .ok_or_else(|| AttachScpError::ScpNotFound(command.scp_hrn.clone()))?;

        // Attach SCP based on target entity type
        match target_hrn.resource_type.as_str() {
            "account" => {
                let mut account = self.account_repository.find_account_by_hrn(&target_hrn).await?
                    .ok_or_else(|| AttachScpError::TargetNotFound(command.target_hrn.clone()))?;
                account.attach_scp(scp_hrn.clone());
                self.account_repository.save_account(account).await?;
            },
            "ou" => {
                let mut ou = self.ou_repository.find_ou_by_hrn(&target_hrn).await?
                    .ok_or_else(|| AttachScpError::TargetNotFound(command.target_hrn.clone()))?;
                ou.attach_scp(scp_hrn.clone());
                self.ou_repository.save_ou(ou).await?;
            },
            _ => return Err(AttachScpError::InvalidTargetType(target_hrn.resource_type.clone())),
        }

        // Return the attach SCP view
        Ok(AttachScpView {
            scp_hrn: scp_hrn.to_string(),
            target_hrn: target_hrn.to_string(),
        })
    }
}