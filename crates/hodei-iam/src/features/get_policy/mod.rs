//! get_policy Feature (Vertical Slice)
//!
//! This module implements the Get Policy feature for IAM following VSA.
//!
//! Structure:
//! - dto.rs              -> Query & View DTOs
//! - error.rs            -> Feature-specific error types
//! - ports.rs            -> Segregated interface (ISP)
//! - use_case.rs         -> Core business logic (GetPolicyUseCase)
//! - adapter.rs          -> Infrastructure adapter implementations
//! - di.rs               -> Dependency Injection helpers
//! - mocks.rs            -> Test-only mock implementations

pub mod adapter;
pub mod di;
pub mod dto;
pub mod error;
pub mod ports;
pub mod use_case;

#[cfg(test)]
mod mocks;

// Public API
pub use adapter::InMemoryPolicyReader;
pub use dto::{GetPolicyQuery, PolicyView};
pub use error::GetPolicyError;
pub use ports::PolicyReader;
pub use use_case::GetPolicyUseCase;
