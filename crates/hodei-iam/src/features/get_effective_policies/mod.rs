//! get_effective_policies Feature (Vertical Slice)
//!
//! This module wires together the components that make up the segregated
//! Get Effective Policies feature for IAM. It follows the required VSA (Vertical Slice
//! Architecture) + Clean Architecture structure:
//!
//! - dto.rs              -> Query & Response DTOs
//! - error.rs            -> Feature-specific error types
//! - ports.rs            -> Segregated interface definitions (ISP)
//! - use_case.rs         -> Core business logic (GetEffectivePoliciesUseCase)
//! - di.rs               -> Dependency Injection helpers
//! - mocks.rs            -> Test-only mock implementations of ports
//! - use_case_test.rs    -> Unit tests for the use case
//!
//! Re-exports below intentionally expose ONLY what the application layer needs:
//! - Query / Response DTOs
//! - Use case
//! - Error and Port traits
//!
//! Internal mocks remain private (or test-gated) to avoid leaking test utilities
//! across crate boundaries.

pub mod dto;
pub mod error;
pub mod ports;
pub mod use_case;
// Mocks are kept internal (they are used by unit tests inside the crate)
mod mocks;

// ---------------------------------------------------------------------------
// PUBLIC RE-EXPORTS (Feature API Surface)
// ---------------------------------------------------------------------------
pub use dto::{EffectivePoliciesResponse, GetEffectivePoliciesQuery};
pub use error::{GetEffectivePoliciesError, GetEffectivePoliciesResult};
pub use ports::{GroupFinderPort, PolicyFinderPort, UserFinderPort};
pub use use_case::GetEffectivePoliciesUseCase;

// ---------------------------------------------------------------------------
// TEST SUPPORT (Optional re-export under cfg(test))
// ---------------------------------------------------------------------------
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use mocks::{MockGroupFinderPort, MockPolicyFinderPort, MockUserFinderPort};
