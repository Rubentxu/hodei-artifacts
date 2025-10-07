//! SurrealDB infrastructure implementations
//!
//! This module contains all SurrealDB-specific implementations of
//! repositories and transaction management.
pub mod account_repository;
pub mod ou_repository;
pub mod scp_repository;
pub mod unit_of_work;

// Re-export commonly used types
pub use account_repository::SurrealAccountRepository;
pub use ou_repository::SurrealOuRepository;
pub use scp_repository::SurrealScpRepository;
pub use unit_of_work::{SurrealUnitOfWork, SurrealUnitOfWorkFactory};
