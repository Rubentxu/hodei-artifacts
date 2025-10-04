//! SurrealDB infrastructure implementations
//! 
//! This module contains all SurrealDB-specific implementations of
//! repositories and transaction management.
pub mod unit_of_work;
pub mod account_repository;
pub mod ou_repository;
pub mod scp_repository;

// Re-export commonly used types
pub use unit_of_work::{
    SurrealUnitOfWork, 
    SurrealUnitOfWorkFactory,
    TransactionalAccountRepository,
    TransactionalOuRepository,
    TransactionalScpRepository,
};
pub use account_repository::SurrealAccountRepository;
pub use ou_repository::SurrealOuRepository;
pub use scp_repository::SurrealScpRepository;
