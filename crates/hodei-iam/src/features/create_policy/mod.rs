//! create_policy Feature (Vertical Slice)
//!
//! This module wires together the components that make up the new segregated
//! Create Policy feature for IAM. It follows the required VSA (Vertical Slice
//! Architecture) + Clean Architecture structure:
//!
//! - dto.rs              -> Command & View DTOs
//! - error.rs            -> Feature-specific error types
//! - ports.rs            -> Segregated interface definitions (ISP)
//! - use_case.rs         -> Core business logic (CreatePolicyUseCase)
//! - validator.rs        -> Cedar policy validator implementation
//! - di.rs               -> Dependency Injection helpers
//! - mocks.rs            -> Test-only mock implementations of ports
//! - use_case_test.rs    -> Unit tests for the use case
//!
//! Re-exports below intentionally expose ONLY what the application layer needs:
//! - Command / View DTOs
//! - Use case
//! - Error and Port traits
//! - Validator implementation
//!
//! Internal mocks remain private (or test-gated) to avoid leaking test utilities
//! across crate boundaries.
//!
//! Future additions (other CRUD operations) will live in their own vertical slices:
//! - delete_policy
//! - update_policy
//! - get_policy
//! - list_policies
//!
//! This segregation replaces the former monolithic `create_policy` feature and
//! enforces Interface Segregation (ISP) strictly.
pub mod dto;
pub mod error;
pub mod ports;
pub mod use_case;
mod validator;
// Mocks are kept internal (they are used by unit tests inside the crate)
mod mocks;

pub mod factories;
// ---------------------------------------------------------------------------
// PUBLIC RE-EXPORTS (Feature API Surface)
// ---------------------------------------------------------------------------
pub use dto::{CreatePolicyCommand, PolicyView};
pub use error::CreatePolicyError;
pub use ports::{CreatePolicyPort, PolicyValidationError, PolicyValidator, ValidationResult};
pub use use_case::CreatePolicyUseCase;
pub use validator::CedarPolicyValidator;
// ---------------------------------------------------------------------------
// TEST SUPPORT (Optional re-export under cfg(test))
// ---------------------------------------------------------------------------
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use mocks::{MockCreatePolicyPort, MockPolicyValidator};
