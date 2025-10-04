//! Application layer for the shared kernel
//!
//! This module contains application-level abstractions and contracts
//! that are shared across different bounded contexts.
pub mod ports;

// Re-export commonly used types
pub use ports::{UnitOfWork, UnitOfWorkError, UnitOfWorkFactory};
