use crate::features::create_ou::dto::{CreateOuCommand, OuView};
use crate::features::create_ou::error::CreateOuError;
use crate::features::create_ou::ports::{CreateOuUnitOfWork, CreateOuUnitOfWorkFactory};
use crate::shared::domain::ou::OrganizationalUnit;
use std::sync::Arc;

/// Use case for creating organizational units with transactional guarantees
///
/// This implementation uses the UnitOfWork pattern to ensure atomic operations
/// and consistency.
pub struct CreateOuUseCase<UWF: CreateOuUnitOfWorkFactory> {
    uow_factory: Arc<UWF>,
}

impl<UWF: CreateOuUnitOfWorkFactory> CreateOuUseCase<UWF> {
    pub fn new(uow_factory: Arc<UWF>) -> Self {
        Self { uow_factory }
    }

    pub async fn execute(&self, command: CreateOuCommand) -> Result<OuView, CreateOuError> {
        // Create a new UnitOfWork for this operation
        let mut uow = self.uow_factory.create().await?;

        // Begin the transaction
        uow.begin().await?;

        // Execute the business logic within the transaction
        let result = self.execute_within_transaction(&command, &mut uow).await;

        // Commit or rollback based on the result
        match result {
            Ok(view) => {
                uow.commit().await?;
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
        command: &CreateOuCommand,
        uow: &mut UWF::UnitOfWork,
    ) -> Result<OuView, CreateOuError> {
        // Validar el nombre de la OU
        if command.name.is_empty() {
            return Err(CreateOuError::InvalidOuName);
        }

        // Crear la OU
        let ou = OrganizationalUnit::new(command.name.clone(), command.parent_hrn.clone());

        // Guardar la OU dentro de la transacción
        let ou_repo = uow.ous();
        ou_repo.save(&ou).await?;

        // Devolver la vista de la OU
        Ok(OuView {
            hrn: ou.hrn,
            name: ou.name,
            parent_hrn: ou.parent_hrn,
        })
    }
}
