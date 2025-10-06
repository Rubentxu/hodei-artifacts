use super::ports::{CreateGroupRepositories, CreateGroupUnitOfWork};
use crate::internal::application::ports::GroupRepository;
use std::error::Error as StdError;
use std::sync::Arc;

/// Generic Unit of Work implementation for create_group feature
///
/// This implementation uses trait objects to work with any repository implementation.
/// It provides a simple transactional wrapper around repository operations.
pub struct GenericCreateGroupUnitOfWork {
    group_repository: Arc<dyn GroupRepository>,
    // Note: This is a simplified UoW. In a production system with SurrealDB,
    // this would wrap an actual database transaction.
    transaction_active: std::sync::Mutex<bool>,
}

impl GenericCreateGroupUnitOfWork {
    pub fn new(group_repository: Arc<dyn GroupRepository>) -> Self {
        Self {
            group_repository,
            transaction_active: std::sync::Mutex::new(false),
        }
    }
}

#[async_trait::async_trait]
impl CreateGroupUnitOfWork for GenericCreateGroupUnitOfWork {
    async fn begin(&self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let mut active = self
            .transaction_active
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;

        if *active {
            return Err("Transaction already active".into());
        }

        *active = true;
        tracing::debug!("Transaction started for create_group");
        Ok(())
    }

    async fn commit(&self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let mut active = self
            .transaction_active
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;

        if !*active {
            return Err("No active transaction to commit".into());
        }

        *active = false;
        tracing::debug!("Transaction committed for create_group");
        Ok(())
    }

    async fn rollback(&self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let mut active = self
            .transaction_active
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;

        if !*active {
            return Err("No active transaction to rollback".into());
        }

        *active = false;
        tracing::debug!("Transaction rolled back for create_group");
        // Note: Actual rollback logic would be implemented by the underlying database
        Ok(())
    }

    fn repositories(&self) -> CreateGroupRepositories {
        CreateGroupRepositories::new(self.group_repository.clone())
    }
}
