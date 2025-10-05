use async_trait::async_trait;
use std::sync::Arc;
use thiserror::Error;

/// Error types for UnitOfWork operations
#[derive(Debug, Error)]
pub enum UnitOfWorkError {
    #[error("Transaction error: {0}")]
    Transaction(String),
    #[error("Connection error: {0}")]
    Connection(String),
    #[error("Commit failed: {0}")]
    CommitFailed(String),
    #[error("Rollback failed: {0}")]
    RollbackFailed(String),
}



/// Unit of Work trait for managing transactions
/// 
/// This trait establishes a contract for transactional operations across different
/// persistence providers. It follows the Unit of Work pattern to ensure that
/// multiple operations are executed atomically.
/// 
/// ## Design Decision
/// Repositories are obtained from the UnitOfWork itself rather than being passed
/// a transaction handle. This ensures that all repository operations are automatically
/// bound to the transaction context without requiring explicit transaction management
/// in the business logic.
#[async_trait]
pub trait UnitOfWork: Send + Sync {
    /// Type for account repository bound to this transaction
    type AccountRepository: Send + Sync;
    
    /// Type for organizational unit repository bound to this transaction
    type OuRepository: Send + Sync;
    
    /// Type for service control policy repository bound to this transaction
    type ScpRepository: Send + Sync;

    /// Begin a new transaction
    async fn begin(&mut self) -> Result<(), UnitOfWorkError>;
    
    /// Commit the current transaction
    async fn commit(&mut self) -> Result<(), UnitOfWorkError>;
    
    /// Rollback the current transaction
    async fn rollback(&mut self) -> Result<(), UnitOfWorkError>;
    
    /// Get a repository for account operations bound to this transaction
    fn accounts(&self) -> Arc<Self::AccountRepository>;
    
    /// Get a repository for organizational unit operations bound to this transaction
    fn ous(&self) -> Arc<Self::OuRepository>;
    
    /// Get a repository for service control policy operations bound to this transaction
    fn scps(&self) -> Arc<Self::ScpRepository>;
}

/// Factory for creating UnitOfWork instances
/// 
/// This allows dependency injection of UnitOfWork creation while keeping the
/// business logic decoupled from the specific implementation.
#[async_trait]
pub trait UnitOfWorkFactory: Send + Sync {
    /// Type of UnitOfWork this factory creates
    type UnitOfWork: UnitOfWork;
    
    /// Create a new UnitOfWork instance
    async fn create(&self) -> Result<Self::UnitOfWork, UnitOfWorkError>;
}
