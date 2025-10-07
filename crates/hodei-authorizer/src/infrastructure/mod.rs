//! Infrastructure layer for hodei-authorizer
//!
//! This module contains concrete implementations of infrastructure
//! components used by the authorization system.

pub mod surreal;

// Re-export commonly used types
pub use surreal::SurrealOrganizationBoundaryProvider;
