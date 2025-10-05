use crate::features::create_scp::dto::{CreateScpCommand, ScpView};
use crate::features::create_scp::error::CreateScpError;
use crate::features::create_scp::ports::{CreateScpUnitOfWork, CreateScpUnitOfWorkFactory};
use crate::shared::domain::scp::ServiceControlPolicy;
use kernel::Hrn;
use std::sync::Arc;

/// Use case for creating service control policies with transactional guarantees
///
/// This implementation uses the UnitOfWork pattern to ensure atomic operations
/// and consistency.
pub struct CreateScpUseCase<UWF: CreateScpUnitOfWorkFactory> {
    uow_factory: Arc<UWF>,
}

impl<UWF: CreateScpUnitOfWorkFactory> CreateScpUseCase<UWF> {
    pub fn new(uow_factory: Arc<UWF>) -> Self {
        Self { uow_factory }
    }

    pub async fn execute(&self, command: CreateScpCommand) -> Result<ScpView, CreateScpError> {
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
        command: &CreateScpCommand,
        uow: &mut UWF::UnitOfWork,
    ) -> Result<ScpView, CreateScpError> {
        // Validar el nombre de la SCP
        if command.name.is_empty() {
            return Err(CreateScpError::InvalidScpName);
        }

        // Validar el documento de la SCP
        if command.document.is_empty() {
            return Err(CreateScpError::InvalidScpDocument);
        }

        // Crear el HRN para la SCP
        let scp_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "scp".to_string(),
            command.name.clone(),
        );

        // Crear la SCP
        let scp =
            ServiceControlPolicy::new(scp_hrn, command.name.clone(), command.document.clone());

        // Guardar la SCP dentro de la transacción
        let scp_repo = uow.scps();
        scp_repo.save(&scp).await?;

        // Devolver la vista de la SCP
        Ok(ScpView {
            hrn: scp.hrn,
            name: scp.name,
            document: scp.document,
        })
    }
}
