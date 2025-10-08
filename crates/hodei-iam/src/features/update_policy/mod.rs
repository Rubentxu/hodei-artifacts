//! update_policy Feature (Vertical Slice)
//!
//! This module implements the segregated feature for updating an existing IAM policy.
//! It follows the VSA (Vertical Slice Architecture) + Clean Architecture structure.
//!
//! - dto.rs              -> Command and View DTOs
//! - error.rs            -> Feature-specific error types
//! - ports.rs            -> Segregated interface definition (UpdatePolicyPort)
//! - use_case.rs         -> Core business logic (UpdatePolicyUseCase)
//! - adapter.rs          -> Infrastructure adapter implementations (stub/in-memory)
//! - di.rs               -> Dependency Injection helpers
//! - mocks.rs            -> Test-only mock implementations of ports
//! - use_case_test.rs    -> Unit tests for the use case
//!
//! # Update Semantics
//!
//! Policy updates follow these rules:
//! - Policy must exist before updating
//! - Policy content is validated (Cedar syntax) before persistence
//! - Description can be updated independently or together with content
//! - System-protected policies may have restricted update capabilities
//! - Updated timestamp is automatically tracked
//! - Optimistic locking via version/etag (future enhancement)

pub mod di;
pub mod dto;
pub mod error;
pub mod mocks;
pub mod ports;
pub mod use_case;
// Test file is not a module, so it's not declared here.

// ---------------------------------------------------------------------------
// PUBLIC RE-EXPORTS (Feature API Surface)
// ---------------------------------------------------------------------------
pub use dto::{PolicyView, UpdatePolicyCommand};
pub use error::UpdatePolicyError;
pub use ports::{PolicyValidationError, PolicyValidator, UpdatePolicyPort, ValidationResult};
pub use use_case::UpdatePolicyUseCase;

// ---------------------------------------------------------------------------
// TEST SUPPORT (Optional re-export under cfg(test))
// ---------------------------------------------------------------------------
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use mocks::{MockPolicyValidator, MockUpdatePolicyPort};
