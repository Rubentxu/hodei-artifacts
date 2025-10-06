use super::ports::{AddUserToGroupRepositories, AddUserToGroupUnitOfWork};
use crate::internal::application::ports::{GroupRepository, UserRepository};
use std::error::Error as StdError;
use std::sync::Arc;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;

/// SurrealDB-based Unit of Work implementation for add_user_to_group feature
///
/// This implementation provides real transactional guarantees using SurrealDB's
/// transaction support. The transaction is managed at the database connection level.
///
/// # Transaction Lifecycle
///
/// 1. `begin()` - Starts a database transaction
/// 2. Repository operations - Execute within the transaction context
/// 3. `commit()` or `rollback()` - Finalize the transaction
///
/// # Note
///
/// SurrealDB 2.x uses implicit transactions. Operations are automatically transactional
/// when executed on the same connection. For explicit transaction control, we can use
/// the query API with BEGIN TRANSACTION, COMMIT, and CANCEL statements.
pub struct GenericAddUserToGroupUnitOfWork {
    user_repository: Arc<dyn UserRepository>,
    group_repository: Arc<dyn GroupRepository>,
    db: Surreal<Any>,
    transaction_active: std::sync::Mutex<bool>,
}

impl GenericAddUserToGroupUnitOfWork {
    /// Create a new Unit of Work with SurrealDB transaction support
    ///
    /// # Arguments
    ///
    /// * `user_repository` - Repository for user operations
    /// * `group_repository` - Repository for group operations
    /// * `db` - SurrealDB connection for transaction management
    pub fn new(
        user_repository: Arc<dyn UserRepository>,
        group_repository: Arc<dyn GroupRepository>,
        db: Surreal<Any>,
    ) -> Self {
        Self {
            user_repository,
            group_repository,
            db,
            transaction_active: std::sync::Mutex::new(false),
        }
    }
}

#[async_trait::async_trait]
impl AddUserToGroupUnitOfWork for GenericAddUserToGroupUnitOfWork {
    async fn begin(&self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        // Check transaction state before starting
        {
            let active = self
                .transaction_active
                .lock()
                .map_err(|e| format!("Lock error: {}", e))?;

            if *active {
                return Err("Transaction already active".into());
            }
        } // Lock released here

        // Begin a real SurrealDB transaction
        self.db
            .query("BEGIN TRANSACTION;")
            .await
            .map_err(|e| {
                tracing::error!("Failed to begin transaction: {}", e);
                Box::new(e) as Box<dyn StdError + Send + Sync>
            })?;

        // Update transaction state after successful begin
        {
            let mut active = self.transaction_active.lock().unwrap();
            *active = true;
        }
        
        tracing::info!("SurrealDB transaction started for add_user_to_group");
        Ok(())
    }

    async fn commit(&self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        // Check transaction state before committing
        {
            let active = self
                .transaction_active
                .lock()
                .map_err(|e| format!("Lock error: {}", e))?;

            if !*active {
                return Err("No active transaction to commit".into());
            }
        } // Lock released here

        // Commit the SurrealDB transaction
        self.db
            .query("COMMIT TRANSACTION;")
            .await
            .map_err(|e| {
                tracing::error!("Failed to commit transaction: {}", e);
                Box::new(e) as Box<dyn StdError + Send + Sync>
            })?;

        // Update transaction state after successful commit
        {
            let mut active = self.transaction_active.lock().unwrap();
            *active = false;
        }
        
        tracing::info!("SurrealDB transaction committed for add_user_to_group");
        Ok(())
    }

    async fn rollback(&self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        // Check transaction state before rollback
        {
            let active = self
                .transaction_active
                .lock()
                .map_err(|e| format!("Lock error: {}", e))?;

            if !*active {
                return Err("No active transaction to rollback".into());
            }
        } // Lock released here

        // Rollback the SurrealDB transaction using CANCEL
        self.db
            .query("CANCEL TRANSACTION;")
            .await
            .map_err(|e| {
                tracing::error!("Failed to rollback transaction: {}", e);
                Box::new(e) as Box<dyn StdError + Send + Sync>
            })?;

        // Update transaction state after successful rollback
        {
            let mut active = self.transaction_active.lock().unwrap();
            *active = false;
        }
        
        tracing::warn!("SurrealDB transaction rolled back for add_user_to_group");
        Ok(())
    }

    fn repositories(&self) -> AddUserToGroupRepositories {
        AddUserToGroupRepositories::new(self.user_repository.clone(), self.group_repository.clone())
    }
}
