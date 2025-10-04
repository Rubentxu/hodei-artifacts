use async_trait::async_trait;
use crate::features::move_account::ports::MoveAccountUnitOfWork;
use crate::features::move_account::error::MoveAccountError;
use shared::application::ports::unit_of_work::UnitOfWork;

pub struct MoveAccountSurrealUnitOfWorkAdapter {
    inner_uow: crate::shared::infrastructure::surreal::SurrealUnitOfWork,
}

impl MoveAccountSurrealUnitOfWorkAdapter {
    pub fn new(uow: crate::shared::infrastructure::surreal::SurrealUnitOfWork) -> Self {
        Self {
            inner_uow: uow,
        }
    }
}

#[async_trait]
impl MoveAccountUnitOfWork for MoveAccountSurrealUnitOfWorkAdapter {
    async fn begin(&mut self) -> Result<(), MoveAccountError> {
        self.inner_uow.begin().await
            .map_err(|e| MoveAccountError::OuRepositoryError(crate::shared::application::ports::ou_repository::OuRepositoryError::DatabaseError(e.to_string())))
    }

    async fn commit(&mut self) -> Result<(), MoveAccountError> {
        self.inner_uow.commit().await
            .map_err(|e| MoveAccountError::OuRepositoryError(crate::shared::application::ports::ou_repository::OuRepositoryError::DatabaseError(e.to_string())))
    }

    async fn rollback(&mut self) -> Result<(), MoveAccountError> {
        self.inner_uow.rollback().await
            .map_err(|e| MoveAccountError::OuRepositoryError(crate::shared::application::ports::ou_repository::OuRepositoryError::DatabaseError(e.to_string())))
    }

    fn accounts(&self) -> std::sync::Arc<dyn crate::shared::application::ports::account_repository::AccountRepository> {
        // Note: This is a simplified implementation that would need proper adaptation
        // based on the actual SurrealUnitOfWork implementation
        unimplemented!("Needs proper implementation based on SurrealUnitOfWork")
    }

    fn ous(&self) -> std::sync::Arc<dyn crate::shared::application::ports::ou_repository::OuRepository> {
        // Note: This is a simplified implementation that would need proper adaptation
        // based on the actual SurrealUnitOfWork implementation
        unimplemented!("Needs proper implementation based on SurrealUnitOfWork")
    }
}
