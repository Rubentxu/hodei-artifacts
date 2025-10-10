//! delete_policy Feature (Vertical Slice)
//!
//! This module implements the segregated feature for deleting an IAM policy.
//! It follows the VSA (Vertical Slice Architecture) + Clean Architecture structure.
//!
//! - dto.rs              -> Command DTO
//! - error.rs            -> Feature-specific error types
//! - ports.rs            -> Segregated interface definition (DeletePolicyPort)
//! - use_case.rs         -> Core business logic (DeletePolicyUseCase)
//! - factories.rs        -> Dependency Injection helpers
//! - mocks.rs            -> Test-only mock implementations of the port
//! - use_case_test.rs    -> Unit tests for the use case
//!
//! Re-exports below expose only what the application layer needs.

pub mod dto;
pub mod error;
pub mod factories;
pub mod mocks;
pub mod ports;
pub mod use_case;

#[cfg(test)]
mod use_case_test;
// Test file is not a module, so it's not declared here.

// ---------------------------------------------------------------------------
// PUBLIC RE-EXPORTS (Feature API Surface)
// ---------------------------------------------------------------------------
/// Public API for the delete_policy feature
pub use dto::DeletePolicyCommand;
pub use error::DeletePolicyError;
pub use ports::DeletePolicyPort;
pub use use_case::DeletePolicyUseCase;

// ---------------------------------------------------------------------------
// TEST SUPPORT (Optional re-export under cfg(test))
// ---------------------------------------------------------------------------
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use mocks::MockDeletePolicyPort;
