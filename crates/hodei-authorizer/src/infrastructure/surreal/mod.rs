//! SurrealDB infrastructure implementations for hodei-authorizer
//!
//! This module contains SurrealDB-specific implementations of
//! authorization infrastructure components.

pub mod organization_boundary_provider;

#[cfg(test)]
mod organization_boundary_provider_test;

// Re-export commonly used types
pub use organization_boundary_provider::SurrealOrganizationBoundaryProvider;
