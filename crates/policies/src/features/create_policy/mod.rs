//! Feature: create_policy
//!
//! This feature implements the creation (insert/persist) of authorization policies
//! in the `policies` crate following Vertical Slice Architecture (VSA) principles.
//!
//! ## Responsibilities
//!
//! - Validate policy syntax and semantics (delegating to Cedar-aware validator)
//! - Ensure ID uniqueness and versioning
//! - Persist policy document and metadata (scope, enabled, created_at, etc.)
//! - Emit domain events (e.g., `PolicyCreated`) when event bus is available
//! - Keep Cedar types internal - only expose strings and DTOs externally
//!
//! ## Architecture
//!
//! This feature follows Clean Architecture with VSA:
//!
//! ```text
//! create_policy/
//! ├── mod.rs           (module exports)
//! ├── dto.rs           (CreatePolicyCommand, CreatedPolicyDto)
//! ├── error.rs         (CreatePolicyError)
//! ├── ports.rs         (PolicyPersister, PolicyIdGenerator, PolicyValidator)
//! ├── use_case.rs      (CreatePolicyUseCase - business logic)
//! └── (future: adapter.rs, di.rs, event_handler.rs)
//! ```
//!
//! ## Usage Example
//!
//! ```rust,ignore
//! use policies::features::create_policy::{
//!     CreatePolicyCommand, CreatePolicyUseCase,
//! };
//!
//! let use_case = CreatePolicyUseCase::new(
//!     persister,
//!     validator,
//!     id_generator,
//! );
//!
//! let command = CreatePolicyCommand {
//!     policy_document: "permit(principal, action, resource);".to_string(),
//!     scope: "system".to_string(),
//!     provided_id: None,
//!     enabled: true,
//!     description: Some("Allow all".to_string()),
//!     tags: vec![],
//! };
//!
//! let result = use_case.execute(command)?;
//! println!("Created policy ID: {}", result.id);
//! ```

pub mod adapter;
pub mod di;
pub mod dto;
pub mod error;
pub mod ports;
pub mod use_case;

#[cfg(test)]
pub mod mocks;

#[cfg(test)]
mod use_case_test;

// Re-exports for ergonomic use
pub use adapter::{CedarPolicyValidator, InMemoryPolicyPersister, UuidPolicyIdGenerator};
pub use di::{CreatePolicyContainer, ProductionContainer, create_production_container};
pub use dto::{CreatePolicyCommand, CreatedPolicyDto, PolicyContent};
pub use error::CreatePolicyError;
pub use ports::{PolicyIdGenerator, PolicyPersister, PolicyValidator};
pub use use_case::CreatePolicyUseCase;

#[cfg(test)]
pub use mocks::{MockPolicyIdGenerator, MockPolicyPersister, MockPolicyValidator};
