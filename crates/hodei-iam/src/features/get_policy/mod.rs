//! get_policy Feature (Vertical Slice)
//!
//! This module implements the Get Policy feature for IAM following VSA.
//!
//! Structure:
//! - dto.rs              -> Query & View DTOs
//! - error.rs            -> Feature-specific error types
//! - ports.rs            -> Segregated interface (ISP)
//! - use_case.rs         -> Core business logic (GetPolicyUseCase)
//! - mocks.rs            -> Test-only mock implementations

pub mod dto;
pub mod error;
pub mod ports;
mod use_case;
#[cfg(test)]
mod mocks;

// Public API
pub use dto::{GetPolicyQuery, PolicyView};
pub use error::GetPolicyError;
pub use ports::PolicyReader;
pub use use_case::GetPolicyUseCase;
