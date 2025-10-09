//! Register IAM Schema Feature
//!
//! This feature orchestrates the registration of all IAM entity types and action types
//! with the policies engine, and triggers the schema building process.
//!
//! This is a bootstrapping feature that should be executed during application startup
//! to ensure that the Cedar schema includes all IAM-specific types.

pub mod dto;
pub mod error;
pub mod factories;
pub mod ports;
pub mod use_case;

#[cfg(test)]
mod use_case_test;

// Re-export for convenience
pub use dto::{RegisterIamSchemaCommand, RegisterIamSchemaResult};
pub use error::RegisterIamSchemaError;
pub use ports::RegisterIamSchemaPort;
pub use use_case::RegisterIamSchemaUseCase;
