use crate::features::move_account::dto::MoveAccountCommand;
use crate::features::move_account::error::MoveAccountError;
use crate::features::move_account::ports::{MoveAccountUnitOfWork, MoveAccountUnitOfWorkFactory};
use std::sync::Arc;

/// Transactional MoveAccountUseCase using UnitOfWork pattern
///
/// This implementation ensures atomic operations across multiple repositories
/// by using the UnitOfWork pattern for transaction management.
pub struct MoveAccountUseCase<UWF: MoveAccountUnitOfWorkFactory> {
    uow_factory: Arc<UWF>,
}

impl<UWF: MoveAccountUnitOfWorkFactory> MoveAccountUseCase<UWF> {
    pub fn new(uow_factory: Arc<UWF>) -> Self {
        Self { uow_factory }
    }

    pub async fn execute(&self, command: MoveAccountCommand) -> Result<(), MoveAccountError> {
        // Create a new UnitOfWork for this operation
        let mut uow = self.uow_factory.create().await?;

        // Begin the transaction
        uow.begin().await?;

        // Execute the business logic within the transaction
        let result = self.execute_within_transaction(&command, &mut uow).await;

        // Commit or rollback based on the result
        match result {
            Ok(_) => {
                uow.commit().await?;
                Ok(())
            }
            Err(e) => {
                // Attempt to rollback, but don't hide the original error
                if let Err(rollback_err) = uow.rollback().await {
                    eprintln!("Failed to rollback transaction: {}", rollback_err);
                }
                Err(e)
            }
        }
    }

    async fn execute_within_transaction<UOW: MoveAccountUnitOfWork>(
        &self,
        command: &MoveAccountCommand,
        uow: &mut UOW,
    ) -> Result<(), MoveAccountError> {
        // Get repositories from the UnitOfWork
        let account_repo = uow.accounts();
        let ou_repo = uow.ous();

        // 1. Cargar la Account a mover
        let mut account = account_repo
            .find_by_hrn(&command.account_hrn)
            .await?
            .ok_or(MoveAccountError::AccountNotFound)?;

        // 2. Cargar la OU de origen
        let mut source_ou = ou_repo
            .find_by_hrn(&command.source_ou_hrn)
            .await?
            .ok_or(MoveAccountError::SourceOuNotFound)?;

        // 3. Cargar la OU de destino
        let mut target_ou = ou_repo
            .find_by_hrn(&command.target_ou_hrn)
            .await?
            .ok_or(MoveAccountError::TargetOuNotFound)?;

        // 4. Llamar a source_ou.remove_child_account(...)
        source_ou.remove_child_account(&account.hrn);

        // 5. Llamar a account.set_parent(...)
        account.parent_hrn = Some(command.target_ou_hrn.clone());

        // 6. Llamar a target_ou.add_child_account(...)
        target_ou.add_child_account(account.hrn.clone());

        // 7. Guardar los tres agregados modificados (account, source_ou, target_ou)
        // Todas las operaciones ocurren dentro de la misma transacción
        account_repo.save(&account).await?;
        ou_repo.save(&source_ou).await?;
        ou_repo.save(&target_ou).await?;

        Ok(())
    }
}
