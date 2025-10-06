//! list_policies Feature (Vertical Slice)
//!
//! This module implements the List Policies feature for IAM following VSA.
//!
//! Structure:
//! - dto.rs              -> Query & Response DTOs with pagination
//! - error.rs            -> Feature-specific error types
//! - ports.rs            -> Segregated interface (ISP)
//! - use_case.rs         -> Core business logic (ListPoliciesUseCase)
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
pub use adapter::InMemoryPolicyLister;
pub use dto::{ListPoliciesQuery, ListPoliciesResponse, PageInfo, PolicySummary};
pub use error::ListPoliciesError;
pub use ports::PolicyLister;
pub use use_case::ListPoliciesUseCase;

